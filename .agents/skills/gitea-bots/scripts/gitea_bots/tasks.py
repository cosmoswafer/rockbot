"""Mirror a stage-2 plan checklist into the issue body (``sync-tasks``).

Gitea computes task progress from the issue body only, so a ``- [ ]`` list in a
plan comment is invisible to its counter. This keeps everything before
``## Tasks``, replaces that section, and is a no-op when already in sync.
"""

from __future__ import annotations

import re
import sqlite3
from pathlib import Path

from .clock import now_utc
from .db import Database
from .errors import GiteaError
from .rules import (
    extract_tasks,
    strip_one_trailing_newline,
    strip_tasks_section,
    trim_trailing_blank_lines,
)

_TASK_PREFIX = re.compile(r"^- \[[ xX]\]\s*")


class TaskSyncer:
    def __init__(self, config, client, logger, db=None):
        self.config = config
        self.client = client
        self.log = logger
        self.db = db or Database(config.db_file, logger)

    def _path(self, num) -> str:
        return f"/repos/{self.config.repo}/issues/{num}"

    def sync(self, num, plan_path) -> None:
        path = Path(plan_path)
        if not path.is_file():
            raise GiteaError(f"plan file not found: {plan_path}")
        plan = path.read_text(encoding="utf-8")
        tasks = extract_tasks(plan)
        if not tasks:
            self.log.event(
                f"sync-tasks: no checklist in {plan_path} — nothing to sync"
            )
            return

        issue = self.client.get(self._path(num))
        current = issue.get("body") or ""
        head_part = "\n".join(
            trim_trailing_blank_lines(strip_tasks_section(current))
        )
        new_body = f"{head_part}\n\n## Tasks\n\n" + "\n".join(tasks) + "\n"

        if strip_one_trailing_newline(new_body) == strip_one_trailing_newline(current):
            self._mirror(num, tasks)
            self.log.event(f"sync-tasks: #{num} already in sync")
            return

        self.client.patch(self._path(num), payload={"body": new_body})
        self._mirror(num, tasks)
        count = sum(1 for task in tasks if task.startswith("- ["))
        self.log.event(f"sync-tasks: #{num} updated ({count} item(s))")

    def _mirror(self, num, tasks) -> None:
        """Record the checklist in the ``tasks`` table for queryable progress."""
        rows = []
        for position, line in enumerate(tasks):
            done = line.startswith("- [x]") or line.startswith("- [X]")
            rows.append((position, _TASK_PREFIX.sub("", line), done))
        try:
            with self.db.transaction() as conn:
                self.db.replace_tasks(num, rows, synced_at=now_utc(), conn=conn)
        except sqlite3.Error as exc:
            # The body is the source of truth; a mirror failure is a WARN.
            self.log.warn(f"sync-tasks: could not mirror tasks for #{num}: {exc}")
