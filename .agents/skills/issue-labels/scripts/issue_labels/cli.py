"""Command-line entry point for the shared issue-labels skill.

Resolves the repo, API base and token from ``origin`` + ``.env``, then applies,
reconciles or migrates the canonical status label set. A staged workflow can
inject a marker-aware ``derive`` callable while keeping every documented
command intact.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from .gitea import GiteaClient, GiteaError, GiteaSettings
from .labels import normalize_status_set
from .manager import StatusManager, derive_body_status

STATUS_EPILOG = """\
usage:
  set-status.py <issue> <label> [<label> ...]
  set-status.py --reconcile <issue>
  set-status.py --migrate

Canonical labels (one word, general-purpose, no stage numbers): seeing,
responded, working, question, committed. Repo labels display each with a leading
emoji (👀 seeing, 💬 responded, 🔨 working, ❓ question, ✅ committed); the bare
token stays the input name. `committed` is additive (the issue also keeps
`responded`); `question` is transient. `none` clears every status label.
Multiple labels are merged as a set; unrelated labels are preserved and the
operation is idempotent.

Legacy input aliases (accepted for one release): inspect -> seeing,
plan -> responded, implement -> responded + committed, questions -> question,
done -> responded + committed.

--reconcile derives the label set from the issue body (and, in a workflow that
injects a derive callable, from its comment markers). --migrate renames the
legacy and bare repo labels in place to their canonical emoji names (preserving
label IDs) and creates `question`.
"""


class _Logger:
    """Minimal output surface used by :class:`StatusManager`."""

    def warn(self, message: str) -> None:
        print(message, file=sys.stderr)

    def event(self, message: str) -> None:
        print(message)


def _script(script_path):
    path = Path(script_path or sys.argv[0]).resolve()
    return path.parent


def main_status(argv=None, script_path=None, *, derive=None) -> int:
    """Run the label CLI. ``derive`` may inject workflow marker awareness."""
    argv = list(sys.argv[1:] if argv is None else argv)
    script_dir = _script(script_path)
    logger = _Logger()
    parser = argparse.ArgumentParser(
        prog="set-status.py",
        description="Apply a set of issue status labels to an issue.",
        epilog=STATUS_EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("issue", nargs="?", default="")
    parser.add_argument("status", nargs="*", default=[])
    parser.add_argument("--reconcile", action="store_true")
    parser.add_argument("--migrate", action="store_true")
    if not argv:
        parser.print_help()
        return 0
    args = parser.parse_args(argv)

    try:
        settings = GiteaSettings.load(script_dir)
        settings.require()
        client = GiteaClient(
            settings.api_base, settings.token, timeout=settings.timeout
        )
        status = StatusManager(
            settings, client, logger, derive=derive or derive_body_status
        )

        if args.migrate:
            logger.event("set-status: " + status.migrate_labels())
            return 0

        if args.reconcile:
            if not args.issue.isdigit():
                logger.warn("usage: set-status.py --reconcile <issue>")
                return 1
            _, text, _ = status.reconcile(int(args.issue))
            logger.event("set-status: " + text)
            return 0

        if not args.issue.isdigit():
            logger.warn(
                "usage: set-status.py <issue> <none|seeing|responded|working|"
                "question|committed ...>"
            )
            return 1
        if not args.status or normalize_status_set(args.status) is None:
            logger.warn(
                f"unknown status: '{' '.join(args.status)}' (expected none|"
                "seeing|responded|working|question|committed, or legacy "
                "aliases inspect|plan|implement|questions|done)"
            )
            return 1
        _, text = status.apply(int(args.issue), args.status)
        logger.event("set-status: " + text)
        return 0
    except GiteaError as exc:
        logger.warn(str(exc))
        return 1
