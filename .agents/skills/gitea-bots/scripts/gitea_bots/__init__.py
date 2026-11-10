"""gitea_bots — Python implementation of the gitea-bots issue watcher tooling.

Design goals
------------
* **High cohesion** — every module owns exactly one concern (HTTP, state,
  inbox, notification, rules, CLI, ...).
* **Low coupling** — modules talk through small, explicit interfaces and are
  wired together in :mod:`gitea_bots.cli`. Pure decision logic lives in
  :mod:`gitea_bots.rules` and has no I/O, so it is trivially unit-testable.
* **Shared label plumbing** — the canonical status vocabulary, the label
  manager and the generic Gitea client are vendored in ``scripts/issue_labels``
  so the watcher works without the separate ``issue-labels`` documentation
  skill (which is curl-only and needs no Python).

The read-only watcher (:mod:`gitea_bots.watcher`) surfaces events; it never
creates, edits, assigns, comments on, or closes issues. Its only writes are
status labels — the startup reconcile and the pickup ``seeing`` label applied
before a dispatched worker starts (#144) — performed in-process by
:mod:`gitea_bots.status`.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

# The status vocabulary/manager/client are vendored at
# ``scripts/issue_labels`` next to this package, so the tooling has no
# cross-skill dependency and installs with the gitea-bots directory alone.
# ISSUE_LABELS_SCRIPTS remains as an escape hatch for an out-of-tree package.
_SCRIPTS_DIR = Path(__file__).resolve().parents[1]
_SHARED_SCRIPTS = Path(
    os.environ.get("ISSUE_LABELS_SCRIPTS") or _SCRIPTS_DIR
)
if not (_SHARED_SCRIPTS / "issue_labels").is_dir():
    raise ImportError(
        "the vendored issue_labels package was not found at "
        f"'{_SHARED_SCRIPTS / 'issue_labels'}' (expected next to "
        f"'{Path(__file__).resolve().parents[2]}'; set ISSUE_LABELS_SCRIPTS "
        "to override)"
    )
if str(_SHARED_SCRIPTS) not in sys.path:
    sys.path.insert(0, str(_SHARED_SCRIPTS))

# Bump when the state schema / event semantics change. A state written by an
# older version triggers one catch-up scan so previously-consumed events can be
# re-surfaced instead of lost. The value is stored in `meta.version`; the
# legacy `state.json` import preserves it.
# 3: issue-body at-mentions surface as `DIRECTED COMMENT on #N (issue body)`
#    and are deduped by a `body_mention_sha` cursor (#185).
EVENTS_VERSION = 3

__version__ = "2.3.0"
