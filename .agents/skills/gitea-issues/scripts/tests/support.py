"""Test helpers: a routing fake for the HTTP layer, plus CLI runners.

Nothing here touches the network: :class:`FakeTransport` answers from a route
table and records every call, so a test can assert on the exact payload sent —
which is how the API gotchas (``milestone`` not ``milestone_id``, numeric label
IDs, ``Authorization: token`` not ``Bearer``) stay fixed.
"""

from __future__ import annotations

import contextlib
import io
import json
import sys
import urllib.parse
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE.parent) not in sys.path:
    sys.path.insert(0, str(HERE.parent))

from gitea_issues import cli  # noqa: E402
from gitea_issues.client import GiteaClient  # noqa: E402
from gitea_issues.env import Settings  # noqa: E402

API_BASE = "https://gitea.test/api/v1"
REPO = "Atom/demo"
TOKEN = "super-secret-token"
REPO_PATH = f"/repos/{REPO}"


@dataclass
class Call:
    method: str
    path: str
    query: dict
    payload: object
    headers: dict

    def suffix(self) -> str:
        return self.path[len(API_BASE):] if self.path.startswith(API_BASE) else self.path


@dataclass
class CliResult:
    code: int
    stdout: str
    stderr: str

    @property
    def json(self):
        return json.loads(self.stdout) if self.stdout.strip() else None

    def __bool__(self):
        return self.code == 0


class FakeTransport:
    """``(method, url, headers, body) -> Response`` backed by a route table.

    Routes are keyed ``"METHOD /path"`` (the path includes ``/api/v1``). A
    responder is a status/obj pair, a bare object (200), or a callable that
    receives the decoded request and returns either of those.
    """

    def __init__(self):
        self.routes: dict = {}
        self.calls: list[Call] = []

    # -- wiring -------------------------------------------------------------
    def route(self, spec: str, responder) -> "FakeTransport":
        method, _, path = spec.partition(" ")
        self.routes[f"{method.upper()} {_absolute(path)}"] = responder
        return self

    def unroute(self, spec: str) -> None:
        method, _, path = spec.partition(" ")
        self.routes.pop(f"{method.upper()} {_absolute(path)}", None)

    def __call__(self, method, url, headers, body):
        split = urllib.parse.urlsplit(url)
        path = f"{split.scheme}://{split.netloc}{split.path}"
        query = {k: v[0] if len(v) == 1 else v
                 for k, v in urllib.parse.parse_qs(split.query).items()}
        payload = json.loads(body.decode("utf-8")) if body else None
        self.calls.append(Call(method.upper(), path, query, payload, dict(headers)))

        responder = self.routes.get(f"{method.upper()} {path}")
        if responder is None:
            return _response(404, {"message": f"no route: {method} {path}"})
        if callable(responder):
            result = responder({"payload": payload, "query": query,
                                "method": method.upper(), "path": path})
        else:
            result = responder
        if isinstance(result, tuple):
            return _response(result[0], result[1])
        return _response(200, result)

    # -- assertions ---------------------------------------------------------
    def find(self, method: str, suffix: str = "") -> list[Call]:
        """Every recorded call of ``method`` whose path ends with ``suffix``."""
        out = []
        for call in self.calls:
            if call.method != method.upper():
                continue
            if suffix and not call.suffix().endswith(suffix):
                continue
            out.append(call)
        return out

    def last(self, method: str, suffix: str = "") -> Call:
        found = self.find(method, suffix)
        if not found:
            raise AssertionError(f"no {method} call to ...{suffix!r} was made")
        return found[-1]

    def writes(self) -> list[Call]:
        return [call for call in self.calls if call.method in
                ("POST", "PUT", "PATCH", "DELETE")]

    def methods(self) -> list[str]:
        return [f"{call.method} {call.suffix()}" for call in self.calls]


def _absolute(path: str) -> str:
    if path.startswith("https://") or path.startswith("http://"):
        split = urllib.parse.urlsplit(path)
        path = split.path
    if path.startswith("/api/v1"):
        return f"{API_BASE}{path[len('/api/v1'):]}"
    if not path.startswith("/"):
        path = "/" + path
    return f"{API_BASE}{path}"


def _response(status: int, obj) -> "object":
    from gitea_issues.client import Response

    body = b"" if obj is None else json.dumps(obj, ensure_ascii=False).encode()
    return Response(status, body)


def make_settings(**overrides) -> Settings:
    base = dict(api_base=API_BASE, repo=REPO, token=TOKEN, timeout=5,
                remote_name="origin", remote_url=f"https://gitea.test/{REPO}.git",
                env_files=["/tmp/.env"])
    base.update(overrides)
    return Settings(**base)


def make_client(transport=None, **overrides) -> GiteaClient:
    return GiteaClient(make_settings(**overrides), transport=transport or
                      FakeTransport())


def run_cli(argv, transport, settings=None) -> CliResult:
    """Run the CLI end to end with an injected fake transport."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = cli.main(list(argv), transport=transport,
                        settings=settings or make_settings())
    return CliResult(code, out.getvalue(), err.getvalue())


# ---------------------------------------------------------------------------
# one small demo repo used by most tests
# ---------------------------------------------------------------------------
LABELS = [
    {"id": 29, "name": "bug", "color": "ee0701"},
    {"id": 56, "name": "db-migration", "color": "5319e7"},
    {"id": 61, "name": "\U0001f440 seeing", "color": "1d76db"},
    {"id": 62, "name": "\U0001f4ac responded", "color": "fbca04"},
    {"id": 63, "name": "\U0001f528 working", "color": "5319e7"},
    {"id": 64, "name": "❓ question", "color": "d93f0b"},
    {"id": 65, "name": "✅ committed", "color": "0e8a16"},
]

MILESTONES = [
    {"id": 87, "title": "v3.7.19", "state": "open", "open_issues": 3,
     "closed_issues": 1, "created_at": "2026-11-01T10:00:00+08:00",
     "due_on": "2026-12-01T00:00:00+08:00"},
    {"id": 86, "title": "v3.7.20", "state": "open", "open_issues": 2,
     "closed_issues": 0, "created_at": "2026-11-15T09:00:00+08:00",
     "due_on": None},
    {"id": 80, "title": "v3.7.18", "state": "closed", "open_issues": 0,
     "closed_issues": 9, "created_at": "2026-09-01T09:00:00+08:00",
     "due_on": None},
]

BODY_687 = """## Summary

Ring shows a stale total after logging a meal.

## Tasks

- [x] Recompute the total when a record is added
- [ ] Recompute the total when a record is deleted
- [ ] Add a regression test

## Related

- #615
"""

BODY_690 = """## Summary

Investigate the EXIF clock problem.

## Tasks

```md
- [ ] this example line is documentation, not a task
```

- [ ] Read the camera pipeline
- [ ] Decide where the timestamp comes from

## Acceptance criteria

- [ ] Records use server time
"""

ISSUES = [
    {"number": 687, "title": "Daily ring: stale total after adding a meal",
     "state": "open", "body": BODY_687, "comments": 2,
     "created_at": "2026-11-10T08:00:00+08:00",
     "updated_at": "2026-11-20T08:00:00+08:00",
     "html_url": f"https://gitea.test/{REPO}/issues/687",
     "labels": [LABELS[0], LABELS[3]],
     "milestone": {"id": 87, "title": "v3.7.19"},
     "assignees": [{"login": "bot"}],
     "user": {"login": "jimmy"}},
    {"number": 690, "title": "Quick camera: EXIF date beats server time",
     "state": "open", "body": BODY_690, "comments": 0,
     "created_at": "2026-11-12T08:00:00+08:00",
     "updated_at": "2026-11-22T08:00:00+08:00",
     "html_url": f"https://gitea.test/{REPO}/issues/690",
     "labels": [{"id": 63, "name": "\U0001f528 working"}],
     "milestone": {"id": 86, "title": "v3.7.20"},
     "assignees": [],
     "user": {"login": "bot"}},
    {"number": 700, "title": "Old closed issue", "state": "closed",
     "body": "## Tasks\n\n- [x] Done\n", "comments": 1,
     "created_at": "2026-10-01T08:00:00+08:00",
     "updated_at": "2026-10-09T08:00:00+08:00",
     "html_url": f"https://gitea.test/{REPO}/issues/700",
     "labels": [{"id": 65, "name": "✅ committed"}],
     "milestone": None, "assignees": [], "user": {"login": "jimmy"}},
    {"number": 701, "title": "A pull request", "state": "open", "body": "",
     "comments": 0, "labels": [], "milestone": None, "assignees": [],
     "user": {"login": "bot"},
     "pull_request": {"url": f"https://gitea.test/{REPO}/pulls/701"}},
]

COMMENTS = [
    {"id": 500, "body": "Plan: recompute on every write.",
     "created_at": "2026-11-11T08:00:00+08:00",
     "user": {"login": "bot"}},
    {"id": 501, "body": "Committed in 1a2b3c4.",
     "created_at": "2026-11-19T08:00:00+08:00",
     "user": {"login": "jimmy"}},
]


def demo_transport(login="bot", user_scope=True) -> FakeTransport:
    """A transport that answers like a small Gitea repo with the data above."""
    transport = FakeTransport()
    transport.route(f"GET {REPO_PATH}", {"full_name": REPO, "name": "demo"})
    transport.route(f"GET {REPO_PATH}/labels",
                    lambda ctx: _page(LABELS, ctx["query"]))
    transport.route(f"GET {REPO_PATH}/milestones",
                    lambda ctx: _page(
                        [m for m in MILESTONES
                         if ctx["query"].get("state") in (None, "all")
                         or m["state"] == ctx["query"]["state"]], ctx["query"]))
    transport.route(f"GET {REPO_PATH}/issues",
                    lambda ctx: _page([i for i in ISSUES if _matches_state(
                        i["state"], ctx["query"].get("state"))], ctx["query"]))
    for issue in ISSUES:
        transport.route(f"GET {REPO_PATH}/issues/{issue['number']}", issue)
        transport.route(f"GET {REPO_PATH}/issues/{issue['number']}/labels",
                        issue["labels"])
        transport.route(f"GET {REPO_PATH}/issues/{issue['number']}/comments",
                        COMMENTS if issue["number"] == 687 else [])
        transport.route(f"PATCH {REPO_PATH}/issues/{issue['number']}",
                        lambda ctx, issue=issue: _merge(issue, ctx["payload"]))
        transport.route(f"PUT {REPO_PATH}/issues/{issue['number']}/labels",
                        lambda ctx: labels_for_ids(ctx["payload"]))
        transport.route(f"POST {REPO_PATH}/issues/{issue['number']}/comments",
                        {"id": 502, "html_url": "https://gitea.test/x",
                         "body": "posted"})
    transport.route(f"POST {REPO_PATH}/issues", lambda ctx: _merge(
        {"number": 900, "state": "open", "html_url":
         f"https://gitea.test/{REPO}/issues/900", "labels": [],
         "milestone": None, "assignees": [], "user": {"login": "bot"},
         "comments": 0, "body": "", "title": "", "created_at": "",
         "updated_at": ""}, ctx["payload"]))
    transport.route(f"POST {REPO_PATH}/labels", lambda ctx: _merge(
        {"id": 99, "color": "ededed"}, ctx["payload"]))
    transport.route(f"POST {REPO_PATH}/milestones", lambda ctx: _merge(
        {"id": 88, "state": "open", "open_issues": 0, "closed_issues": 0,
         "created_at": "2026-11-24T00:00:00+08:00", "due_on": None},
        ctx["payload"]))
    transport.route(f"PATCH {REPO_PATH}/milestones/87",
                    lambda ctx: _merge(MILESTONES[0], ctx["payload"]))
    if user_scope:
        transport.route("GET /user", {"login": login})
    else:
        transport.route("GET /user", (403, {"message": "token does not have at "
                         "least one of required scope(s): [read:user]"}))
    return transport


def _page(items, query):
    limit = int(query.get("limit") or len(items) or 1)
    page = int(query.get("page") or 1)
    return items[(page - 1) * limit:page * limit]


def _matches_state(state, wanted):
    """Gitea returns everything when ``state`` is omitted or ``all``."""
    return wanted in (None, "", "all") or state == wanted


def labels_for_ids(payload):
    """Map a ``{"labels": [ids]}`` payload (or a label list) to label objects."""
    wanted = set(ids_of(payload))
    return [label for label in LABELS if label["id"] in wanted]


def ids_of(payload):
    """Label IDs from a request payload, a label object list, or a mixed list."""
    if isinstance(payload, dict):
        items = payload.get("labels") or []
    elif isinstance(payload, list):
        items = payload
    else:
        return []
    out = []
    for item in items:
        value = item.get("id") if isinstance(item, dict) else item
        if value is not None:
            out.append(int(value))
    return out


def _merge(base, patch):
    out = dict(base)
    out.update(patch or {})
    if isinstance(out.get("milestone"), int):
        out["milestone"] = next(
            ({"id": m["id"], "title": m["title"]} for m in MILESTONES
             if m["id"] == out["milestone"]), None)
    if isinstance(out.get("labels"), list):
        out["labels"] = labels_for_ids({"labels": out["labels"]})
    if isinstance(out.get("assignees"), list):
        out["assignees"] = [{"login": login} for login in out["assignees"]]
    return out
