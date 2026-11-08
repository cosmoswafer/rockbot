#!/usr/bin/env python3
"""Prune disposable files under ``./tmp`` without touching live watcher state.

``./tmp`` is gitignored, local-only scratch.  Most of it can be deleted at any
time, but a handful of files under ``tmp/gitea-bots/`` are the live watcher
store and must never be removed while the agent tooling depends on them.

The command is a **dry run by default**.  Pass ``--apply`` to delete.  Browser
profiles are retained unless ``--browsers`` is given, because deleting a
profile also drops its localStorage/cookies (a fresh profile is created on the
next launch).  ``legacy-json-*`` archives are pruned only after the documented
#155 migration condition is satisfied.

Examples::

    python3 scripts/prune_tmp.py                  # preview the safe prune
    python3 scripts/prune_tmp.py --apply          # delete safe scratch
    python3 scripts/prune_tmp.py --browsers       # preview, including profiles
    python3 scripts/prune_tmp.py --apply --truncate-logs
"""

from __future__ import annotations

import argparse
import os
import shutil
import sqlite3
import sys
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TMP = REPO_ROOT / "tmp"

# Files the live watcher reads or writes.  ``state.db-*`` sidecars are matched by
# prefix.  These are never selected by any category.
LIVE_STATE_FILES = frozenset(
    {
        "pending.md",
        "poll.lock",
        "pending.lock",
        "watcher.pid",
        "watcher.meta",
        "watcher.log",
        "notify.log",
    }
)
LIVE_STATE_PREFIXES = ("state.db",)

# Pre-migration flat state: still authoritative until ``migrate_from_files()``
# has archived it, so never delete it as scratch.
LEGACY_STATE_FILES = frozenset({"state.json", "events.jsonl"})
LEGACY_STATE_DIRS = frozenset({"pending", "pending-ack", "ack"})

# Top-level tmp/ entries with a dedicated category.
ARTIFACT_DIRS = ("artifacts", "verify", "site", "shots", "bria")
BROWSER_DIR = "browsers"
GITEA_BOTS_DIR = "gitea-bots"


@dataclass
class PrunePlan:
    """One preview of what a prune would remove."""

    root: Path
    categories: dict[str, list[Path]] = field(default_factory=dict)
    notes: list[str] = field(default_factory=list)
    truncate_logs: list[Path] = field(default_factory=list)

    def entries(self) -> list[Path]:
        seen: dict[Path, None] = {}
        for paths in self.categories.values():
            for path in paths:
                seen.setdefault(path, None)
        return list(seen)


@dataclass
class PruneResult:
    removed: list[Path] = field(default_factory=list)
    failed: list[tuple[Path, str]] = field(default_factory=list)
    truncated: list[Path] = field(default_factory=list)


def human_size(size: int) -> str:
    """Format a byte count with binary units."""

    value = float(size)
    for unit in ("B", "KiB", "MiB", "GiB", "TiB"):
        if value < 1024 or unit == "TiB":
            if unit == "B":
                return f"{int(value)} B"
            return f"{value:.1f} {unit}"
        value /= 1024
    raise AssertionError("unreachable")


def entry_size(path: Path) -> int:
    """Total size of a file, symlink or directory tree (no symlink following)."""

    if path.is_symlink() or not path.is_dir():
        try:
            return path.lstat().st_size
        except OSError:
            return 0
    total = 0
    for base, _dirs, files in os.walk(path):
        for name in files:
            try:
                total += (Path(base) / name).lstat().st_size
            except OSError:
                continue
    return total


def entry_file_count(path: Path) -> int:
    """Number of files a removal would delete (directories count their leaves)."""

    if path.is_symlink() or not path.is_dir():
        return 1
    return sum(len(files) for _base, _dirs, files in os.walk(path))


def tmp_totals(root: Path) -> tuple[int, int]:
    """Return ``(bytes, files)`` for everything currently under *root*."""

    total = 0
    files = 0
    if not root.exists():
        return 0, 0
    for base, _dirs, names in os.walk(root):
        for name in names:
            path = Path(base) / name
            try:
                total += path.lstat().st_size
            except OSError:
                continue
            files += 1
    return total, files


def is_live_state(path: Path) -> bool:
    """True when *path* is live watcher state rather than disposable scratch."""

    if path.parent.name != GITEA_BOTS_DIR:
        return False
    return live_state_name(path.name)


def live_state_name(name: str) -> bool:
    if name in LIVE_STATE_FILES or name in LEGACY_STATE_FILES:
        return True
    if name in LEGACY_STATE_DIRS:
        return True
    return any(name.startswith(prefix) for prefix in LIVE_STATE_PREFIXES)


def _is_browser_entry(path: Path) -> bool:
    if not path.is_dir():
        return False
    return path.name.startswith(("chrome", "firefox"))


def _gitea_scratch(state_dir: Path) -> list[Path]:
    if not state_dir.is_dir():
        return []
    selected: list[Path] = []
    for entry in sorted(state_dir.iterdir()):
        name = entry.name
        if live_state_name(name):
            continue
        # Handled separately so the migration gate cannot be bypassed.
        if name.startswith("legacy-json-"):
            continue
        selected.append(entry)
    return selected


def legacy_archives_eligible(state_dir: Path) -> tuple[bool, str]:
    """Check the #155 migration condition before allowing archive removal.

    The archives are the only copy of the pre-SQLite flat files, so they are
    pruned only when ``state.db`` says the import completed and no flat state
    remains.
    """

    database = state_dir / "state.db"
    if not database.is_file():
        return False, "state.db is missing, so the migration marker cannot be read"
    for name in sorted(LEGACY_STATE_FILES):
        if (state_dir / name).exists():
            return False, f"legacy flat state still present: {name}"
    for name in sorted(LEGACY_STATE_DIRS):
        if (state_dir / name).exists():
            return False, f"legacy flat state still present: {name}/"
    try:
        connection = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
        try:
            row = connection.execute(
                "SELECT value FROM meta WHERE key = 'imported_from_files'"
            ).fetchone()
        finally:
            connection.close()
    except sqlite3.Error as exc:
        return False, f"cannot read state.db: {exc}"
    if not row or str(row[0]) != "1":
        return False, "meta.imported_from_files is not 1"
    return True, "state.db has imported_from_files=1 and no flat state remains"


def _legacy_archives(state_dir: Path) -> list[Path]:
    if not state_dir.is_dir():
        return []
    return sorted(state_dir.glob("legacy-json-*"))


def _root_scratch(root: Path) -> list[Path]:
    selected: list[Path] = []
    for entry in sorted(root.iterdir()):
        name = entry.name
        if name == GITEA_BOTS_DIR or name in ARTIFACT_DIRS or name == BROWSER_DIR:
            continue
        if _is_browser_entry(entry):
            continue
        selected.append(entry)
    return selected


def _artifact_entries(root: Path) -> list[Path]:
    return [root / name for name in ARTIFACT_DIRS if (root / name).exists()]


def _browser_entries(root: Path) -> list[Path]:
    selected: list[Path] = []
    browser_dir = root / BROWSER_DIR
    if browser_dir.is_dir():
        selected.append(browser_dir)
    selected.extend(
        entry for entry in sorted(root.iterdir()) if _is_browser_entry(entry)
    )
    return selected


def watcher_running(state_dir: Path) -> bool:
    """Best-effort PID check using the live ``watcher.pid`` file."""

    try:
        pid = int((state_dir / "watcher.pid").read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return False
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def build_plan(
    root: Path,
    *,
    include_browsers: bool = False,
    truncate_logs: bool = False,
) -> PrunePlan:
    """Build the removal preview for *root* without deleting anything."""

    if not root.exists():
        raise FileNotFoundError(f"tmp directory does not exist: {root}")
    state_dir = root / GITEA_BOTS_DIR
    plan = PrunePlan(root=root)

    plan.categories["gitea-bots scratch"] = _gitea_scratch(state_dir)

    eligible, reason = legacy_archives_eligible(state_dir)
    archives = _legacy_archives(state_dir)
    if archives:
        if eligible:
            plan.categories["legacy-json archives"] = archives
            plan.notes.append(f"legacy archives prunable: {reason}")
        else:
            plan.notes.append(f"legacy archives kept: {reason}")

    plan.categories["artifacts"] = _artifact_entries(root)
    plan.categories["root scratch"] = _root_scratch(root)
    if include_browsers:
        plan.categories["browser profiles"] = _browser_entries(root)
    else:
        plan.notes.append(
            "browser profiles kept (pass --browsers to include them; "
            "deleting one costs its cookies/localStorage)"
        )

    if truncate_logs:
        if watcher_running(state_dir):
            plan.notes.append(
                "watcher.log/notify.log kept: watcher is running (stop it first)"
            )
        else:
            plan.truncate_logs = [
                path
                for path in (state_dir / "watcher.log", state_dir / "notify.log")
                if path.is_file()
            ]
    return plan


def _guard(path: Path) -> None:
    if is_live_state(path):
        raise RuntimeError(f"refusing to remove live state: {path}")


def remove_entry(path: Path) -> None:
    """Remove one file, symlink or directory tree."""

    _guard(path)
    if path.is_symlink() or not path.is_dir():
        path.unlink()
    else:
        shutil.rmtree(path)


def execute(plan: PrunePlan, *, apply: bool, out=print) -> PruneResult:
    """Print the plan and, when *apply*, perform it."""

    result = PruneResult()
    before_bytes, before_files = tmp_totals(plan.root)
    planned = plan.entries()
    planned_bytes = sum(entry_size(path) for path in planned)
    planned_files = sum(entry_file_count(path) for path in planned)

    mode = "apply" if apply else "dry run (pass --apply to delete)"
    out(f"tmp prune — {mode}")
    out(f"root: {plan.root}")
    for name, paths in plan.categories.items():
        if not paths:
            continue
        size = sum(entry_size(path) for path in paths)
        out(f"\n{name}: {len(paths)} entr{'y' if len(paths) == 1 else 'ies'}, {human_size(size)}")
        for path in paths:
            out(f"  {'-' if apply else '?'} {path.relative_to(plan.root)}")
    for note in plan.notes:
        out(f"note: {note}")
    if plan.truncate_logs:
        for path in plan.truncate_logs:
            out(f"truncate: {path.relative_to(plan.root)}")

    if not apply:
        after_bytes = before_bytes - planned_bytes
        after_files = before_files - planned_files
        out(
            f"\nbefore: {human_size(before_bytes)} / {before_files} files; "
            f"would remove {human_size(planned_bytes)} / {planned_files} files; "
            f"after: {human_size(max(after_bytes, 0))} / {max(after_files, 0)} files"
        )
        return result

    for path in planned:
        try:
            remove_entry(path)
        except (OSError, RuntimeError) as exc:
            result.failed.append((path, str(exc)))
            out(f"error: {path}: {exc}")
        else:
            result.removed.append(path)
    for path in plan.truncate_logs:
        try:
            path.write_text("", encoding="utf-8")
        except OSError as exc:
            result.failed.append((path, str(exc)))
            out(f"error: {path}: {exc}")
        else:
            result.truncated.append(path)

    after_bytes, after_files = tmp_totals(plan.root)
    out(
        f"\nbefore: {human_size(before_bytes)} / {before_files} files; "
        f"removed {len(result.removed)} entries; "
        f"after: {human_size(after_bytes)} / {after_files} files"
    )
    if result.truncated:
        out(f"truncated {len(result.truncated)} log file(s)")
    if result.failed:
        out(f"FAILED: {len(result.failed)} entr{'y' if len(result.failed) == 1 else 'ies'} could not be removed")
        return result
    return result


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Preview or apply a safe prune of ./tmp (dry run by default).",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="delete the previewed entries instead of only printing them",
    )
    parser.add_argument(
        "--browsers",
        action="store_true",
        help="include browser profiles (loses their cookies/localStorage)",
    )
    parser.add_argument(
        "--truncate-logs",
        action="store_true",
        help="empty watcher.log/notify.log after confirming the watcher is stopped",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=DEFAULT_TMP,
        help=f"tmp directory to prune (default: {DEFAULT_TMP})",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        plan = build_plan(
            args.root.resolve(),
            include_browsers=args.browsers,
            truncate_logs=args.truncate_logs,
        )
    except FileNotFoundError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    result = execute(plan, apply=args.apply)
    return 1 if result.failed else 0


if __name__ == "__main__":
    sys.exit(main())
