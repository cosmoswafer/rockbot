"""Live, opt-in write-data probe of the real Gitea server.

The read-only probe (:mod:`gitea_bots.live_probe`) can never prove the write
endpoints the tooling uses: create an issue, comment, label, assign, close and
delete. This probe exercises exactly that lifecycle against one **dummy issue**
it creates itself, then removes it again.

Safety model:

* every mutating call targets the issue number this run created
  (:meth:`WriteProbe._target_path`); a pre-existing issue is never touched;
* the created issue is re-read and its title marker verified before the first
  mutation;
* its body deliberately carries neither ``## Summary`` nor ``## Tasks``, so a
  live watcher can never auto-plan or re-assign it;
* a :class:`FileLock` under ``tmp/gitea-bots-live-write-probe/`` refuses
  overlapping runs and keeps the probe away from a live watcher's state;
* ``cleanup()`` runs in ``finally``: the dummy issue is deleted. When deletion
  is forbidden (non-admin token), the probe clears labels/assignees, keeps the
  issue closed and reports the leftover loudly instead of failing silently.

Run it directly (needs ``GITEA_TOKEN`` in ``.env`` and network access):

    python3 scripts/probe-live-write.py

or through the opt-in unittest adapter:

    GITEA_BOTS_LIVE_WRITE=1 python3 scripts/run-tests.py
"""

from __future__ import annotations

from pathlib import Path

from issue_labels.gitea import GiteaClient
from issue_labels.labels import label_status

from .clock import now_utc
from .errors import GiteaError
from .live_probe import Check, ProbeReport, ScenarioResult
from .locks import FileLock, LockBusy
from .rules import has_summary, has_tasks

# A dedicated state dir keeps the probe away from a live watcher's
# ./tmp/gitea-bots/ state (one-watcher rule).
PROBE_STATE = Path("tmp") / "gitea-bots-live-write-probe"
TITLE_MARKER = "[probe] gitea-bots write-data"
BODY = (
    "Dummy issue created by `scripts/probe-live-write.py`.\n\n"
    "It deliberately has no `## Summary` or `## Tasks` section, so a live\n"
    "watcher can never auto-plan or re-assign it. Safe to delete.\n"
)
DELETE_FORBIDDEN = "HTTP 403"


class WriteProbe:
    """Exercise the write lifecycle against one dummy issue, then delete it."""

    def __init__(self, config, client, logger, *, lock_path=None):
        self.config = config
        self.client = client
        self.log = logger
        self.lock_path = (
            Path(lock_path)
            if lock_path is not None
            else config.repo_root / PROBE_STATE / "lock"
        )
        self.number = 0
        self.title = ""
        self.temp_label_id = 0
        self.leftover_url = ""
        self.cleaned_up = False

    # ------------------------------------------------------------------ run
    def run(self) -> ProbeReport:
        report = ProbeReport()
        scenario = ScenarioResult("write-data lifecycle")
        report.scenarios.append(scenario)
        try:
            with FileLock(self.lock_path):
                try:
                    self._create(scenario)
                    self._comment(scenario)
                    self._label(scenario)
                    self._assign(scenario)
                    self._close(scenario)
                finally:
                    self._cleanup(scenario)
        except LockBusy:
            scenario.error = f"another write probe holds {self.lock_path}"
        except Exception as exc:  # noqa: BLE001 - the report carries the failure
            if not scenario.error:
                scenario.error = str(exc)
        return report

    # ---------------------------------------------------------------- steps
    def _create(self, scenario) -> None:
        self.title = f"{TITLE_MARKER} {now_utc()}"
        created = self.client.post(
            f"/repos/{self.config.repo}/issues",
            payload={"title": self.title, "body": BODY},
        )
        number = created.get("number") if isinstance(created, dict) else None
        self.number = int(number) if isinstance(number, int) else 0
        scenario.checks.append(
            Check(
                "dummy issue created",
                self.number > 0,
                f"#{self.number}" if self.number else "no issue number returned",
            )
        )
        if self.number <= 0:
            raise AssertionError("create did not return an issue number")

        issue = self._get_issue()
        title = issue.get("title") or ""
        scenario.checks.append(
            Check(
                "created issue carries the probe title marker",
                title.startswith(TITLE_MARKER),
                title,
            )
        )
        body = issue.get("body") or ""
        scenario.checks.append(
            Check(
                "dummy body is plan-ineligible",
                not has_summary(body) and not has_tasks(body),
            )
        )

    def _comment(self, scenario) -> None:
        created = self.client.post(
            self._target_path("/comments"),
            payload={"body": "probe comment — safe to delete"},
        )
        comment_id = created.get("id") if isinstance(created, dict) else None
        comments = self.client.get_all(self._target_path("/comments"))
        ids = {
            entry.get("id") for entry in comments if isinstance(entry, dict)
        }
        scenario.checks.append(
            Check(
                "comment visible on the dummy issue",
                isinstance(comment_id, int) and comment_id in ids,
                f"comment {comment_id!r}",
            )
        )

    def _label(self, scenario) -> None:
        label = self._pick_label()
        label_id = label.get("id")
        self.client.put(
            self._target_path("/labels"), payload={"labels": [label_id]}
        )
        issue = self._get_issue()
        current = {
            entry.get("id")
            for entry in issue.get("labels") or []
            if isinstance(entry, dict)
        }
        scenario.checks.append(
            Check(
                "label applied to the dummy issue",
                label_id in current,
                f"label {label.get('name')!r}",
            )
        )

    def _pick_label(self) -> dict:
        """Reuse an existing non-status label; create a temporary one if needed."""
        labels = self.client.get_all(f"/repos/{self.config.repo}/labels")
        for label in labels:
            if not isinstance(label, dict):
                continue
            if isinstance(label.get("id"), int) and not label_status(label.get("name")):
                return label

        name = f"{TITLE_MARKER} label {now_utc()}"
        created = self.client.post(
            f"/repos/{self.config.repo}/labels",
            payload={
                "name": name,
                "color": "ededed",
                "description": "temporary gitea-bots write probe label",
            },
        )
        label_id = created.get("id") if isinstance(created, dict) else None
        if not isinstance(label_id, int):
            raise AssertionError("could not create a temporary probe label")
        self.temp_label_id = label_id
        return {"id": label_id, "name": name}

    def _assign(self, scenario) -> None:
        login = self.config.self_login
        if not login:
            raise AssertionError("no token user resolved for the write probe")
        self.client.patch(self._target_path(), payload={"assignees": [login]})
        issue = self._get_issue()
        logins = {
            entry.get("login")
            for entry in issue.get("assignees") or []
            if isinstance(entry, dict)
        }
        scenario.checks.append(
            Check(
                "issue assigned to the token user",
                login in logins,
                f"assignee {login!r}",
            )
        )

    def _close(self, scenario) -> None:
        self.client.patch(self._target_path(), payload={"state": "closed"})
        issue = self._get_issue()
        scenario.checks.append(
            Check(
                "dummy issue closed",
                issue.get("state") == "closed",
                f"state {issue.get('state')!r}",
            )
        )

    # -------------------------------------------------------------- cleanup
    def _cleanup(self, scenario) -> None:
        if self.number <= 0:
            return
        url = self._issue_url()
        try:
            self.client.delete(self._target_path())
        except GiteaError as exc:
            if DELETE_FORBIDDEN in str(exc):
                self._fallback_cleanup(scenario, exc, url)
            else:
                self.leftover_url = url
                scenario.checks.append(
                    Check("dummy issue deleted", False, f"{exc}; leftover {url}")
                )
            return

        self.cleaned_up = True
        scenario.checks.append(
            Check("dummy issue deleted", True, f"#{self.number}")
        )
        self._delete_temp_label(scenario)

    def _fallback_cleanup(self, scenario, exc, url) -> None:
        """Delete was forbidden: neutralise the issue and fail loudly."""
        self.leftover_url = url
        try:
            self.client.put(self._target_path("/labels"), payload={"labels": []})
            self.client.patch(
                self._target_path(),
                payload={"assignees": [], "state": "closed"},
            )
        except GiteaError as fallback_exc:
            scenario.checks.append(
                Check(
                    "dummy issue neutralised after a forbidden delete",
                    False,
                    f"{fallback_exc}; leftover {url}",
                )
            )
        else:
            scenario.checks.append(
                Check(
                    "dummy issue neutralised after a forbidden delete",
                    False,
                    f"{exc}; labels/assignees cleared and issue closed, "
                    f"leftover {url}",
                )
            )
        self._delete_temp_label(scenario)

    def _delete_temp_label(self, scenario) -> None:
        if not self.temp_label_id:
            return
        try:
            self.client.delete(
                f"/repos/{self.config.repo}/labels/{self.temp_label_id}"
            )
        except GiteaError as exc:
            scenario.checks.append(
                Check("temporary probe label deleted", False, str(exc))
            )
            return
        self.temp_label_id = 0
        scenario.checks.append(Check("temporary probe label deleted", True))

    # -------------------------------------------------------------- helpers
    def _target_path(self, suffix: str = "") -> str:
        """Build a path for the issue this run created (never a pre-existing one)."""
        assert self.number > 0, "probe target was not created"
        return f"/repos/{self.config.repo}/issues/{self.number}{suffix}"

    def _get_issue(self) -> dict:
        data = self.client.get(self._target_path())
        return data if isinstance(data, dict) else {}

    def _issue_url(self) -> str:
        base = (self.config.api_base or "").rstrip("/")
        if base.endswith("/api/v1"):
            base = base[: -len("/api/v1")]
        if not base:
            return f"#{self.number}"
        return f"{base}/{self.config.repo}/issues/{self.number}"


def build_write_probe(config, logger) -> WriteProbe:
    """Apply the probe overrides and wire a write :class:`WriteProbe`."""
    config.state_dir = (config.repo_root / PROBE_STATE).resolve()
    config.notify_enabled = False
    config.spawn_workers = False
    config.auto_plan_workers = False
    config.start_reconcile = False
    config.all_comments = False
    config.verbose = False

    client = GiteaClient(
        config.api_base, config.token, timeout=config.api_timeout
    )
    login = client.resolve_login()
    if not login:
        logger.die("could not resolve the token user for the write probe")
    config.self_login = login
    return WriteProbe(config, client, logger)
