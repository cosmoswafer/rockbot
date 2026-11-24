"""Editing the Markdown checklist inside an issue body.

Gitea has no checkbox API: the body *is* the progress record. This module
parses a body, selects tasks, and rewrites only the checkbox lines, so nothing
else in the body is touched by accident.

Fenced code blocks are skipped everywhere — an example ``- [ ]`` inside a
```md block is documentation, not a task to tick.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

TASK_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?P<marker>[-*+])[ \t]+\[(?P<state>[ xX])\][ \t]*"
    r"(?P<text>.*?)\s*$"
)
HEADING_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?P<hashes>#{1,6})[ \t]+(?P<title>.*?)\s*#*\s*$"
)
FENCE_RE = re.compile(r"^[ \t]{0,3}(?P<fence>`{3,}|~{3,})")
INDEX_RE = re.compile(r"\d+([,-]\s*\d+)*")


class TaskSelection(Exception):
    """A target matched nothing, or matched more than one task."""


@dataclass
class Task:
    index: int  # 1-based, document order
    line: int  # 0-based line number in the body
    indent: str
    marker: str
    done: bool
    text: str

    @property
    def box(self) -> str:
        return "[x]" if self.done else "[ ]"

    def render(self) -> str:
        return f"{self.indent}{self.marker} {self.box} {self.text}"

    def shown(self) -> str:
        return f"{self.index:>3}  {self.box} {self.text}"


def join_body(lines: list[str], original: str) -> str:
    """Rejoin lines, preserving whether the body ended with a newline."""
    text = "\n".join(lines)
    return f"{text}\n" if (original or "").endswith("\n") else text


def _scan(body: str):
    """Yield ``(line_number, line, inside_code_fence)`` for every line."""
    fence = None
    for number, line in enumerate((body or "").splitlines()):
        match = FENCE_RE.match(line)
        if match:
            marker = match.group("fence")[0] * 3
            if fence is None:
                fence = marker
                yield number, line, True
                continue
            if line.strip().startswith(fence):
                fence = None
        yield number, line, fence is not None


def parse_tasks(body: str) -> list[Task]:
    """Every checklist item outside code fences, in document order."""
    tasks: list[Task] = []
    for number, line, in_fence in _scan(body):
        if in_fence:
            continue
        match = TASK_RE.match(line)
        if not match:
            continue
        tasks.append(
            Task(
                index=len(tasks) + 1,
                line=number,
                indent=match.group("indent"),
                marker=match.group("marker"),
                done=match.group("state").lower() == "x",
                text=match.group("text"),
            )
        )
    return tasks


def progress(tasks: list[Task]) -> str:
    """``"2/5"`` — the counter Gitea shows next to the issue title."""
    return f"{sum(1 for task in tasks if task.done)}/{len(tasks)}"


def resolve(tasks: list[Task], target: str) -> list[Task]:
    """Select tasks by index (``3``, ``1,4``, ``2-5``), by ``all``, or by text.

    Text selection must be unique: two matches raise rather than guessing,
    because ticking the wrong task writes a false progress record.
    """
    if not tasks:
        raise TaskSelection(
            "this issue body has no `- [ ]` checklist items to select."
        )
    wanted = (target or "").strip()
    if wanted.lower() in ("all", "*"):
        return list(tasks)

    if INDEX_RE.fullmatch(wanted):
        picked: list[Task] = []
        for token in wanted.split(","):
            token = token.strip()
            if "-" in token:
                low, _, high = token.partition("-")
                low, high = sorted((int(low), int(high)))
                if high > len(tasks):
                    raise TaskSelection(
                        f"range {token} goes past the last task — this body "
                        f"has {len(tasks)} tasks."
                    )
                picked.extend(tasks[low - 1:high])
            else:
                if not 1 <= int(token) <= len(tasks):
                    raise TaskSelection(
                        f"task {token} does not exist — this body has "
                        f"{len(tasks)} tasks."
                    )
                picked.append(tasks[int(token) - 1])
        return picked

    matches = [task for task in tasks if wanted.casefold() in task.text.casefold()]
    if not matches:
        raise TaskSelection(
            f"no task matches {wanted!r}. Tasks are:\n"
            + "\n".join(task.shown() for task in tasks)
        )
    if len(matches) > 1:
        raise TaskSelection(
            f"{wanted!r} matches {len(matches)} tasks — select one by its "
            "index:\n" + "\n".join(task.shown() for task in matches)
        )
    return matches


def edit_checkboxes(body: str, targets: list[str], done: bool) -> tuple[str, list[Task]]:
    """Tick (or untick) every task the targets select.

    Returns ``(new_body, touched)``. Unticking tasks that are already unticked
    still counts as a selection, so the caller can report what happened.
    """
    tasks = parse_tasks(body)
    selected: list[Task] = []
    for target in targets:
        selected.extend(resolve(tasks, target))
    if not selected:
        return body, []

    lines = (body or "").splitlines()
    for task in selected:
        lines[task.line] = Task(
            index=task.index,
            line=task.line,
            indent=task.indent,
            marker=task.marker,
            done=done,
            text=task.text,
        ).render()
    return join_body(lines, body), selected


def _normalize(heading: str) -> str:
    return re.sub(r"[*_`]", "", (heading or "")).strip().casefold()


def find_section(body: str, heading: str) -> int:
    """Line number of ``heading`` (case-insensitive, ``#`` optional), else -1."""
    target = _normalize(heading)
    for number, line, in_fence in _scan(body):
        if in_fence:
            continue
        match = HEADING_RE.match(line)
        if match and _normalize(match.group("title")) == target:
            return number
    return -1


def drop_section(body: str, heading: str) -> tuple[str, bool]:
    """Delete a Markdown section: its heading, its text, up to the next heading.

    Sub-sections (deeper heading levels) go with it. The body is left with
    exactly one blank line where the section was.
    """
    target = _normalize(heading)
    out: list[str] = []
    skip_level: int | None = None
    removed = False

    for _, line, in_fence in _scan(body):
        found = None if in_fence else HEADING_RE.match(line)
        level = len(found.group("hashes")) if found else None

        if skip_level is not None:
            if level is not None and level <= skip_level:
                skip_level = None  # this heading starts the next section
            else:
                removed = True
                continue

        if level is not None and _normalize(found.group("title")) == target:
            skip_level = level
            removed = True
            while out and not out[-1].strip():
                out.pop()
            out.append("")  # keep one blank line before whatever follows
            continue
        out.append(line)

    if not removed:
        return body, False
    while out and not out[-1].strip():
        out.pop()
    return join_body(out, body), True


def render_checklist(tasks: list[Task]) -> str:
    """A numbered view of the checklist — what ``tasks <n>`` prints."""
    if not tasks:
        return "no checklist items in this body"
    return "\n".join(
        [f"tasks {progress(tasks)} done"] + [task.shown() for task in tasks]
    )
