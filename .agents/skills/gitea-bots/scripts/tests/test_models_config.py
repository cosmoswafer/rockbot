"""Tests for API normalization and configuration derivation."""

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from gitea_bots.config import Config, derive_remote, load_dotenv
from gitea_bots.models import Issue


class ModelTests(unittest.TestCase):
    def test_from_api_normalizes_fields(self):
        data = {
            "number": 7,
            "title": "Line1\nLine2",
            "state": "open",
            "body": "## Summary\n\nHi.\n\n## Tasks\n\n- [ ] x\n",
            "user": {"login": "alice"},
            "labels": [{"name": "responded", "id": 5}, {"name": "🤖 agent", "id": 9}],
            "assignees": [{"login": "testbot"}, {"login": "alice"}],
            "milestone": {"title": "M1"},
            "html_url": "https://gitea.test/Atom/example/issues/7",
            "updated_at": "2026-11-08T00:00:00Z",
            "created_at": "2026-11-08T00:00:00Z",
        }
        issue = Issue.from_api(data)
        self.assertEqual(issue.title, "Line1 Line2")
        self.assertEqual(issue.labels_str, "responded,🤖 agent")
        self.assertEqual(issue.assignees_str, "alice,testbot")
        self.assertEqual(issue.milestone, "M1")
        self.assertTrue(issue.has_tasks and issue.has_summary)
        self.assertTrue(issue.assigned_to("testbot"))
        self.assertFalse(issue.assigned_to(""))
        self.assertFalse(issue.is_pull_request())

    def test_pull_request_is_detected(self):
        self.assertTrue(Issue.from_api({"pull_request": {"merged": False}}).is_pull_request())

    def test_missing_fields_are_tolerated(self):
        issue = Issue.from_api({})
        self.assertEqual(issue.author, "unknown")
        self.assertEqual(issue.labels_str, "")
        self.assertFalse(issue.has_tasks)


class RemoteTests(unittest.TestCase):
    def test_ssh_url_with_port(self):
        self.assertEqual(
            derive_remote("ssh://git@git.tokyofy.top:33022/Atom/openrouter-image-runner.git"),
            ("git.tokyofy.top", "Atom/openrouter-image-runner"),
        )

    def test_scp_style(self):
        self.assertEqual(
            derive_remote("git@gitea.test:Atom/example.git"),
            ("gitea.test", "Atom/example"),
        )

    def test_https_url(self):
        self.assertEqual(
            derive_remote("https://gitea.test/Atom/example.git"),
            ("gitea.test", "Atom/example"),
        )


class ConfigLoadTests(unittest.TestCase):
    @staticmethod
    def _load(env):
        with tempfile.TemporaryDirectory() as tmp, \
                patch("gitea_bots.config.load_dotenv", return_value={}), \
                patch("gitea_bots.config.git_output",
                      return_value="git@gitea.test:Atom/example.git"), \
                patch.dict(os.environ, env, clear=True):
            return Config.load(Path(tmp), None, None)

    def test_archive_workers_defaults_to_visible_workers(self):
        # #147: reply/AUTO PLAN workers are TUI-visible unless opted in.
        self.assertFalse(self._load({}).archive_workers)

    def test_archive_workers_opt_in(self):
        self.assertTrue(
            self._load({"GITEA_BOTS_ARCHIVE_WORKERS": "1"}).archive_workers
        )


class DotenvTests(unittest.TestCase):
    def test_quotes_comments_export_and_expansion(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_file = Path(tmp) / ".env"
            env_file.write_text(
                "# a comment\n"
                "GITEA_TOKEN='secret'\n"
                "export GITEA_BOTS_SELF=\"testbot\"\n"
                "FLAG=${GITEA_BOTS_NOTIFY:-0}\n"
                "EMPTY=\n",
                encoding="utf-8",
            )
            with patch.dict(os.environ, {"GITEA_BOTS_NOTIFY": "1"}):
                values = load_dotenv(env_file)
            self.assertEqual(values["GITEA_TOKEN"], "secret")
            self.assertEqual(values["GITEA_BOTS_SELF"], "testbot")
            self.assertEqual(values["FLAG"], "1")  # expanded from process env
            self.assertEqual(values["EMPTY"], "")

    def test_missing_file_returns_empty(self):
        self.assertEqual(load_dotenv(None), {})


if __name__ == "__main__":
    unittest.main()
