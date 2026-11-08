"""Live, opt-in end-to-end probe: real dummy issues -> real watcher -> cleanup.

The read-only probe (:mod:`gitea_bots.live_probe`) proves API shapes and the
write probe (:mod:`gitea_bots.write_probe`) proves the raw write lifecycle.
Neither runs the watcher. This probe closes that gap: it creates controlled
dummy issues on the real Gitea server, lets the **real** watcher poll and
classify them, then asserts the emitted events, ``state.db`` transitions,
status labels and idempotence — and deletes everything afterwards.

Safety model:

* a dedicated state directory (``tmp/gitea-bots-live-e2e/``) and a sibling
  :class:`FileLock` keep the run away from a live watcher's ``tmp/gitea-bots/``
  state and refuse overlapping runs;
* the probe refuses to start while the production ``watcher.pid`` points at a
  live ``watch-issues.py`` process (stop it first);
* every fixture is created by this run and re-read (title marker verified)
  before its first mutation; every mutating issue path is guarded by
  :meth:`E2EProbe._issue_path`;
* notification/worker spawns are disabled: the poll runs with
  ``notify_armed=False`` and the optional pickup case uses a recording notifier
  that never starts an ``opencode`` process;
* ``cleanup()`` runs in ``finally``: every created issue is deleted and a
  ``GET`` is required to answer ``404``. When deletion is forbidden (non-admin
  token), the probe clears labels/assignees, keeps the issue closed, records
  the leftover URL and fails loudly.

Run it directly (needs ``GITEA_TOKEN`` in ``.env`` and network access):

    python3 scripts/probe-e2e.py

or through the opt-in unittest adapter:

    GITEA_BOTS_LIVE_E2E=1 python3 scripts/run-tests.py
"""

from __future__ import annotations

import json
import os
import re
import shutil
from dataclasses import dataclass
from pathlib import Path
from unittest import mock

from issue_labels.gitea import GiteaClient
from issue_labels.labels import label_status

from .clock import compact_utc
from .db import Database
from .errors import GiteaError
from .inbox import Inbox
from .live_probe import Check, ProbeReport, ScenarioResult
from .locks import FileLock, LockBusy
from .notify import Notifier
from .rules import STAGE2_MARKER, STAGE3_MARKER
from .state import StateStore
from .status import StatusManager
from .watcher import Watcher

# Dedicated state and lock paths; neither is the live watcher's ./tmp/gitea-bots/.
PROBE_STATE = Path("tmp") / "gitea-bots-live-e2e"
PROBE_LOCK = Path("tmp") / "gitea-bots-live-e2e.lock"
TITLE_MARKER = "[probe] gitea-bots e2e"
SUMMARY = (
    "## Summary\n\n"
    "Dummy issue created by `scripts/probe-e2e.py`. Safe to delete.\n"
)
PLAN_BODY = SUMMARY + "\n## Tasks\n\n- [ ] probe task\n"
NO_SUMMARY = (
    "Dummy issue created by `scripts/probe-e2e.py`.\n\n"
    "It deliberately has no `## Summary` section, so it is not plan-eligible.\n"
    "Safe to delete.\n"
)
DELETE_FORBIDDEN = "HTTP 403"
NOT_FOUND = "HTTP 404"

_EVENT_LINE = re.compile(
    r"^\d{4}-\d{2}-\d{2}T\S+\s+"
    r"(NEW ISSUE|UPDATED ISSUE|DIRECTED COMMENT|AUTO PLAN|REASSIGNED|NEW COMMENT)"
    r"(?:\s+\(catch-up\))?(?:\s+on)?\s+#(\d+)\b"
)

# Expected watcher tags for the first incremental poll, and the tags that must
# never appear for that fixture. A closed issue created after the baseline is
# still first-seen-tracked (NEW ISSUE); the case proves it is never planned.
CASE_EXPECTATIONS = {
    "A": ({"AUTO PLAN"}, set()),
    "B": ({"NEW ISSUE"}, {"AUTO PLAN"}),
    "C": ({"NEW ISSUE"}, {"AUTO PLAN"}),
    "D": ({"NEW ISSUE"}, {"AUTO PLAN"}),
    "E": ({"NEW ISSUE", "DIRECTED COMMENT"}, {"AUTO PLAN"}),
    "F": ({"NEW ISSUE"}, {"AUTO PLAN", "DIRECTED COMMENT"}),
    "H": ({"NEW ISSUE"}, {"AUTO PLAN", "DIRECTED COMMENT"}),
    "I": ({"NEW ISSUE", "DIRECTED COMMENT"}, {"AUTO PLAN"}),
}

CASE_TITLES = {
    "A": "assigned Summary-only emits AUTO PLAN",
    "B": "unassigned Summary-only never AUTO PLANs",
    "C": "assigned issue without Summary never AUTO PLANs",
    "D": "planned issue never AUTO PLANs",
    "E": "human at-mention becomes DIRECTED COMMENT",
    "F": "bot output without at-mention stays quiet",
    "H": "closed issue is tracked, never planned",
    "I": "issue-body at-mention becomes DIRECTED COMMENT",
}


def parse_event_tags(events_text: str) -> dict[int, list[str]]:
    """Map issue number -> event tags from the watcher's emitted text.

    Anchored on the timestamped event header (and not the indented ``    | ``
    comment bodies), so titles or quoted text never count as events. Pure and
    exported for the offline tests.
    """
    tags: dict[int, list[str]] = {}
    for line in (events_text or "").splitlines():
        match = _EVENT_LINE.match(line)
        if not match:
            continue
        tags.setdefault(int(match.group(2)), []).append(match.group(1))
    return tags


def report_to_dict(report: ProbeReport) -> dict:
    """Machine-readable form of a probe report (used by the artifact writer)."""
    return {
        "ok": report.ok,
        "scenarios": [
            {
                "name": scenario.name,
                "ok": scenario.ok,
                "error": scenario.error,
                "notes": list(scenario.notes),
                "checks": [
                    {"name": check.name, "ok": check.ok, "detail": check.detail}
                    for check in scenario.checks
                ],
            }
            for scenario in report.scenarios
        ],
    }


def write_report_artifact(report: ProbeReport, config) -> dict[str, Path]:
    """Write the rendered report (``.log``) and structured form (``.json``)."""
    out_dir = Path(config.repo_root) / "tmp" / "artifacts"
    out_dir.mkdir(parents=True, exist_ok=True)
    stamp = compact_utc()
    log_path = out_dir / f"gitea-bots-e2e-{stamp}.log"
    json_path = out_dir / f"gitea-bots-e2e-{stamp}.json"
    log_path.write_text(report.render() + "\n", encoding="utf-8")
    json_path.write_text(
        json.dumps(report_to_dict(report), indent=2) + "\n", encoding="utf-8"
    )
    return {"log": log_path, "json": json_path}


@dataclass(frozen=True)
class FixtureSpec:
    """One controlled issue the probe creates."""

    key: str
    body: str
    assigned: bool = False
    closed: bool = False
    comments: tuple[str, ...] = ()


class RecordingNotifier(Notifier):
    """Record dispatch decisions without starting an ``opencode`` process.

    The notifier's decision logic (``human_reply_blocks`` / AUTO PLAN detection
    and ``_mark_picked_up``) stays live; only the final ``_spawn`` is replaced.
    """

    def __init__(self, config, inbox, status, logger):
        super().__init__(config, inbox, status, logger)
        self.spawned: list[dict] = []

    def _spawn(self, args, title: str, *, archived: bool = True) -> None:
        self.spawned.append(
            {"title": title, "args": list(args), "archived": archived}
        )

    def _shared(self, event_count: int, delegate: bool) -> None:
        self.spawned.append(
            {"title": "shared", "args": [], "archived": True}
        )


class E2EProbe:
    """Create dummy issues, run the real watcher, assert, then delete them."""

    def __init__(self, config, client, logger, *, lock_path=None,
                 production_state_dir=None):
        self.config = config
        self.client = client
        self.log = logger
        self.lock_path = (
            Path(lock_path)
            if lock_path is not None
            else config.repo_root / PROBE_LOCK
        )
        self.production_state_dir = (
            Path(production_state_dir)
            if production_state_dir is not None
            else config.repo_root / "tmp" / "gitea-bots"
        )
        self.report = ProbeReport()
        self.scenarios: dict[str, ScenarioResult] = {}
        self.run_id = ""
        self.fixtures: dict[str, int] = {}
        self.created: list[int] = []
        self.label_ids_before: set[int] = set()
        self.label_snapshot_ok = False
        self.poll1_text = ""
        self.poll1_event_count = 0
        self.poll1_tags: dict[int, list[str]] = {}
        self._started = False
        self._db = None
        self._state = None
        self._inbox = None
        self._status = None
        self._watcher = None
        self.leftover_urls: list[str] = []
        self.cleaned_up = False

    # ---------------------------------------------------------------- reporting
    def _add(self, key: str, name: str) -> ScenarioResult:
        scenario = ScenarioResult(name)
        self.scenarios[key] = scenario
        self.report.scenarios.append(scenario)
        return scenario

    def _scenario(self, key: str, name: str) -> ScenarioResult:
        return self.scenarios.get(key) or self._add(key, name)

    def _fail(self, key: str, message: str) -> None:
        self._scenario(key, key).error = message

    # ------------------------------------------------------------------ run
    def run(self) -> ProbeReport:
        locked = False
        try:
            with FileLock(self.lock_path):
                locked = True
                if not self._ensure_safe():
                    return self.report
                self._started = True
                self._execute()
        except LockBusy:
            self._fail(
                "safety", f"another end-to-end probe holds {self.lock_path}"
            )
        except Exception as exc:  # noqa: BLE001 - the report carries the failure
            self._fail("aborted", f"{type(exc).__name__}: {exc}")
        finally:
            if locked and self._started:
                try:
                    self._cleanup()
                except Exception as exc:  # noqa: BLE001 - cleanup is best-effort
                    self._fail(
                        "cleanup",
                        f"cleanup failed: {type(exc).__name__}: {exc}",
                    )
        return self.report

    def _ensure_safe(self) -> bool:
        scenario = self._add("safety", "safety guards")
        if not self.config.self_login:
            scenario.checks.append(
                Check("token user resolved", False, "config.self_login is empty")
            )
            return False
        pid_file = self.production_state_dir / "watcher.pid"
        if self._watcher_running(pid_file):
            scenario.checks.append(
                Check(
                    "production watcher is stopped",
                    False,
                    f"{pid_file} points at a live watcher — run "
                    "'watch-issues.py stop' first",
                )
            )
            return False
        scenario.checks.append(
            Check("production watcher is stopped", True, str(pid_file))
        )
        scenario.checks.append(
            Check("token user resolved", True, self.config.self_login)
        )
        return True

    @staticmethod
    def _watcher_running(pid_file) -> bool:
        """Whether ``pid_file`` names a live ``watch-issues.py`` process."""
        try:
            pid = Path(pid_file).read_text(encoding="utf-8").strip()
        except OSError:
            return False
        return Watcher._pid_is_watcher(pid)

    def _execute(self) -> None:
        self._prepare()
        self._baseline()
        self._create_fixtures()
        self._first_poll()
        self._idempotence()
        self._reassign()
        self._notify_case()
        self._labels_case()

    # ------------------------------------------------------------- preparation
    def _prepare(self) -> None:
        if self.config.state_dir.exists():
            shutil.rmtree(self.config.state_dir)
        self.config.state_dir.mkdir(parents=True, exist_ok=True)
        self.run_id = f"{compact_utc()}-{os.getpid()}"
        self._db = Database(self.config.db_file, self.log)
        self._db.ensure_schema()
        self._state = StateStore(self.config, self.log, self._db)
        self._inbox = Inbox(self.config, self.log, self._db)
        self._status = StatusManager(self.config, self.client, self.log)
        notifier = RecordingNotifier(
            self.config, self._inbox, self._status, self.log
        )
        self._watcher = Watcher(
            self.config,
            self.client,
            self._state,
            self._inbox,
            notifier,
            self._status,
            self.log,
        )
        self._snapshot_labels()

    def _snapshot_labels(self) -> None:
        """Record pre-existing label ids so cleanup only deletes new ones."""
        try:
            labels = self.client.get_all(
                f"/repos/{self.config.repo}/labels"
            )
        except GiteaError as exc:
            self.log.warn(f"e2e: could not snapshot repository labels: {exc}")
            return
        self.label_ids_before = {
            label.get("id")
            for label in labels
            if isinstance(label, dict) and isinstance(label.get("id"), int)
        }
        self.label_snapshot_ok = True

    # ------------------------------------------------------------------- polls
    def _poll(self):
        ok, poll = self._watcher.poll_once(0, False)
        if not ok:
            raise RuntimeError("watcher poll failed — see the probe log")
        return poll

    def _baseline(self) -> None:
        scenario = self._add("baseline", "baseline poll is silent")
        poll = self._poll()
        scenario.checks.append(
            Check("baseline poll succeeds", True, f"scanned {poll.scanned}")
        )
        scenario.checks.append(
            Check(
                "baseline emitted no events",
                poll.event_count == 0,
                f"{poll.event_count} event(s)",
            )
        )
        scenario.checks.append(
            Check(
                "baseline published no pending batch",
                not self._inbox.pending_batches(),
            )
        )
        scenario.notes.append(f"{poll.scanned} open issue(s) recorded")

    # ---------------------------------------------------------------- fixtures
    def _fixture_specs(self) -> list[FixtureSpec]:
        login = self.config.self_login
        return [
            FixtureSpec("A", SUMMARY, assigned=True),
            FixtureSpec("B", SUMMARY),
            FixtureSpec("C", NO_SUMMARY, assigned=True),
            FixtureSpec("D", PLAN_BODY, assigned=True),
            FixtureSpec(
                "E",
                SUMMARY,
                comments=(
                    f"Probe plan marker.\n\n{STAGE2_MARKER}",
                    f"@{login} probe e2e directed reply",
                ),
            ),
            FixtureSpec(
                "F",
                SUMMARY,
                comments=(
                    f"Probe bot output — no at-mention.\n\n{STAGE3_MARKER}",
                ),
            ),
            FixtureSpec("H", SUMMARY, closed=True),
            FixtureSpec("I", self._body_mention_body()),
        ]

    def _body_mention_body(self) -> str:
        """A Summary-only body plus a mention paragraph (unassigned, case I).

        Unassigned so it cannot also AUTO PLAN: the poll must surface exactly
        one DIRECTED COMMENT request from the body, deduped on the next poll.
        """
        return (
            SUMMARY
            + f"\n@{self.config.self_login} probe e2e issue-body mention\n"
        )

    def _create_fixtures(self) -> None:
        scenario = self._add("fixtures", "fixture creation")
        for spec in self._fixture_specs():
            number = self._create_issue(spec)
            self.fixtures[spec.key] = number
            scenario.checks.append(
                Check(
                    f"case {spec.key} created and title marker verified",
                    number > 0,
                    f"#{number}",
                )
            )
            for body in spec.comments:
                self.client.post(
                    self._issue_path(number, "/comments"), payload={"body": body}
                )
            if spec.closed:
                self.client.patch(
                    self._issue_path(number), payload={"state": "closed"}
                )
        scenario.notes.append(
            "fixtures: " + ", ".join(
                f"{key}=#{number}" for key, number in self.fixtures.items()
            )
        )

    def _create_issue(self, spec: FixtureSpec) -> int:
        title = f"{TITLE_MARKER} {self.run_id} {spec.key} — {CASE_TITLES[spec.key]}"
        created = self.client.post(
            f"/repos/{self.config.repo}/issues",
            payload={"title": title, "body": spec.body},
        )
        number = created.get("number") if isinstance(created, dict) else None
        if not isinstance(number, int) or number <= 0:
            raise AssertionError(
                f"create did not return an issue number for case {spec.key}"
            )
        # Record immediately so cleanup can always delete it, then verify the
        # title marker before the first mutation of this issue.
        self.created.append(number)
        issue = self._get_issue(number)
        actual = issue.get("title") or ""
        if actual != title:
            raise AssertionError(
                f"#{number} title marker mismatch: expected {title!r}, "
                f"got {actual!r}"
            )
        if spec.assigned:
            self.client.patch(
                self._issue_path(number),
                payload={"assignees": [self.config.self_login]},
            )
        return number

    def _issue_path(self, number: int, suffix: str = "") -> str:
        """Only mutate an issue this run created."""
        assert number in self.created, (
            f"refusing to mutate issue #{number}: not created by this run"
        )
        return f"/repos/{self.config.repo}/issues/{number}{suffix}"

    # -------------------------------------------------------------- first poll
    def _first_poll(self) -> None:
        poll = self._poll()
        self.poll1_text = poll.events_text
        self.poll1_event_count = poll.event_count
        self.poll1_tags = parse_event_tags(poll.events_text)
        scenario = self._add("poll-1", "first incremental poll")
        scenario.checks.append(
            Check(
                "incremental poll emitted events",
                poll.event_count > 0,
                f"{poll.event_count} event(s)",
            )
        )

        for key in ("A", "B", "C", "D", "E", "F", "H", "I"):
            self._evaluate_case(key)

        fixture_numbers = set(self.fixtures.values())
        auto_plan = {
            number
            for number, tags in self.poll1_tags.items()
            if number in fixture_numbers and "AUTO PLAN" in tags
        }
        directed = {
            number
            for number, tags in self.poll1_tags.items()
            if number in fixture_numbers and "DIRECTED COMMENT" in tags
        }
        scenario.checks.append(
            Check(
                "A is the only fixture with AUTO PLAN",
                auto_plan == {self.fixtures["A"]},
                f"fixtures: {sorted(auto_plan)}",
            )
        )
        scenario.checks.append(
            Check(
                "E and I are the only fixtures with DIRECTED COMMENT",
                directed == {self.fixtures["E"], self.fixtures["I"]},
                f"fixtures: {sorted(directed)}",
            )
        )

        self._check_state_transitions(scenario)
        self._check_durable_events(scenario)

    def _evaluate_case(self, key: str) -> None:
        expected, forbidden = CASE_EXPECTATIONS[key]
        scenario = self._add(key, f"case {key}: {CASE_TITLES[key]}")
        actual = self.poll1_tags.get(self.fixtures[key], [])
        detail = ", ".join(actual) or "no event"
        for tag in sorted(expected):
            scenario.checks.append(
                Check(f"{tag} emitted", tag in actual, detail)
            )
        for tag in sorted(forbidden):
            scenario.checks.append(
                Check(f"{tag} not emitted", tag not in actual, detail)
            )

    def _check_state_transitions(self, scenario: ScenarioResult) -> None:
        issues = self._state.load()["issues"]

        def snapshot(key):
            return issues.get(str(self.fixtures[key]), {})

        scenario.checks.append(
            Check(
                "state records A's AUTO PLAN marker",
                bool(snapshot("A").get("auto_plan_at")),
                f"auto_plan_at={snapshot('A').get('auto_plan_at')!r}",
            )
        )
        for key in ("B", "C", "D", "F", "H"):
            scenario.checks.append(
                Check(
                    f"state has no AUTO PLAN marker for {key}",
                    not snapshot(key).get("auto_plan_at"),
                    f"auto_plan_at={snapshot(key).get('auto_plan_at')!r}",
                )
            )
        scenario.checks.append(
            Check(
                "state tracks H as closed",
                snapshot("H").get("state") == "closed",
                f"state={snapshot('H').get('state')!r}",
            )
        )
        scenario.checks.append(
            Check(
                "state records E's comment cursor",
                int(snapshot("E").get("last_comment_id") or 0) > 0,
                f"last_comment_id={snapshot('E').get('last_comment_id')!r}",
            )
        )
        scenario.checks.append(
            Check(
                "state records I's body-mention cursor",
                bool(snapshot("I").get("body_mention_sha")),
                f"body_mention_sha={snapshot('I').get('body_mention_sha')!r}",
            )
        )
        scenario.checks.append(
            Check(
                "state seeds D's timeline cursor",
                int(snapshot("D").get("last_assignee_event_id") or 0) > 0,
                f"last_assignee_event_id="
                f"{snapshot('D').get('last_assignee_event_id')!r}",
            )
        )

    def _check_durable_events(self, scenario: ScenarioResult) -> None:
        batches = self._inbox.pending_batches()
        scenario.checks.append(
            Check(
                "first poll published one durable batch",
                len(batches) == 1,
                f"{len(batches)} batch(es)",
            )
        )
        if not batches:
            return
        lines = self._inbox.db.event_lines(batches[-1]["id"])
        text = "\n".join(lines)
        scenario.checks.append(
            Check(
                "durable batch carries A's AUTO PLAN",
                bool(re.search(
                    rf"AUTO PLAN #{self.fixtures['A']}(?:\s|\"|$)", text
                )),
            )
        )
        scenario.checks.append(
            Check(
                "durable batch carries E's DIRECTED COMMENT",
                bool(re.search(
                    rf"DIRECTED COMMENT(?: \(catch-up\))? on "
                    rf"#{self.fixtures['E']}(?:\s|$)",
                    text,
                )),
            )
        )
        try:
            pending = self._inbox.pending_file.read_text(encoding="utf-8")
        except OSError:
            pending = ""
        scenario.checks.append(
            Check(
                "pending.md carries the batch",
                f"#{self.fixtures['A']}" in pending
                and f"#{self.fixtures['E']}" in pending,
            )
        )

    # ------------------------------------------------------------- idempotence
    def _idempotence(self) -> None:
        scenario = self._add("idempotence", "second poll is idempotent")
        before = self._state.load()["issues"].get(
            str(self.fixtures["A"]), {}
        )
        before_auto_plan = before.get("auto_plan_at")
        before_comment = self._state.load()["issues"].get(
            str(self.fixtures["E"]), {}
        ).get("last_comment_id")
        batches_before = len(self._inbox.pending_batches())

        poll = self._poll()
        tags = parse_event_tags(poll.events_text)
        fixture_numbers = set(self.fixtures.values())
        repeated = {
            number: tags[number]
            for number in fixture_numbers
            if tags.get(number)
        }
        scenario.checks.append(
            Check(
                "no fixture event is re-emitted",
                not repeated,
                ", ".join(
                    f"#{number}: {', '.join(names)}"
                    for number, names in sorted(repeated.items())
                ) or "no fixture events",
            )
        )
        after = self._state.load()["issues"]
        scenario.checks.append(
            Check(
                "AUTO PLAN is not duplicated",
                after.get(str(self.fixtures["A"]), {}).get("auto_plan_at")
                == before_auto_plan,
                f"before={before_auto_plan!r}",
            )
        )
        scenario.checks.append(
            Check(
                "comment cursor is unchanged",
                after.get(str(self.fixtures["E"]), {}).get("last_comment_id")
                == before_comment,
                f"before={before_comment!r}",
            )
        )
        scenario.checks.append(
            Check(
                "no duplicate fixture batch was published",
                self._fixture_events_in_new_batches(batches_before) == [],
            )
        )
        scenario.notes.append(
            f"{poll.event_count} unrelated event(s) tolerated"
            if poll.event_count else "poll emitted no events at all"
        )

    def _fixture_events_in_new_batches(self, batches_before: int) -> list[str]:
        """Fixture-scoped lines from batches published after the given count."""
        fixture_numbers = set(self.fixtures.values())
        found = []
        for batch in self._inbox.pending_batches()[batches_before:]:
            for line in self._inbox.db.event_lines(batch["id"]):
                match = _EVENT_LINE.match(line)
                if match and int(match.group(2)) in fixture_numbers:
                    found.append(line)
        return found

    # -------------------------------------------------------------- reassignment
    def _reassign(self) -> None:
        scenario = self._add("G", "case G: unassign -> assign emits REASSIGNED once")
        number = self.fixtures["D"]
        before = self._state.load()["issues"].get(str(number), {})
        before_cursor = int(before.get("last_assignee_event_id") or 0)

        self.client.patch(self._issue_path(number), payload={"assignees": []})
        self.client.patch(
            self._issue_path(number),
            payload={"assignees": [self.config.self_login]},
        )

        poll = self._poll()
        tags = parse_event_tags(poll.events_text)
        actual = tags.get(number, [])
        scenario.checks.append(
            Check(
                "REASSIGNED emitted",
                "REASSIGNED" in actual,
                ", ".join(actual) or "no event",
            )
        )
        scenario.checks.append(
            Check(
                "REASSIGNED emitted exactly once",
                actual.count("REASSIGNED") == 1,
                f"{actual.count('REASSIGNED')} time(s)",
            )
        )
        scenario.checks.append(
            Check(
                "no duplicate AUTO PLAN for the planned issue",
                "AUTO PLAN" not in actual,
                ", ".join(actual) or "no event",
            )
        )
        after_cursor = int(
            self._state.load()["issues"]
            .get(str(number), {})
            .get("last_assignee_event_id")
            or 0
        )
        scenario.checks.append(
            Check(
                "timeline cursor advanced",
                after_cursor > before_cursor,
                f"{before_cursor} -> {after_cursor}",
            )
        )

    # --------------------------------------------------------------- pickup case
    def _notify_case(self) -> None:
        scenario = self._add(
            "notify", "pickup label is applied without spawning OpenCode"
        )
        if not self.poll1_text:
            scenario.notes.append("skipped: the first poll emitted no events")
            return
        notifier = RecordingNotifier(
            self.config, self._inbox, self._status, self.log
        )
        saved = (
            self.config.notify_enabled,
            self.config.spawn_workers,
            self.config.auto_plan_workers,
            self.config.model,
        )
        self.config.notify_enabled = True
        self.config.spawn_workers = True
        self.config.auto_plan_workers = True
        self.config.model = "probe/noop"
        try:
            with mock.patch(
                "gitea_bots.notify.shutil.which",
                return_value="/usr/bin/opencode",
            ), mock.patch(
                "gitea_bots.notify.subprocess.Popen",
                side_effect=AssertionError(
                    "the e2e probe must never start an OpenCode process"
                ),
            ), mock.patch(
                "gitea_bots.notify.subprocess.run",
                side_effect=AssertionError(
                    "the e2e probe must never invoke the OpenCode CLI"
                ),
            ):
                notifier.notify(
                    self.poll1_event_count,
                    events_text=self.poll1_text,
                    self_login=self.config.self_login,
                    shared_login=True,
                )
        finally:
            (
                self.config.notify_enabled,
                self.config.spawn_workers,
                self.config.auto_plan_workers,
                self.config.model,
            ) = saved

        titles = [entry["title"] for entry in notifier.spawned]
        scenario.checks.append(
            Check(
                "no OpenCode process was started",
                True,
                "recording notifier only",
            )
        )
        scenario.checks.append(
            Check(
                "batch reporter recorded",
                any("event(s)" in title for title in titles),
                "; ".join(titles) or "nothing recorded",
            )
        )
        scenario.checks.append(
            Check(
                "reply worker recorded for E",
                any(
                    f"reply work for #{self.fixtures['E']}" in title
                    for title in titles
                ),
                "; ".join(titles) or "nothing recorded",
            )
        )
        scenario.checks.append(
            Check(
                "reply worker recorded for I (issue-body mention)",
                any(
                    f"reply work for #{self.fixtures['I']}" in title
                    for title in titles
                ),
                "; ".join(titles) or "nothing recorded",
            )
        )
        scenario.checks.append(
            Check(
                "AUTO PLAN worker recorded for A",
                any(
                    "auto-plan work for" in title
                    and f"#{self.fixtures['A']}" in title
                    for title in titles
                ),
                "; ".join(titles) or "nothing recorded",
            )
        )
        for key in ("A", "E", "I"):
            scenario.checks.append(
                Check(
                    f"case {key} shows the seeing pickup label",
                    self._has_status_label(self.fixtures[key], "seeing"),
                    ", ".join(sorted(self._status_labels(self.fixtures[key]))),
                )
            )

    # --------------------------------------------------------------------- labels
    def _labels_case(self) -> None:
        scenario = self._add("labels", "real status label reconcile")
        expected = {
            "A": {"seeing"},
            "B": set(),
            "C": set(),
            "D": {"responded"},
            "E": {"responded"},
            "F": {"responded", "committed"},
            "H": set(),
            "I": {"seeing"},
        }
        for key, want in expected.items():
            number = self.fixtures[key]
            changed, text, derived = self._status.reconcile(number)
            actual = self._status_labels(number)
            scenario.checks.append(
                Check(
                    f"case {key} derived labels",
                    set(derived) == want,
                    f"derived={sorted(derived)}, expected={sorted(want)}",
                )
            )
            scenario.checks.append(
                Check(
                    f"case {key} repository labels",
                    actual == want,
                    f"actual={sorted(actual)}, expected={sorted(want)}",
                )
            )
            scenario.notes.append(f"#{number} {text}{' (changed)' if changed else ''}")

    # -------------------------------------------------------------------- cleanup
    def _cleanup(self) -> None:
        scenario = self._scenario("cleanup", "cleanup")
        if not self.created:
            scenario.notes.append("no fixture issue was created")
            self.cleaned_up = not self.leftover_urls
            return

        deleted = 0
        for number in reversed(self.created):
            try:
                self.client.delete(self._issue_path(number))
            except GiteaError as exc:
                if DELETE_FORBIDDEN in str(exc):
                    self._neutralise(scenario, number, exc)
                else:
                    url = self._issue_url(number)
                    self.leftover_urls.append(url)
                    scenario.checks.append(
                        Check(f"#{number} deleted", False, f"{exc}; leftover {url}")
                    )
                continue
            deleted += 1
            scenario.checks.append(Check(f"#{number} deleted", True))
            self._verify_gone(scenario, number)

        self._delete_new_labels(scenario)
        self._scan_leftovers(scenario)
        self.cleaned_up = deleted == len(self.created) and not self.leftover_urls

    def _verify_gone(self, scenario: ScenarioResult, number: int) -> None:
        try:
            data = self.client.get(self._issue_path(number))
        except GiteaError as exc:
            scenario.checks.append(
                Check(
                    f"#{number} GET answers 404",
                    NOT_FOUND in str(exc),
                    str(exc),
                )
            )
            return
        scenario.checks.append(
            Check(
                f"#{number} GET answers 404",
                False,
                f"issue is still readable: {data!r}",
            )
        )

    def _neutralise(self, scenario: ScenarioResult, number: int, exc) -> None:
        """Delete was forbidden: clear labels/assignees, keep it closed, fail."""
        url = self._issue_url(number)
        self.leftover_urls.append(url)
        try:
            self.client.put(
                self._issue_path(number, "/labels"), payload={"labels": []}
            )
            self.client.patch(
                self._issue_path(number),
                payload={"assignees": [], "state": "closed"},
            )
        except GiteaError as fallback_exc:
            scenario.checks.append(
                Check(
                    f"#{number} neutralised after a forbidden delete",
                    False,
                    f"{fallback_exc}; leftover {url}",
                )
            )
            return
        scenario.checks.append(
            Check(
                f"#{number} neutralised after a forbidden delete",
                False,
                f"{exc}; labels/assignees cleared and issue closed, "
                f"leftover {url}",
            )
        )
        self._verify_neutralised(scenario, number, url)

    def _verify_neutralised(self, scenario, number: int, url: str) -> None:
        try:
            issue = self._get_issue(number)
        except GiteaError as exc:
            scenario.checks.append(
                Check(
                    f"#{number} neutralised state readable",
                    False,
                    f"{exc}; leftover {url}",
                )
            )
            return
        labels = issue.get("labels") or []
        assignees = issue.get("assignees") or []
        scenario.checks.append(
            Check(
                f"#{number} neutralised state verified",
                issue.get("state") == "closed" and not labels and not assignees,
                f"state={issue.get('state')!r}, labels={len(labels)}, "
                f"assignees={len(assignees)}, leftover {url}",
            )
        )

    def _delete_new_labels(self, scenario: ScenarioResult) -> None:
        if not self.label_snapshot_ok:
            scenario.notes.append("repository label snapshot unavailable")
            return
        try:
            labels = self.client.get_all(
                f"/repos/{self.config.repo}/labels"
            )
        except GiteaError as exc:
            scenario.checks.append(
                Check("new repository labels cleaned up", False, str(exc))
            )
            return
        failed = []
        for label in labels:
            if not isinstance(label, dict):
                continue
            label_id = label.get("id")
            if not isinstance(label_id, int) or label_id in self.label_ids_before:
                continue
            try:
                self.client.delete(
                    f"/repos/{self.config.repo}/labels/{label_id}"
                )
            except GiteaError as exc:
                failed.append(f"{label.get('name')!r}: {exc}")
        scenario.checks.append(
            Check(
                "new repository labels cleaned up",
                not failed,
                "; ".join(failed),
            )
        )

    def _scan_leftovers(self, scenario: ScenarioResult) -> None:
        survivors = []
        try:
            issues = self.client.get_all(
                f"/repos/{self.config.repo}/issues",
                {"state": "all", "sort": "updated", "direction": "desc"},
            )
        except GiteaError as exc:
            scenario.checks.append(
                Check("leftover scan completed", False, str(exc))
            )
            return
        for issue in issues:
            if not isinstance(issue, dict):
                continue
            title = issue.get("title") or ""
            if title.startswith(TITLE_MARKER) and self.run_id in title:
                survivors.append(self._issue_url(issue.get("number")))
        for url in survivors:
            if url not in self.leftover_urls:
                self.leftover_urls.append(url)
        scenario.checks.append(
            Check(
                "no probe issue marker survives",
                not survivors,
                ", ".join(survivors) or "none",
            )
        )

    # -------------------------------------------------------------------- helpers
    def _get_issue(self, number: int) -> dict:
        data = self.client.get(self._issue_path(number))
        return data if isinstance(data, dict) else {}

    def _status_labels(self, number: int) -> set[str]:
        issue = self._get_issue(number)
        names = {
            label.get("name")
            for label in issue.get("labels") or []
            if isinstance(label, dict)
        }
        return {status for status in (label_status(name) for name in names) if status}

    def _has_status_label(self, number: int, status: str) -> bool:
        return status in self._status_labels(number)

    def _issue_url(self, number) -> str:
        base = (self.config.api_base or "").rstrip("/")
        if base.endswith("/api/v1"):
            base = base[: -len("/api/v1")]
        if not base:
            return f"#{number}"
        return f"{base}/{self.config.repo}/issues/{number}"


def build_e2e_probe(config, logger) -> E2EProbe:
    """Apply the probe overrides and wire an :class:`E2EProbe`."""
    production_state_dir = config.state_dir
    config.state_dir = (config.repo_root / PROBE_STATE).resolve()
    config.notify_enabled = False
    config.spawn_workers = False
    config.auto_plan_workers = False
    config.start_reconcile = False
    config.all_comments = False
    config.verbose = False
    config.notify_in_foreground = False
    # The probe exists to exercise AUTO PLAN and shared-login mentions, so
    # force those predicates on even when the shell disabled them.
    config.auto_plan = True
    config.shared_login = True

    client = GiteaClient(
        config.api_base, config.token, timeout=config.api_timeout
    )
    login = client.resolve_login()
    if not login:
        logger.die("could not resolve the token user for the end-to-end probe")
    config.self_login = login
    return E2EProbe(
        config,
        client,
        logger,
        production_state_dir=production_state_dir,
    )
