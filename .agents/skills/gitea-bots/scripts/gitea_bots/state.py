"""Durable watcher state on top of the local SQLite store (``state.db``).

The global metadata (``version``, ``repo``, ``api``, ``initialized_at``,
``last_poll``) and the per-issue cursor snapshots live in :mod:`gitea_bots.db`.
:meth:`StateStore.load_or_init` returns a read-only dict view for the watcher's
decision code; :meth:`commit_poll` / :meth:`commit_issues` write one atomic
transaction per poll, so events are published before ``last_poll`` advances.

A corrupt database is renamed to ``state.db.corrupt.<ts>`` and re-baselined;
a database owned by another repo is renamed to ``state.db.repo.<ts>``.
"""

from __future__ import annotations

import os
import sqlite3

from . import EVENTS_VERSION
from .clock import compact_utc, now_utc
from .db import Database


class StateStore:
    def __init__(self, config, logger, db=None):
        self.config = config
        self.log = logger
        self.db = db or Database(config.db_file, logger)
        self.path = self.db.path

    def _new(self) -> dict:
        return {
            "version": EVENTS_VERSION,
            "repo": self.config.repo,
            "api": self.config.api_base,
            "initialized_at": now_utc(),
            "last_poll": None,
            "issues": {},
        }

    def _write_meta(self, state: dict) -> None:
        with self.db.transaction() as conn:
            for key in ("version", "repo", "api", "initialized_at", "last_poll"):
                value = state.get(key)
                self.db.set_meta(
                    key, "" if value is None else str(value), conn=conn
                )

    def _backup(self, reason: str) -> None:
        """Close the connection and move the DB (and WAL sidecars) aside."""
        self.db.quarantine(reason)

    def load(self) -> dict:
        """Return the current state as a dict without re-baselining."""
        self.db.ensure_schema()
        meta = self.db.all_meta()
        version = meta.get("version") or "1"
        try:
            version = int(version)
        except (TypeError, ValueError):
            version = 1
        return {
            "version": version,
            "repo": meta.get("repo") or self.config.repo,
            "api": meta.get("api") or self.config.api_base,
            "initialized_at": meta.get("initialized_at") or "",
            "last_poll": meta.get("last_poll") or None,
            "issues": self.db.issue_map(),
        }

    def load_or_init(self) -> dict:
        """Return a valid state, re-baselining (with a backup) when needed."""
        try:
            self.db.ensure_schema()
            repo = self.db.get_meta("repo") or ""
            if not repo:
                state = self._new()
                self._write_meta(state)
                return state
            if repo != self.config.repo:
                self.log.warn(
                    f"state belongs to repo '{repo}' — re-baselining for "
                    f"'{self.config.repo}'"
                )
                self._backup("repo")
                state = self._new()
                self._write_meta(state)
                return state
            return self.load()
        except sqlite3.DatabaseError as exc:
            self.log.warn(
                f"state database is unreadable or corrupt — re-baselining: "
                f"{self.path} ({exc})"
            )
            self._backup("corrupt")
            state = self._new()
            self._write_meta(state)
            return state

    def write(self, state: dict) -> None:
        """Replace metadata and issue snapshots atomically (tests/tools)."""
        with self.db.transaction() as conn:
            conn.execute("DELETE FROM issues")
            for number, snapshot in (state.get("issues") or {}).items():
                self.db.upsert_issue(int(number), snapshot, conn=conn)
            for key in ("version", "repo", "api", "initialized_at", "last_poll"):
                value = state.get(key)
                self.db.set_meta(
                    key, "" if value is None else str(value), conn=conn
                )

    # --------------------------------------------------------------- commits
    def commit_poll(self, updates, *, last_poll=None, version=None,
                    scanned=0, emitted=0, ok=True, error=None) -> None:
        """Commit one poll's issue snapshots, watermark, and poll row.

        Publishing happens in :class:`~gitea_bots.inbox.Inbox` *before* this
        call, so a poll's events are durable before ``last_poll`` advances.
        """
        with self.db.transaction() as conn:
            for number, snapshot in (updates or {}).items():
                self.db.upsert_issue(int(number), snapshot, conn=conn)
            if version is not None:
                self.db.set_meta("version", str(version), conn=conn)
            if last_poll is not None:
                self.db.set_meta("last_poll", last_poll, conn=conn)
            self.db.record_poll(
                scanned, emitted, ok, error, conn=conn
            )

    def commit_issues(self, updates) -> None:
        """Commit per-issue snapshots without touching the poll watermark."""
        with self.db.transaction() as conn:
            for number, snapshot in (updates or {}).items():
                self.db.upsert_issue(int(number), snapshot, conn=conn)

    def reset(self) -> str | None:
        """Move the DB aside; the next load starts a new baseline.

        Returns the backup path (the base DB file) when something was moved.
        """
        self.db.close()
        existed = self.path.exists()
        if not existed:
            return None
        stamp = compact_utc()
        backup = self.path.with_name(self.path.name + f".bak.{stamp}")
        for path in (
            self.path,
            self.path.with_name(self.path.name + "-wal"),
            self.path.with_name(self.path.name + "-shm"),
        ):
            if not path.exists():
                continue
            try:
                os.replace(path, path.with_name(path.name + f".bak.{stamp}"))
            except OSError:
                pass
        return backup

    # --- small pure accessors ------------------------------------------------
    @staticmethod
    def get_issue(state: dict, number) -> dict:
        return state.get("issues", {}).get(str(number)) or {}

    @staticmethod
    def version(state: dict) -> int:
        try:
            return int(state.get("version", 1))
        except (TypeError, ValueError):
            return 1

    @staticmethod
    def is_baseline(state: dict) -> bool:
        return state.get("last_poll") is None

    # Backwards-compatible read alias (status/tests): no init, no writes.
    def _read(self):
        return self.load()
