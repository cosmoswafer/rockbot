"""Tests for the SQLite store and the one-shot legacy-file migration."""

import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from gitea_bots.db import SCHEMA_VERSION, Database
from gitea_bots.errors import BotFatal
from gitea_bots.log import Logger
from tests.fakes import make_config


def _logger():
    return Logger(out=io.StringIO(), err=io.StringIO())


class DatabaseSchemaTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.db = Database(self.dir / "state.db", _logger())

    def tearDown(self):
        self.db.close()
        self.tmp.cleanup()

    def test_schema_and_version(self):
        self.db.ensure_schema()
        self.assertEqual(self.db.user_version(), SCHEMA_VERSION)
        tables = {
            row["name"]
            for row in self.db.connect().execute(
                "SELECT name FROM sqlite_master WHERE type = 'table'"
            )
        }
        self.assertTrue(
            {"meta", "issues", "batches", "events", "tasks", "polls"} <= tables
        )
        mode = self.db.connect().execute("PRAGMA journal_mode").fetchone()[0]
        self.assertEqual(mode.lower(), "wal")

    def test_reopen_is_idempotent(self):
        self.db.ensure_schema()
        with self.db.transaction() as conn:
            self.db.set_meta("repo", "Atom/example", conn=conn)
            self.db.upsert_issue(7, {"title": "X"}, conn=conn)
        self.db.close()
        again = Database(self.dir / "state.db", _logger())
        again.ensure_schema()
        self.assertEqual(again.get_meta("repo"), "Atom/example")
        self.assertEqual(again.get_issue(7)["title"], "X")
        self.assertEqual(again.user_version(), SCHEMA_VERSION)
        again.close()

    def test_tasks_mirror(self):
        self.db.ensure_schema()
        with self.db.transaction() as conn:
            self.db.replace_tasks(
                5, [(0, "one", False), (1, "two", True)], conn=conn
            )
        rows = self.db.tasks(5)
        self.assertEqual([row["text"] for row in rows], ["one", "two"])
        self.assertEqual([row["done"] for row in rows], [0, 1])


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        (self.dir / "pending").mkdir()
        (self.dir / "pending-ack").mkdir()
        (self.dir / "ack").mkdir()
        (self.dir / "state.json").write_text(json.dumps({
            "version": 2,
            "repo": "Atom/example",
            "api": "https://gitea.test/api/v1",
            "initialized_at": "2026-11-01T00:00:00Z",
            "last_poll": "2026-11-08T00:00:00Z",
            "issues": {
                "7": {
                    "updated_at": "2026-11-08T00:00:00Z",
                    "title": "Seven",
                    "state": "open",
                    "labels": "",
                    "assignees": "testbot",
                    "milestone": "",
                    "last_comment_id": 70,
                    "auto_plan_at": "2026-11-07T00:00:00Z",
                    "last_assignee_event_id": 9,
                }
            },
        }), encoding="utf-8")
        (self.dir / "pending" / "batch-pending.batch").write_text(
            "id: batch-pending\nrepo: Atom/example\nevents: 1\n"
            "created_at: 2026-11-08T00:00:01Z\n\n"
            "2026-11-08T00:00:01Z NEW ISSUE #7 \"Seven\" by alice — url\n",
            encoding="utf-8",
        )
        (self.dir / "pending-ack" / "batch-acked.batch").write_text(
            "id: batch-acked\nrepo: Atom/example\nevents: 1\n"
            "created_at: 2026-11-08T00:00:02Z\n\n"
            "2026-11-08T00:00:02Z UPDATED ISSUE #7 — changed: labels — url\n",
            encoding="utf-8",
        )
        (self.dir / "ack" / "batch-stray.batch").write_text(
            "id: batch-stray\nrepo: Atom/example\nevents: 1\n"
            "created_at: 2026-11-08T00:00:03Z\n\n"
            "2026-11-08T00:00:03Z UPDATED ISSUE #7 — changed: title — url\n",
            encoding="utf-8",
        )
        records = [
            {"batch": "batch-acked", "at": "2026-11-08T00:00:02Z",
             "repo": "Atom/example",
             "line": "2026-11-08T00:00:02Z UPDATED ISSUE #7 — changed: labels — url"},
            {"at": "2026-11-07T00:00:00Z", "repo": "Atom/example",
             "line": "2026-11-07T00:00:00Z OLD EVENT"},
            {"batch": "batch-unknown", "at": "2026-11-06T00:00:00Z",
             "repo": "Atom/example",
             "line": "2026-11-06T00:00:00Z OLDER EVENT"},
        ]
        (self.dir / "events.jsonl").write_text(
            "\n".join(json.dumps(record) for record in records) + "\n",
            encoding="utf-8",
        )
        self.db = Database(self.dir / "state.db", _logger())

    def tearDown(self):
        self.db.close()
        self.tmp.cleanup()

    def test_migration_imports_everything_once(self):
        self.assertTrue(self.db.migrate_from_files(self.dir))

        self.assertEqual(self.db.get_meta("repo"), "Atom/example")
        self.assertEqual(self.db.get_meta("last_poll"), "2026-11-08T00:00:00Z")
        self.assertEqual(self.db.get_meta("imported_from_files"), "1")
        self.assertEqual(self.db.get_issue(7)["title"], "Seven")
        self.assertEqual(self.db.get_issue(7)["auto_plan_at"], "2026-11-07T00:00:00Z")

        pending = {batch["id"] for batch in self.db.pending_batches()}
        self.assertEqual(pending, {"batch-pending"})
        self.assertEqual(len(self.db.event_lines("batch-pending")), 1)
        # The duplicate events.jsonl line for the archived batch is not added.
        self.assertEqual(len(self.db.event_lines("batch-acked")), 1)
        self.assertIn(
            "2026-11-07T00:00:00Z OLD EVENT",
            self.db.event_lines("legacy-jsonl"),
        )
        unknown = self.db.connect().execute(
            "SELECT event_count, acked_at FROM batches WHERE id = 'batch-unknown'"
        ).fetchone()
        self.assertEqual(unknown["event_count"], 1)
        self.assertIsNotNone(unknown["acked_at"])

        # Legacy files are archived, and a second run is a no-op.
        self.assertFalse((self.dir / "state.json").exists())
        self.assertFalse((self.dir / "pending").exists())
        self.assertEqual(len(list(self.dir.glob("legacy-json-*"))), 1)
        self.assertFalse(self.db.migrate_from_files(self.dir))

    def test_migration_rolls_back_and_keeps_files_on_error(self):
        with mock.patch.object(
            Database, "_import_events_jsonl", side_effect=sqlite3.Error("boom")
        ):
            with self.assertRaises(BotFatal):
                self.db.migrate_from_files(self.dir)
        self.assertTrue((self.dir / "state.json").exists())
        self.assertFalse(self.db.has_data())

    def test_migration_is_skipped_when_db_already_has_data(self):
        self.db.ensure_schema()
        with self.db.transaction() as conn:
            self.db.upsert_issue(1, {"title": "existing"}, conn=conn)
        self.assertFalse(self.db.migrate_from_files(self.dir))
        self.assertTrue((self.dir / "state.json").exists())
        self.assertEqual(self.db.get_meta("imported_from_files"), "1")

    def test_cli_quarantines_a_corrupt_database(self):
        from gitea_bots.cli import _database
        bad = self.dir / "broken"
        bad.mkdir()
        path = bad / "state.db"
        path.write_text("not a database", encoding="utf-8")
        logger = _logger()
        db = _database(make_config(bad), logger)
        self.assertEqual(db.get_meta("repo"), None)
        self.assertEqual(len(list(bad.glob("state.db.corrupt.*"))), 1)
        self.assertIn("state database is unreadable or corrupt", logger.err.getvalue())
        db.close()


if __name__ == "__main__":
    unittest.main()
