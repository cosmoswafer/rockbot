"""Canonical issue status labels and their derivation.

Pure module: no I/O and no side effects. It owns the vocabulary, the Gitea
display/colour metadata, the generic Markdown body helpers, the label-set
projection and the legacy alias resolution. Workflow markers (stage numbers,
hidden comments) are deliberately *not* known here — callers pass marker state
in as booleans, so this package can be used by any issue-tracking flow.
"""

from __future__ import annotations

import re

# --- Canonical status labels ----------------------------------------------
# One word + colour each, with no stage numbering: readable on any repo and
# reusable across agent-driven projects.
STATUS_NAMES = ("seeing", "responded", "working", "question", "committed")
# A leading emoji makes the label list scannable. The bare token stays the
# canonical status name (CLI input, workflow rules); the emoji is only a
# display prefix, stripped again by `label_status` when reading repo labels.
STATUS_EMOJI = {
    "seeing": "👀",
    "responded": "💬",
    "working": "🔨",
    "question": "❓",
    "committed": "✅",
}
STATUS_DESCRIPTIONS = {
    "seeing": "a background agent has picked the issue up and is working on it",
    "responded": "a reply, answer or plan is posted; agent waiting",
    "working": "implementing code/tests",
    "question": "blocked, waiting on human input",
    "committed": "a commit has been pushed",
}
STATUS_COLORS = {
    "seeing": "1d76db",
    "responded": "fbca04",
    "working": "5319e7",
    "question": "d93f0b",
    "committed": "0e8a16",
}
# Old skill-specific names are accepted as input aliases for one release so
# external callers do not break; each resolves to the label set of the workflow
# state it represented (e.g. `implement` -> responded + committed).
RETIRED_NAMES = ("questions", "done")
LEGACY_NAMES = ("inspect", "plan", "implement")
STATUS_ALIASES = {
    "inspect": ("seeing",),
    "plan": ("responded",),
    "implement": ("responded", "committed"),
    "questions": ("question",),
    "done": ("responded", "committed"),
}
# Every label the label set owns (canonical + retired + legacy). Used to decide
# which labels `apply()` strips before merging the derived/wanted set.
STATUS_LABEL_NAMES = (
    frozenset(STATUS_NAMES) | frozenset(RETIRED_NAMES)
    | frozenset(LEGACY_NAMES) | frozenset(STATUS_ALIASES)
)

_LEADING_SYMBOLS = re.compile(r"^[^0-9A-Za-z]+")


def status_label_name(status: str) -> str:
    """Display name for a canonical status label: ``👀 seeing``, ``💬 responded``…"""
    return f"{STATUS_EMOJI[status]} {status}"


def label_status(name) -> str | None:
    """Bare status behind a repo label name, or ``None``.

    Tolerates the emoji display prefix and the bare/legacy spelling, so a repo
    mid-migration still matches: ``👀 seeing``, ``seeing`` and ``inspect`` all
    resolve to a status owned by the label set.
    """
    if not name:
        return None
    text = _LEADING_SYMBOLS.sub("", str(name).strip()).strip()
    return text if text in STATUS_LABEL_NAMES else None


# --- Body sections --------------------------------------------------------
_TASKS_HEADING = re.compile(r"^## Tasks[ \t\r]*$", re.MULTILINE)
_SUMMARY_HEADING = re.compile(r"^## Summary[ \t\r]*$", re.MULTILINE)


def has_tasks(body: str) -> bool:
    return bool(_TASKS_HEADING.search(body or ""))


def has_summary(body: str) -> bool:
    return bool(_SUMMARY_HEADING.search(body or ""))


# --- Status derivation (set-status --reconcile) ---------------------------
def derive_status(body, current_labels, *, stage2_marker=False,
                  stage3_marker=False):
    """Derive the expected status **label set**, returning ``(frozenset, note)``.

    The projection is set-based so ``committed`` can be additive (the issue keeps
    ``responded``) and ``question`` can be transient. Mapping:

    * stage-3 marker -> {responded, committed};
    * stage-2 marker -> {responded};
    * Summary body -> ``{seeing}`` only when the issue already carries it (a
      pickup marker); otherwise none. An unmarked body plan (``## Tasks`` with
      no stage-2 marker) is reconciled to ``{responded}`` and flagged with
      ``note=True``;
    * ``committed`` is additive/sticky once present;
    * ``working`` / ``question`` stay sticky while stage 3 is in flight (no
      stage-3 marker yet) until the agent explicitly replaces them.

    ``seeing`` is never invented from the body shape: it means a background
    agent picked the issue up and is working on it, so only the picking agent
    writes it. The projection merely preserves it while the issue is still
    unplanned, so reconciliation does not erase a worker's label.

    Workflow markers arrive as booleans so this module has no knowledge of any
    particular staged workflow.
    """
    note = False
    current = {
        status for status in (label_status(name) for name in (current_labels or []))
        if status
    }
    if stage3_marker:
        labels = {"responded", "committed"}
    elif stage2_marker:
        labels = {"responded"}
    elif has_summary(body):
        labels = {"seeing"} if "seeing" in current else set()
        if has_tasks(body):            # unmarked body plan
            labels, note = {"responded"}, True
    else:
        labels = set()

    if "committed" in current:            # additive, sticky
        labels.add("committed")
    active = current & {"working", "question"}
    if active and not stage3_marker:
        labels = active | (labels & {"committed"})
    return frozenset(labels), note


_SPLIT_STATUS = re.compile(r"[,\s]+")


def normalize_status_set(want):
    """Resolve status input into a frozenset of canonical labels, or ``None``.

    Accepts a single name, a comma/whitespace-separated string, or any iterable
    of names (including the frozenset from :func:`derive_status`). ``none`` (or
    an empty input) yields the empty set. Legacy aliases resolve to the full
    label set of the workflow state they represented; unknown input -> ``None``.
    """
    if want is None:
        return None
    if isinstance(want, str):
        parts = [p for p in _SPLIT_STATUS.split(want.strip()) if p]
    else:
        try:
            items = list(want)
        except TypeError:
            return None
        parts = []
        for item in items:
            parts.extend(p for p in _SPLIT_STATUS.split(str(item).strip()) if p)
    out = set()
    for part in parts:
        if part == "none":
            continue
        if part in STATUS_NAMES:
            out.add(part)
        elif part in STATUS_ALIASES:
            target = STATUS_ALIASES[part]
            out.update(target if isinstance(target, (tuple, list, set, frozenset))
                       else (target,))
        else:
            return None
    return frozenset(out)


def normalize_status(want):
    """Backward-compatible alias for :func:`normalize_status_set`."""
    return normalize_status_set(want)
