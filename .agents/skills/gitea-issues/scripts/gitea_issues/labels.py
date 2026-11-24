"""Repo labels, issue labels, and the shared five-label status vocabulary.

Two API facts drive every function here:

* an issue ``PATCH`` **silently ignores** ``labels`` — labels change only
  through ``PUT /repos/{owner}/{repo}/issues/{n}/labels``;
* that ``PUT`` **replaces the whole set**, and takes **numeric label IDs**
  (names return a JSON unmarshal error). So a safe write is always: read the
  current set, keep what is unrelated, add what is wanted, write it back.

The status names and colours match the ``issue-labels`` skill, so a board
handled by hand, by OpenCode, or by ``gitea-bots`` reads the same everywhere.

The client already knows which repo to talk to (it comes from ``--repo`` or the
remote), so these functions take a client only.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

DEFAULT_COLOR = "ededed"
# Gitea stores colours as 6 hex digits without the leading '#'.
COLOR_RE = re.compile(r"^[0-9a-fA-F]{6}$")
_MAX_LISTED = 20


class LabelError(Exception):
    """A label name, colour or set could not be used as given."""


@dataclass(frozen=True)
class Status:
    bare: str
    display: str
    color: str
    description: str


STATUS_VOCABULARY = (
    Status("seeing", "👀 seeing", "1d76db",
           "picked the issue up; working on it"),
    Status("responded", "💬 responded", "fbca04",
           "a reply, answer or plan is posted; waiting on the human"),
    Status("working", "🔨 working", "5319e7", "implementing code/tests"),
    Status("question", "❓ question", "d93f0b", "blocked, waiting on human input"),
    Status("committed", "✅ committed", "0e8a16", "a commit has been pushed"),
)

# Names older repos still carry. Accepted as input; stripped from an issue when
# a new status set is applied.
LEGACY_ALIASES = {
    "inspect": ("seeing",),
    "plan": ("responded",),
    "implement": ("responded", "committed"),
    "questions": ("question",),
    "done": ("responded", "committed"),
}

_BY_BARE = {status.bare: status for status in STATUS_VOCABULARY}


# ---------------------------------------------------------------------------
# names
# ---------------------------------------------------------------------------
def bare(name: str) -> str:
    """Strip the emoji/display prefix: ``"👀 seeing"`` -> ``"seeing"``."""
    return re.sub(r"^[^0-9A-Za-z\u4e00-\u9fff]+", "", (name or "")).strip()


def is_status_label(name: str) -> bool:
    """True for any status label, current or legacy — the "unrelated" filter."""
    known = {item.casefold() for item in _BY_BARE} | {
        item.casefold() for item in LEGACY_ALIASES
    }
    return bare(name).casefold() in known


def expand_tokens(tokens) -> list[str]:
    """Map status tokens (bare, emoji, or legacy) to canonical display names.

    ``none`` clears the status. Order is preserved, duplicates dropped.
    """
    out: list[str] = []
    for token in tokens or []:
        key = bare((token or "").strip()).casefold()
        if not key or key == "none":
            continue
        if key in _BY_BARE:
            names = [_BY_BARE[key].display]
        elif key in LEGACY_ALIASES:
            names = [
                _BY_BARE[each].display for each in LEGACY_ALIASES[key]
            ]
        else:
            raise LabelError(
                f"unknown status {token!r}. Use one of: "
                + ", ".join(status.bare for status in STATUS_VOCABULARY)
                + " ('none' clears them). Legacy names are also accepted: "
                + ", ".join(sorted(LEGACY_ALIASES))
            )
        for name in names:
            if name not in out:
                out.append(name)
    return out


def normalize_color(value: str, label_name: str) -> str:
    """Gitea wants 6 hex digits with no ``#``."""
    color = (value or DEFAULT_COLOR).strip().lstrip("#")
    if not COLOR_RE.fullmatch(color):
        raise LabelError(
            f"colour {value!r} for label {label_name!r} is not 6 hex digits "
            "(e.g. 0e8a16)."
        )
    return color.lower()


# ---------------------------------------------------------------------------
# repo labels
# ---------------------------------------------------------------------------
def fetch_labels(client) -> list[dict]:
    """Every label defined in the repo (paginated)."""
    return client.get_all(client.repo_path("labels"), limit=100)


def label_index(labels) -> dict:
    """Lookup tables: exact name, folded name, and folded name without emoji."""
    index = {"exact": {}, "folded": {}, "short": {}}
    for label in labels or []:
        name = str(label.get("name") or "")
        if not name or label.get("id") is None:
            continue
        label_id = int(label["id"])
        index["exact"].setdefault(name, label_id)
        index["folded"].setdefault(name.casefold(), label_id)
        index["short"].setdefault(bare(name).casefold(), label_id)
    return index


def find_label_id(index: dict, name: str):
    """Resolve a label name to an ID, tolerating case and the emoji prefix."""
    for table, key in (
        ("exact", name),
        ("folded", name.casefold()),
        ("short", name.casefold()),
        ("short", bare(name).casefold()),
    ):
        found = index[table].get(key)
        if found is not None:
            return found
    return None


def create_label(client, name: str, color: str = DEFAULT_COLOR,
                 description: str = "") -> dict:
    """POST one label."""
    return client.post(
        client.repo_path("labels"),
        {
            "name": name,
            "color": normalize_color(color, name),
            "description": description or "",
        },
    )


def ensure_labels(client, items, dry_run: bool = False) -> dict:
    """Create the missing labels among ``items``.

    ``items`` is a list of names or of ``(name, color, description)`` tuples.
    Idempotent: an existing label is left alone. Returns
    ``{ids, created, existed}``.
    """
    specs = []
    for item in items or []:
        if isinstance(item, (tuple, list)):
            name, color, description = (list(item) + ["", ""])[:3]
            specs.append((name, color or DEFAULT_COLOR, description or ""))
        else:
            specs.append((item, DEFAULT_COLOR, ""))

    index = label_index(fetch_labels(client))
    ids: dict = {}
    created: list[str] = []
    existed: list[str] = []
    for name, color, description in specs:
        found = find_label_id(index, name)
        if found is not None:
            ids[name] = found
            existed.append(name)
            continue
        ids[name] = None
        if dry_run:
            created.append(name)
            continue
        try:
            label = create_label(client, name, color, description)
        except Exception as error:  # noqa: BLE001 - re-raised with context
            raise LabelError(_create_failure(name, error)) from error
        new_id = label.get("id") if isinstance(label, dict) else None
        ids[name] = int(new_id) if new_id is not None else None
        created.append(name)
    return {"ids": ids, "created": created, "existed": existed,
            "dry_run": bool(dry_run)}


def _create_failure(name: str, error) -> str:
    text = str(error)
    if "HTTP 5" in text and any(ord(char) > 0xFFFF for char in name):
        return (
            f"could not create label {name!r}: {text}. This server appears to "
            "reject 4-byte emoji in label names — retry with the bare name "
            f"({bare(name)!r}) and keep the colour and description."
        )
    return f"could not create label {name!r}: {text}"


def resolve_label_ids(client, names, create: bool = False) -> dict:
    """Label names -> numeric IDs, optionally creating what is missing."""
    names = [name for name in (names or []) if str(name).strip()]
    if not names:
        return {"ids": [], "created": []}
    index = label_index(fetch_labels(client))
    ids: list[int] = []
    missing: list[str] = []
    for name in names:
        found = find_label_id(index, name)
        if found is None:
            missing.append(name)
        elif found not in ids:
            ids.append(found)
    if missing and create:
        result = ensure_labels(client, missing)
        ids.extend(i for i in result["ids"].values() if i is not None)
        return {"ids": ids, "created": result["created"]}
    if missing:
        listed = sorted(index["exact"])
        raise LabelError(
            "no such label(s) in this repo: "
            + ", ".join(repr(name) for name in missing)
            + ". Create them with `label create NAME --color HEX`, or pass "
            "`--create`. Existing labels: "
            + ", ".join(listed[:_MAX_LISTED])
            + (f" … (+{len(listed) - _MAX_LISTED} more)"
               if len(listed) > _MAX_LISTED else "")
        )
    return {"ids": ids, "created": []}


# ---------------------------------------------------------------------------
# issue labels
# ---------------------------------------------------------------------------
def issue_labels(client, number) -> list[dict]:
    """The labels currently on an issue (ID + name)."""
    return client.get(client.repo_path(f"issues/{number}/labels")) or []


def apply_issue_labels(client, number, ids) -> list[dict]:
    """Write the full label set for an issue — PUT replaces it."""
    payload = {"labels": [int(label_id) for label_id in ids]}
    return client.put(client.repo_path(f"issues/{number}/labels"), payload) or []


def merge_labels(current, add_ids, remove_ids, drop_status: bool = False) -> list[int]:
    """Compute the full set to write: keep unrelated, drop listed, add wanted.

    ``drop_status`` removes every status label (current or legacy spelling)
    before adding, which keeps the status vocabulary a single set instead of
    accumulating stale boxes.
    """
    remove = {int(label_id) for label_id in (remove_ids or [])}
    keep: list[int] = []
    for label in current or []:
        label_id = label.get("id")
        if label_id is None:
            continue
        label_id = int(label_id)
        if label_id in remove:
            continue
        if drop_status and is_status_label(str(label.get("name") or "")):
            continue
        if label_id not in keep:
            keep.append(label_id)
    for label_id in add_ids or []:
        label_id = int(label_id)
        if label_id not in keep:
            keep.append(label_id)
    return keep


def set_status(client, number, tokens, dry_run: bool = False) -> dict:
    """Apply the canonical status set to an issue, keeping unrelated labels."""
    names = expand_tokens(tokens)
    current = issue_labels(client, number)
    before = [str(label.get("name") or "") for label in current]
    if dry_run:
        return {"wanted": names, "before": before, "dry_run": True}
    if names:
        # Status labels must exist to be applied; report which were added.
        wanted = ensure_labels(client, [
            (name, status.color, status.description)
            for name in names
            for status in STATUS_VOCABULARY
            if status.display == name
        ])
        ids = [i for i in wanted["ids"].values() if i is not None]
        created = wanted["created"]
    else:
        ids, created = [], []
    applied = apply_issue_labels(
        client, number, merge_labels(current, ids, [], drop_status=True)
    )
    return {
        "wanted": names,
        "before": before,
        "created": created,
        "applied": [str(label.get("name") or "") for label in applied],
    }
