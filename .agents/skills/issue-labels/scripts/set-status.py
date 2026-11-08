#!/usr/bin/env python3
"""set-status.py — apply, reconcile or migrate the shared issue status labels.

Resolves the Gitea repo/API/token from ``origin`` + ``.env``. Requires only the
standard library plus ``requests``.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    from issue_labels.cli import main_status
except ImportError as exc:  # pragma: no cover - dependency guard
    sys.stderr.write(
        f"set-status.py needs Python 3.10+ and 'requests' "
        f"(only non-stdlib dependency): {exc}\n"
    )
    sys.exit(1)

if __name__ == "__main__":
    sys.exit(main_status(script_path=__file__))
