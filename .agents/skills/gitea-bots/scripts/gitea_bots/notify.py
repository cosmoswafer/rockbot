"""Agent notification (#131): same-model delivery + pinned reply workers.

Design
------
* **fresh (default, #137)** — an archived, minimal-context session is spawned per
  event batch, but only when a model has been resolved and passed via
  ``--model``. A model-less spawn is refused (that was the
  ``opencode/iceman-free`` free-tier failure). This keeps the long-lived
  interactive session out of the routine report/ack loop and avoids re-sending
  its accumulated history on every poll.
* **shared (opt-in)** — events are delivered back into the launching
  OpenCode session (``$OPENCODE_SESSION_ID``), so they run on the model the
  user already selected there. Same-model holds *by construction*; no
  detached, model-less session handles notifications. Opt in with
  ``GITEA_BOTS_NOTIFY_SESSION_MODE=shared``.
* **Reply workers** — when a batch contains a *human reply* (an at-mention of
  the bot login; #136 shared-login mode), a fresh background session pinned to
  the resolved model (``--model``) is spawned per replying issue, each carrying
  only that issue's event block. Workers are visible in the TUI unless
  ``GITEA_BOTS_ARCHIVE_WORKERS=1``. The worker works out what was asked from the
  issue and the repo's ``AGENTS.md`` — there is no stage vocabulary (#134).
  Dispatch is concurrent and bounded by ``GITEA_BOTS_MAX_CONCURRENCY`` (#138).
  The interactive session only reports + acks.

Model resolution: ``GITEA_BOTS_MODEL`` (explicit override/fallback) or, by
default, the launching session's model via
``opencode api get /api/session/<id>`` -> ``data.model``. If neither resolves,
no detached session is spawned and the events stay safely in ``pending.md``.

Notification is always post-lock and best-effort: it never holds ``poll.lock``
and never blocks the next poll. Spawned children use ``close_fds=True`` and
``start_new_session=True``.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from concurrent.futures import ThreadPoolExecutor

from .clock import now_utc
from .errors import GiteaError
from .rules import auto_plan_issues_in_batch, human_reply_blocks


class Notifier:
    def __init__(self, config, inbox, status, logger):
        self.config = config
        self.inbox = inbox
        self.status = status
        self.log = logger

    # ------------------------------------------------------------------ model
    def resolve_model(self):
        """Return ``provider/model[#variant]`` for pinning spawned sessions.

        Prefers the explicit ``GITEA_BOTS_MODEL`` override, then reads the
        launching session's model from the OpenCode API. Returns ``None`` when
        neither is available (callers must then refuse model-less spawns).
        """
        override = (self.config.model or "").strip()
        if override:
            return override
        session = self.config.notify_session
        if not session:
            return None
        try:
            proc = subprocess.run(
                ["opencode", "api", "get", f"/api/session/{session}"],
                capture_output=True, text=True, timeout=20, close_fds=True,
            )
            data = json.loads(proc.stdout)
            model = (data.get("data") or {}).get("model") or {}
            provider, model_id = model.get("providerID"), model.get("id")
            if not provider or not model_id:
                return None
            variant = model.get("variant")
            suffix = f"#{variant}" if variant and variant != "default" else ""
            return f"{provider}/{model_id}{suffix}"
        except (OSError, subprocess.SubprocessError, ValueError, TypeError):
            return None

    # ----------------------------------------------------------------- notify
    def notify(self, event_count: int, events_text: str = "", self_login: str = "",
               shared_login: bool = True) -> None:
        if not self.config.notify_enabled:
            self.log.verbose("agent notification disabled")
            return
        if shutil.which("opencode") is None:
            self.log.warn("opencode CLI not found — cannot notify the agent")
            return

        model = self.resolve_model()
        if model:
            self.log.log(
                f"notification model: {model} "
                f"(session: {self.config.notify_session or 'none'})"
            )
        else:
            self.log.warn(
                "could not resolve a notification model — no model-less "
                "detached session will be spawned; events remain in pending.md"
            )

        # #136: with a shared bot/human login, an at-mention of the bot login
        # is a human reply even when the author equals the token user, so it
        # delegates to a pinned worker. #138: one worker per replying issue,
        # each carrying only its own event block.
        blocks = human_reply_blocks(
            events_text, self_login, shared_login=shared_login
        )
        # #140: an AUTO PLAN event authorizes investigation/planning for an
        # eligible assigned issue, so it delegates too. Issues already covered
        # by a human reply are dropped to avoid two workers on one issue, and
        # the whole queued set gets a single worker per poll (the startup
        # catch-up can queue many issues at once).
        auto_plans = []
        if self.config.auto_plan and self.config.auto_plan_workers:
            auto_plans = [
                number for number in auto_plan_issues_in_batch(events_text)
                if number not in blocks
            ]
        authorized = bool(blocks or auto_plans)
        # Delegate only when a worker will actually run: with workers disabled
        # (or no model) the notification session itself answers the reply.
        delegate = authorized and self.config.spawn_workers and bool(model)
        mode = self.config.notify_mode

        if mode == "shared":
            if self.config.notify_session:
                self._shared(event_count, delegate)
            else:
                self.log.warn(
                    "shared mode has no session id (set GITEA_BOTS_NOTIFY_SESSION "
                    "or OPENCODE_SESSION_ID) — events remain in pending.md"
                )
        elif mode == "fresh":
            if model:
                self._fresh(event_count, model, delegate=delegate)
            else:
                self.log.warn(
                    "fresh mode requested but no model resolved — no detached "
                    "session spawned; events remain in pending.md"
                )
        else:
            self.log.warn(
                f"unknown GITEA_BOTS_NOTIFY_SESSION_MODE='{mode}' — using fresh"
            )
            if model:
                self._fresh(event_count, model, delegate=delegate)
            else:
                self.log.warn(
                    "no model resolved — no detached session spawned; events "
                    "remain in pending.md"
                )

        if not authorized or not self.config.spawn_workers:
            return
        if not model:
            self.log.warn(
                "human reply or AUTO PLAN in batch but no model resolved — "
                "no worker spawned; events remain in pending.md"
            )
            return
        # #144: a dispatched worker means a background agent has picked the
        # issue up, so show `seeing` on the board before it starts. Best-effort:
        # a label failure is a WARN and must never block the spawn.
        if delegate:
            self._mark_picked_up(sorted(set(blocks) | set(auto_plans)))
        if blocks:
            self._spawn_workers(blocks, model)
        if auto_plans:
            self._spawn_auto_plan_worker(event_count, model, events_text,
                                         auto_plans)

    def _mark_picked_up(self, numbers) -> None:
        """Apply the ``seeing`` pickup label before workers spawn (#144).

        ``seeing`` means a background agent is working on the issue. Each write
        is independent: one failure is a ``WARN`` and never blocks the others,
        the notification or the worker spawns.
        """
        for number in numbers:
            try:
                changed, text = self.status.apply(number, "seeing")
            except GiteaError as exc:
                self.log.warn(f"pickup label failed for #{number} — {exc}")
                continue
            if changed:
                self.log.verbose(f"pickup label: {text}")

    # ------------------------------------------------------------ delivery modes
    def _shared(self, event_count: int, delegate: bool) -> None:
        session = self.config.notify_session
        payload = json.dumps({
            "text": self.inbox.notify_text(event_count, delegate=delegate),
            "delivery": "queue",
        })
        cmd = [
            "opencode", "api", "post",
            f"/api/session/{session}/synthetic", "-d", payload,
        ]
        try:
            subprocess.run(
                cmd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, timeout=20, close_fds=True,
            )
            self.log.verbose(f"notified agent session {session}")
        except (OSError, subprocess.SubprocessError):
            self.log.warn(
                f"agent notification failed (session {session}) — events remain "
                f"in {self.inbox.pending_file}"
            )

    def _fresh(self, event_count: int, model: str, delegate: bool = False) -> None:
        title = f"gitea-bots: {event_count} event(s) @ {now_utc()}"
        self._spawn(
            ["--model", model, self.inbox.notify_text(event_count, delegate=delegate)],
            title,
        )

    def _spawn_workers(self, blocks, model) -> None:
        """Spawn one pinned reply worker per issue, bounded (#138)."""
        numbers = sorted(blocks)
        workers = min(max(1, self.config.max_concurrency), len(numbers))
        with ThreadPoolExecutor(max_workers=workers) as pool:
            futures = [
                pool.submit(self._spawn_worker, number, blocks[number], model)
                for number in numbers
            ]
            for future in futures:
                future.result()
        issues = ",".join(str(number) for number in numbers)
        self.log.log(
            f"spawned pinned reply worker(s) for issue(s) {issues} on model {model}"
        )

    def _spawn_worker(self, number, block, model) -> None:
        title = f"gitea-bots: reply work for #{number} @ {now_utc()}"
        self._spawn(
            ["--model", model,
             self.inbox.worker_text(1, block, issues=[number])],
            title,
            archived=self.config.archive_workers,
        )

    def _spawn_auto_plan_worker(self, event_count, model, events_text,
                                numbers) -> None:
        """Spawn one pinned planning worker for the batch's AUTO PLAN set (#140)."""
        issues = ",".join(f"#{number}" for number in numbers)
        title = f"gitea-bots: auto-plan work for {issues} @ {now_utc()}"
        self._spawn(
            ["--model", model,
             self.inbox.auto_plan_worker_text(event_count, events_text, numbers)],
            title,
            archived=self.config.archive_workers,
        )
        self.log.log(
            f"spawned pinned AUTO PLAN worker for issue(s) "
            f"{','.join(str(number) for number in numbers)} on model {model}"
        )

    def _spawn(self, args, title: str, *, archived: bool = True) -> None:
        """Detach one ``opencode run`` session; output lands in notify.log.

        ``archived`` controls TUI visibility only: omitting ``--archived`` still
        runs and persists the session, it just shows up in the session list.
        Workers follow ``GITEA_BOTS_ARCHIVE_WORKERS``; the batch reporter keeps
        the default so a poll loop does not flood the picker.
        """
        cmd = ["opencode", "run"]
        if archived:
            cmd.append("--archived")
        cmd += ["--auto", "--format", "json", "--title", title, *args]
        self.inbox.ensure_dirs()
        try:
            logf = open(self.inbox.notify_log, "ab", buffering=0)
        except OSError:
            self.log.warn("could not open notify.log — events remain in pending.md")
            return
        try:
            subprocess.Popen(
                cmd, stdin=subprocess.DEVNULL, stdout=logf,
                stderr=subprocess.STDOUT, start_new_session=True,
                close_fds=True,
            )
        except OSError:
            self.log.warn(
                "could not spawn a session — events remain in pending.md"
            )
        finally:
            logf.close()
        self.log.verbose(f"spawned session: {title}")
