#!/usr/bin/env python3
"""probe-live.py — opt-in, read-only probe of the real Gitea server.

Requires only the standard library plus ``requests``. See
gitea_bots/README.md and the skill's SKILL.md.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.cli import main_probe
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"probe-live.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_probe(script_path=__file__))
