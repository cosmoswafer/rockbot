#!/usr/bin/env python3
"""sync-tasks.py — mirror a plan's ``## Tasks`` checklist into an issue body.

Requires only the standard library plus ``requests``.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.cli import main_tasks
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"sync-tasks.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_tasks(script_path=__file__))
