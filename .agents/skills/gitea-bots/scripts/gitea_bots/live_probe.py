"""Live, read-only probe of the real Gitea server.

The documented trigger→response scenarios depend on real API reads:

* **AUTO PLAN** — list open issues, then read each body for ``## Summary`` /
  ``## Tasks`` and its comments for the stage-2 plan marker.
* **DIRECTED COMMENT** — list an issue's comments and classify the human
  at-mentions that are not quoted code or bot output.
* **REASSIGNED** — list an issue's timeline and inspect ``type=assignees``
  events.
* **Status labels** — resolve the repository label shape the status manager
  consumes.

The offline unit suite proves the logic against ``FakeClient``; this probe
proves the real server still returns the shapes those paths consume and that the
same scenario predicates hold on real data. It is **strictly read-only**:
:class:`ReadOnlyClient` rejects every non-GET request, and the notifier/status
stubs refuse to write, so a regression fails the probe instead of mutating the
tracker.

Run it directly (needs ``GITEA_TOKEN`` in ``.env`` and network access):

    python3 scripts/probe-live.py

or through the opt-in unittest adapter:

    GITEA_BOTS_LIVE=1 python3 scripts/run-tests.py
"""

from __future__ import annotations

import shutil
from dataclasses import dataclass, field
from pathlib import Path

from .client import GiteaClient
from .db import Database
from .inbox import Inbox
from .models import Issue
from .rules import (
    TAG_DIRECTED,
    classify_comment,
    comments_have_stage2_marker,
    has_bot_mention,
    is_bot_output,
)
from .state import StateStore
from .watcher import Watcher

# A dedicated state dir keeps the probe away from a live watcher's
# ./tmp/gitea-bots/ state (one-watcher rule).
PROBE_STATE = Path("tmp") / "gitea-bots-live-probe"
DEFAULT_ISSUE_LIMIT = 10


@dataclass
class Check:
    """One assertion inside a scenario, with an explicit pass/fail."""

    name: str
    ok: bool
    detail: str = ""


@dataclass
class ScenarioResult:
    """The checks and notes collected for one documented scenario."""

    name: str
    checks: list[Check] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)
    error: str = ""

    @property
    def ok(self) -> bool:
        return not self.error and all(check.ok for check in self.checks)


@dataclass
class ProbeReport:
    """All scenario results plus their rendered, human-readable form."""

    scenarios: list[ScenarioResult] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return bool(self.scenarios) and all(s.ok for s in self.scenarios)

    def render(self) -> str:
        lines = []
        passed = 0
        for scenario in self.scenarios:
            status = "PASS" if scenario.ok else "FAIL"
            lines.append(f"[{status}] {scenario.name}")
            for check in scenario.checks:
                mark = "ok" if check.ok else "FAIL"
                suffix = f" — {check.detail}" if check.detail else ""
                lines.append(f"  {mark}: {check.name}{suffix}")
            for note in scenario.notes:
                lines.append(f"  note: {note}")
            if scenario.error:
                lines.append(f"  error: {scenario.error}")
            if scenario.ok:
                passed += 1
        lines.append(f"{passed}/{len(self.scenarios)} scenario(s) passed")
        return "\n".join(lines)


class ReadOnlyClient(GiteaClient):
    """Refuse anything that could mutate the real tracker."""

    def request(self, method, path, *, params=None, payload=None):
        if method.upper() != "GET":
            raise AssertionError(
                f"live probe is read-only — refusing {method.upper()} {path}"
            )
        return super().request(method, path, params=params, payload=payload)


class NoopNotifier:
    """Fail loudly if the probe ever tries to notify or spawn a worker."""

    def notify(self, *args, **kwargs):
        raise AssertionError("live probe must not notify or spawn workers")


class NoopStatus:
    """Fail loudly if the probe ever tries to write a status label."""

    def apply(self, *args, **kwargs):
        raise AssertionError("live probe must not write labels")

    def reconcile(self, *args, **kwargs):
        raise AssertionError("live probe must not write labels")


def _summarize(errors, limit: int = 3) -> str:
    if not errors:
        return ""
    shown = "; ".join(errors[:limit])
    remaining = len(errors) - limit
    return shown + (f"; (+{remaining} more)" if remaining > 0 else "")


class LiveProbe:
    """Run the documented scenarios against a real (read-only) client."""

    def __init__(self, config, client, logger, *, issue_limit=DEFAULT_ISSUE_LIMIT):
        self.config = config
        self.client = client
        self.log = logger
        self.issue_limit = max(0, int(issue_limit))
        self.issues: list[Issue] = []
        self.comments: dict[int, list] = {}
        self.timeline: dict[int, list] = {}

    # ------------------------------------------------------------------ run
    def run(self) -> ProbeReport:
        report = ProbeReport()
        try:
            self._load()
        except Exception as exc:  # noqa: BLE001 - the report carries the failure
            report.scenarios.append(
                ScenarioResult(name="read live repository data", error=str(exc))
            )
            return report

        self.log.verbose(
            f"live probe: {len(self.issues)} open issue(s), "
            f"reading comments/timeline for the first {self.issue_limit}"
        )
        for scenario in (
            self.scenario_auto_plan,
            self.scenario_directed_comment,
            self.scenario_reassigned,
            self.scenario_labels,
        ):
            try:
                result = scenario()
            except Exception as exc:  # noqa: BLE001 - collect instead of aborting
                result = ScenarioResult(name=scenario.__name__, error=str(exc))
            report.scenarios.append(result)
        return report

    def _load(self) -> None:
        raw_issues = self.client.get_all(
            f"/repos/{self.config.repo}/issues",
            {"state": "open", "sort": "updated", "direction": "desc"},
        )
        self.issues = [
            Issue.from_api(raw) for raw in raw_issues if isinstance(raw, dict)
        ]
        for issue in self.issues[: self.issue_limit]:
            if not isinstance(issue.number, int):
                continue
            self.comments[issue.number] = self.client.get_all(
                f"/repos/{self.config.repo}/issues/{issue.number}/comments"
            )
            self.timeline[issue.number] = self.client.get_all(
                f"/repos/{self.config.repo}/issues/{issue.number}/timeline"
            )

    # ------------------------------------------------------------- scenarios
    def scenario_auto_plan(self) -> ScenarioResult:
        """AUTO PLAN path: poll the real open set with the real watcher."""
        result = ScenarioResult("AUTO PLAN baseline poll")
        if self.config.state_dir.exists():
            shutil.rmtree(self.config.state_dir)

        db = Database(self.config.db_file, self.log)
        db.ensure_schema()
        watcher = Watcher(
            self.config,
            self.client,
            StateStore(self.config, self.log, db),
            Inbox(self.config, self.log, db),
            NoopNotifier(),
            NoopStatus(),
            self.log,
        )
        ok, poll = watcher.poll_once(report_existing=1, notify_armed=False)
        result.checks.append(Check("baseline poll succeeds", bool(ok)))
        result.checks.append(
            Check("open issues were scanned", poll.scanned > 0,
                  f"{poll.scanned} issue(s)")
        )
        result.checks.append(
            Check("baseline poll emitted events", poll.event_count > 0,
                  f"{poll.event_count} event(s)")
        )
        result.checks.append(
            Check(
                "issue-level events present",
                "NEW ISSUE" in poll.events_text
                or "AUTO PLAN" in poll.events_text,
            )
        )

        # AUTO PLAN is assignment-only (#150). Report whether the live repo
        # currently has a candidate; when it does, the baseline poll must have
        # emitted the corresponding AUTO PLAN line.
        candidates = [
            issue.number
            for issue in self.issues
            if issue.state == "open"
            and issue.assigned_to(self.config.self_login)
            and issue.has_summary
            and not issue.has_tasks
            and not comments_have_stage2_marker(
                self.comments.get(issue.number, [])
            )
        ]
        result.notes.append(f"{len(candidates)} AUTO PLAN candidate(s)")
        if candidates:
            result.checks.append(
                Check(
                    "AUTO PLAN emitted for live candidate(s)",
                    "AUTO PLAN" in poll.events_text,
                )
            )
        else:
            result.notes.append(
                "no AUTO PLAN candidate (assignment-only predicate)"
            )
        return result

    def scenario_directed_comment(self) -> ScenarioResult:
        """DIRECTED COMMENT path: comments + at-mention classification."""
        result = ScenarioResult("DIRECTED COMMENT classification")
        checked = directed = 0
        shape_errors: list[str] = []
        kind_errors: list[str] = []
        for issue in self.issues[: self.issue_limit]:
            for comment in self.comments.get(issue.number, []):
                cid = comment.get("id") if isinstance(comment, dict) else None
                user = comment.get("user") if isinstance(comment, dict) else None
                if not (
                    isinstance(comment, dict)
                    and isinstance(cid, int)
                    and isinstance(comment.get("body"), str)
                    and isinstance(user, dict)
                    and isinstance(user.get("login"), str)
                ):
                    shape_errors.append(
                        f"issue #{issue.number} comment {cid!r} has an "
                        "unexpected shape"
                    )
                    continue

                author = user.get("login", "")
                body = comment.get("body") or ""
                tag = classify_comment(
                    author,
                    body,
                    self_login=self.config.self_login,
                    all_comments=False,
                    shared_login=self.config.shared_login,
                )
                expected_directed = has_bot_mention(
                    body, self.config.self_login
                ) and (
                    self.config.shared_login
                    or author != self.config.self_login
                )
                if is_bot_output(body):
                    expected = None
                elif expected_directed:
                    expected = TAG_DIRECTED
                    directed += 1
                else:
                    expected = None
                if tag != expected:
                    kind_errors.append(
                        f"issue #{issue.number} comment {cid}: expected "
                        f"{expected!r}, got {tag!r}"
                    )
                checked += 1

        result.checks.append(
            Check("comments have the expected shape", not shape_errors,
                  _summarize(shape_errors))
        )
        result.checks.append(
            Check(
                "at-mentions classify as DIRECTED COMMENT",
                not kind_errors,
                _summarize(kind_errors),
            )
        )
        result.notes.append(
            f"{checked} comment(s) scanned, {directed} directed mention(s)"
        )
        return result

    def scenario_reassigned(self) -> ScenarioResult:
        """REASSIGNED path: timeline shape + assignee events."""
        result = ScenarioResult("REASSIGNED timeline shape")
        checked = assignee_events = 0
        shape_errors: list[str] = []
        login_errors: list[str] = []
        for issue in self.issues[: self.issue_limit]:
            for event in self.timeline.get(issue.number, []):
                eid = event.get("id") if isinstance(event, dict) else None
                if not (
                    isinstance(event, dict)
                    and isinstance(eid, int)
                    and isinstance(event.get("type"), str)
                ):
                    shape_errors.append(
                        f"issue #{issue.number} timeline event {eid!r} has an "
                        "unexpected shape"
                    )
                    continue
                checked += 1
                if event.get("type") != "assignees":
                    continue
                assignee_events += 1
                if int(eid) <= 0:
                    shape_errors.append(
                        f"issue #{issue.number} assignee event has id {eid!r}"
                    )
                if event.get("removed_assignee") in (True, "true"):
                    continue
                login = (event.get("assignee") or {}).get("login", "")
                if not login:
                    login_errors.append(
                        f"issue #{issue.number} assignee event {eid} has no "
                        "assignee.login"
                    )

        result.checks.append(
            Check("timeline events have the expected shape", not shape_errors,
                  _summarize(shape_errors))
        )
        result.checks.append(
            Check(
                "assignee events carry an assignee login",
                not login_errors,
                _summarize(login_errors),
            )
        )
        result.notes.append(
            f"{checked} timeline event(s) scanned, "
            f"{assignee_events} assignee event(s)"
        )
        return result

    def scenario_labels(self) -> ScenarioResult:
        """Status labels resolve by name/id/colour on the real server."""
        result = ScenarioResult("status label shape")
        labels = self.client.get_all(f"/repos/{self.config.repo}/labels")
        errors: list[str] = []
        if not isinstance(labels, list):
            errors.append(f"expected a list, got {type(labels).__name__}")
            labels = []
        for label in labels:
            if not (
                isinstance(label, dict)
                and isinstance(label.get("id"), int)
                and isinstance(label.get("name"), str)
                and isinstance(label.get("color"), str)
            ):
                errors.append(f"label {label!r} has an unexpected shape")
        result.checks.append(
            Check("repository labels have the expected shape", not errors,
                  _summarize(errors))
        )
        result.notes.append(f"{len(labels)} repository label(s)")
        return result


def build_probe(config, logger, *, issue_limit=DEFAULT_ISSUE_LIMIT) -> LiveProbe:
    """Apply the probe overrides and wire a read-only :class:`LiveProbe`."""
    config.state_dir = (config.repo_root / PROBE_STATE).resolve()
    config.notify_enabled = False
    config.spawn_workers = False
    config.auto_plan_workers = False
    config.start_reconcile = False
    config.all_comments = False
    config.verbose = False

    client = ReadOnlyClient(
        config.api_base, config.token, timeout=config.api_timeout
    )
    login = client.resolve_login()
    if not login:
        logger.die("could not resolve the token user for the live probe")
    config.self_login = login
    return LiveProbe(config, client, logger, issue_limit=issue_limit)
