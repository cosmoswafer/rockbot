"""Tests for ``scripts/prune_tmp.py`` (stdlib unittest only)."""

from __future__ import annotations

import os
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS.parent
sys.path.insert(0, str(SCRIPTS))

import prune_tmp  # noqa: E402

LIVE_NAMES = (
    "state.db-wal",
    "state.db-shm",
    "pending.md",
    "poll.lock",
    "pending.lock",
    "watcher.meta",
    "watcher.log",
    "notify.log",
)


def make_state_db(path: Path, *, imported: str | None) -> None:
    connection = sqlite3.connect(path)
    try:
        connection.execute(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT)"
        )
        connection.execute(
            "DELETE FROM meta WHERE key = 'imported_from_files'"
        )
        if imported is not None:
            connection.execute(
                "INSERT INTO meta (key, value) VALUES ('imported_from_files', ?)",
                (imported,),
            )
        connection.commit()
    finally:
        connection.close()


class PruneTmpTest(unittest.TestCase):
    def setUp(self) -> None:
        (REPO_ROOT / "tmp").mkdir(exist_ok=True)
        self._tmp = tempfile.TemporaryDirectory(dir=REPO_ROOT / "tmp")
        self.root = Path(self._tmp.name)
        self.state = self.root / "gitea-bots"
        self.state.mkdir()
        make_state_db(self.state / "state.db", imported=None)
        for name in LIVE_NAMES:
            (self.state / name).write_text("live\n", encoding="utf-8")
        (self.state / "watcher.pid").write_text("0\n", encoding="utf-8")
        (self.state / "scratch.json").write_text("{}\n", encoding="utf-8")
        (self.state / "pending.ack.20260101T000000Z.md").write_text(
            "acked\n", encoding="utf-8"
        )
        (self.state / "claims").mkdir()
        archive = self.state / "legacy-json-20260101T000000Z"
        archive.mkdir()
        (archive / "state.json").write_text("{}\n", encoding="utf-8")

        for name in ("verify", "site", "shots", "bria"):
            directory = self.root / name
            directory.mkdir()
            (directory / "artifact.bin").write_bytes(b"x" * 16)

        for name in ("chrome-profile", "firefox-profile", "chrome2"):
            directory = self.root / name
            directory.mkdir()
            (directory / "profile.bin").write_bytes(b"p" * 16)
        (self.root / "browsers").mkdir()
        (self.root / "browsers" / "profile.bin").write_bytes(b"p" * 16)

        (self.root / "stray.json").write_text("{}\n", encoding="utf-8")
        (self.root / "issue158").mkdir()
        (self.root / "issue158" / "scratch.md").write_text("x\n", encoding="utf-8")

        self.output: list[str] = []

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def run_plan(self, *, apply: bool, browsers: bool = False, logs: bool = False) -> prune_tmp.PruneResult:
        plan = prune_tmp.build_plan(
            self.root, include_browsers=browsers, truncate_logs=logs
        )
        return prune_tmp.execute(plan, apply=apply, out=self.output.append)

    def test_dry_run_removes_nothing(self) -> None:
        self.run_plan(apply=False)
        self.assertTrue(any("dry run" in line for line in self.output))
        self.assertTrue((self.state / "scratch.json").exists())
        self.assertTrue((self.root / "stray.json").exists())
        self.assertTrue((self.root / "verify" / "artifact.bin").exists())

    def test_apply_keeps_live_state(self) -> None:
        result = self.run_plan(apply=True)
        self.assertFalse(result.failed)
        for name in LIVE_NAMES + ("state.db", "watcher.pid"):
            self.assertTrue((self.state / name).exists(), name)
        self.assertFalse((self.state / "scratch.json").exists())
        self.assertFalse((self.state / "pending.ack.20260101T000000Z.md").exists())
        self.assertFalse((self.state / "claims").exists())
        self.assertFalse((self.root / "verify").exists())
        self.assertFalse((self.root / "site").exists())
        self.assertFalse((self.root / "stray.json").exists())
        self.assertFalse((self.root / "issue158").exists())

    def test_browsers_are_opt_in(self) -> None:
        self.run_plan(apply=True)
        self.assertTrue((self.root / "chrome-profile").exists())
        self.assertTrue((self.root / "firefox-profile").exists())
        self.assertTrue((self.root / "browsers").exists())

        self.run_plan(apply=True, browsers=True)
        self.assertFalse((self.root / "chrome-profile").exists())
        self.assertFalse((self.root / "firefox-profile").exists())
        self.assertFalse((self.root / "chrome2").exists())
        self.assertFalse((self.root / "browsers").exists())

    def test_legacy_archive_kept_without_marker(self) -> None:
        self.run_plan(apply=True)
        self.assertTrue((self.state / "legacy-json-20260101T000000Z").exists())
        self.assertTrue(
            any("legacy archives kept" in line for line in self.output)
        )

    def test_legacy_archive_pruned_when_marker_set(self) -> None:
        make_state_db(self.state / "state.db", imported="1")
        self.run_plan(apply=True)
        self.assertFalse((self.state / "legacy-json-20260101T000000Z").exists())

    def test_legacy_archive_kept_while_flat_state_remains(self) -> None:
        make_state_db(self.state / "state.db", imported="1")
        (self.state / "state.json").write_text("{}\n", encoding="utf-8")
        self.run_plan(apply=True)
        self.assertTrue((self.state / "legacy-json-20260101T000000Z").exists())
        self.assertTrue((self.state / "state.json").exists())

    def test_truncate_logs_refuses_running_watcher(self) -> None:
        (self.state / "watcher.pid").write_text(f"{os.getpid()}\n", encoding="utf-8")
        self.run_plan(apply=True, logs=True)
        self.assertNotEqual(
            (self.state / "watcher.log").read_text(encoding="utf-8"), ""
        )
        self.assertNotEqual(
            (self.state / "notify.log").read_text(encoding="utf-8"), ""
        )

    def test_truncate_logs_when_watcher_stopped(self) -> None:
        (self.state / "watcher.pid").write_text("0\n", encoding="utf-8")
        self.run_plan(apply=True, logs=True)
        self.assertEqual((self.state / "watcher.log").read_text(encoding="utf-8"), "")
        self.assertEqual((self.state / "notify.log").read_text(encoding="utf-8"), "")

    def test_live_state_guard(self) -> None:
        with self.assertRaises(RuntimeError):
            prune_tmp.remove_entry(self.state / "state.db")
        self.assertTrue((self.state / "state.db").exists())


if __name__ == "__main__":
    unittest.main()
