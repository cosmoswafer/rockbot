#!/usr/bin/env python3
"""watch-issues.py — Python entry point for the gitea-bots watcher.

Requires only the standard library plus ``requests``. See
gitea_bots/README.md and the skill's SKILL.md.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.cli import main_watch
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"watch-issues.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_watch(script_path=__file__))
