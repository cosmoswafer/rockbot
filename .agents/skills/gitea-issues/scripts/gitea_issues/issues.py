"""Reading and writing issues, and the views the CLI prints.

Everything the issue endpoints do — list with filters, fetch one (plus its
comments), create from the standard template, edit fields, comment, change
state — lives here, so :mod:`cli` only parses arguments and prints results.

Known endpoint behaviour handled here:

* the issues list returns pull requests too — they are dropped unless asked
  for, because ``type=issues`` is missing on older Gitea servers;
* server-side filters (``labels=``, ``milestones=``) take numeric IDs and are
  not equally supported everywhere, so results are filtered locally as well;
* an issue ``PATCH`` silently ignores ``labels`` — see :mod:`labels`.
"""

from __future__ import annotations

from . import labels as label_ops
from . import tasks as task_ops
from .client import is_pull_request

STATES = ("open", "closed", "all")
DEFAULT_COLUMNS = ("number", "state", "tasks", "labels", "milestone",
                   "comments", "title")


def _text(value) -> str:
    return "" if value is None else str(value)


def _names(items) -> list[str]:
    out = []
    for item in items or []:
        name = _text(item.get("name") or item.get("title") or item.get("login"))
        if name:
            out.append(name)
    return out


def issue_row(issue, base_url: str = "") -> dict:
    """One issue as plain, printable fields (body reduced to its checklist)."""
    number = issue.get("number")
    milestone = issue.get("milestone") or {}
    checklist = task_ops.parse_tasks(_text(issue.get("body")))
    return {
        "number": number,
        "state": _text(issue.get("state")),
        "title": _text(issue.get("title")),
        "tasks": task_ops.progress(checklist) if checklist else "",
        "labels": _names(issue.get("labels")),
        "milestone": _text(milestone.get("title")),
        "milestone_id": milestone.get("id"),
        "assignees": _names(issue.get("assignees")),
        "author": _text((issue.get("user") or {}).get("login")),
        "comments": issue.get("comments"),
        "updated": _text(issue.get("updated_at"))[:10],
        "created": _text(issue.get("created_at"))[:10],
        "url": _text(issue.get("html_url"))
        or (base_url and f"{base_url}/issues/{number}")
        or "",
        "is_pull": is_pull_request(issue),
    }


def list_issues(client, *, state: str = "open", labels=None, milestone=None,
                assignee: str = "", author: str = "", search: str = "",
                includes_pulls: bool = False, limit: int = 30,
                all_pages: bool = False) -> list[dict]:
    """Fetch issues as ``issue_row`` dicts, then filter what the server left in.

    ``limit`` caps the items wanted; ``all_pages`` walks every page instead.
    Label and milestone filters are applied locally as well as server-side,
    because older Gitea servers ignore those query parameters.
    """
    params = {"state": state} if state in STATES else {}
    if assignee:
        params["assignee"] = assignee
    if author:
        params["author"] = author
    if search:
        params["q"] = search
    if not includes_pulls:
        params["type"] = "issues"

    per_page = 100 if all_pages else min(max(limit, 1), 100)
    items = client.get_all(client.repo_path("issues"), params, limit=per_page,
                          max_pages=100 if all_pages else 1)
    if not all_pages:
        items = items[:limit]

    wanted_labels = {str(name).casefold() for name in (labels or [])}
    wanted_milestone = _text(milestone).casefold()
    rows = []
    for item in items:
        if not isinstance(item, dict):
            continue
        if not includes_pulls and is_pull_request(item):
            continue
        row = issue_row(item, client.settings.repo_url)
        if wanted_labels:
            on_issue = set()
            for name in row["labels"]:
                on_issue.add(name.casefold())
                on_issue.add(label_ops.bare(name).casefold())
            if not wanted_labels & on_issue:
                continue
        if wanted_milestone and wanted_milestone not in {
            row["milestone"].casefold(), _text(row["milestone_id"])
        }:
            continue
        rows.append(row)
    return rows


def fetch_issue(client, number) -> dict:
    issue = client.get(client.repo_path(f"issues/{number}"))
    if not isinstance(issue, dict) or issue.get("number") is None:
        raise LookupError(f"issue {number} was not returned by the server.")
    return issue


def fetch_comments(client, number, limit: int = 50) -> list[dict]:
    items = client.get_all(
        client.repo_path(f"issues/{number}/comments"), limit=limit, max_pages=3
    )
    return [item for item in items if isinstance(item, dict)]


def create_payload(title: str, body: str, assignees=None, milestone_id=None,
                   label_ids=None) -> dict:
    """Build a create-issue payload with Gitea's exact key names."""
    payload = {"title": _text(title), "body": body or ""}
    if assignees:
        payload["assignees"] = list(assignees)
    if milestone_id is not None:
        payload["milestone"] = int(milestone_id)  # not "milestone_id"!
    if label_ids:
        payload["labels"] = [int(label_id) for label_id in label_ids]
    return payload


def create_issue(client, payload, expected_label_ids=None) -> dict:
    """POST an issue, then make sure the requested labels really landed.

    ``labels`` on ``POST`` works on recent Gitea servers and is ignored by
    older ones; re-asserting it through the labels endpoint (only when the
    response is missing something) keeps the behaviour identical everywhere.
    """
    created = client.post(client.repo_path("issues"), payload)
    if not isinstance(created, dict):
        raise RuntimeError("the server did not return the new issue.")
    wanted = {int(label_id) for label_id in (expected_label_ids or [])}
    if wanted:
        got = {label_id for label_id in (_label_id(item)
                                        for item in created.get("labels") or [])
               if label_id is not None}
        if not wanted <= got:
            created["labels"] = label_ops.apply_issue_labels(
                client, created["number"], sorted(got | wanted)
            )
    return created


def _label_id(item):
    """Label references arrive as objects or as bare IDs depending on the server."""
    if isinstance(item, dict):
        value = item.get("id")
        return int(value) if value is not None else None
    if isinstance(item, int):
        return item
    return None


def patch_issue(client, number, **fields):
    """PATCH only the fields that were actually given."""
    payload = {key: value for key, value in fields.items() if value is not None}
    if not payload:
        return fetch_issue(client, number)
    return client.patch(client.repo_path(f"issues/{number}"), payload)


def set_body(client, number, body: str):
    return patch_issue(client, number, body=body)


def set_state(client, number, state: str):
    if state not in ("open", "closed"):
        raise ValueError(f"state must be open or closed, got {state!r}.")
    return patch_issue(client, number, state=state)


def post_comment(client, number, body: str) -> dict:
    if not (body or "").strip():
        raise ValueError("a comment cannot be empty.")
    return client.post(client.repo_path(f"issues/{number}/comments"),
                       {"body": body})


def merge_assignees(current, add=None, remove=None) -> list[str]:
    """Read-modify-write the assignee list (PATCH replaces it wholesale)."""
    out = [_text(login) for login in (current or []) if _text(login)]
    for login in add or []:
        if login not in out:
            out.append(login)
    for login in remove or []:
        out = [item for item in out if item != login]
    return out


def append_related(body: str, references) -> str:
    """Add ``#123`` references under ``## Related``, creating the section once.

    Used when a review finds a problem and a follow-up issue must point back at
    the original. Existing content is never reordered.
    """
    refs = [ref for ref in (references or []) if str(ref).strip()]
    if not refs:
        return body or ""
    lines = [f"- #{str(ref).lstrip('#')}" for ref in refs]
    text = (body or "").rstrip("\n")
    if task_ops.find_section(text, "Related") >= 0:
        return f"{text}\n" + "\n".join(lines) + "\n"
    separator = "\n\n" if text.strip() else ""
    return f"{text}{separator}## Related\n\n" + "\n".join(lines) + "\n"


def render_issue(row: dict, body: str = "", comments=None) -> str:
    """A readable single-issue view."""
    out = [
        f"#{row.get('number')}  {row.get('title')}",
        f"state     {row.get('state')}"
        + (f"  tasks {row.get('tasks')}" if row.get("tasks") else ""),
    ]
    if row.get("labels"):
        out.append(f"labels    {', '.join(row['labels'])}")
    if row.get("milestone"):
        out.append(f"milestone {row['milestone']} (id {row.get('milestone_id')})")
    if row.get("assignees"):
        out.append(f"assignees {', '.join(row['assignees'])}")
    out.append(f"author    {row.get('author')}  comments {row.get('comments')}")
    out.append(f"url       {row.get('url')}")
    if body:
        out += ["", "--- body " + "-" * 48, body.rstrip("\n")]
    for comment in comments or []:
        author = _text((comment.get("user") or {}).get("login"))
        out += [
            "",
            f"--- comment {comment.get('id')} by {author} "
            f"({_text(comment.get('created_at'))[:16]})",
            _text(comment.get("body")).rstrip("\n"),
        ]
    return "\n".join(out)
