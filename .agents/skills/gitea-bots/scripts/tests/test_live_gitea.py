"""Opt-in unittest adapter for the live Gitea probe (read-only).

The probe itself lives in :mod:`gitea_bots.live_probe` and has a direct entry
point (``scripts/probe-live.py``). This module keeps the historical opt-in gate
so the regular offline suite stays network-free:

    GITEA_BOTS_LIVE=1 python3 scripts/run-tests.py

It only builds the probe and asserts its report — no scenario logic lives here.
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from gitea_bots.config import Config
from gitea_bots.live_probe import build_probe
from gitea_bots.log import Logger

LIVE = os.environ.get("GITEA_BOTS_LIVE") == "1"
SCRIPTS_DIR = Path(__file__).resolve().parents[1]


@unittest.skipUnless(LIVE, "set GITEA_BOTS_LIVE=1 to probe the real Gitea server")
class LiveGiteaProbeTest(unittest.TestCase):
    """Run the shared live probe and surface its rendered report on failure."""

    def test_live_probe_passes(self):
        logger = Logger()
        config = Config.load(
            SCRIPTS_DIR, SCRIPTS_DIR / "watch-issues.py", logger
        )
        config.require_remote(logger)
        config.require_token(logger)
        report = build_probe(config, logger).run()
        print("\n" + report.render())
        self.assertTrue(report.ok, report.render())
