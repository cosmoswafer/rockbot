"""The watcher's runtime identity: code mtimes + effective config values.

A running loop loads its modules and :class:`~gitea_bots.config.Config` once at
startup, so a merged default or a ``GITEA_BOTS_*`` change stays inactive until
the operator restarts it. :class:`Fingerprint` captures those inputs as a
stable, comparable value:

- ``files``: nanosecond mtimes of ``watch-issues.py`` and every
  ``gitea_bots/*.py`` module, keyed by a relative path;
- ``config``: the effective behaviour values resolved by ``Config.load`` (not
  the raw environment), with the token and other per-invocation identities
  excluded.

Both the loop (warn on drift) and ``status`` (report drift) compare the same
fingerprint, so a doc-only commit no longer looks stale while an env-only
change does. This module is pure: no network, no clock, no config mutation.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, fields
from pathlib import Path
from typing import Iterable, Mapping

from .config import Config

# Every ``Config`` dataclass field must be classified below; the unit tests
# enforce that a future toggle cannot silently escape the fingerprint.
FINGERPRINT_CONFIG_FIELDS = (
    "repo_root",
    "state_dir",
    "log_file",
    "api_timeout",
    "interval",
    "catchup_minutes",
    "all_comments",
    "auto_plan",
    "auto_plan_workers",
    "start_reconcile",
    "verbose",
    "notify_enabled",
    "notify_mode",
    "notify_in_foreground",
    "model",
    "spawn_workers",
    "archive_workers",
    "shared_login",
    "max_concurrency",
)

# Deliberately excluded, with the reason it is not a loop input:
# - token/env: credentials; ``watcher.meta`` is inspectable.
# - repo/api_base: the bound remote, fixed for a state directory.
# - self_login: resolved at runtime from ``/user``.
# - script_path: covered by the ``files`` map.
# - notify_session: derived from ``OPENCODE_SESSION_ID``, so two shells that
#   read the same meta would otherwise always look like drift.
FINGERPRINT_EXCLUDED_FIELDS = {
    "token": "credential — never written to watcher.meta",
    "env": "raw merged environment — contains the token",
    "repo": "bound remote identity, fixed for a state directory",
    "api_base": "derived from the bound remote",
    "self_login": "resolved at runtime, not a startup input",
    "script_path": "covered by the files map",
    "notify_session": "per-invocation session routing (OPENCODE_SESSION_ID)",
}


def _normalise(value) -> str:
    """A stable string for one effective config value."""
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, bool):
        return "1" if value else "0"
    if value is None:
        return ""
    return str(value)


def _file_label(path: Path, base_dir: Path | None) -> str:
    if base_dir is not None:
        try:
            return path.relative_to(base_dir).as_posix()
        except ValueError:
            pass
    return path.as_posix()


def config_values(config: Config) -> dict:
    """The effective, non-secret behaviour values of ``config``."""
    return {
        name: _normalise(getattr(config, name))
        for name in FINGERPRINT_CONFIG_FIELDS
    }


@dataclass(frozen=True)
class Fingerprint:
    """Comparable snapshot of the watcher's code and effective config."""

    files: dict
    config: dict

    @classmethod
    def capture(
        cls,
        files: Iterable,
        config: Config,
        *,
        base_dir: Path | None = None,
    ) -> "Fingerprint":
        base = Path(base_dir) if base_dir is not None else None
        mtimes = {}
        for raw in files:
            path = Path(raw)
            try:
                mtime = path.stat().st_mtime_ns
            except OSError:
                continue
            mtimes[_file_label(path, base)] = int(mtime)
        return cls(files=mtimes, config=config_values(config))

    def to_json(self) -> str:
        return json.dumps(
            {"files": self.files, "config": self.config}, sort_keys=True
        )

    @classmethod
    def from_json(cls, raw: str) -> "Fingerprint | None":
        """Parse a stored fingerprint; ``None`` when absent or malformed."""
        if not raw:
            return None
        try:
            data = json.loads(raw)
        except (TypeError, ValueError):
            return None
        if not isinstance(data, Mapping):
            return None
        files = data.get("files")
        config = data.get("config")
        if not isinstance(files, Mapping) or not isinstance(config, Mapping):
            return None
        try:
            file_mtimes = {str(k): int(v) for k, v in files.items()}
            config_values_map = {str(k): str(v) for k, v in config.items()}
        except (TypeError, ValueError):
            return None
        return cls(files=file_mtimes, config=config_values_map)

    def diff(self, current: "Fingerprint") -> list:
        """Stable labels for every changed input, e.g. ``file:gitea_bots/x.py``."""
        changed = []
        for name in sorted(set(self.files) | set(current.files)):
            if self.files.get(name) != current.files.get(name):
                changed.append(f"file:{name}")
        for name in sorted(set(self.config) | set(current.config)):
            if self.config.get(name) != current.config.get(name):
                changed.append(f"config:{name}")
        return sorted(changed)


def unclassified_config_fields() -> set:
    """Config dataclass fields missing from both fingerprint sets (test aid)."""
    known = set(FINGERPRINT_CONFIG_FIELDS) | set(FINGERPRINT_EXCLUDED_FIELDS)
    return {field.name for field in fields(Config)} - known
