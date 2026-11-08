"""Local SQLite store for watcher state, event history, polls and tasks.

One stdlib ``sqlite3`` database under the state directory (``state.db``) owns
all durable bookkeeping: the global metadata (``repo`` / ``api`` /
``last_poll`` / schema ``version``), the per-issue cursor snapshots, pending
batches and their event lines, the ``## Tasks`` checklist mirror, and one row
per poll. ``pending.md`` remains only a generated, human/agent-facing view.

The schema version is ``PRAGMA user_version`` (``SCHEMA_VERSION``). WAL keeps
readers non-blocking while the loop writes, and ``busy_timeout`` bounds
contention. Every logical operation commits in one explicit transaction, so a
poll's state + event rows are atomic and publishing still happens before
``last_poll`` advances.

:meth:`Database.migrate_from_files` imports the pre-#135 flat files exactly once,
inside one ``BEGIN IMMEDIATE`` transaction, then archives the old files under
``legacy-json-<ts>/``. That compatibility path is retired once no known state
directory still contains flat files (target 2027-01-01); see
``gitea_bots/README.md`` for the removal condition.
"""

from __future__ import annotations

import json
import os
import shutil
import sqlite3
from contextlib import contextmanager
from pathlib import Path

from . import EVENTS_VERSION
from .clock import compact_utc, now_utc

SCHEMA_VERSION = 1

SCHEMA = """
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
CREATE TABLE IF NOT EXISTS issues (
    number INTEGER PRIMARY KEY,
    updated_at TEXT,
    title TEXT,
    state TEXT,
    labels TEXT,
    assignees TEXT,
    milestone TEXT,
    last_comment_id INTEGER NOT NULL DEFAULT 0,
    auto_plan_at TEXT,
    last_assignee_event_id INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS batches (
    id TEXT PRIMARY KEY,
    repo TEXT,
    event_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    acked_at TEXT,
    legacy INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT REFERENCES batches(id),
    at TEXT,
    repo TEXT,
    line TEXT
);
CREATE INDEX IF NOT EXISTS events_batch ON events(batch_id);
CREATE INDEX IF NOT EXISTS events_at ON events(at);
CREATE TABLE IF NOT EXISTS tasks (
    issue INTEGER,
    position INTEGER,
    text TEXT,
    done INTEGER NOT NULL DEFAULT 0,
    synced_at TEXT,
    PRIMARY KEY (issue, position)
);
CREATE TABLE IF NOT EXISTS polls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT,
    scanned INTEGER NOT NULL DEFAULT 0,
    emitted INTEGER NOT NULL DEFAULT 0,
    ok INTEGER NOT NULL DEFAULT 1,
    error TEXT
);
"""

_ISSUE_FIELDS = (
    "updated_at",
    "title",
    "state",
    "labels",
    "assignees",
    "milestone",
    "last_comment_id",
    "auto_plan_at",
    "last_assignee_event_id",
)
_META_FIELDS = ("version", "repo", "api", "initialized_at", "last_poll")


def _as_int(value, default: int = 0) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _parse_batch(path: Path):
    """Return ``(meta, body)`` for a legacy ``.batch`` file."""
    lines = path.read_text(encoding="utf-8").splitlines()
    meta = {}
    idx = len(lines)
    for i, line in enumerate(lines):
        if line == "":
            idx = i + 1
            break
        if ":" in line:
            key, value = line.split(":", 1)
            meta[key.strip()] = value.strip()
    return meta, "\n".join(lines[idx:])


def _event_line_count(body: str) -> int:
    return sum(1 for line in body.splitlines() if line[:4].isdigit())


class Database:
    """One SQLite connection and schema, shared by the state and inbox stores."""

    def __init__(self, path, logger=None):
        self.path = Path(path)
        self.log = logger
        self._conn = None
        self._schema_ready = False

    # ------------------------------------------------------- connection / schema
    def connect(self):
        if self._conn is None:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            conn = sqlite3.connect(
                self.path, timeout=5.0, isolation_level=None
            )
            try:
                conn.row_factory = sqlite3.Row
                conn.execute("PRAGMA journal_mode=WAL")
                conn.execute("PRAGMA synchronous=NORMAL")
                conn.execute("PRAGMA busy_timeout=5000")
                conn.execute("PRAGMA foreign_keys=ON")
            except BaseException:
                conn.close()
                raise
            self._conn = conn
        return self._conn

    def close(self) -> None:
        if self._conn is not None:
            try:
                self._conn.close()
            finally:
                self._conn = None
                self._schema_ready = False

    def quarantine(self, reason: str):
        """Close and rename the DB (and WAL sidecars) aside.

        Returns the base backup path, or ``None`` when nothing was moved.
        """
        self.close()
        if not self.path.exists():
            return None
        suffix = f".{reason}.{compact_utc()}"
        moved = None
        for path in (
            self.path,
            self.path.with_name(self.path.name + "-wal"),
            self.path.with_name(self.path.name + "-shm"),
        ):
            if not path.exists():
                continue
            target = path.with_name(path.name + suffix)
            try:
                os.replace(path, target)
            except OSError:
                continue
            if path == self.path:
                moved = target
        return moved

    def ensure_schema(self) -> None:
        if self._schema_ready:
            return
        conn = self.connect()
        conn.executescript(SCHEMA)
        if self.user_version() < SCHEMA_VERSION:
            conn.execute(f"PRAGMA user_version = {int(SCHEMA_VERSION)}")
        self._schema_ready = True

    def user_version(self) -> int:
        row = self.connect().execute("PRAGMA user_version").fetchone()
        return _as_int(row[0] if row is not None else 0)

    @contextmanager
    def transaction(self):
        """Run a block in one ``BEGIN IMMEDIATE`` transaction."""
        self.ensure_schema()
        conn = self.connect()
        conn.execute("BEGIN IMMEDIATE")
        try:
            yield conn
        except BaseException:
            conn.execute("ROLLBACK")
            raise
        else:
            conn.execute("COMMIT")

    # -------------------------------------------------------------------- meta
    def get_meta(self, key, default=None):
        self.ensure_schema()
        row = self.connect().execute(
            "SELECT value FROM meta WHERE key = ?", (key,)
        ).fetchone()
        return row["value"] if row is not None else default

    def all_meta(self) -> dict:
        self.ensure_schema()
        return {
            row["key"]: row["value"]
            for row in self.connect().execute("SELECT key, value FROM meta")
        }

    def set_meta(self, key, value, conn=None) -> None:
        (conn or self.connect()).execute(
            "INSERT INTO meta (key, value) VALUES (?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )

    # ------------------------------------------------------------------ issues
    def issue_map(self) -> dict:
        self.ensure_schema()
        rows = self.connect().execute(
            "SELECT * FROM issues ORDER BY number"
        ).fetchall()
        return {
            str(row["number"]): {
                field: ("" if row[field] is None else row[field])
                for field in _ISSUE_FIELDS
            }
            for row in rows
        }

    def get_issue(self, number):
        self.ensure_schema()
        row = self.connect().execute(
            "SELECT * FROM issues WHERE number = ?", (int(number),)
        ).fetchone()
        if row is None:
            return {}
        return {
            field: ("" if row[field] is None else row[field])
            for field in _ISSUE_FIELDS
        }

    def upsert_issue(self, number, snapshot, conn=None) -> None:
        snapshot = snapshot or {}
        values = (
            int(number),
            snapshot.get("updated_at") or "",
            snapshot.get("title") or "",
            snapshot.get("state") or "",
            snapshot.get("labels") or "",
            snapshot.get("assignees") or "",
            snapshot.get("milestone") or "",
            _as_int(snapshot.get("last_comment_id")),
            snapshot.get("auto_plan_at") or None,
            _as_int(snapshot.get("last_assignee_event_id")),
        )
        (conn or self.connect()).execute(
            "INSERT INTO issues (number, updated_at, title, state, labels, "
            "assignees, milestone, last_comment_id, auto_plan_at, "
            "last_assignee_event_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) "
            "ON CONFLICT(number) DO UPDATE SET "
            "updated_at = excluded.updated_at, title = excluded.title, "
            "state = excluded.state, labels = excluded.labels, "
            "assignees = excluded.assignees, milestone = excluded.milestone, "
            "last_comment_id = excluded.last_comment_id, "
            "auto_plan_at = excluded.auto_plan_at, "
            "last_assignee_event_id = excluded.last_assignee_event_id",
            values,
        )

    # ----------------------------------------------------------------- batches
    def add_batch(self, batch_id, repo, event_count, created_at, *,
                  legacy=False, acked_at=None, conn=None) -> None:
        (conn or self.connect()).execute(
            "INSERT INTO batches (id, repo, event_count, created_at, acked_at, "
            "legacy) VALUES (?, ?, ?, ?, ?, ?) "
            "ON CONFLICT(id) DO UPDATE SET repo = excluded.repo, "
            "event_count = excluded.event_count, created_at = excluded.created_at",
            (
                batch_id,
                repo or "",
                _as_int(event_count),
                created_at or "",
                acked_at,
                1 if legacy else 0,
            ),
        )

    def pending_batches(self) -> list:
        self.ensure_schema()
        rows = self.connect().execute(
            "SELECT * FROM batches WHERE acked_at IS NULL "
            "ORDER BY created_at, id"
        ).fetchall()
        return [dict(row) for row in rows]

    def ack_batch(self, batch_id, conn=None) -> bool:
        cursor = (conn or self.connect()).execute(
            "UPDATE batches SET acked_at = ? "
            "WHERE id = ? AND acked_at IS NULL",
            (now_utc(), batch_id),
        )
        return cursor.rowcount > 0

    # ------------------------------------------------------------------ events
    def add_event(self, batch_id, at, repo, line, conn=None) -> None:
        (conn or self.connect()).execute(
            "INSERT INTO events (batch_id, at, repo, line) VALUES (?, ?, ?, ?)",
            (batch_id, at or "", repo or "", line),
        )

    def add_events(self, batch_id, at, repo, lines, conn=None) -> None:
        for line in lines:
            self.add_event(batch_id, at, repo, line, conn=conn)

    def event_lines(self, batch_id) -> list:
        self.ensure_schema()
        return [
            row["line"]
            for row in self.connect().execute(
                "SELECT line FROM events WHERE batch_id = ? ORDER BY id",
                (batch_id,),
            )
        ]

    def count_events(self) -> int:
        self.ensure_schema()
        row = self.connect().execute("SELECT COUNT(*) FROM events").fetchone()
        return _as_int(row[0] if row is not None else 0)

    # ------------------------------------------------------------------- polls
    def record_poll(self, scanned, emitted, ok, error=None, *, at=None,
                    conn=None) -> None:
        (conn or self.connect()).execute(
            "INSERT INTO polls (at, scanned, emitted, ok, error) "
            "VALUES (?, ?, ?, ?, ?)",
            (
                at or now_utc(),
                _as_int(scanned),
                _as_int(emitted),
                1 if ok else 0,
                error,
            ),
        )

    def count_polls(self) -> int:
        self.ensure_schema()
        row = self.connect().execute("SELECT COUNT(*) FROM polls").fetchone()
        return _as_int(row[0] if row is not None else 0)

    # ------------------------------------------------------------------- tasks
    def replace_tasks(self, issue, tasks, *, synced_at=None, conn=None) -> None:
        """Replace one issue's mirrored checklist.

        ``tasks`` is an iterable of ``(position, text, done)`` tuples.
        """
        conn = conn or self.connect()
        conn.execute("DELETE FROM tasks WHERE issue = ?", (int(issue),))
        stamp = synced_at or now_utc()
        for position, text, done in tasks:
            conn.execute(
                "INSERT INTO tasks (issue, position, text, done, synced_at) "
                "VALUES (?, ?, ?, ?, ?)",
                (int(issue), int(position), text, 1 if done else 0, stamp),
            )

    def tasks(self, issue) -> list:
        self.ensure_schema()
        return [
            dict(row)
            for row in self.connect().execute(
                "SELECT issue, position, text, done, synced_at FROM tasks "
                "WHERE issue = ? ORDER BY position",
                (int(issue),),
            )
        ]

    # ----------------------------------------------------------------- migrate
    def has_data(self) -> bool:
        self.ensure_schema()
        for table in ("issues", "batches", "events", "polls", "tasks"):
            if self.connect().execute(
                f"SELECT 1 FROM {table} LIMIT 1"
            ).fetchone() is not None:
                return True
        return False

    def _warn(self, message: str) -> None:
        if self.log is not None:
            self.log.warn(message)

    def _die(self, message: str):
        if self.log is not None:
            self.log.die(message)
        raise RuntimeError(message)

    def migrate_from_files(self, state_dir) -> bool:
        """Import the legacy flat files once and archive them.

        The database writes happen in one transaction. If anything fails, the
        transaction rolls back and the legacy files are left untouched; a
        one-shot marker (``meta.imported_from_files``) prevents a re-import.
        """
        state_dir = Path(state_dir)
        self.ensure_schema()
        if (self.get_meta("imported_from_files") or "") == "1":
            return False
        if self.has_data():
            # A previous import committed its rows but did not reach the marker
            # (for example the archive step failed). Never import twice.
            self.set_meta("imported_from_files", "1")
            return False

        state_json = state_dir / "state.json"
        events_jsonl = state_dir / "events.jsonl"
        batch_dirs = [
            (state_dir / "pending", True),
            (state_dir / "pending-ack", False),
            (state_dir / "ack", False),
        ]
        batches = []
        for directory, pending in batch_dirs:
            if directory.is_dir():
                batches.extend(
                    (path, pending) for path in sorted(directory.glob("*.batch"))
                )

        if not (state_json.exists() or events_jsonl.exists() or batches):
            return False

        try:
            with self.transaction() as conn:
                self._import_state_json(state_json, conn)
                self._import_batches(batches, conn)
                self._import_events_jsonl(events_jsonl, conn)
                self.set_meta("imported_from_files", "1", conn=conn)
        except sqlite3.Error as exc:
            self._die(
                f"legacy-file migration failed ({exc}) — rolled back; the flat "
                "files are untouched"
            )

        self._archive_legacy(state_dir, state_json, events_jsonl, batch_dirs)
        if self.log is not None:
            self.log.log(
                "legacy flat files migrated to state.db "
                f"({len(batches)} batch file(s)); originals archived under "
                f"{state_dir}/legacy-json-*"
            )
        return True

    def _import_state_json(self, path: Path, conn) -> None:
        if not path.is_file():
            return
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            self._warn(f"legacy state file is unreadable — not importing: {path}")
            return
        if not isinstance(data, dict):
            self._warn(f"legacy state file has an unexpected shape: {path}")
            return
        for key in _META_FIELDS:
            if key in data:
                value = data[key]
                self.set_meta(
                    key, "" if value is None else str(value), conn=conn
                )
        issues = data.get("issues") or {}
        if not isinstance(issues, dict):
            return
        for number, snapshot in issues.items():
            if isinstance(snapshot, dict):
                self.upsert_issue(_as_int(number), snapshot, conn=conn)

    def _import_batches(self, batches, conn) -> None:
        seen = set()
        for path, pending in batches:
            meta, body = _parse_batch(path)
            batch_id = meta.get("id") or path.stem
            if batch_id in seen:
                continue
            seen.add(batch_id)
            created = meta.get("created_at") or batch_id
            repo = meta.get("repo") or self.get_meta("repo", "") or ""
            count = _as_int(meta.get("events"), _event_line_count(body))
            legacy = (meta.get("legacy") or "").lower() == "true"
            self.add_batch(
                batch_id, repo, count, created,
                legacy=legacy,
                acked_at=None if pending else created,
                conn=conn,
            )
            self.add_events(batch_id, created, repo, body.splitlines(), conn=conn)

    def _import_events_jsonl(self, path: Path, conn) -> None:
        if not path.is_file():
            return
        existing = {
            (row["batch_id"], row["line"])
            for row in conn.execute("SELECT batch_id, line FROM events")
        }
        counts = {}
        known = {
            row["id"] for row in conn.execute("SELECT id FROM batches")
        }
        default_repo = self.get_meta("repo", "") or ""
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except OSError as exc:
            self._warn(f"could not read legacy event log: {path} ({exc})")
            return
        for raw in lines:
            try:
                record = json.loads(raw)
            except ValueError:
                continue
            if not isinstance(record, dict):
                continue
            line = record.get("line")
            if not isinstance(line, str):
                continue
            batch_id = record.get("batch") or "legacy-jsonl"
            at = record.get("at") or ""
            repo = record.get("repo") or default_repo
            if batch_id not in known:
                self.add_batch(
                    batch_id, repo, 0, at or now_utc(),
                    legacy=True, acked_at=at or now_utc(), conn=conn,
                )
                known.add(batch_id)
            key = (batch_id, line)
            if key in existing:
                continue
            self.add_event(batch_id, at, repo, line, conn=conn)
            existing.add(key)
            counts[batch_id] = counts.get(batch_id, 0) + 1
        for batch_id, count in counts.items():
            conn.execute(
                "UPDATE batches SET event_count = "
                "(SELECT COUNT(*) FROM events WHERE batch_id = ?) "
                "WHERE id = ? AND event_count = 0",
                (batch_id, batch_id),
            )

    def _archive_legacy(self, state_dir, state_json, events_jsonl,
                        batch_dirs) -> None:
        dest = state_dir / f"legacy-json-{compact_utc()}"
        try:
            dest.mkdir(parents=True, exist_ok=True)
        except OSError as exc:
            self._warn(f"could not create legacy archive directory {dest}: {exc}")
            return
        paths = [state_json, events_jsonl]
        paths.extend(directory for directory, _ in batch_dirs if directory.is_dir())
        for path in paths:
            if not path.exists():
                continue
            try:
                shutil.move(str(path), str(dest / path.name))
            except OSError as exc:
                self._warn(f"could not archive {path}: {exc}")
