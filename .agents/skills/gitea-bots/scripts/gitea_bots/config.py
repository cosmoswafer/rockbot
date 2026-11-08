"""Configuration: environment, repo root, remote derivation, paths, identity.

A single :class:`Config` value object is built once in the CLI and injected into
every other component. Reading the environment / ``.env`` / git remote is the
only responsibility here; nothing in this module performs network I/O.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

from issue_labels.gitea import derive_remote, git_output, load_dotenv

from .errors import GiteaError


def _as_int(value, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def _flag(value, default: bool = False) -> bool:
    if value is None:
        return default
    return str(value).strip() == "1"


@dataclass
class Config:
    # --- paths ---
    repo_root: Path = field(default_factory=lambda: Path.cwd())
    state_dir: Path = field(default_factory=lambda: Path("tmp/gitea-bots"))
    script_path: Path | None = None
    log_file: Path | None = None

    # --- identity / API ---
    repo: str = ""
    api_base: str = ""
    token: str = ""
    self_login: str = ""
    api_timeout: int = 25

    # --- behaviour toggles ---
    interval: int = 20
    catchup_minutes: int = 60
    all_comments: bool = False
    auto_plan: bool = True
    # #140: AUTO PLAN authorizes stage 2, so the notifier spawns one pinned
    # stage-2 worker per poll for the queued set (gated on `auto_plan`). `0`
    # keeps AUTO PLAN as a notification only.
    auto_plan_workers: bool = True        # GITEA_BOTS_AUTO_PLAN_WORKERS
    # #150: planning starts on assignment to the token user or on an explicit
    # at-mention request, never because an issue happens to be unassigned.
    start_reconcile: bool = True
    verbose: bool = False
    notify_enabled: bool = True           # GITEA_BOTS_NOTIFY
    # #137: fresh is the default. Each event batch is handled by a fresh,
    # minimal-context session pinned to the resolved model, so a long-lived
    # interactive session does not re-send its whole history every poll.
    # `shared` stays an explicit opt-in for inline delivery (#131).
    notify_mode: str = "fresh"            # fresh | shared
    notify_session: str = ""
    notify_in_foreground: bool = False   # test hook for `check`
    model: str = ""                      # GITEA_BOTS_MODEL (explicit override/fallback)
    spawn_workers: bool = True           # GITEA_BOTS_SPAWN_WORKERS
    # #147: reply/AUTO PLAN workers are visible in the TUI by default; `1`
    # restores the old hidden behaviour. The batch reporter always stays
    # archived so a poll loop cannot flood the session picker.
    archive_workers: bool = False        # GITEA_BOTS_ARCHIVE_WORKERS
    # #136: the bot and human share one Gitea account. When true, any at-mention
    # of the bot login is treated as a human reply regardless of author; when
    # false, the old login-equality guard applies (dedicated-bot deployments).
    shared_login: bool = True            # GITEA_BOTS_SHARED_LOGIN
    # #138: bound the per-issue comment/timeline prefetch and the per-issue
    # stage-worker dispatch; clamped to >= 1 by the loader.
    max_concurrency: int = 8             # GITEA_BOTS_MAX_CONCURRENCY

    # raw merged environment (kept for introspection/tests, never printed)
    env: dict = field(default_factory=dict, repr=False)

    # ------------------------------------------------------------------ paths
    @property
    def db_file(self) -> Path:
        return self.state_dir / "state.db"

    @property
    def pid_file(self) -> Path:
        return self.state_dir / "watcher.pid"

    @property
    def meta_file(self) -> Path:
        return self.state_dir / "watcher.meta"

    @property
    def poll_lock_file(self) -> Path:
        return self.state_dir / "poll.lock"

    @property
    def notify_log(self) -> Path:
        return self.state_dir / "notify.log"

    @property
    def effective_log_file(self) -> Path:
        return self.log_file or (self.state_dir / "watcher.log")

    # ------------------------------------------------------------------ setup
    @classmethod
    def load(cls, script_dir, script_path, logger, *, interval=None) -> "Config":
        """Build a Config from the environment; remote/token errors are deferred
        to :meth:`require_remote` / :meth:`require_token` so offline commands
        (``status``/``inbox``/``ack``/``stop``/``reset``) work without them."""
        script_dir = Path(script_dir).resolve()
        process_env = {k: v for k, v in os.environ.items() if v != ""}
        cwd_env = load_dotenv(Path.cwd() / ".env")

        root_override = process_env.get("GITEA_BOTS_ROOT") or cwd_env.get("GITEA_BOTS_ROOT")
        if root_override:
            repo_root = Path(root_override).resolve()
        else:
            top = git_output(["rev-parse", "--show-toplevel"], script_dir)
            repo_root = Path(top).resolve() if top else (script_dir / "../../../..").resolve()

        # Priority (low -> high): cwd .env < repo-root .env < process env.
        # The repo-root .env is authoritative for the token, but an explicitly
        # exported value in the process environment always wins (override=False).
        env = dict(cwd_env)
        env.update(load_dotenv(repo_root / ".env"))
        env.update(process_env)

        state_dir = Path(
            env.get("GITEA_BOTS_STATE_DIR") or (repo_root / "tmp/gitea-bots")
        ).resolve()
        log_override = env.get("GITEA_BOTS_LOG")
        log_file = Path(log_override).resolve() if log_override else None

        host, repo = derive_remote(git_output(["remote", "get-url", "origin"], repo_root))
        api_base = f"https://{host}/api/v1" if host else ""

        cfg = cls(
            repo_root=repo_root,
            state_dir=state_dir,
            script_path=Path(script_path).resolve() if script_path else None,
            log_file=log_file,
            repo=repo,
            api_base=api_base,
            token=env.get("GITEA_TOKEN", "") or "",
            self_login=env.get("GITEA_BOTS_SELF", "") or "",
            api_timeout=_as_int(env.get("GITEA_BOTS_API_TIMEOUT"), 25),
            interval=_as_int(interval if interval is not None else env.get("GITEA_BOTS_INTERVAL"), 20),
            catchup_minutes=_as_int(env.get("GITEA_BOTS_CATCHUP_MINUTES"), 60),
            all_comments=_flag(env.get("GITEA_BOTS_ALL_COMMENTS")),
            auto_plan=_flag(env.get("GITEA_BOTS_AUTO_PLAN"), True),
            auto_plan_workers=_flag(env.get("GITEA_BOTS_AUTO_PLAN_WORKERS"), True),
            start_reconcile=_flag(env.get("GITEA_BOTS_START_RECONCILE"), True),
            verbose=_flag(env.get("GITEA_BOTS_VERBOSE")),
            notify_enabled=_flag(env.get("GITEA_BOTS_NOTIFY"), True),
            notify_mode=env.get("GITEA_BOTS_NOTIFY_SESSION_MODE", "fresh") or "fresh",
            notify_session=env.get("GITEA_BOTS_NOTIFY_SESSION")
            or env.get("OPENCODE_SESSION_ID", "") or "",
            notify_in_foreground=_flag(env.get("GITEA_BOTS_NOTIFY_IN_FOREGROUND")),
            model=env.get("GITEA_BOTS_MODEL", "") or "",
            spawn_workers=_flag(env.get("GITEA_BOTS_SPAWN_WORKERS"), True),
            archive_workers=_flag(env.get("GITEA_BOTS_ARCHIVE_WORKERS"), False),
            shared_login=_flag(env.get("GITEA_BOTS_SHARED_LOGIN"), True),
            max_concurrency=max(
                1, _as_int(env.get("GITEA_BOTS_MAX_CONCURRENCY"), 8)
            ),
            env=env,
        )
        return cfg

    # ------------------------------------------------------------- validation
    def require_remote(self, logger) -> None:
        if "/" not in self.repo or not self.api_base:
            logger.die("cannot derive Gitea host/owner-repo from origin — is 'origin' set?")

    def require_token(self, logger) -> None:
        if not self.token:
            logger.die("GITEA_TOKEN is empty — set it in .env (it is never printed)")

    def require_interval(self, logger) -> None:
        if self.interval < 5:
            logger.die(f"interval must be an integer >= 5 seconds (got: {self.interval})")

    def resolve_self(self, client, logger) -> None:
        """Resolve the token user's login for AUTO PLAN / directed mentions.

        Skipped when ``GITEA_BOTS_SELF`` is pinned. A failed ``/user`` lookup is
        a warning, not fatal — AUTO PLAN simply stays disabled for the run.
        """
        if self.self_login:
            return
        try:
            login = client.resolve_login()
        except GiteaError:
            login = ""
        if login:
            self.self_login = login
        else:
            logger.warn(
                "could not resolve the token user; AUTO PLAN / scan-unplanned "
                "need GITEA_BOTS_SELF"
            )
