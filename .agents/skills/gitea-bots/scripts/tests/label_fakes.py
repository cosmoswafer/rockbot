"""Test support for the shared issue_labels package."""

import re
from types import SimpleNamespace

from issue_labels.gitea import GiteaError
from issue_labels.manager import StatusManager


def _strip_emoji(text):
    return re.sub(r"[^\x00-\uFFFF]+", "", str(text or "")).strip()


class FakeClient:
    """In-memory stand-in for :class:`issue_labels.gitea.GiteaClient`.

    ``reject_emoji`` emulates a Gitea whose label store cannot hold 4-byte
    characters: any label write carrying an emoji name fails with HTTP 500 and
    changes nothing. ``strip_on_reject`` goes one step further and mirrors the
    observed half-succeeded variant — the label is stored/renamed with the
    emoji stripped *and* the request still fails with 500. ``reject_status``
    picks the status code the hostile server answers with.
    """

    def __init__(self, repo="Atom/example", reject_emoji=False,
                 strip_on_reject=False, reject_status=500):
        self.repo = repo
        self.issues = []
        self.comments = {}
        self.labels = []
        self.writes = []
        self.next_label_id = 500
        self.reject_emoji = reject_emoji
        self.strip_on_reject = strip_on_reject
        self.reject_status = reject_status
        self._prefix = f"/repos/{repo}"

    def _emoji_rejection(self, method, path, payload):
        """GiteaError for a label write whose name carries emoji, else None."""
        if not (self.reject_emoji or self.strip_on_reject):
            return None
        name = payload.get("name") if isinstance(payload, dict) else None
        if not name or _strip_emoji(name) == name:
            return None
        return GiteaError(f"HTTP {self.reject_status} from {method} {path}",
                          status=self.reject_status)

    @staticmethod
    def _paginate(items, params):
        limit = int(params.get("limit", 50))
        page = int(params.get("page", 1))
        return list(items)[(page - 1) * limit: page * limit]

    def get(self, path, params=None):
        params = params or {}
        if path == "/user":
            return {"login": "testbot"}
        if path == self._prefix + "/labels":
            return list(self.labels)
        prefix = re.escape(self._prefix)
        match = re.match(prefix + r"/issues/(\d+)/comments$", path)
        if match:
            return self._paginate(self.comments.get(int(match.group(1)), []), params)
        match = re.match(prefix + r"/issues/(\d+)$", path)
        if match:
            num = int(match.group(1))
            found = next(
                (i for i in self.issues if i.get("number") == num),
                {"number": num, "body": "", "labels": []},
            )
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
        if path == self._prefix + "/labels" and isinstance(payload, dict):
            rejection = self._emoji_rejection("POST", path, payload)
            if rejection is not None and not self.strip_on_reject:
                raise rejection
            name = payload.get("name")
            if rejection is not None:  # strip_on_reject: store stripped, then fail
                name = _strip_emoji(name)
            label = {
                "id": self.next_label_id,
                "name": name,
                "color": payload.get("color"),
                "description": payload.get("description"),
            }
            self.next_label_id += 1
            self.labels.append(label)
            if rejection is not None:
                raise rejection
            return {"id": label["id"]}
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
        label_match = re.match(re.escape(self._prefix) + r"/labels/(\d+)$", path)
        if label_match and isinstance(payload, dict):
            rejection = self._emoji_rejection("PATCH", path, payload)
            if rejection is not None:
                if not self.strip_on_reject:
                    raise rejection
                payload = dict(payload, name=_strip_emoji(payload.get("name")))
            label_id = int(label_match.group(1))
            for label in self.labels:
                if label.get("id") == label_id:
                    for key in ("name", "color", "description"):
                        if key in payload:
                            label[key] = payload[key]
        return None


class RecordingLogger:
    def __init__(self):
        self.warnings = []
        self.events = []

    def warn(self, message):
        self.warnings.append(message)

    def event(self, message):
        self.events.append(message)


def issue(num, body="## Summary\n\nHi.\n", labels=None):
    return {"number": num, "body": body, "labels": labels or []}


def comment(cid, body, author="alice", num=1):
    return {
        "id": cid,
        "created_at": "2026-11-08T00:00:00Z",
        "body": body,
        "user": {"login": author},
        "html_url": f"https://gitea.test/Atom/example/issues/{num}#issuecomment-{cid}",
    }


def make_manager(client=None, derive=None, logger=None):
    config = SimpleNamespace(repo="Atom/example", api_timeout=25)
    client = client or FakeClient()
    logger = logger or RecordingLogger()
    kwargs = {"derive": derive} if derive is not None else {}
    manager = StatusManager(config, client, logger, **kwargs)
    return manager, client, logger
