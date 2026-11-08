#!/usr/bin/env python3
"""probe-e2e.py — opt-in end-to-end probe of the real watcher.

Creates controlled dummy issues on the real Gitea server, lets the real watcher
poll and classify them, asserts events/state/labels/idempotence and deletes
every fixture afterwards. Stop the production watcher first. See
gitea_bots/README.md and the skill's SKILL.md.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.cli import main_probe_e2e
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"probe-e2e.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_probe_e2e(script_path=__file__))
