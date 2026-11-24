#!/usr/bin/env python3
"""Run the gitea-issues helper tests: stdlib unittest, fully offline.

    python3 .agents/skills/gitea-issues/scripts/run-tests.py
    python3 .agents/skills/gitea-issues/scripts/run-tests.py -v
    python3 .agents/skills/gitea-issues/scripts/run-tests.py test_tasks

No network, no token, no database: the HTTP layer is replaced by a routing fake
(:mod:`tests.support`), so the suite is safe to run on every change.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main(argv: list[str]) -> int:
    sys.path.insert(0, str(HERE))
    loader = unittest.TestLoader()
    if argv:
        names = [name if name.startswith("tests.") else f"tests.{name}"
                 for name in argv]
        suite = loader.loadTestsFromNames(names)
    else:
        suite = loader.discover(str(HERE / "tests"), pattern="test_*.py",
                                top_level_dir=str(HERE))
    verbosity = 2 if "-v" in sys.argv else 1
    result = unittest.TextTestRunner(verbosity=verbosity).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main([arg for arg in sys.argv[1:] if arg != "-v"]))
