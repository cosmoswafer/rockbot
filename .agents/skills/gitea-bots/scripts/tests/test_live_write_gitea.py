"""Opt-in unittest adapter for the live Gitea write-data probe.

The probe itself lives in :mod:`gitea_bots.write_probe` and has a direct entry
point (``scripts/probe-live-write.py``). This module keeps the opt-in gate so
the regular offline suite stays network-free (and never creates issues):

    GITEA_BOTS_LIVE_WRITE=1 python3 scripts/run-tests.py

It only builds the probe and asserts its report — no lifecycle logic lives here.
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from gitea_bots.config import Config
from gitea_bots.log import Logger
from gitea_bots.write_probe import build_write_probe

LIVE_WRITE = os.environ.get("GITEA_BOTS_LIVE_WRITE") == "1"
SCRIPTS_DIR = Path(__file__).resolve().parents[1]


@unittest.skipUnless(
    LIVE_WRITE,
    "set GITEA_BOTS_LIVE_WRITE=1 to exercise the real Gitea write endpoints",
)
class LiveWriteProbeTest(unittest.TestCase):
    """Run the write probe once and surface its rendered report on failure."""

    def test_live_write_lifecycle_creates_and_cleans_up_one_dummy_issue(self):
        logger = Logger()
        config = Config.load(
            SCRIPTS_DIR, SCRIPTS_DIR / "watch-issues.py", logger
        )
        config.require_remote(logger)
        config.require_token(logger)
        probe = build_write_probe(config, logger)
        report = probe.run()
        print("\n" + report.render())
        self.assertTrue(report.ok, report.render())
        self.assertTrue(probe.cleaned_up)
        self.assertEqual(probe.leftover_url, "")


if __name__ == "__main__":
    unittest.main()
