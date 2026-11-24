"""The standard issue body and the rules that make a body acceptable.

Every new issue is created from :data:`ISSUE_TEMPLATE`. The ``## Tasks``
checklist is mandatory: Gitea renders ``- [ ]`` items as a progress counter in
the issue list, so the body itself is the progress record (there is no checkbox
API). ``## Acceptance criteria`` is rejected — it only copies the tasks and adds
unworkable items to that counter.
"""

from __future__ import annotations

import re

ISSUE_TEMPLATE = """## Summary

<!-- 1-3 sentences: what is wrong / what should change, and why. -->

## Tasks

- [ ] <concrete, verifiable task>
- [ ] <concrete, verifiable task>

## Related

- <!-- related issues / PRs (#123), files, screenshots, references -->
"""

# Headings that count as the task section, so a translated issue still passes.
TASK_HEADINGS = ("tasks", "待辦", "待办", "todo", "checklist")
BANNED_HEADINGS = ("acceptance criteria", "acceptance", "驗收標準", "验收标准")

_HEADING_RE = re.compile(r"^\s{0,3}#{1,6}\s+(.*?)\s*#*\s*$")


def headings(body: str) -> list[str]:
    """Every ATX heading title in the body, in order, lowercased."""
    found = []
    for line in (body or "").splitlines():
        match = _HEADING_RE.match(line)
        if match:
            found.append(match.group(1).strip().casefold())
    return found


def has_task_section(body: str) -> bool:
    return any(
        heading in TASK_HEADINGS
        or any(heading.startswith(prefix) for prefix in TASK_HEADINGS)
        for heading in headings(body)
    )


def has_banned_section(body: str) -> str:
    """Return the offending heading, or ``""`` when the body is clean."""
    for heading in headings(body):
        if heading in BANNED_HEADINGS or any(
            heading.startswith(prefix) for prefix in BANNED_HEADINGS
        ):
            return heading
    return ""


def validate_issue_body(body: str, require_tasks: bool = True) -> list[str]:
    """Problems with a proposed issue body, as plain-language sentences.

    An empty list means the body may be filed. Callers turn each problem into
    an error the agent can act on, or a warning under ``--force``.
    """
    text = body or ""
    problems: list[str] = []
    if not text.strip():
        problems.append("the body is empty — file issues with real content.")
        return problems

    banned = has_banned_section(text)
    if banned:
        problems.append(
            f"the body has a '## {banned}' section — acceptance criteria are "
            "not allowed; express them as verifiable `- [ ]` tasks instead."
        )

    checklist = re.findall(r"^\s*[-*+]\s+\[[ xX]\]\s+\S", text, re.MULTILINE)
    if require_tasks and not checklist:
        problems.append(
            "the body has no `- [ ]` checklist items. Add a `## Tasks` "
            "section with at least one concrete, verifiable task."
        )
    elif require_tasks and not has_task_section(text):
        problems.append(
            "the checklist is not under a `## Tasks` heading — Gitea's "
            "progress counter and the milestone review both look there."
        )
    return problems
