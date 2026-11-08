"""Normalized data containers for Gitea API objects.

One place to turn raw API JSON into clean, comparable values (sorted label /
assignee strings, sanitized single-line fields). Consumers depend on this small
shape rather than on Gitea's response format, which keeps the rest of the
package decoupled from the API.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

from .rules import has_summary, has_tasks

_WHITESPACE = re.compile(r"[\t\r\n]+")


def _clean(value) -> str:
    """Collapse tabs/newlines into spaces so a field is one comparable line."""
    return _WHITESPACE.sub(" ", value or "")


@dataclass
class Issue:
    number: int = 0
    title: str = ""
    state: str = ""
    author: str = ""
    body: str = ""
    labels: list = field(default_factory=list)
    label_ids: list = field(default_factory=list)
    assignees: list = field(default_factory=list)
    milestone: str = ""
    html_url: str = ""
    created_at: str = ""
    updated_at: str = ""
    has_tasks: bool = False
    has_summary: bool = False
    raw: dict = field(default_factory=dict)

    @classmethod
    def from_api(cls, data: dict) -> "Issue":
        user = data.get("user") or {}
        raw_labels = [l for l in (data.get("labels") or []) if isinstance(l, dict)]
        raw_assignees = [a for a in (data.get("assignees") or []) if isinstance(a, dict)]
        milestone = data.get("milestone")
        milestone_title = milestone.get("title", "") if isinstance(milestone, dict) else ""
        body = data.get("body") or ""
        return cls(
            number=data.get("number"),
            title=_clean(data.get("title")),
            state=data.get("state") or "",
            author=_clean(user.get("login") or "unknown"),
            body=body,
            labels=[l.get("name", "") for l in raw_labels],
            label_ids=[l.get("id") for l in raw_labels if l.get("id") is not None],
            assignees=[a.get("login", "") for a in raw_assignees],
            milestone=_clean(milestone_title),
            html_url=data.get("html_url") or "",
            created_at=data.get("created_at") or "",
            updated_at=data.get("updated_at") or "",
            has_tasks=has_tasks(body),
            has_summary=has_summary(body),
            raw=data,
        )

    @property
    def labels_str(self) -> str:
        return ",".join(sorted(self.labels))

    @property
    def assignees_str(self) -> str:
        return ",".join(sorted(self.assignees))

    def is_pull_request(self) -> bool:
        return self.raw.get("pull_request") is not None

    def assigned_to(self, login: str) -> bool:
        return bool(login) and login in self.assignees
