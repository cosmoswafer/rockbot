"""Opt-in unittest adapter for the live end-to-end watcher probe.

The probe itself lives in :mod:`gitea_bots.e2e_probe` and has a direct entry
point (``scripts/probe-e2e.py``). This module keeps the opt-in gate so the
regular offline suite stays network-free (and never creates issues):

    GITEA_BOTS_LIVE_E2E=1 python3 scripts/run-tests.py

Stop the production watcher first; the probe refuses to run while its
``watcher.pid`` points at a live process.
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from gitea_bots.config import Config
from gitea_bots.e2e_probe import build_e2e_probe, write_report_artifact
from gitea_bots.log import Logger

LIVE_E2E = os.environ.get("GITEA_BOTS_LIVE_E2E") == "1"
SCRIPTS_DIR = Path(__file__).resolve().parents[1]


@unittest.skipUnless(
    LIVE_E2E,
    "set GITEA_BOTS_LIVE_E2E=1 to drive the real watcher against real dummy "
    "issues (stop the production watcher first)",
)
class LiveE2EProbeTest(unittest.TestCase):
    """Run the end-to-end probe once and surface its rendered report."""

    def test_live_e2e_creates_events_labels_and_cleans_up(self):
        logger = Logger()
        config = Config.load(
            SCRIPTS_DIR, SCRIPTS_DIR / "watch-issues.py", logger
        )
        config.require_remote(logger)
        config.require_token(logger)
        probe = build_e2e_probe(config, logger)
        report = probe.run()
        artifacts = write_report_artifact(report, config)
        print("\n" + report.render())
        print(f"artifact: {artifacts['log']} (json: {artifacts['json']})")
        self.assertTrue(report.ok, report.render())
        self.assertTrue(probe.cleaned_up)
        self.assertEqual(probe.leftover_urls, [])


if __name__ == "__main__":
    unittest.main()
