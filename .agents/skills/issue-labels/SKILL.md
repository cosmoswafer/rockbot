---
name: issue-labels
description: Canonical issue status label set (👀 seeing, 💬 responded, 🔨 working, ❓ question, ✅ committed), its lifecycle, derivation and migration helpers. Use when labelling Gitea issues, tracking issue progress manually or from a terminal, reconciling drifted labels, or wiring the label set into another agent workflow.
compatibility: Requires Python 3.10+, the `requests` package, and `git` on PATH. Reads the Gitea token from `.env` and the repo from the `origin` remote.
metadata:
  version: "1.0.0"
---

# issue-labels — shared issue status labels

A small, self-contained status label set for Gitea issues. The same vocabulary
is used by the `gitea-bots` staged workflow and by manual issue tracking with
the `gitea-issues` skill, so every issue looks the same whether a bot or a human
moves it.

## The five labels

Each label is one word with a leading emoji in its **display name**. The bare
token is the canonical status name (CLI input, workflow rules); the emoji is
only a display prefix that the helpers strip when reading repo labels.

| Canonical | Display | Colour | Meaning |
| --- | --- | --- | --- |
| `seeing` | `👀 seeing` | `1d76db` blue | a background agent has picked the issue up and is working on it |
| `responded` | `💬 responded` | `fbca04` yellow | a reply, answer or plan is posted; waiting on the human |
| `working` | `🔨 working` | `5319e7` purple | implementing code/tests |
| `question` | `❓ question` | `d93f0b` orange | blocked, waiting on human input |
| `committed` | `✅ committed` | `0e8a16` green | a commit has been pushed |

Gitea stores colours as 6-digit hex **without** the leading `#`.

### Lifecycle

```
seeing → responded → working / question → responded + committed
```

- `committed` is **additive**: an issue keeps `responded` as well.
- `question` is **transient**: it can appear at any point and returns to the
  previous label once answered.
- `working` and `question` are **sticky** while implementation is in flight, so
  a restart never downgrades in-progress work.
- `seeing` is a **pickup marker**, not a filing marker: filing an issue writes
  no label, and only the agent that actually takes the issue writes `seeing`.
  It stays sticky while the issue is unplanned.

## Derivation and reconciliation

The label set is a projection of the issue body (and, in a staged workflow, its
markers). The projection returns a **set** of canonical labels plus an
optional `note` flag:

| Issue state | Derived labels |
| --- | --- |
| a workflow's stage-3 completion marker | `responded` + `committed` |
| a workflow's stage-2 plan marker | `responded` |
| `## Summary` body that already carries `seeing` | `seeing` (preserved, never invented) |
| `## Summary` body with no `seeing` | none |
| unmarked body plan (`## Tasks` with no plan marker) | `responded`, with `note` |
| anything else | none |

Sticky rules: `committed` is preserved once present; `working` / `question`
survive reconciliation while there is no stage-3 marker; `seeing` is preserved
while the issue is unplanned, so a startup reconcile never erases a pickup
label and never invents one.

The Python `derive_status()` is marker-agnostic:

```python
derive_status(body, current_labels, *, stage2_marker=False, stage3_marker=False)
```

A workflow that owns markers (for example `gitea-bots`) injects a derive
callable that computes those booleans from its comments. The shared CLI's
`--reconcile` is therefore body-only; the `gitea-bots` `set-status.py` wrapper
stays marker-aware.

## CLI

```bash
# Apply a set (merges, preserves unrelated labels, idempotent)
python3 .agents/skills/issue-labels/scripts/set-status.py 42 responded
python3 .agents/skills/issue-labels/scripts/set-status.py 42 responded committed
python3 .agents/skills/issue-labels/scripts/set-status.py 42 none   # clear

# Reconcile the label set from the issue body
python3 .agents/skills/issue-labels/scripts/set-status.py --reconcile 42

# Rename legacy/bare repo labels to their canonical emoji names (in place)
python3 .agents/skills/issue-labels/scripts/set-status.py --migrate
```

The CLI derives `GITEA_API`, `owner/repo` and `GITEA_TOKEN` from `origin` and
`.env`. Repository labels are created with the canonical colours and
descriptions on first use, and an existing label whose colour or description
drifted is repaired in place. `--migrate` renames `inspect`, `plan` and
`implement` in place (preserving label IDs and assignments), renames a bare
canonical label to its emoji form, and creates `question`; it is idempotent.
Physical deletion of leftover old labels is a manual follow-up.

### Legacy input aliases

Accepted for one release so external callers do not break:

| Input | Resolves to |
| --- | --- |
| `inspect` | `seeing` |
| `plan` | `responded` |
| `implement` | `responded` + `committed` |
| `questions` | `question` |
| `done` | `responded` + `committed` |

The retired/legacy repo labels are never created, and every `apply` /
`--reconcile` strips them from the issue before merging the new set.

## Manual labelling flow (any Gitea client)

Gitea's issue `PATCH` **silently ignores `labels`**. To change labels, resolve
numeric label IDs and use the dedicated endpoint:

```bash
# 1. List labels and resolve the IDs you want to keep/set
source .env
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
  | jq -r '.[] | [.id, .name] | @tsv'

# 2. PUT the FULL label set (replaces; include unrelated labels you keep)
curl -s -X PUT \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"labels": [24, 31]}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/42/labels" \
  | jq '.[].name'
```

Create a missing canonical label with its exact display name and colour:

```bash
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"🔨 working","color":"5319e7","description":"implementing code/tests"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels" \
  | jq '{id, name, color}'
```

Or let the CLI do all of it: `set-status.py 42 working`.

## Package layout

```
issue-labels/
├── SKILL.md
└── scripts/
    ├── set-status.py             # CLI entry point (sys.path bootstrap)
    ├── run-tests.py
    ├── issue_labels/
    │   ├── __init__.py           # public surface
    │   ├── labels.py             # pure vocabulary, body sections, derive/normalize
    │   ├── manager.py            # StatusManager (apply/reconcile/migrate), LabelError
    │   ├── gitea.py              # GiteaClient, GiteaError, .env / remote helpers
    │   └── cli.py                # argparse entry (apply/--reconcile/--migrate)
    └── tests/
```

`labels.py` has no I/O. `manager.py` writes labels through the injected client
and calls an injected `derive(body, comments, current_labels)`; the default
inspects only the body.

## Consuming from another skill

A sibling skill can add this package to `sys.path` and re-export what it needs:

```python
from pathlib import Path
import sys

SHARED = Path(__file__).resolve().parents[3] / "issue-labels" / "scripts"
if str(SHARED) not in sys.path:
    sys.path.insert(0, str(SHARED))

from issue_labels.labels import derive_status, status_label_name
from issue_labels.manager import StatusManager
```

The `gitea-bots` skill does exactly this in `gitea_bots/__init__.py`; its
`status.py` injects a marker-aware derive and its `rules.py` re-exports the
shared vocabulary so existing call sites keep working.

## Tests

```bash
cd .agents/skills/issue-labels/scripts && python3 run-tests.py
```
