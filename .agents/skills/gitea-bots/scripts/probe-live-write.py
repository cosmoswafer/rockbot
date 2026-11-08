#!/usr/bin/env python3
"""probe-live-write.py — opt-in write-data probe of the real Gitea server.

Creates one dummy issue, exercises the create/comment/label/assign/close/delete
lifecycle and cleans up after itself. Requires only the standard library plus
``requests``. See gitea_bots/README.md and the skill's SKILL.md.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.cli import main_probe_write
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"probe-live-write.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_probe_write(script_path=__file__))
