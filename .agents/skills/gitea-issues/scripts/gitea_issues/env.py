"""Where the server, the repo and the token come from — derived, never hardcoded.

The API base and the ``owner/repo`` path are read from a git remote (``origin``
by default); the token comes from the process environment or the project's
``.env``. Precedence is lowest to highest: ``.env`` in the current directory,
``.env`` at the repository root, the real process environment — a properly
exported token is never clobbered by a stale ``.env`` value.

Nothing in this module ever returns the token in an error message, and nothing
prints it. A missing token fails here, with a sentence that says what to do,
instead of failing later as a bare HTTP 401 or 404.
"""

from __future__ import annotations

import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_TIMEOUT = 25
_TOKEN_KEYS = ("GITEA_TOKEN",)

_VAR_RE = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}")


class ConfigError(Exception):
    """The environment is not usable: no remote, no token, no server."""


# ---------------------------------------------------------------------------
# .env parsing
# ---------------------------------------------------------------------------
def _expand(value: str, so_far: dict, process_env: dict) -> str:
    """Expand ``${VAR}`` and ``${VAR:-default}`` against parsed values + env."""

    def repl(match):
        key, default = match.group(1), match.group(2)
        return so_far.get(key) or process_env.get(key) or (default or "")

    return _VAR_RE.sub(repl, value)


def parse_env_text(text: str, process_env: dict | None = None) -> dict:
    """Parse ``.env`` text into a dict.

    Supports ``export`` prefixes, surrounding single/double quotes, ``#``
    comments and ``${VAR}`` / ``${VAR:-default}`` expansion. Blank and
    non-``key=value`` lines are ignored, so shell prompt noise or multiline
    heredocs in a hand-edited file do not break the read.
    """
    process_env = process_env or {}
    values: dict = {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        if line.startswith("export "):
            line = line[len("export "):].strip()
        key, val = line.split("=", 1)
        key, val = key.strip(), val.strip()
        if not key or " " in key:
            continue
        if len(val) >= 2 and val[0] == val[-1] and val[0] in ("'", '"'):
            val = val[1:-1]
        values[key] = _expand(val, values, process_env)
    return values


def env_file_candidates(
    cwd: Path | None = None,
    root: Path | None = None,
    override: str | None = None,
) -> list[Path]:
    """Env files to read, lowest precedence first.

    ``override`` (``--env-file`` / ``GITEA_ENV_FILE``) replaces the search with
    that single file, so an explicit choice always wins.
    """
    if override:
        path = Path(override).expanduser()
        return [path] if path.is_file() else []
    found: list[Path] = []
    for directory in [p for p in (cwd, root) if p]:
        candidate = Path(directory) / ".env"
        if candidate.is_file() and candidate not in found:
            found.append(candidate)
    return found


def read_env_files(paths: list[Path], process_env: dict | None = None) -> dict:
    """Merge the given env files in order; later files win."""
    merged: dict = {}
    for path in paths:
        try:
            merged.update(parse_env_text(path.read_text(encoding="utf-8"),
                                        process_env))
        except OSError:
            continue
    return merged


# ---------------------------------------------------------------------------
# git remote
# ---------------------------------------------------------------------------
def git_output(args: list[str], cwd: Path | None = None) -> str:
    """Run git and return trimmed stdout, or ``""`` when git is unavailable."""
    try:
        out = subprocess.run(
            ["git", *(["-C", str(cwd)] if cwd else []), *args],
            capture_output=True,
            text=True,
            check=True,
        )
    except (subprocess.CalledProcessError, OSError):
        return ""
    return out.stdout.strip()


def derive_remote(url: str) -> tuple[str, str]:
    """Split a git remote into ``(host, owner/repo)``, dropping ports and ``.git``.

    Handles all four shapes a clone can have::

        https://host/owner/repo.git
        ssh://git@host:33022/owner/repo.git
        git@host:owner/repo.git
        owner/repo            (no host — returned as the path only)

    The port belongs to git (SSH), never to the API, which is HTTPS.
    """
    remote = (url or "").strip()
    if remote.endswith(".git"):
        remote = remote[: -len(".git")]
    if "://" in remote:
        bare = remote.split("://", 1)[1]
        if "@" in bare:
            bare = bare.split("@", 1)[1]
        host_part, _, path = bare.partition("/")
    elif ":" in remote:
        bare = remote.split("@", 1)[1] if "@" in remote else remote
        host_part, _, path = bare.partition(":")
    else:
        return "", remote
    return host_part.split(":", 1)[0], path


def repo_root(start: Path | None = None) -> Path:
    top = git_output(["rev-parse", "--show-toplevel"], start or Path.cwd())
    return Path(top).resolve() if top else Path.cwd().resolve()


# ---------------------------------------------------------------------------
# settings
# ---------------------------------------------------------------------------
def _as_int(value, default: int) -> int:
    try:
        return int(str(value))
    except (TypeError, ValueError):
        return default


def _as_bool(value) -> bool:
    return str(value or "").strip().lower() in ("1", "true", "yes", "on")


@dataclass
class Settings:
    """Everything the client needs, resolved and validated."""

    api_base: str = ""
    repo: str = ""
    token: str = ""
    timeout: int = DEFAULT_TIMEOUT
    insecure: bool = False
    remote_name: str = "origin"
    remote_url: str = ""
    env_files: list[str] = field(default_factory=list)

    # --- safe to print -------------------------------------------------
    def summary(self) -> dict:
        """Diagnostics view — the token is reduced to a length, never a value."""
        return {
            "api_base": self.api_base,
            "repo": self.repo,
            "remote": self.remote_name,
            "remote_url": self.remote_url,
            "env_files": list(self.env_files),
            "timeout": self.timeout,
            "insecure": self.insecure,
            "token_chars": len(self.token),
        }

    @property
    def repo_url(self) -> str:
        """Web UI URL of the repository (the API base minus ``/api/v1``)."""
        return self.api_base.rsplit("/api", 1)[0] if self.api_base else ""

    def issues_url(self, number) -> str:
        return f"{self.repo_url}/issues/{number}" if self.repo_url else ""

    # --- hard failures -------------------------------------------------
    def require(self) -> "Settings":
        """Raise a :class:`ConfigError` explaining exactly what is missing."""
        problems: list[str] = []
        if not self.api_base:
            problems.append(
                "cannot derive the API base — `git remote get-url "
                f"{self.remote_name}` returned "
                f"{self.remote_url!r}. Fix it with "
                f"`git remote set-url {self.remote_name} <url>`, pass "
                "`--remote <name>`, or pass `--api https://host/api/v1`."
            )
        if "/" not in self.repo or not self.repo.strip("/"):
            problems.append(
                "cannot derive owner/repo — pass `--repo owner/repo` or fix "
                "the remote. Note that Gitea repo slugs are case-sensitive."
            )
        if not self.token:
            where = " or ".join(self.env_files) if self.env_files else ".env"
            problems.append(
                "GITEA_TOKEN is empty — add `GITEA_TOKEN=<token>` to "
                f"{where} (or export it). The token is read silently and "
                "never printed."
            )
        if problems:
            raise ConfigError("\n".join(problems))
        return self


def load_settings(
    repo: str | None = None,
    remote: str | None = None,
    api: str | None = None,
    timeout: int | None = None,
    insecure: bool = False,
    env_file: str | None = None,
    cwd: Path | None = None,
    root: Path | None = None,
    process_env: dict | None = None,
) -> Settings:
    """Resolve settings from flags, environment and ``.env``.

    Flag > process environment > repository-root ``.env`` > current-directory
    ``.env``. Reads are cheap and offline, so this is safe to call per command.
    ``cwd``/``root`` are injectable for tests; they default to the real
    directory and the git toplevel.
    """
    cwd = Path(cwd or Path.cwd())
    root = Path(root) if root else repo_root(cwd)
    process_env = (
        {k: v for k, v in os.environ.items() if v}
        if process_env is None
        else {k: v for k, v in process_env.items() if v}
    )

    remote_name = remote or process_env.get("GITEA_REMOTE") or "origin"
    root = repo_root(cwd)
    files = env_file_candidates(
        cwd=cwd,
        root=root,
        override=env_file or process_env.get("GITEA_ENV_FILE"),
    )

    merged = read_env_files(files, process_env)
    merged.update(process_env)

    remote_url = git_output(["remote", "get-url", remote_name], root or cwd)
    host, remote_repo = derive_remote(remote_url)

    # --api / GITEA_API wins over derivation so a non-standard HTTPS port or a
    # reverse proxy can be handled without editing the git remote.
    explicit_api = api or merged.get("GITEA_API")
    if explicit_api:
        api_base = explicit_api.rstrip("/")
        if not api_base.endswith("/api/v1"):
            api_base = f"{api_base}/api/v1"
    else:
        api_base = f"https://{host}/api/v1" if host else ""

    return Settings(
        api_base=api_base,
        repo=(repo or merged.get("GITEA_OWNER_REPO") or remote_repo).strip("/"),
        token=merged.get("GITEA_TOKEN", "") or "",
        timeout=_as_int(
            timeout or merged.get("GITEA_API_TIMEOUT"), DEFAULT_TIMEOUT
        ),
        insecure=bool(insecure) or _as_bool(merged.get("GITEA_INSECURE")),
        remote_name=remote_name,
        remote_url=remote_url,
        env_files=[str(p) for p in files],
    )
