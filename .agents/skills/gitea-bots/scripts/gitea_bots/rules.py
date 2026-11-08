"""Pure workflow rules — the decision core of the tool.

No I/O and no side effects: directed @-mentions, stage markers, body sections,
status derivation and checklist handling are all plain functions. Keeping this
module pure is what makes the interesting behaviour unit-testable with zero
mocking of HTTP/state.

There is deliberately no command grammar here: a human reply that at-mentions
the bot login is the only request signal, and what it asks for is worked out
from the issue and the repo's `AGENTS.md`, not from a skill-defined vocabulary.
"""

from __future__ import annotations

import hashlib
import re

from issue_labels.labels import (
    LEGACY_NAMES,
    RETIRED_NAMES,
    STATUS_ALIASES,
    STATUS_COLORS,
    STATUS_DESCRIPTIONS,
    STATUS_EMOJI,
    STATUS_LABEL_NAMES,
    STATUS_NAMES,
    has_summary,
    has_tasks,
    label_status,
    normalize_status,
    normalize_status_set,
    status_label_name,
)
from issue_labels.labels import derive_status as _derive_status

# --- Workflow markers (authored by the agent in plan / completion comments) ---
STAGE2_MARKER = "<!-- gitea-bots: stage=2 -->"
STAGE3_MARKER = "<!-- gitea-bots: stage=3 -->"

# The canonical status vocabulary, body helpers and generic projection are
# re-exported above from the shared issue-labels skill so existing call sites
# (watcher, models, tests) keep importing them from `gitea_bots.rules`.

# --- Event tags -----------------------------------------------------------
TAG_NEW_COMMENT = "NEW COMMENT"
TAG_DIRECTED = "DIRECTED COMMENT"


def compile_directed_mention(login: str) -> re.Pattern:
    """Word-bounded ``@login``; a trailing ``-`` is excluded so @login-2 is not
    a false positive. Case-insensitive, matching the raw comment body."""
    return re.compile(
        r"(^|[^A-Za-z0-9_])@" + re.escape(login) + r"([^A-Za-z0-9_-]|$)",
        re.IGNORECASE | re.MULTILINE,
    )


# --- Shared-login helpers (#136) -------------------------------------------
# The bot is designed to run on the same Gitea account as the human, so the
# author login cannot distinguish human from bot. The invariant is instead:
# *the bot never writes an at-mention* — any at-mention of the bot login
# outside code spans is a human reply. Bot output carries stage markers.
_FENCED_CODE = re.compile(r"(?:`{3,}|~{3,}).*?(?:`{3,}|~{3,})", re.DOTALL)
_INLINE_CODE = re.compile(r"`[^`]*`")
_INDENTED_CODE = re.compile(r"^(?: {4}|\t).*$", re.MULTILINE)


def strip_code_spans(body: str) -> str:
    """Drop fenced / inline / indented code so quoted mentions are examples."""
    text = _FENCED_CODE.sub(" ", body or "")
    text = _INLINE_CODE.sub(" ", text)
    return _INDENTED_CODE.sub(" ", text)


def has_bot_mention(body: str, self_login: str) -> bool:
    """A word-bounded at-mention of the bot login outside code spans."""
    if not self_login:
        return False
    return bool(compile_directed_mention(self_login).search(strip_code_spans(body)))


def is_bot_output(body: str) -> bool:
    """Only the bot writes stage markers; a reliable output discriminator."""
    return (comments_have_marker([{"body": body}], STAGE2_MARKER)
            or comments_have_marker([{"body": body}], STAGE3_MARKER))


def is_directed_mention(author, body, self_login, *, shared_login=True):
    """An at-mention of the bot login is a human reply (#136 shared-login mode).

    The bot never writes at-mentions, so any at-mention outside code spans is
    treated as human regardless of the author account. Bot output (stage
    markers) and quoted/fenced examples are exempt. With ``shared_login=False``
    (dedicated-bot deployments) the old login-equality guard is restored.
    Fails closed when the bot identity is unknown.
    """
    if not self_login or is_bot_output(body):
        return False
    if not shared_login and author == self_login:
        return False
    return has_bot_mention(body, self_login)


# --- Issue-body mentions (#185) -------------------------------------------
# An at-mention in the issue body is the same request signal as one in a
# comment (the bot never writes at-mentions, so it is always human). Bodies
# have no monotonic ID, so the cursor is a digest of the mention paragraphs
# only: a `sync-tasks.py` rewrite or any unrelated body edit does not re-emit.
def body_mention_paragraphs(author, body, self_login, *, shared_login=True):
    """Blank-line-separated paragraphs of ``body`` that at-mention the bot.

    Reuses the same code-stripping, bot-output exemption and dedicated-bot
    author guard as :func:`is_directed_mention`, so a mention quoted in a
    fenced/inline/indented code block stays an example, not a request.
    """
    if not self_login or is_bot_output(body):
        return []
    if not shared_login and author == self_login:
        return []
    stripped = strip_code_spans(body)
    paragraphs = (p.strip() for p in re.split(r"\n\s*\n", stripped))
    return [p for p in paragraphs if p and has_bot_mention(p, self_login)]


def body_mention_digest(paragraphs) -> str:
    """Stable content digest of the mention paragraphs (``""`` when none)."""
    paragraphs = list(paragraphs or [])
    if not paragraphs:
        return ""
    return hashlib.sha256("\n\n".join(paragraphs).encode("utf-8")).hexdigest()


def classify_comment(author, body, *, self_login, all_comments,
                     shared_login=True):
    """Return the event tag for a new comment, or ``None`` to ignore it.

    All-comments mode surfaces everything as ``NEW COMMENT``; otherwise only a
    directed at-mention of the bot login is surfaced, as ``DIRECTED COMMENT``.
    A human reply is the request — the skill does not parse a command grammar.
    Plain self-authored comments with no at-mention stay dropped (surface them
    with ``GITEA_BOTS_ALL_COMMENTS=1``).
    """
    if all_comments:
        return TAG_NEW_COMMENT
    if is_directed_mention(author, body, self_login, shared_login=shared_login):
        return TAG_DIRECTED
    return None


# --- Markers --------------------------------------------------------------
def comments_have_marker(comments, marker: str) -> bool:
    return any(marker in (c.get("body") or "") for c in comments)


def comments_have_stage2_marker(comments) -> bool:
    return comments_have_marker(comments, STAGE2_MARKER)


def comments_have_stage3_marker(comments) -> bool:
    return comments_have_marker(comments, STAGE3_MARKER)


# --- Body / task-list helpers --------------------------------------------
# `has_tasks` / `has_summary` are re-exported above from the shared skill.
# Line-level variants used by the task-list helpers (CRLF is stripped first).
_TASKS_LINE = re.compile(r"^## Tasks[ \t]*$")
_TASK_LINE = re.compile(r"^- \[[ xX]\]")


# --- Status derivation (set-status --reconcile) ---------------------------
def derive_status(body, comments, current_labels):
    """Derive the expected status **label set**, returning ``(frozenset, note)``.

    Thin workflow adapter over the shared marker-agnostic projection: the
    stage-2 / stage-3 markers are computed from the comment list and passed as
    booleans. Mapping (see the shared ``issue_labels.labels.derive_status``):

    * stage-3 marker -> {responded, committed};
    * stage-2 marker -> {responded};
    * Summary body -> {seeing} only when the issue already carries it;
      otherwise none. An unmarked body plan (``## Tasks`` with no stage-2
      marker) is reconciled to ``{responded}`` and flagged with ``note=True``;
    * ``committed`` is additive/sticky once present;
    * ``working`` / ``question`` stay sticky while stage 3 is in flight (no
      stage-3 marker yet) until the agent explicitly replaces them.

    ``seeing`` means a background agent picked the issue up, so the projection
    never invents it; it only preserves an existing pickup label.
    """
    return _derive_status(
        body,
        current_labels,
        stage2_marker=comments_have_stage2_marker(comments),
        stage3_marker=comments_have_stage3_marker(comments),
    )


# --- Task-list handling (sync-tasks) --------------------------------------
def extract_tasks(text: str):
    """Checklist lines from the plan's ``## Tasks`` section (stops at next ``##``)."""
    lines = (text or "").replace("\r", "").split("\n")
    in_tasks = False
    out = []
    for line in lines:
        if _TASKS_LINE.match(line):
            in_tasks = True
            continue
        if in_tasks and line.startswith("## "):
            in_tasks = False
        if in_tasks and _TASK_LINE.match(line):
            out.append(line)
    return out


def strip_tasks_section(body: str):
    """Return the body's lines with any ``## Tasks`` section removed."""
    lines = (body or "").replace("\r", "").split("\n")
    skip = False
    out = []
    for line in lines:
        if _TASKS_LINE.match(line):
            skip = True
            continue
        if skip and line.startswith("## "):
            skip = False
        if not skip:
            out.append(line)
    return out


def trim_trailing_blank_lines(lines):
    """Drop trailing blank/whitespace-only lines for byte-stable bodies."""
    last = 0
    for i, line in enumerate(lines, 1):
        if line.strip():
            last = i
    return lines[:last]


def strip_one_trailing_newline(text: str) -> str:
    return text[:-1] if text.endswith("\n") else text


# --- Batch-level human-reply detection (#131/#136/#134) -------------------
# An emitted event line looks like:
#   2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (comment 2716) by saru — https://...
#   2026-11-08T05:07:56Z DIRECTED COMMENT on #130 (issue body) by saru — https://...
# (optionally with " (catch-up)" after the tag). Indented bodies ("    | ...")
# are not matched.
_DIRECTED_EVENT_LINE = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\S+\s+"
    r"DIRECTED COMMENT(?:\s+\(catch-up\))?\s+"
    r"on\s+#(\d+)\s+\((?:comment\s+\d+|issue body)\)\s+by\s+(.+?)\s+—\s+"
)


# Read the comment body already emitted under a DIRECTED COMMENT as indented
# ``    | `` lines — no change to the durable event format.
_BODY_LINE = re.compile(r"^\s*\| ?(.*)$")


def human_reply_blocks(events_text, self_login, *, shared_login=True):
    """Return ``{issue_number: block_text}`` for human replies in a batch.

    A human reply is a ``DIRECTED COMMENT`` whose body at-mentions the bot
    login outside code spans (or, in dedicated-bot mode, whose author differs
    from the token user). The reply may come from a comment or from the issue
    body itself (``(issue body)``). The block is the emitted event line plus its
    indented ``    | `` body lines, so a per-issue worker receives exactly its
    own event and no other issue's payload. Multiple replies for one issue in
    the same batch are concatenated in emission order. An unknown bot identity
    returns ``{}`` (fail closed) — better to leave events in ``pending.md``
    than spawn a worker on an unclassifiable reply.
    """
    if not self_login:
        return {}
    lines = (events_text or "").splitlines()
    blocks: dict[int, str] = {}
    for index, line in enumerate(lines):
        match = _DIRECTED_EVENT_LINE.match(line)
        if not match:
            continue
        author = match.group(2).strip()
        block = [line]
        body_parts = []
        for following in lines[index + 1:]:
            found = _BODY_LINE.match(following)
            if not found:
                break
            body_parts.append(found.group(1))
            block.append(following)
        body = "\n".join(body_parts)
        if not is_directed_mention(author, body, self_login,
                                   shared_login=shared_login):
            continue
        number = int(match.group(1))
        text = "\n".join(block) + "\n"
        blocks[number] = blocks[number] + text if number in blocks else text
    return blocks


# --- AUTO PLAN batch detection (#140) -------------------------------------
# The second stage-2 authorization shape is a single tagged event line:
#   2026-11-08T07:31:23Z AUTO PLAN #138 "title" by saru — https://...
# Anchoring on the timestamped header (and not the indented ``    | `` comment
# bodies) keeps titles, quoted text and other event tags from authorizing a
# stage.
_AUTO_PLAN_EVENT_LINE = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\S+\s+AUTO PLAN\s+#(\d+)(?:\s|\"|$)"
)


def auto_plan_issues_in_batch(events_text):
    """Sorted, deduped issue numbers from ``AUTO PLAN`` event lines.

    Pure and side-effect free. Only real event headers match, so an issue title
    that merely mentions ``AUTO PLAN``, a comment body indented under another
    event, or a non-timestamped hand-written line never authorizes stage 2.
    """
    numbers = set()
    for line in (events_text or "").splitlines():
        match = _AUTO_PLAN_EVENT_LINE.match(line)
        if match:
            numbers.add(int(match.group(1)))
    return sorted(numbers)
