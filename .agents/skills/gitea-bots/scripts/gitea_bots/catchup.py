"""catch-up.py — explicit one-shot catch-up, separate from the watcher loop.

The background loop never replays downtime: its first poll records state
silently. This command is the explicit opt-in, and it reuses the same
components as the watcher (``Config``, ``GiteaClient``, ``StateStore``,
``Inbox``, ``Notifier``) through the CLI composition root.

Modes
-----
``scan`` (default)
    Full open-set scan, no ``last_poll`` window. Emits one ``AUTO PLAN`` per
    plan-eligible issue whose body is literally ``## Summary``-only and has no
    plan marker or status label. Deduped by ``auto_plan_at``.

``replay``
    One incremental poll over the stored ``last_poll`` window: publishes the
    ``NEW``/``UPDATED``/``DIRECTED`` events that accumulated while the watcher
    was stopped. Advances ``last_poll`` exactly like a normal poll.

Both modes require the watcher loop to be stopped: a live loop would
interleave polls and notifications. `scan` additionally takes ``poll.lock`` and
refuses while another poll is active. ``--notify`` spawns the usual
notification/reply workers; without it the events are only printed and queued in
``pending.md``.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .cli import build_components
from .config import Config
from .errors import BotFatal, GiteaError
from .log import Logger

CATCHUP_EPILOG = """\
modes:
  scan             Full open-set AUTO PLAN catch-up (default). No last_poll
                   window; emits only unprepared Summary-only issues.
  replay           Re-run one incremental poll over the last_poll window and
                   publish the events accumulated while the watcher was down.

The watcher loop must be stopped first. `scan` also takes poll.lock, so it
refuses while any poll is active. GITEA_TOKEN is read from .env and never
printed.
"""


def main_catchup(argv=None, script_path=None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)
    script = Path(script_path or sys.argv[0]).resolve()
    logger = Logger()

    parser = argparse.ArgumentParser(
        prog="catch-up.py",
        description=(
            "Explicit, one-shot catch-up over the Gitea backlog. The "
            "background watcher itself never replays downtime."
        ),
        epilog=CATCHUP_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("mode", nargs="?", default="scan",
                        choices=("scan", "replay"))
    parser.add_argument("--notify", action="store_true",
                        help="spawn the normal notification/reply workers")
    parser.add_argument("--report-existing", action="store_true")
    parser.add_argument("--report-existing-all", action="store_true")
    if not argv:
        parser.print_help()
        return 0
    args = parser.parse_args(argv)

    report_existing = 2 if args.report_existing_all else (
        1 if args.report_existing else 0
    )
    try:
        config = Config.load(script.parent, script, logger)
        logger.verbose_enabled = config.verbose
        config.require_remote(logger)
        config.require_token(logger)
        watcher, client, _, _ = build_components(config, logger)
        config.resolve_self(client, logger)
        return watcher.catch_up(
            args.mode,
            notify_armed=args.notify,
            report_existing=report_existing,
        )
    except BotFatal:
        return 1
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1


if __name__ == "__main__":  # pragma: no cover
    sys.exit(main_catchup(script_path=__file__))