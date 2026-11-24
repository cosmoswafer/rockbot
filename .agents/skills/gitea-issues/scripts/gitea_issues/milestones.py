"""Milestones: lookup by title, newest-open resolution, creation, assignment.

The one rule that silently breaks work: assignment uses the JSON key
``milestone`` with a **numeric ID**. ``milestone_id`` and milestone *names* are
ignored without an error by both ``POST`` and ``PATCH`` — so every path here
goes through :func:`resolve`, which turns a reference into a validated ID or a
message that lists the real choices.
"""

from __future__ import annotations

import re

DUE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


class MilestoneError(Exception):
    """A milestone reference could not be resolved, or a write was rejected."""


def list_milestones(client, state: str = "open"):
    params = {} if state in ("", "all") else {"state": state}
    return client.get_all(client.repo_path("milestones"), params, limit=50)


def _title(milestone) -> str:
    return str(milestone.get("title") or "")


def summary_rows(milestones) -> list[dict]:
    return [
        {
            "id": item.get("id"),
            "title": _title(item),
            "state": item.get("state"),
            "open": item.get("open_issues"),
            "closed": item.get("closed_issues"),
            "due": item.get("due_on") or "",
        }
        for item in milestones or []
    ]


def latest(client, state: str = "open"):
    """The most recently created milestone — the current development target."""
    milestones = [
        item for item in list_milestones(client, state)
        if item.get("id") is not None
    ]
    if not milestones:
        raise MilestoneError(
            f"this repo has no {state} milestones. Create one with "
            "`milestone create TITLE [--due YYYY-MM-DD]`."
        )
    milestones.sort(key=lambda item: (str(item.get("created_at") or ""),
                                      int(item["id"])))
    return milestones[-1]


def resolve(client, ref, create: bool = False, due: str = "",
            description: str = ""):
    """Turn ``ref`` (numeric ID, exact title, or unique title fragment) into a milestone.

    ``--create-milestone`` creates it when the title is missing, which is how a
    new version milestone is opened without a second command.
    """
    wanted = str(ref or "").strip()
    if not wanted:
        raise MilestoneError("an empty milestone reference is not a milestone.")

    milestones = list_milestones(client, "all")
    if wanted.lstrip("+").isdigit():
        number = int(wanted)
        for item in milestones:
            if item.get("id") == number:
                return item
        if not create:
            raise MilestoneError(
                f"no milestone with id {number}. Open milestones: "
                + _choices(milestones)
            )

    folded = {item.get("id"): item for item in milestones
              if _title(item).casefold() == wanted.casefold()}
    if len(folded) == 1:
        return next(iter(folded.values()))
    if folded:
        raise MilestoneError(
            f"{wanted!r} matches several milestones: "
            + ", ".join(f"#{i} {_title(m)} ({m.get('state')})"
                        for i, m in sorted(folded.items()))
        )

    fragments = {
        item.get("id"): item for item in milestones
        if wanted.casefold() in _title(item).casefold()
    }
    if len(fragments) == 1:
        return next(iter(fragments.values()))
    if len(fragments) > 1:
        raise MilestoneError(
            f"{wanted!r} is ambiguous — it matches "
            + ", ".join(_title(m) for m in sorted(fragments.values(),
                                                  key=_title))
        )
    if not create:
        raise MilestoneError(
            f"no milestone matching {wanted!r}. Open milestones: "
            + _choices(milestones)
            + " — create it with `milestone create` or pass "
            "`--create-milestone`."
        )
    return create_milestone(client, wanted, due=due, description=description)[0]


def _choices(milestones) -> str:
    open_items = [item for item in milestones or [] if item.get("state") == "open"]
    if not open_items:
        return "(none open)"
    return ", ".join(
        f"{_title(item)} (id {item.get('id')})" for item in open_items[:10]
    )


def create_milestone(client, title: str, due: str = "", description: str = ""):
    """POST a milestone. ``--due`` must be a plain ``YYYY-MM-DD`` date."""
    title = (title or "").strip()
    if not title:
        raise MilestoneError("a milestone needs a title.")
    payload = {"title": title, "description": description or ""}
    if due:
        if not DUE_RE.fullmatch(due):
            raise MilestoneError(
                f"--due {due!r} is not YYYY-MM-DD (e.g. 2026-12-31)."
            )
        payload["due_date"] = due
    existing = {
        _title(item).casefold(): item for item in list_milestones(client, "all")
    }
    if title.casefold() in existing:
        item = existing[title.casefold()]
        return item, False
    return client.post(client.repo_path("milestones"), payload), True


def assign(client, number, milestone_id):
    """PATCH the issue with ``{"milestone": <numeric id>}`` (0 clears it)."""
    return client.patch(
        client.repo_path(f"issues/{number}"), {"milestone": int(milestone_id)}
    )


def set_state(client, milestone_id, state: str):
    if state not in ("open", "closed"):
        raise MilestoneError(f"milestone state must be open or closed, got {state!r}.")
    return client.patch(
        client.repo_path(f"milestones/{milestone_id}"), {"state": state}
    )
