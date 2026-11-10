"""CLI entry points and composition root.

Wiring happens here and only here: each component is constructed once with its
explicit dependencies and handed to the command handlers. This is the single
place that knows about every module, which keeps the modules themselves loosely
coupled.
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
from pathlib import Path

from issue_labels.cli import main_status as _shared_main_status

from .client import GiteaClient
from .config import Config
from .db import Database
from .e2e_probe import build_e2e_probe, write_report_artifact
from .errors import BotFatal, GiteaError
from .inbox import Inbox
from .live_probe import DEFAULT_ISSUE_LIMIT, build_probe
from .log import Logger
from .notify import Notifier
from .rules import derive_status
from .state import StateStore
from .status import StatusManager
from .tasks import TaskSyncer
from .watcher import Watcher
from .write_probe import build_write_probe

WATCH_EPILOG = """\
commands:
  check            Poll once, print events, exit. Refuses to run while the
                   background watcher holds the poll lock.
  loop (run)       Run the poll loop in the foreground (Ctrl-C stops it).
  start            Start the loop in the background (PID file + watcher.log).
                   On the first baseline poll, reports existing open issues.
  inbox            Print pending events the agent has not acknowledged.
  ack [batch-id]   Mark pending events as seen (oldest batch by default).
  status           Show whether the loop is running, plus state stats.
  stop             Stop the background loop (SIGTERM, then SIGKILL).
  reset            Move state.db aside; the next poll re-baselines.
  scan-unplanned   List plan-eligible, unplanned Summary-only issues.

options:
  -i, --interval N          Poll interval in seconds (min 5, default 20).
  --report-existing         On the baseline poll, emit NEW ISSUE events for
                            existing open issues (default for start/loop).
  --report-existing-all     Same, including closed issues.
  --no-report-existing      Record the baseline silently.

environment (see the skill's SKILL.md for the full list):
  GITEA_BOTS_INTERVAL, GITEA_BOTS_CATCHUP_MINUTES, GITEA_BOTS_STATE_DIR,
  GITEA_BOTS_LOG, GITEA_BOTS_ROOT, GITEA_BOTS_ALL_COMMENTS, GITEA_BOTS_SELF,
  GITEA_BOTS_AUTO_PLAN, GITEA_BOTS_START_RECONCILE,
  GITEA_BOTS_VERBOSE, GITEA_BOTS_NOTIFY, GITEA_BOTS_NOTIFY_SESSION_MODE
  (default fresh; shared = deliver into the launching session),
  GITEA_BOTS_NOTIFY_SESSION (default $OPENCODE_SESSION_ID; used for
  shared delivery and model resolution),
  GITEA_BOTS_MODEL (explicit model override/fallback),
  GITEA_BOTS_SPAWN_WORKERS (1 = spawn pinned reply workers for human replies),
  GITEA_BOTS_MAX_CONCURRENCY (default 8; bounds per-issue reads and worker
  dispatch),
  GITEA_BOTS_SHARED_LOGIN (1 = bot and human share one account, default; 0 =
  dedicated bot account, restoring the login-equality guard).

State lives in ./tmp/gitea-bots/ (state.db, pending.md, watcher.pid,
watcher.meta, watcher.log, notify.log, poll.lock, pending.lock). state.db is
the authoritative SQLite store (issues, batches, events, tasks, polls);
pending.md is a generated view. Query it directly with
`sqlite3 ./tmp/gitea-bots/state.db '...'`. Pre-#135 flat files are imported
once on first use and archived under legacy-json-<ts>/; see gitea_bots/README.md
for when that compatibility path is removed.
Rule: exactly one watcher may poll a given state dir. GITEA_TOKEN is sourced
silently from .env and is never printed.
"""

TASKS_EPILOG = """\
usage:
  sync-tasks.py <issue-number> <plan-file>

Copies the plan's `## Tasks` checklist into the issue body (Gitea computes task
progress from the body only), preserving everything before that section.
"""

PROBE_EPILOG = """\
Runs the documented scenarios against the real Gitea server and prints a
per-scenario PASS/FAIL report. The probe is strictly read-only: ReadOnlyClient
refuses every non-GET request and the notifier/status stubs refuse to write.
It uses its own state directory (tmp/gitea-bots-live-probe/), never a live
watcher's tmp/gitea-bots/ state.

Requires GITEA_TOKEN in .env and network access. Running the script is the
opt-in; there is no GITEA_BOTS_LIVE gate here (the unittest adapter keeps one).
"""

WRITE_PROBE_EPILOG = """\
Creates one dummy issue on the real Gitea server, exercises create → comment →
label → assign → close → delete and prints a per-step PASS/FAIL report. Every
mutation targets the issue this run created, and the issue is deleted in a
`finally` cleanup. When the token may not delete issues (non-admin), the probe
clears labels/assignees, keeps the dummy closed and fails loudly with its URL.

The dummy body has no `## Summary` / `## Tasks`, so a live watcher can never
auto-plan or re-assign it. Its lock/state lives in
tmp/gitea-bots-live-write-probe/, never a live watcher's tmp/gitea-bots/ state.

Requires GITEA_TOKEN in .env and network access. Running the script is the
opt-in; there is no GITEA_BOTS_LIVE_WRITE gate here (the unittest adapter
keeps one).
"""

E2E_PROBE_EPILOG = """\
Creates controlled dummy issues on the real Gitea server, lets the real watcher
poll and classify them (NEW ISSUE / AUTO PLAN / DIRECTED COMMENT / REASSIGNED),
asserts the emitted events, state.db transitions, real status labels and
idempotence, then deletes every dummy it created.

STOP THE PRODUCTION WATCHER FIRST. The probe refuses to run while its
watcher.pid points at a live watch-issues.py process, because the live loop
would consume the dummy issues (AUTO PLAN / reply workers). It uses
tmp/gitea-bots-live-e2e/ and a sibling lock, never the live watcher's
tmp/gitea-bots/ state.

Delete is admin-gated. When the token may not delete issues, the probe clears
labels/assignees, keeps the dummy closed, records the leftover URL and fails
loudly. A report is written under tmp/artifacts/gitea-bots-e2e-<ts>.log (plus a
structured .json sibling).

Requires GITEA_TOKEN in .env and network access. Running the script is the
opt-in; the unittest adapter keeps the GITEA_BOTS_LIVE_E2E=1 gate.
"""


def _script(script_path):
    path = Path(script_path or sys.argv[0]).resolve()
    return path.parent, path


def _database(config, logger):
    """Open the SQLite store and run the one-shot legacy-file migration."""
    db = Database(config.db_file, logger)
    try:
        db.ensure_schema()
    except sqlite3.DatabaseError as exc:
        logger.warn(
            f"state database is unreadable or corrupt — backing it up: "
            f"{db.path} ({exc})"
        )
        db.quarantine("corrupt")
        db.ensure_schema()
    db.migrate_from_files(config.state_dir)
    return db


def build_components(config, logger):
    client = GiteaClient(config.api_base, config.token, timeout=config.api_timeout)
    db = _database(config, logger)
    state = StateStore(config, logger, db)
    inbox = Inbox(config, logger, db)
    status = StatusManager(config, client, logger)
    notifier = Notifier(config, inbox, status, logger)
    watcher = Watcher(config, client, state, inbox, notifier, status, logger)
    return watcher, client, inbox, status


# ---------------------------------------------------------------------------
# watch-issues.py
# ---------------------------------------------------------------------------
def main_watch(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir, script_path = _script(script_path)
    logger = Logger()

    parser = argparse.ArgumentParser(
        prog="watch-issues.py",
        description=(
            "Poll the project's Gitea repo for new issues and human replies."
        ),
        epilog=WATCH_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("command", nargs="?", default="",
                        help="check|loop|start|inbox|ack|status|stop|reset|scan-unplanned")
    parser.add_argument("batch_id", nargs="?", default=None,
                        help="batch id for ack")
    parser.add_argument("-i", "--interval", type=int, default=None)
    parser.add_argument("--report-existing", action="store_true")
    parser.add_argument("--report-existing-all", "--all-existing", action="store_true")
    parser.add_argument("--no-report-existing", action="store_true")
    args = parser.parse_args(argv)

    cmd = args.command or ""
    if cmd in ("", "help"):
        parser.print_help()
        return 0

    report_existing = -1
    if args.report_existing_all:
        report_existing = 2
    elif args.report_existing:
        report_existing = 1
    elif args.no_report_existing:
        report_existing = 0
    if report_existing < 0:
        report_existing = 1 if cmd in ("loop", "run", "start") else 0

    try:
        config = Config.load(script_dir, script_path, logger, interval=args.interval)
        logger.verbose_enabled = config.verbose
        watcher, client, inbox, _ = build_components(config, logger)

        # Offline commands never touch the network, token, or /user lookup.
        if cmd == "status":
            watcher.status()
            return 0
        if cmd == "stop":
            watcher.stop()
            return 0
        if cmd == "reset":
            return watcher.reset()
        if cmd == "inbox":
            inbox.inbox()
            return 0
        if cmd == "ack":
            inbox.ack(args.batch_id)
            return 0

        config.require_interval(logger)
        config.require_remote(logger)
        config.require_token(logger)
        config.resolve_self(client, logger)

        if cmd in ("check", "once"):
            return watcher.check(report_existing)
        if cmd == "scan-unplanned":
            return watcher.scan_unplanned()
        if cmd in ("loop", "run"):
            watcher.loop(report_existing)
            return 0
        if cmd == "start":
            watcher.start(report_existing)
            return 0
        logger.die(f"unknown command: {cmd} (try --help)")
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1
    return 1


# ---------------------------------------------------------------------------
# set-status.py
# ---------------------------------------------------------------------------
def main_status(argv=None, script_path=None) -> int:
    """Forward to the vendored issue_labels CLI, injecting the workflow derive.

    The service commands (apply / ``--reconcile`` / ``--migrate``) and their
    help text live in ``issue_labels.cli``. Only the projection differs:
    here the stage markers in the comment list are marker-aware.
    """
    return _shared_main_status(argv, script_path, derive=derive_status)


# ---------------------------------------------------------------------------
# sync-tasks.py
# ---------------------------------------------------------------------------
def main_tasks(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir, script_path = _script(script_path)
    logger = Logger()
    parser = argparse.ArgumentParser(
        prog="sync-tasks.py",
        description="Mirror a plan checklist into the issue body.",
        epilog=TASKS_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("issue", nargs="?", default="")
    parser.add_argument("plan", nargs="?", default="")
    if not argv:
        parser.print_help()
        return 0
    args = parser.parse_args(argv)

    try:
        config = Config.load(script_dir, script_path, logger)
        logger.verbose_enabled = config.verbose
        config.require_remote(logger)
        config.require_token(logger)
        client = GiteaClient(config.api_base, config.token, timeout=config.api_timeout)
        db = _database(config, logger)
        tasks = TaskSyncer(config, client, logger, db)
        if not args.issue.isdigit():
            logger.die("usage: sync-tasks.py <issue-number> <plan-file>")
        tasks.sync(int(args.issue), args.plan)
        return 0
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1


# ---------------------------------------------------------------------------
# probe-live.py
# ---------------------------------------------------------------------------
def main_probe(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir, script_path = _script(script_path)
    logger = Logger()
    parser = argparse.ArgumentParser(
        prog="probe-live.py",
        description=(
            "Probe the real Gitea server's read-only API shapes (opt-in)."
        ),
        epilog=PROBE_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "-n", "--issues", type=int, default=DEFAULT_ISSUE_LIMIT,
        help=(
            "Number of recently updated open issues to read comments/timeline "
            f"for (default {DEFAULT_ISSUE_LIMIT})."
        ),
    )
    parser.add_argument(
        "--verbose", action="store_true",
        help="Print probe progress in addition to the report.",
    )
    args = parser.parse_args(argv)
    if args.issues < 0:
        parser.error("--issues must be >= 0")

    try:
        config = Config.load(script_dir, script_path, logger)
        logger.verbose_enabled = args.verbose
        config.require_remote(logger)
        config.require_token(logger)
        probe = build_probe(config, logger, issue_limit=args.issues)
        report = probe.run()
        logger.event(report.render())
        return 0 if report.ok else 1
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1


# ---------------------------------------------------------------------------
# probe-live-write.py
# ---------------------------------------------------------------------------
def main_probe_write(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir, script_path = _script(script_path)
    logger = Logger()
    parser = argparse.ArgumentParser(
        prog="probe-live-write.py",
        description=(
            "Probe the real Gitea server's write endpoints with one dummy "
            "issue (opt-in, cleans up after itself)."
        ),
        epilog=WRITE_PROBE_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--verbose", action="store_true",
        help="Print probe progress in addition to the report.",
    )
    args = parser.parse_args(argv)

    try:
        config = Config.load(script_dir, script_path, logger)
        logger.verbose_enabled = args.verbose
        config.require_remote(logger)
        config.require_token(logger)
        probe = build_write_probe(config, logger)
        report = probe.run()
        logger.event(report.render())
        if probe.leftover_url:
            logger.warn(f"probe issue was left behind: {probe.leftover_url}")
        return 0 if report.ok else 1
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1


# ---------------------------------------------------------------------------
# probe-e2e.py
# ---------------------------------------------------------------------------
def main_probe_e2e(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir, script_path = _script(script_path)
    logger = Logger()
    parser = argparse.ArgumentParser(
        prog="probe-e2e.py",
        description=(
            "Run the real watcher against dummy issues on the real Gitea "
            "server, assert the result, then delete everything (opt-in)."
        ),
        epilog=E2E_PROBE_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--verbose", action="store_true",
        help="Print probe progress in addition to the report.",
    )
    args = parser.parse_args(argv)

    try:
        config = Config.load(script_dir, script_path, logger)
        logger.verbose_enabled = args.verbose
        config.require_remote(logger)
        config.require_token(logger)
        probe = build_e2e_probe(config, logger)
        report = probe.run()
        logger.event(report.render())
        artifacts = write_report_artifact(report, config)
        logger.log(
            f"e2e report: {artifacts['log']} (json: {artifacts['json']})"
        )
        for url in probe.leftover_urls:
            logger.warn(f"probe issue was left behind: {url}")
        return 0 if report.ok else 1
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1
