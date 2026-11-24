#!/usr/bin/env python3
"""gitea.py — command line front end for the gitea-issues skill.

    python3 .agents/skills/gitea-issues/scripts/gitea.py <command> [options]

Standard library only: no pip install, no venv, no ``requests``. Run
``…/gitea.py --help`` for the command list and ``…/gitea.py doctor`` to check
that the remote, ``.env`` and the token are usable before anything is written.
"""

from __future__ import annotations

import sys
from pathlib import Path

if sys.version_info < (3, 8):  # pragma: no cover - interpreter guard
    sys.stderr.write(
        "gitea.py needs Python 3.8 or newer (this is "
        f"{sys.version.split()[0]}).\n"
    )
    sys.exit(1)

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gitea_issues.cli import main  # noqa: E402  - path set up above

if __name__ == "__main__":
    sys.exit(main())
