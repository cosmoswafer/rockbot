#!/usr/bin/env python3
"""catch-up.py — explicit backlog catch-up, separate from the watcher loop.

The background watcher never replays downtime. This one-shot command opts in:
``scan`` runs the full open-set AUTO PLAN scan; ``replay`` re-publishes the
events accumulated since ``last_poll``. Reuses the ``gitea_bots`` package.
Requires only the standard library plus ``requests``.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from gitea_bots.catchup import main_catchup
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"catch-up.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_catchup(script_path=__file__))