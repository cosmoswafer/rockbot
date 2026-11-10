"""Shared test doubles: an in-memory Gitea client and a wiring harness.

The fake client implements the exact ``GiteaClient`` surface used by the
package, so the watcher/status/tasks logic is exercised end-to-end against
real code paths with zero network access — a direct benefit of the low-coupling
design.
"""

from __future__ import annotations

import io
import re
from pathlib import Path

from gitea_bots.clock import to_epoch
from gitea_bots.config import Config
from gitea_bots.db import Database
from gitea_bots.errors import GiteaError
from gitea_bots.inbox import Inbox
from gitea_bots.log import Logger
from gitea_bots.notify import Notifier
from gitea_bots.state import StateStore
from gitea_bots.status import StatusManager
from gitea_bots.tasks import TaskSyncer
from gitea_bots.watcher import Watcher

FUTURE = "2099-01-01T00:00:00Z"
BASE = "2026-11-08T00:00:00Z"


def issue(num, title="T", body="## Summary\n\nHi.\n", assignees=None,
          state="open", updated_at="2026-11-08T00:00:10Z",
          created_at=BASE, labels=None, author="alice"):
    """Build a Gitea-shaped issue dict."""
    return {
        "number": num,
        "title": title,
        "state": state,
        "updated_at": updated_at,
        "created_at": created_at,
        "body": body,
        "user": {"login": author},
        "labels": labels or [],
        "assignees": [{"login": login} for login in (assignees or [])],
        "milestone": None,
        "html_url": f"https://gitea.test/Atom/example/issues/{num}",
    }


def future_issue(num, **kwargs):
    """An issue whose timestamps always fall inside an incremental window."""
    kwargs.setdefault("updated_at", FUTURE)
    kwargs.setdefault("created_at", FUTURE)
    return issue(num, **kwargs)


def comment(cid, body, author="alice", num=1, created_at=FUTURE):
    return {
        "id": cid,
        "created_at": created_at,
        "body": body,
        "user": {"login": author},
        "html_url": f"https://gitea.test/Atom/example/issues/{num}#issuecomment-{cid}",
    }


class FakeClient:
    """A duck-typed stand-in for :class:`gitea_bots.client.GiteaClient`."""

    def __init__(self, repo="Atom/example", *, raise_missing_issue=False):
        self.repo = repo
        # The e2e probe verifies a delete with a GET, so it needs the real 404
        # behaviour; the watcher/status tests keep the permissive default.
        self.raise_missing_issue = raise_missing_issue
        self.issues = []                 # baseline / open-set issues
        self.incremental_issues = None  # overrides GET issues?...&since=... when set
        self.comments = {}               # num -> [comment]
        self.timeline = {}               # num -> [event]
        self.labels = []                 # repo labels
        self.writes = []                 # (method, path, payload)
        self.next_label_id = 500
        self.next_timeline_id = 1000
        self._prefix = f"/repos/{repo}"

    # -- helpers --------------------------------------------------------------
    @staticmethod
    def _paginate(items, params):
        limit = int(params.get("limit", 50))
        page = int(params.get("page", 1))
        return list(items)[(page - 1) * limit: page * limit]

    # -- interface ------------------------------------------------------------
    def get(self, path, params=None):
        params = params or {}
        if path == "/user":
            return {"login": "testbot"}
        if path == self._prefix + "/labels":
            return list(self.labels)
        if path == self._prefix + "/issues":
            since = params.get("since")
            if since is not None and self.incremental_issues is not None:
                return self._paginate(self.incremental_issues, params)
            items = list(self.issues)
            state = params.get("state", "all")
            if state and state != "all":
                items = [i for i in items if i.get("state") == state]
            if since:
                epoch = to_epoch(since) or 0
                items = [
                    i for i in items
                    if (to_epoch(i.get("updated_at", "")) or 0) >= epoch
                ]
            return self._paginate(items, params)

        prefix = re.escape(self._prefix)
        match = re.match(prefix + r"/issues/(\d+)/comments$", path)
        if match:
            return self._paginate(self.comments.get(int(match.group(1)), []), params)
        match = re.match(prefix + r"/issues/(\d+)/timeline$", path)
        if match:
            return self._paginate(self.timeline.get(int(match.group(1)), []), params)
        match = re.match(prefix + r"/issues/(\d+)$", path)
        if match:
            num = int(match.group(1))
            found = next(
                (i for i in self.issues if i.get("number") == num), None
            )
            if found is None:
                if self.raise_missing_issue:
                    raise GiteaError(f"HTTP 404 from GET {path} — not found")
                return {"number": num, "body": "", "labels": []}
            return dict(found)
        return None

    def get_all(self, path, params=None, limit=50, max_pages=20):
        out = []
        base = dict(params or {})
        for page in range(1, max_pages + 1):
            data = self.get(path, dict(base, limit=limit, page=page))
            if not isinstance(data, list):
                break
            out.extend(data)
            if len(data) < limit:
                break
        return out

    def post(self, path, payload=None, params=None):
        self.writes.append(("POST", path, payload))
        if path == self._prefix + "/issues" and isinstance(payload, dict):
            number = max(
                (current.get("number", 0) for current in self.issues), default=0
            ) + 1
            # Freshly created rows must fall inside an incremental `since`
            # window; the watcher tests set their own fixed timestamps.
            created = future_issue(
                number,
                title=payload.get("title", "T"),
                body=payload.get("body", ""),
                author="testbot",
            )
            self.issues.append(created)
            return {
                "number": number,
                "title": created["title"],
                "body": created["body"],
            }
        match = re.match(re.escape(self._prefix) + r"/issues/(\d+)/comments$", path)
        if match and isinstance(payload, dict):
            num = int(match.group(1))
            bucket = self.comments.setdefault(num, [])
            cid = max((entry.get("id", 0) for entry in bucket), default=0) + 1
            created = comment(cid, payload.get("body", ""), author="testbot", num=num)
            bucket.append(created)
            self._touch(num)
            return {"id": cid}
        if path == self._prefix + "/labels" and isinstance(payload, dict):
            label_id = self.next_label_id
            self.next_label_id += 1
            label = {
                "id": label_id,
                "name": payload.get("name"),
                "color": payload.get("color"),
                "description": payload.get("description"),
            }
            self.labels.append(label)
            return {"id": label_id}
        return {}

    def put(self, path, payload=None):
        self.writes.append(("PUT", path, payload))
        match = re.match(re.escape(self._prefix) + r"/issues/(\d+)/labels$", path)
        if match and isinstance(payload, dict):
            num = int(match.group(1))
            ids = set(payload.get("labels", []))
            chosen = [label for label in self.labels if label.get("id") in ids]
            for current in self.issues:
                if current.get("number") == num:
                    current["labels"] = [
                        {"id": label["id"], "name": label["name"]} for label in chosen
                    ]
        return None

    def patch(self, path, payload=None):
        self.writes.append(("PATCH", path, payload))
        match = re.match(re.escape(self._prefix) + r"/issues/(\d+)$", path)
        if match and isinstance(payload, dict):
            num = int(match.group(1))
            for current in self.issues:
                if current.get("number") != num:
                    continue
                if "body" in payload:
                    current["body"] = payload["body"]
                if "state" in payload:
                    current["state"] = payload["state"]
                if "assignees" in payload:
                    old = {a.get("login") for a in current.get("assignees") or []}
                    new = set(payload["assignees"])
                    self._record_assignee_events(num, old, new)
                    current["assignees"] = [
                        {"login": login} for login in payload["assignees"]
                    ]
                self._touch(num)
        label_match = re.match(re.escape(self._prefix) + r"/labels/(\d+)$", path)
        if label_match and isinstance(payload, dict):
            label_id = int(label_match.group(1))
            for label in self.labels:
                if label.get("id") == label_id:
                    for key in ("name", "color", "description"):
                        if key in payload:
                            label[key] = payload[key]
        return None

    def delete(self, path, payload=None):
        self.writes.append(("DELETE", path, payload))
        match = re.match(re.escape(self._prefix) + r"/issues/(\d+)$", path)
        if match:
            num = int(match.group(1))
            self.issues = [
                current for current in self.issues
                if current.get("number") != num
            ]
            # Issue deletion cascades to comments and timeline events.
            self.comments.pop(num, None)
            self.timeline.pop(num, None)
        label_match = re.match(re.escape(self._prefix) + r"/labels/(\d+)$", path)
        if label_match:
            label_id = int(label_match.group(1))
            self.labels = [
                label for label in self.labels if label.get("id") != label_id
            ]
        return None

    def _touch(self, num):
        """Move an issue's ``updated_at`` forward so a poll picks it up."""
        for current in self.issues:
            if current.get("number") == num:
                current["updated_at"] = FUTURE

    def _record_assignee_events(self, num, old, new):
        """Append Gitea-shaped ``assignees`` timeline events for a PATCH."""
        bucket = self.timeline.setdefault(num, [])
        for login in sorted(old - new):
            bucket.append({
                "id": self.next_timeline_id,
                "type": "assignees",
                "assignee": {"login": login},
                "removed_assignee": True,
            })
            self.next_timeline_id += 1
        for login in sorted(new - old):
            bucket.append({
                "id": self.next_timeline_id,
                "type": "assignees",
                "assignee": {"login": login},
                "removed_assignee": False,
            })
            self.next_timeline_id += 1


def make_config(state_dir, **overrides):
    cfg = Config(
        repo_root=Path(state_dir),
        state_dir=Path(state_dir),
        script_path=Path(__file__),
        repo="Atom/example",
        api_base="https://gitea.test/api/v1",
        token="test-token",
        self_login="testbot",
        auto_plan=True,
        start_reconcile=False,
        notify_enabled=False,
        notify_in_foreground=False,
        all_comments=False,
        verbose=False,
        interval=5,
    )
    for key, value in overrides.items():
        setattr(cfg, key, value)
    return cfg


class Harness:
    """Wires the real components around a :class:`FakeClient`."""

    def __init__(self, tmpdir, **config_overrides):
        self.tmp = Path(tmpdir)
        self.out = io.StringIO()
        self.err = io.StringIO()
        self.logger = Logger(verbose=False, out=self.out, err=self.err)
        self.client = FakeClient()
        self.config = make_config(self.tmp, **config_overrides)
        self.db = Database(self.config.db_file, self.logger)
        self.state = StateStore(self.config, self.logger, self.db)
        self.inbox = Inbox(self.config, self.logger, self.db)
        self.status = StatusManager(self.config, self.client, self.logger)
        self.notifier = Notifier(self.config, self.inbox, self.status, self.logger)
        self.tasks = TaskSyncer(self.config, self.client, self.logger, self.db)
        self.watcher = Watcher(
            self.config, self.client, self.state, self.inbox,
            self.notifier, self.status, self.logger,
        )

    @property
    def stdout(self):
        return self.out.getvalue()

    @property
    def stderr(self):
        return self.err.getvalue()

    def reset_output(self):
        self.out.seek(0)
        self.out.truncate(0)
        self.err.seek(0)
        self.err.truncate(0)
