"""The polling watcher — read-only event surfacing plus command verbs.

It *surfaces* events (new/updated issues, directed human replies, AUTO PLAN /
REASSIGNED signals). It never creates, assigns, comments on, or closes
anything and never advances an issue's work. Its only writes are status labels:
the startup reconcile, plus — through the notifier — the pickup ``seeing``
label applied before a dispatched background worker starts (#144), both
delegated in-process to :class:`gitea_bots.status.StatusManager`.

The behaviour was ported from the retired ``watch-issues.sh``. Durable
publishing happens before ``state.db``'s ``last_poll`` advances; notification is
post-lock and best-effort. Issue reads (comments, timeline) are prefetched concurrently with
a bounded pool (#138); classification, event emission and every state/inbox
write stay serial in the main thread.
"""

from __future__ import annotations

import os
import re
import signal
import sqlite3
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from pathlib import Path

from . import EVENTS_VERSION
from .clock import (
    now_utc,
    shift_iso,
    timestamp_in_window,
    to_epoch,
)
from .errors import BotFatal, GiteaError
from .identity import Fingerprint
from .locks import FileLock
from .models import Issue
from .rules import (
    body_mention_digest,
    body_mention_paragraphs,
    classify_comment,
    comments_have_stage2_marker,
    label_status,
)


NOTIFICATION_MODEL_RE = re.compile(r"notification model: (\S+)")


@dataclass
class IssueReads:
    """Concurrently prefetched per-issue reads (#138).

    ``error`` keeps the existing semantics: a failed comment fetch marks the
    poll as errored so ``last_poll`` is not advanced and the events retry.
    """

    comments: list = field(default_factory=list)
    timeline: list | None = None
    error: bool = False


@dataclass
class PollResult:
    ok: bool = True
    scanned: int = 0
    event_count: int = 0
    events_text: str = ""
    notify: bool = False


class EventBuffer:
    """Collects this poll's event lines for both display and durable publish."""

    def __init__(self):
        self.lines: list = []
        self.count = 0

    def emit(self, line: str, output) -> None:
        text = f"{now_utc()} {line}"
        self.lines.append(text)
        output(text)
        self.count += 1

    def body(self, text: str, output) -> None:
        for line in (text or "").splitlines():
            indented = "    | " + line
            self.lines.append(indented)
            output(indented)

    def text(self) -> str:
        return "\n".join(self.lines) + ("\n" if self.lines else "")


class Watcher:
    def __init__(self, config, client, state, inbox, notifier, status, logger):
        self.config = config
        self.client = client
        self.state = state
        self.inbox = inbox
        self.notifier = notifier
        # Named ``status_manager`` so it cannot shadow the ``status()`` command
        # verb below (instance attributes take precedence over class methods).
        self.status_manager = status
        self.log = logger
        self.output = logger.event
        self._poll_lock = FileLock(config.poll_lock_file)
        self._events = EventBuffer()
        # The last drift signature we logged, so a persistent drift warns once
        # per distinct change instead of every poll (#152).
        self._warned_drift = None

    # ------------------------------------------------------------- fetch reads
    def _fetch_comments(self, num):
        raw = self.client.get_all(
            f"/repos/{self.config.repo}/issues/{num}/comments"
        )
        return sorted(
            (c for c in raw if isinstance(c, dict)),
            key=lambda c: int(c.get("id") or 0),
        )

    def _fetch_timeline(self, num):
        raw = self.client.get_all(
            f"/repos/{self.config.repo}/issues/{num}/timeline"
        )
        return sorted(
            (t for t in raw if isinstance(t, dict)),
            key=lambda t: int(t.get("id") or 0),
        )

    def _fetch_issues(self, params):
        raw = self.client.get_all(
            f"/repos/{self.config.repo}/issues", params=params
        )
        return [Issue.from_api(d) for d in raw if isinstance(d, dict)]

    def _fetch_open_issues(self):
        return self._fetch_issues(
            {"state": "open", "sort": "updated", "direction": "desc"}
        )

    # -------------------------------------------------------- concurrent reads
    def _fetch_issue_data(self, issue, baseline) -> IssueReads:
        """Fetch one issue's comments (and timeline when incremental)."""
        try:
            comments = self._fetch_comments(issue.number)
        except GiteaError:
            return IssueReads(error=True)
        timeline = None
        if not baseline and self.config.self_login:
            try:
                timeline = self._fetch_timeline(issue.number)
            except GiteaError:
                # Existing fallback: compare the assignee sets instead.
                timeline = None
        return IssueReads(comments=comments, timeline=timeline)

    def _prefetch_issue_data(self, issues, baseline) -> dict:
        """Map :meth:`_fetch_issue_data` over the issues with bounded concurrency.

        Only the network reads run in the pool. Classification, event emission,
        inbox publishing and ``state.db`` writes remain serial in the main
        thread (single-writer guarantee for ``pending.md`` / ``events``).
        """
        issues = [issue for issue in issues if isinstance(issue.number, int)]
        if self.config.max_concurrency <= 1 or len(issues) <= 1:
            return {
                issue.number: self._fetch_issue_data(issue, baseline)
                for issue in issues
            }
        workers = min(self.config.max_concurrency, len(issues))
        reads = {}
        with ThreadPoolExecutor(max_workers=workers) as pool:
            futures = {
                pool.submit(self._fetch_issue_data, issue, baseline): issue.number
                for issue in issues
            }
            for future in as_completed(futures):
                reads[futures[future]] = future.result()
        return reads

    # ------------------------------------------------------------- predicates
    def _auto_plan_eligible(self, issue) -> bool:
        """Whether the bot is expected to plan this issue (#150).

        Exactly two things start planning: the issue is assigned to the token
        user, or a human reply at-mentions the bot login and explicitly asks
        for a plan. The second is a DIRECTED COMMENT answered by a reply
        worker and never passes through this predicate. An unassigned issue is
        never auto-planned, even when the token user authored it: in
        shared-login mode every issue is self-authored, so authorship proves
        nothing.
        """
        login = self.config.self_login
        return bool(login) and issue.assigned_to(login)

    def _auto_plan_candidate(self, issue, auto_plan_at, comments) -> bool:
        """Open Summary-only issue the bot is expected to plan, not yet planned.

        Stage 3 is never inferred from this. Emitted at most once via
        ``auto_plan_at`` in state.
        """
        if not self.config.auto_plan or auto_plan_at:
            return False
        if issue.state != "open" or issue.has_tasks or not issue.has_summary:
            return False
        if not self._auto_plan_eligible(issue):
            return False
        return not comments_have_stage2_marker(comments)

    # ------------------------------------------------------------- process one
    def _process_issue(self, baseline, report_existing, issue, prev,
                       window_start, window_epoch, catchup, reads=None):
        """Return ``(snapshot, error)``; appends any events to ``self._events``."""
        num = issue.number
        if not isinstance(num, int):
            self.log.warn(f"unexpected issue number in API response: '{num}'")
            return None, True

        had_prev = prev is not None
        p = prev or {}
        prev_updated = p.get("updated_at", "")
        prev_title = p.get("title", "")
        prev_state = p.get("state", "")
        prev_labels = p.get("labels", "")
        prev_assignees = p.get("assignees", "")
        prev_milestone = p.get("milestone", "")
        prev_last_id = int(p.get("last_comment_id") or 0)
        prev_auto_plan_at = p.get("auto_plan_at", "") or ""
        prev_last_assignee_event_id = int(p.get("last_assignee_event_id") or 0)
        prev_body_mention = p.get("body_mention_sha", "") or ""

        data = reads if reads is not None else self._fetch_issue_data(issue, baseline)
        if data.error:
            self.log.warn(f"comment fetch failed for issue #{num} — will retry next poll")
            return None, True
        comments = data.comments

        last_id = max((int(c.get("id") or 0) for c in comments), default=0)
        if had_prev and last_id < prev_last_id:
            last_id = prev_last_id  # comment IDs are monotonic

        # --- comments that are new for this issue (incremental polls only) ----
        new_comments = []
        if not baseline:
            for comment in comments:
                cid = int(comment.get("id") or 0)
                created = comment.get("created_at") or ""
                if had_prev and cid <= prev_last_id:
                    # Already consumed; a state-schema upgrade re-surfaces
                    # recent ones the old version could not deliver.
                    if catchup and timestamp_in_window(created, window_start, window_epoch):
                        new_comments.append(comment)
                    continue
                if not had_prev and not timestamp_in_window(created, window_start, window_epoch):
                    continue
                new_comments.append(comment)

        # --- re-assignment via timeline (#119) -------------------------------
        auto_plan_at = prev_auto_plan_at
        auto_plan_emitted = False
        reassign_emitted = False
        last_assignee_event_id = prev_last_assignee_event_id
        issue_planned = issue.has_tasks or comments_have_stage2_marker(comments)

        if not baseline and self.config.self_login:
            timeline = data.timeline
            if timeline is None:
                self.log.verbose(
                    f"timeline fetch failed for issue #{num} — falling back to "
                    "assignee-set comparison"
                )
            if timeline is not None:
                timeline_max = max((int(t.get("id") or 0) for t in timeline), default=0)
                if had_prev:
                    for event in timeline:
                        tid = int(event.get("id") or 0)
                        if event.get("type") != "assignees" or tid <= 0:
                            continue
                        if tid <= prev_last_assignee_event_id:
                            continue
                        if event.get("removed_assignee") in (True, "true"):
                            continue
                        login = (event.get("assignee") or {}).get("login", "")
                        if login != self.config.self_login:
                            continue
                        if issue_planned:
                            self._events.emit(
                                f"REASSIGNED #{num} \"{issue.title}\" by "
                                f"{issue.author} — {issue.html_url}", self.output)
                            reassign_emitted = True
                        elif not auto_plan_at and self._auto_plan_candidate(issue, "", comments):
                            self._events.emit(
                                f"AUTO PLAN #{num} \"{issue.title}\" by "
                                f"{issue.author} — {issue.html_url}", self.output)
                            auto_plan_at = now_utc()
                            auto_plan_emitted = True
                    # Publish-before-advance: the cursor moves only after the
                    # events above are queued for publishing.
                    if timeline_max > prev_last_assignee_event_id:
                        last_assignee_event_id = timeline_max
                else:
                    # First sight: seed to current max, never replay history.
                    last_assignee_event_id = timeline_max
        elif baseline:
            last_assignee_event_id = prev_last_assignee_event_id

        # --- shape-based AUTO PLAN (#113/#115/#118) --------------------------
        if (not auto_plan_emitted and not reassign_emitted
                and self._auto_plan_candidate(issue, auto_plan_at, comments)):
            if not baseline or report_existing >= 1:
                self._events.emit(
                    f"AUTO PLAN #{num} \"{issue.title}\" by {issue.author} — "
                    f"{issue.html_url}", self.output)
                auto_plan_at = now_utc()
                auto_plan_emitted = True

        # --- at-mention in the issue body (#185) ------------------------------
        # Bodies have no monotonic ID, so the cursor is a digest of the mention
        # paragraphs only. A `sync-tasks.py` rewrite or any unrelated body edit
        # therefore does not re-emit the request.
        body_paragraphs = body_mention_paragraphs(
            issue.author, issue.body, self.config.self_login,
            shared_login=self.config.shared_login,
        )
        body_sha = body_mention_digest(body_paragraphs)
        body_mention_emitted = False
        if body_paragraphs and body_sha != prev_body_mention:
            if baseline:
                # A fresh start reports existing issues, a silent baseline only
                # records the digest for later polls.
                allowed = report_existing >= 1
            elif had_prev:
                allowed = True
            else:
                # Mirror NEW ISSUE: an old untracked issue must not replay an
                # ancient request the first time it enters the window.
                allowed = timestamp_in_window(
                    issue.created_at, window_start, window_epoch)
            if allowed:
                self._events.emit(
                    f"DIRECTED COMMENT on #{num} (issue body) by "
                    f"{issue.author} — {issue.html_url}", self.output)
                self._events.body("\n\n".join(body_paragraphs), self.output)
                body_mention_emitted = True

        # --- issue-level event ----------------------------------------------
        if baseline:
            if report_existing >= 1 and not auto_plan_emitted:
                self._events.emit(
                    f"NEW ISSUE #{num} [{issue.state}] \"{issue.title}\" by "
                    f"{issue.author} — {issue.html_url}", self.output)
        elif had_prev:
            changed = []
            if issue.title != prev_title:
                changed.append("title")
            if issue.state != prev_state:
                changed.append("state")
            if issue.labels_str != prev_labels:
                changed.append("labels")
            if issue.assignees_str != prev_assignees:
                changed.append("assignees")
            if issue.milestone != prev_milestone:
                changed.append("milestone")
            if changed:
                if not auto_plan_emitted and not reassign_emitted:
                    self._events.emit(
                        f"UPDATED ISSUE #{num} — changed: {' '.join(changed)} — "
                        f"{issue.html_url}", self.output)
            elif (issue.updated_at != prev_updated and not new_comments
                  and not auto_plan_emitted and not reassign_emitted
                  and not body_mention_emitted):
                # updated_at moved but no tracked field or new comment explains
                # it (typically an issue-body edit): noise, not a request —
                # unless the edit carried a real at-mention (already emitted).
                self._events.emit(
                    f"UPDATED ISSUE #{num} — changed: body/other — "
                    f"{issue.html_url}", self.output)
        else:
            # First time this issue is seen after baseline.
            if not auto_plan_emitted and not reassign_emitted:
                if timestamp_in_window(issue.created_at, window_start, window_epoch):
                    self._events.emit(
                        f"NEW ISSUE #{num} \"{issue.title}\" by {issue.author} — "
                        f"{issue.html_url}", self.output)
                else:
                    self._events.emit(
                        f"UPDATED ISSUE #{num} \"{issue.title}\" (first seen — "
                        f"previously untracked) — {issue.html_url}", self.output)

        # --- comment-level events -------------------------------------------
        for comment in new_comments:
            cid = int(comment.get("id") or 0)
            author = (comment.get("user") or {}).get("login") or "unknown"
            body = comment.get("body") or ""
            url = comment.get("html_url") or issue.html_url
            tag = " (catch-up)" if (had_prev and cid <= prev_last_id) else ""
            kind = classify_comment(
                author, body,
                self_login=self.config.self_login,
                all_comments=self.config.all_comments,
                shared_login=self.config.shared_login,
            )
            if not kind:
                continue
            self._events.emit(
                f"{kind}{tag} on #{num} (comment {cid}) by {author} — {url}",
                self.output)
            self._events.body(body, self.output)

        snapshot = {
            "updated_at": issue.updated_at,
            "title": issue.title,
            "state": issue.state,
            "labels": issue.labels_str,
            "assignees": issue.assignees_str,
            "milestone": issue.milestone,
            "last_comment_id": last_id,
            "last_assignee_event_id": int(last_assignee_event_id or 0),
            "body_mention_sha": body_sha,
        }
        if auto_plan_at:
            snapshot["auto_plan_at"] = auto_plan_at
        return snapshot, False

    # ------------------------------------------------------------------ polling
    def _poll_body(self, report_existing, notify_armed) -> PollResult:
        self.inbox.ensure_dirs()
        state = self.state.load_or_init()
        baseline = self.state.is_baseline(state)
        catchup = self.state.version(state) < EVENTS_VERSION
        now = now_utc()
        baseline_state = "all" if report_existing >= 2 else "open"
        self._events = EventBuffer()

        since = ""
        issues = []
        try:
            if baseline:
                if report_existing >= 1:
                    self.log.verbose(
                        f"baseline poll: reporting existing {baseline_state} issues")
                else:
                    self.log.verbose("baseline poll: recording existing issues without events")
                issues = self._fetch_issues({
                    "state": baseline_state,
                    "sort": "updated", "direction": "desc",
                })
            else:
                last_poll = state.get("last_poll") or ""
                since = shift_iso(last_poll, -120) or ""  # 120s overlap absorbs skew
                if catchup:
                    self.log.verbose(
                        f"state schema is older than {EVENTS_VERSION} — catch-up "
                        f"scan over the last {self.config.catchup_minutes}min")
                if not since:
                    self.log.warn(
                        f"state last_poll is invalid ('{last_poll}') — re-baselining")
                    baseline = True
                    issues = self._fetch_issues({
                        "state": baseline_state,
                        "sort": "updated", "direction": "desc",
                    })
                else:
                    if catchup:
                        since = (shift_iso(now, -(self.config.catchup_minutes * 60))
                                 or "1970-01-01T00:00:00Z")
                    issues = self._fetch_issues({
                        "state": "all", "sort": "updated",
                        "direction": "asc", "since": since,
                    })
        except GiteaError as exc:
            self.log.warn(
                f"{'baseline' if baseline else 'incremental'} issue fetch failed "
                f"(api: {self.config.api_base}, repo: {self.config.repo}): {exc}")
            return PollResult(ok=False)

        window_start = since
        window_epoch = to_epoch(since) if since else 0.0

        issues = [issue for issue in issues if not issue.is_pull_request()]
        scanned = len(issues)
        updates = {}
        had_error = False
        issues_map = state.get("issues", {})
        # Bounded concurrent comment/timeline reads; decisions and writes stay
        # serial below (#138).
        reads = self._prefetch_issue_data(issues, baseline)
        for issue in issues:
            key = str(issue.number)
            prev = issues_map.get(key) if key in issues_map else None
            snapshot, err = self._process_issue(
                baseline, report_existing, issue, prev,
                window_start, window_epoch, catchup,
                reads.get(issue.number),
            )
            if err:
                had_error = True
                continue
            updates[key] = snapshot

        event_count = self._events.count
        events_text = self._events.text()
        # Publish before advancing state: an event must never be consumed by
        # the state file without having reached pending.md first.
        pub = self.inbox.publish(
            events_text, event_count, baseline=baseline, notify_armed=notify_armed
        )

        advance = True
        if (not baseline) and had_error:
            advance = False
        if not pub["ok"]:
            advance = False

        error = None
        if not advance:
            reasons = []
            if had_error:
                reasons.append("issue read failed")
            if not pub["ok"]:
                reasons.append("event publish failed")
            error = "; ".join(reasons)
        try:
            self.state.commit_poll(
                updates,
                last_poll=now if advance else None,
                version=EVENTS_VERSION,
                scanned=scanned,
                emitted=event_count,
                ok=advance,
                error=error,
            )
        except sqlite3.Error as exc:
            self.log.warn(f"failed to commit poll state: {exc}")
            return PollResult(ok=False, scanned=scanned,
                              event_count=event_count, events_text=events_text)

        return PollResult(ok=True, scanned=scanned, event_count=event_count,
                          events_text=events_text, notify=pub["notify"])

    def poll_once(self, report_existing, notify_armed):
        if not self._poll_lock.acquire():
            self.log.warn(
                f"another poll is already running (lock: {self._poll_lock.path})")
            return False, PollResult(ok=False)
        try:
            result = self._poll_body(report_existing, notify_armed)
        finally:
            self._poll_lock.release()
        # Notification is best-effort and post-lock: it must never hold or
        # inherit poll.lock (issue #116/#122). The batch text + bot identity
        # let the notifier decide whether a human reply needs a pinned
        # reply worker (#131).
        if result.notify:
            self.notifier.notify(
                result.event_count,
                events_text=result.events_text,
                self_login=self.config.self_login,
                shared_login=self.config.shared_login,
            )
        return result.ok, result

    # ---------------------------------------------------------------- commands
    def check(self, report_existing) -> int:
        ok, result = self.poll_once(report_existing, self.config.notify_in_foreground)
        if ok:
            self.log.log(
                f"check complete — repo {self.config.repo}, scanned "
                f"{result.scanned} issue(s), emitted {result.event_count} event(s)")
            return 0
        self.log.warn("check failed — see the warnings above")
        return 1

    def startup_label_reconcile(self) -> None:
        """Repair drifted status labels on open issues before the first poll.

        Scope: every open issue assigned to the token user, plus any open issue
        already carrying a status label. Unrelated labels/backlog are untouched.
        This is one of the watcher's two writes (the other is the notifier's
        pickup ``seeing`` label, #144); a failure is a WARN, never fatal.
        """
        if not self.config.start_reconcile:
            self.log.verbose("startup label reconcile: disabled")
            return
        self.log.verbose(
            "startup label reconcile: reconciling open issues assigned to "
            f"{self.config.self_login or '?'} or already labelled"
        )
        checked = changed = failed = 0
        try:
            issues = self._fetch_open_issues()
        except GiteaError as exc:
            self.log.warn(f"startup label reconcile: issue list fetch failed: {exc}")
            issues = []
        for issue in issues:
            if issue.is_pull_request():
                continue
            assigned = (issue.assigned_to(self.config.self_login)
                        if self.config.self_login else False)
            has_status = any(label_status(name) for name in issue.labels)
            if not (assigned or has_status):
                continue
            checked += 1
            try:
                was_changed, text, _ = self.status_manager.reconcile(issue.number)
            except GiteaError as exc:
                failed += 1
                self.log.warn(f"startup label reconcile: #{issue.number} failed — {exc}")
                continue
            if was_changed:
                changed += 1
                self.log.log(f"startup label reconcile: {text}")
        self.log.log(
            f"startup label reconcile: checked {checked} issue(s), "
            f"updated {changed}, failed {failed}"
        )

    def catch_up_assigned(self, label="catch-up", notify_armed=False) -> None:
        """After the first successful poll, scan the full open set (no window)
        and emit AUTO PLAN once for every plan-eligible, unplanned Summary-only
        issue. Catches issues that predate the watcher or arrived while down.
        """
        if not self.config.auto_plan:
            self.log.verbose(f"{label}: auto-plan disabled")
            return
        if not self.config.self_login:
            self.log.warn(f"{label}: skipped — SELF_LOGIN unresolved (set GITEA_BOTS_SELF)")
            return
        self.inbox.ensure_dirs()
        state = self.state.load_or_init()
        self._events = EventBuffer()
        self.log.verbose(
            f"{label}: checking open plan-eligible issues for "
            f"{self.config.self_login} (full scan, no updated_at window)"
        )
        scanned = skipped = emitted = 0
        updates = {}
        try:
            issues = self._fetch_open_issues()
        except GiteaError as exc:
            self.log.warn(f"{label}: issue list fetch failed: {exc}")
            issues = []
        issues_map = state.get("issues", {})
        candidates = [
            issue for issue in issues
            if not issue.is_pull_request()
            and self._auto_plan_eligible(issue)
        ]
        # Reuse the poll's bounded concurrent comment prefetch (#138).
        reads = self._prefetch_issue_data(candidates, baseline=True)
        for issue in candidates:
            scanned += 1
            prev = issues_map.get(str(issue.number)) or {}
            auto_plan_at = prev.get("auto_plan_at", "") or ""
            last_id = int(prev.get("last_comment_id") or 0)
            last_assignee_event_id = int(prev.get("last_assignee_event_id") or 0)
            data = reads.get(issue.number)
            if data is None or data.error:
                self.log.warn(f"{label}: comment fetch failed for #{issue.number} — skipping")
                skipped += 1
                continue
            comments = data.comments
            if not self._auto_plan_candidate(issue, auto_plan_at, comments):
                if (issue.state == "open" and issue.has_tasks and not auto_plan_at
                        and not comments_have_stage2_marker(comments)):
                    self.log.warn(
                        f"{label}: #{issue.number} has an unmarked plan "
                        "(## Tasks without a stage=2 marker) — not queued; post "
                        "the plan comment with the marker"
                    )
                skipped += 1
                continue
            # Seed the timeline cursor so the next poll does not replay
            # historical assignee events for this newly tracked issue.
            if last_assignee_event_id == 0:
                try:
                    timeline = self._fetch_timeline(issue.number)
                    last_assignee_event_id = max(
                        (int(t.get("id") or 0) for t in timeline), default=0
                    )
                except GiteaError:
                    pass
            self._events.emit(
                f"AUTO PLAN #{issue.number} \"{issue.title}\" by {issue.author} "
                f"— {issue.html_url}", self.output)
            emitted += 1
            updates[str(issue.number)] = {
                "updated_at": issue.updated_at or now_utc(),
                "title": issue.title,
                "state": issue.state,
                "labels": issue.labels_str,
                "assignees": issue.assignees_str,
                "milestone": issue.milestone,
                "last_comment_id": last_id,
                "auto_plan_at": now_utc(),
                "last_assignee_event_id": int(last_assignee_event_id or 0),
                "body_mention_sha": prev.get("body_mention_sha", "") or "",
            }

        event_count = self._events.count
        if event_count > 0:
            pub = self.inbox.publish(
                self._events.text(), event_count,
                baseline=False, notify_armed=notify_armed,
            )
            try:
                if pub["ok"]:
                    self.state.commit_issues(updates)
                else:
                    self.log.warn(
                        f"{label}: event publish failed — not advancing state"
                    )
            except sqlite3.Error as exc:
                self.log.warn(f"{label}: could not commit state: {exc}")
            if pub["notify"]:
                self.notifier.notify(
                    event_count,
                    events_text=self._events.text(),
                    self_login=self.config.self_login,
                    shared_login=self.config.shared_login,
                )
            self.log.log(
                f"{label}: emitted {emitted} AUTO PLAN event(s) "
                f"(eligible: {scanned}, skipped: {skipped})"
            )
        else:
            self.log.log(
                f"{label}: no candidates (eligible: {scanned}, "
                f"skipped: {skipped})"
            )

    def scan_unplanned(self) -> int:
        """Diagnostic: list plan-eligible, unplanned Summary-only issues."""
        self.inbox.ensure_dirs()
        if not self.config.self_login:
            self.log.die("cannot resolve the token user — set GITEA_BOTS_SELF")
        found = 0
        try:
            issues = self._fetch_open_issues()
        except GiteaError as exc:
            self.log.warn(f"issue list fetch failed: {exc}")
            return 1
        for issue in issues:
            if issue.is_pull_request() or not self._auto_plan_eligible(issue):
                continue
            if not issue.has_tasks and not issue.has_summary:
                continue
            try:
                comments = self._fetch_comments(issue.number)
            except GiteaError:
                self.log.warn(f"comment fetch failed for issue #{issue.number} — skipping")
                continue
            if issue.has_tasks:
                if not comments_have_stage2_marker(comments):
                    self.log.warn(
                        f"scan-unplanned: #{issue.number} has a body plan "
                        "(## Tasks) with no stage=2 marker — not an AUTO PLAN "
                        "candidate; post the plan comment with the marker"
                    )
                continue
            if comments_have_stage2_marker(comments):
                continue
            self.output(f"AUTO PLAN #{issue.number} \"{issue.title}\" — {issue.html_url}")
            found += 1
        if found == 0:
            self.output(
                f"scan-unplanned: no unplanned plan-eligible issues for "
                f"{self.config.self_login}"
            )
        else:
            self.output(
                f"scan-unplanned: {found} unplanned plan-eligible issue(s) for "
                f"{self.config.self_login}"
            )
        return 0

    def loop(self, report_existing) -> None:
        self.inbox.ensure_dirs()
        self.config.state_dir.mkdir(parents=True, exist_ok=True)
        self.config.pid_file.write_text(f"{os.getpid()}\n", encoding="utf-8")
        self._write_meta()

        def _terminate(signum, frame):
            raise SystemExit(0)

        signal.signal(signal.SIGINT, _terminate)
        signal.signal(signal.SIGTERM, _terminate)

        self.log.log(
            f"watcher started (pid {os.getpid()}, repo {self.config.repo}, "
            f"api {self.config.api_base}, interval {self.config.interval}s)"
        )
        try:
            try:
                self.startup_label_reconcile()
            except Exception as exc:  # reconcile is best-effort, never fatal
                self.log.warn(f"startup label reconcile failed (continuing): {exc}")
            first_poll_done = False
            while True:
                ok, _ = self.poll_once(report_existing, notify_armed=True)
                # Re-check the startup fingerprint every poll: a merged default
                # or a GITEA_BOTS_* change is inactive until a restart (#152).
                self._check_fingerprint()
                if ok:
                    if self.config.verbose:
                        self.log.log("poll ok")
                    if not first_poll_done:
                        first_poll_done = True
                        try:
                            self.catch_up_assigned("startup auto-plan scan",
                                                   notify_armed=True)
                        except Exception as exc:
                            self.log.warn(f"catch-up failed (continuing): {exc}")
                else:
                    self.log.warn(f"poll failed — retrying in {self.config.interval}s")
                time.sleep(self.config.interval)
        finally:
            self._remove_pid()

    def start(self, report_existing) -> None:
        self.inbox.ensure_dirs()
        self.config.effective_log_file.parent.mkdir(parents=True, exist_ok=True)
        pid = ""
        if self.config.pid_file.is_file():
            pid = self.config.pid_file.read_text(encoding="utf-8").strip()
        if self._pid_is_watcher(pid):
            self.log.die(
                f"watcher already running (pid {pid}) — run "
                "'watch-issues.py stop' first"
            )
        self._clear_pid_files()

        args = ["loop", "--interval", str(self.config.interval)]
        if report_existing == 2:
            args.append("--report-existing-all")
        elif report_existing == 1:
            args.append("--report-existing")
        else:
            args.append("--no-report-existing")
        cmd = [sys.executable, "-u", str(self.config.script_path), *args]
        logf = open(self.config.effective_log_file, "ab", buffering=0)
        try:
            proc = subprocess.Popen(
                cmd, stdin=subprocess.DEVNULL, stdout=logf,
                stderr=subprocess.STDOUT, start_new_session=True,
                close_fds=True,
            )
        except OSError as exc:
            self.log.die(f"could not start watcher: {exc}")
        finally:
            logf.close()
        self.config.pid_file.write_text(f"{proc.pid}\n", encoding="utf-8")
        self.log.log(
            f"watcher started (pid {proc.pid}, interval {self.config.interval}s) "
            f"— log: {self.config.effective_log_file}"
        )

    def status(self) -> None:
        pid = ""
        if self.config.pid_file.is_file():
            pid = self.config.pid_file.read_text(encoding="utf-8").strip()
        running = self._pid_is_watcher(pid)
        meta = self._read_meta()
        if running:
            self.output(f"watcher: running (pid {pid})")
            if not self.config.meta_file.is_file():
                self.output(
                    "  stale-check: unavailable (no watcher.meta; restart the "
                    "watcher to record its fingerprint)"
                )
            else:
                drift = self._fingerprint_drift(meta)
                if drift is None:
                    self.output(
                        "  stale-check: unavailable (restart the watcher once "
                        "to record the fingerprint)"
                    )
                elif drift:
                    self.output(
                        f"  STALE: running watcher started "
                        f"{meta.get('started_at', '?')} — changed since start: "
                        f"{', '.join(drift)} — run 'stop' then 'start' to apply "
                        "(commit-only changes are ignored)"
                    )
            self.output(f"notification model: {self._reported_model(pid, meta)}")
        else:
            self.output("watcher: stopped")
            if pid:
                self.output(
                    f"  stale pid file: {self.config.pid_file} (run 'stop' to clear)"
                )
            self.output(f"notification model: {self._reported_model('', meta)}")

        self.output(f"state: {self.state.path}")
        try:
            data = self.state.load()
        except sqlite3.DatabaseError as exc:
            self.output(f"  (state database unreadable: {exc})")
            data = {}
        if data.get("initialized_at"):
            self.output(f"repo: {data.get('repo', '?')}")
            self.output(f"api: {data.get('api', '?')}")
            self.output(
                f"last_poll: {data.get('last_poll') or 'never (baseline pending)'}"
            )
            self.output(f"tracked issues: {len(data.get('issues', {}))}")
            self.output(f"pending batches: {len(self.inbox.pending_batches())}")
            self.output(f"events: {self.inbox.db.count_events()}")
            self.output(f"polls: {self.inbox.db.count_polls()}")
        else:
            self.output("  (not initialized — first poll will baseline)")
        self.output(f"log: {self.config.effective_log_file}")
        unread = self._count_unread()
        if unread > 0:
            self.output(
                f"pending events: {unread} unread — {self.inbox.pending_file} "
                "(run 'inbox')"
            )
        else:
            self.output("pending events: none")

    def stop(self) -> None:
        pid = ""
        if self.config.pid_file.is_file():
            pid = self.config.pid_file.read_text(encoding="utf-8").strip()
        if not pid:
            self.output("watcher: not running (no pid file)")
            self._clear_pid_files()
            return
        if not self._pid_is_watcher(pid):
            self.output(f"watcher: not running (stale pid {pid})")
            self._clear_pid_files()
            return
        try:
            os.kill(int(pid), signal.SIGTERM)
        except OSError:
            pass
        for _ in range(50):
            try:
                os.kill(int(pid), 0)
            except OSError:
                break
            time.sleep(0.1)
        else:
            self.log.warn(f"watcher (pid {pid}) ignored SIGTERM — sending SIGKILL")
            try:
                os.kill(int(pid), signal.SIGKILL)
            except OSError:
                pass
        self._clear_pid_files()
        self.output(f"watcher: stopped (pid {pid})")

    def reset(self) -> int:
        self.inbox.ensure_dirs()
        if not self._poll_lock.acquire():
            self.log.warn("a poll is in progress — stop the watcher or retry in a moment")
            return 1
        try:
            backup = self.state.reset()
            if backup:
                self.output(
                    f"state cleared — next poll re-baselines (backup: {backup})"
                )
            else:
                self.output("state already clear — next poll will baseline")
        finally:
            self._poll_lock.release()
        return 0

    # -------------------------------------------------------------- process id
    @staticmethod
    def _pid_is_watcher(pid) -> bool:
        """Guard against PID reuse by inspecting one specific /proc entry."""
        if not pid or not str(pid).isdigit():
            return False
        try:
            os.kill(int(pid), 0)
        except OSError:
            return False
        cmdline = Path(f"/proc/{pid}/cmdline")
        if cmdline.is_file():
            try:
                data = cmdline.read_bytes().replace(b"\0", b" ").decode("utf-8", "replace")
            except OSError:
                return False
            return "watch-issues" in data
        return True

    def _identity_files(self):
        files = []
        if self.config.script_path:
            files.append(Path(self.config.script_path))
        files.extend(sorted(Path(__file__).resolve().parent.glob("*.py")))
        return files

    def _script_identity(self):
        """Informational HEAD commit + max mtime; never a staleness input."""
        try:
            commit = subprocess.run(
                ["git", "-C", str(self.config.repo_root), "rev-parse", "--short", "HEAD"],
                capture_output=True, text=True, check=True,
            ).stdout.strip()
        except (subprocess.CalledProcessError, OSError):
            commit = "unknown"
        mtime = 0
        for path in self._identity_files():
            try:
                mtime = max(mtime, int(path.stat().st_mtime))
            except OSError:
                pass
        return commit, str(mtime), str(self.config.script_path)

    def _fingerprint_base_dir(self):
        if self.config.script_path:
            return Path(self.config.script_path).resolve().parent
        return None

    def _current_fingerprint(self) -> Fingerprint:
        return Fingerprint.capture(
            self._identity_files(),
            self.config,
            base_dir=self._fingerprint_base_dir(),
        )

    def _fingerprint_drift(self, meta) -> list | None:
        """Changed labels vs the recorded fingerprint; ``None`` if unavailable."""
        recorded = Fingerprint.from_json(meta.get("fingerprint", ""))
        if recorded is None:
            return None
        return recorded.diff(self._current_fingerprint())

    def _check_fingerprint(self) -> None:
        """Warn once per distinct drift; never restart (one-watcher rule)."""
        try:
            drift = self._fingerprint_drift(self._read_meta())
        except Exception as exc:  # diagnostic only, never fatal to the loop
            self.log.verbose(f"fingerprint check failed: {exc}")
            return
        if drift is None:
            return
        if not drift:
            self._warned_drift = None
            return
        signature = tuple(drift)
        if signature == self._warned_drift:
            self.log.verbose(
                "watcher config drift unchanged: " + ", ".join(drift))
            return
        self._warned_drift = signature
        self.log.warn(
            "watcher config drift: " + ", ".join(drift)
            + " changed since start — run 'stop' then 'start' to apply; "
            "not restarting automatically (one-watcher rule)")

    def _write_meta(self) -> None:
        commit, mtime, path = self._script_identity()
        fingerprint = self._current_fingerprint().to_json()
        try:
            model = self.notifier.resolve_model(timeout=5) or ""
        except Exception:  # diagnostic only, never block startup
            model = ""
        content = (
            f"pid={os.getpid()}\nstarted_at={now_utc()}\n"
            f"script_commit={commit}\nscript_mtime={mtime}\nscript_path={path}\n"
            f"model={model}\nfingerprint={fingerprint}\n"
        )
        try:
            self.config.meta_file.write_text(content, encoding="utf-8")
        except OSError:
            pass

    def _read_meta(self) -> dict:
        meta = {}
        try:
            for line in self.config.meta_file.read_text(encoding="utf-8").splitlines():
                if "=" in line:
                    key, value = line.split("=", 1)
                    meta[key.strip()] = value.strip()
        except OSError:
            pass
        return meta

    # ------------------------------------------------------------ status: model
    def _reported_model(self, pid: str, meta: dict) -> str:
        """Best-effort notification model for ``status`` output.

        Order: the model recorded in ``watcher.meta`` at start, the explicit
        ``GITEA_BOTS_MODEL``, the running watcher's inherited launching session,
        then the last ``notification model:`` line in ``watcher.log``. Never
        prints the token; ``unresolved`` means no model-bearing session was
        available (spawned sessions would be refused).
        """
        recorded = (meta.get("model") or "").strip()
        if recorded:
            return recorded
        override = (self.config.model or "").strip()
        if override:
            return override
        if pid:
            model = self._model_from_pid_env(pid)
            if model:
                return model
        logged = self._last_logged_model()
        if logged:
            return logged
        return (
            "unresolved (set GITEA_BOTS_MODEL or start the watcher from an "
            "OpenCode session)"
        )

    def _model_from_pid_env(self, pid: str) -> str:
        """Read the launching session id from a running watcher's environment."""
        try:
            raw = Path(f"/proc/{pid}/environ").read_bytes()
        except OSError:
            return ""
        wanted = {
            b"GITEA_BOTS_MODEL",
            b"GITEA_BOTS_NOTIFY_SESSION",
            b"OPENCODE_SESSION_ID",
        }
        env = {}
        for item in raw.split(b"\0"):
            key, _, value = item.partition(b"=")
            if key in wanted:
                env[key.decode()] = value.decode("utf-8", "replace")
        override = env.get("GITEA_BOTS_MODEL", "").strip()
        if override:
            return override
        session = (
            env.get("GITEA_BOTS_NOTIFY_SESSION")
            or env.get("OPENCODE_SESSION_ID", "")
        ).strip()
        if not session:
            return ""
        try:
            return self.notifier.model_for_session(session, timeout=5) or ""
        except Exception:  # diagnostics only
            return ""

    def _last_logged_model(self) -> str:
        """Last resolved model recorded in the tail of watcher.log."""
        try:
            with open(self.config.effective_log_file, "rb") as fh:
                fh.seek(0, os.SEEK_END)
                size = fh.tell()
                fh.seek(max(0, size - 65536))
                tail = fh.read().decode("utf-8", errors="replace")
        except OSError:
            return ""
        for line in reversed(tail.splitlines()):
            match = NOTIFICATION_MODEL_RE.search(line)
            if match:
                return match.group(1)
        return ""

    def _remove_pid(self) -> None:
        try:
            pid = self.config.pid_file.read_text(encoding="utf-8").strip()
        except OSError:
            pid = ""
        if pid == str(os.getpid()):
            self._clear_pid_files()

    def _clear_pid_files(self) -> None:
        for path in (self.config.pid_file, self.config.meta_file):
            try:
                path.unlink()
            except OSError:
                pass

    def _count_unread(self) -> int:
        if not self.inbox.pending_file.is_file():
            return 0
        try:
            return sum(
                1 for line in self.inbox.pending_file.read_text(encoding="utf-8").splitlines()
                if re.match(r"^20[0-9][0-9]-", line)
            )
        except OSError:
            return 0
