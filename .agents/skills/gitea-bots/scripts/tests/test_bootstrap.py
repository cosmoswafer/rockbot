"""Tests that the sibling issue-labels skill is wired in (bootstrap contract)."""

import unittest
from pathlib import Path

import gitea_bots
import issue_labels
import issue_labels.gitea as shared_gitea
import issue_labels.labels as shared_labels
import issue_labels.manager as shared_manager
from gitea_bots import rules
from gitea_bots.client import GiteaClient
from gitea_bots.errors import GiteaError
from gitea_bots.status import StatusManager


class BootstrapTests(unittest.TestCase):
    def test_shared_package_resolves_from_the_sibling_skill(self):
        resolved = Path(issue_labels.__file__).resolve()
        self.assertEqual(resolved.parent.name, "issue_labels")
        self.assertIn("issue-labels", resolved.parts)
        self.assertIn("skills", resolved.parts)

    def test_version_reflects_the_new_capability(self):
        self.assertEqual(gitea_bots.__version__, "2.3.0")

    def test_moved_names_are_reexported_not_duplicated(self):
        self.assertIs(rules.status_label_name, shared_labels.status_label_name)
        self.assertIs(rules.label_status, shared_labels.label_status)
        self.assertIs(rules.normalize_status_set, shared_labels.normalize_status_set)
        self.assertIs(rules.has_tasks, shared_labels.has_tasks)
        self.assertIs(rules.has_summary, shared_labels.has_summary)
        self.assertIs(GiteaClient, shared_gitea.GiteaClient)
        self.assertIs(GiteaError, shared_gitea.GiteaError)
        self.assertTrue(issubclass(StatusManager, shared_manager.StatusManager))

    def test_workflow_derive_still_accepts_comments(self):
        body = "## Summary\n\nHi.\n"
        self.assertEqual(
            rules.derive_status(body, [{"body": rules.STAGE2_MARKER}], []),
            (frozenset({"responded"}), False),
        )
        # No pickup label yet: filing must not invent `seeing`.
        self.assertEqual(
            rules.derive_status(body, [], []),
            (frozenset(), False),
        )
        # A pickup label is preserved while the issue is unplanned.
        self.assertEqual(
            rules.derive_status(body, [], ["seeing"]),
            (frozenset({"seeing"}), False),
        )


if __name__ == "__main__":
    unittest.main()
