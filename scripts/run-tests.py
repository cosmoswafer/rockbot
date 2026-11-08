#!/usr/bin/env python3
"""Run the repo ``scripts/`` unit tests (stdlib unittest, no extra dependencies)."""

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

suite = unittest.TestLoader().discover(
    str(HERE / "tests"), pattern="test_*.py", top_level_dir=str(HERE)
)
result = unittest.TextTestRunner(verbosity=2).run(suite)
sys.exit(0 if result.wasSuccessful() else 1)
