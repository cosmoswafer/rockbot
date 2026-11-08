"""Generic Gitea plumbing shared by the label CLI and the gitea-bots skill.

The only external dependency is :mod:`requests`. Everything else is stdlib:
``.env`` parsing, git remote derivation and the small HTTP client live here so
both skills use one implementation.
"""

from __future__ import annotations

import os
import re
import subprocess
import threading
from dataclasses import dataclass
from pathlib import Path

import requests


class GiteaError(Exception):
    """A Gitea API call or environment resolution failed."""


# ---------------------------------------------------------------------------
# .env / git remote
# ---------------------------------------------------------------------------
def _expand(value: str, so_far: dict) -> str:
    """Expand ``${VAR}`` / ``${VAR:-default}`` against parsed values + process env."""
    var_re = re.compile(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}")

    def repl(match):
        key, default = match.group(1), match.group(2)
        return so_far.get(key) or os.environ.get(key) or (default or "")

    return var_re.sub(repl, value)


def load_dotenv(path) -> dict:
    """Minimal, dependency-free ``.env`` reader.

    Supports ``export``, surrounding quotes, ``#`` comments and
    ``${VAR}`` / ``${VAR:-default}`` expansion. Existing process environment
    wins (like ``python-dotenv``'s default ``override=False``): a real exported
    token is never clobbered by a stale ``.env`` value.
    """
    values: dict = {}
    if not path or not Path(path).is_file():
        return values
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError:
        return values
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export "):].strip()
        if "=" not in line:
            continue
        key, val = line.split("=", 1)
        key, val = key.strip(), val.strip()
        if len(val) >= 2 and val[0] == val[-1] and val[0] in ("'", '"'):
            val = val[1:-1]
        values[key] = _expand(val, values)
    return values


def git_output(args, cwd) -> str:
    try:
        out = subprocess.run(
            ["git", "-C", str(cwd), *args],
            capture_output=True, text=True, check=True,
        )
        return out.stdout.strip()
    except (subprocess.CalledProcessError, OSError):
        return ""


def derive_remote(remote: str):
    """Return ``(host, owner/repo)`` derived from a git remote, dropping ports.

    Handles URL-style (``ssh://git@host:port/owner/repo.git``) and scp-style
    (``git@host:owner/repo.git``). Never hardcodes host or owner/repo.
    """
    remote = (remote or "").strip()
    if remote.endswith(".git"):
        remote = remote[:-4]
    if "://" in remote:
        bare = remote.split("://", 1)[1]
        if "@" in bare:
            bare = bare.split("@", 1)[1]
        repo = bare.split("/", 1)[1] if "/" in bare else ""
        hostpart = bare.split("/", 1)[0]
    else:
        bare = remote.split("@", 1)[1] if "@" in remote else remote
        hostpart = bare.split(":", 1)[0]
        repo = bare.split(":", 1)[1] if ":" in bare else ""
    host = hostpart.split(":", 1)[0]
    return host, repo


def _as_int(value, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


@dataclass
class GiteaSettings:
    """Repo/API/token resolution for standalone label operations."""

    repo_root: Path
    repo: str = ""
    api_base: str = ""
    token: str = ""
    timeout: int = 25

    @classmethod
    def load(cls, script_dir) -> "GiteaSettings":
        script_dir = Path(script_dir).resolve()
        process_env = {k: v for k, v in os.environ.items() if v != ""}
        cwd_env = load_dotenv(Path.cwd() / ".env")

        root_override = (
            process_env.get("ISSUE_LABELS_ROOT")
            or cwd_env.get("ISSUE_LABELS_ROOT")
            or process_env.get("GITEA_BOTS_ROOT")
            or cwd_env.get("GITEA_BOTS_ROOT")
        )
        if root_override:
            repo_root = Path(root_override).resolve()
        else:
            top = git_output(["rev-parse", "--show-toplevel"], script_dir)
            repo_root = (
                Path(top).resolve() if top else (script_dir / "../../../..").resolve()
            )

        # Priority (low -> high): cwd .env < repo-root .env < process env.
        env = dict(cwd_env)
        env.update(load_dotenv(repo_root / ".env"))
        env.update(process_env)

        host, repo = derive_remote(git_output(["remote", "get-url", "origin"], repo_root))
        return cls(
            repo_root=repo_root,
            repo=repo,
            api_base=f"https://{host}/api/v1" if host else "",
            token=env.get("GITEA_TOKEN", "") or "",
            timeout=_as_int(
                env.get("GITEA_API_TIMEOUT")
                or env.get("GITEA_BOTS_API_TIMEOUT"),
                25,
            ),
        )

    def require(self) -> None:
        if "/" not in self.repo or not self.api_base:
            raise GiteaError(
                "cannot derive Gitea host/owner-repo from origin — is 'origin' set?"
            )
        if not self.token:
            raise GiteaError("GITEA_TOKEN is empty — set it in .env (never printed)")


# ---------------------------------------------------------------------------
# HTTP client
# ---------------------------------------------------------------------------
class GiteaClient:
    """Thin, injectable Gitea client. The only network surface in the repo."""

    def __init__(self, api_base: str, token: str, timeout: int = 25, session=None):
        self.api_base = (api_base or "").rstrip("/")
        self.token = token or ""
        self.timeout = timeout
        # A caller-supplied session (or a fake) makes the client injectable and
        # is always used as-is (tests, one-shot helpers). Without one, sessions
        # are kept per thread so concurrent prefetch never shares a connection
        # pool across threads (``requests.Session`` is not documented as
        # thread-safe).
        self._session = session
        self._local = threading.local()

    def _get_session(self):
        if self._session is not None:
            return self._session
        session = getattr(self._local, "session", None)
        if session is None:
            session = requests.Session()
            self._local.session = session
        return session

    def _headers(self, has_body: bool) -> dict:
        headers = {
            "Authorization": f"token {self.token}",  # literal 'token', not Bearer
            "Accept": "application/json",
        }
        if has_body:
            headers["Content-Type"] = "application/json"
        return headers

    def request(self, method: str, path: str, *, params=None, payload=None):
        """Perform one API call, returning decoded JSON (or ``None``).

        Raises :class:`GiteaError` on network errors, non-2xx responses, or
        invalid JSON. The token is never included in any error message.
        """
        url = self.api_base + path
        try:
            resp = self._get_session().request(
                method,
                url,
                params=params,
                json=payload if payload is not None else None,
                headers=self._headers(payload is not None),
                timeout=self.timeout,
            )
        except requests.RequestException as exc:
            raise GiteaError(
                f"network error calling {method} {path}: {type(exc).__name__}"
            ) from exc

        if not 200 <= resp.status_code < 300:
            message = ""
            try:
                data = resp.json()
                if isinstance(data, dict):
                    message = str(data.get("message") or data.get("error") or "")
            except ValueError:
                message = ""
            suffix = f" — {message.splitlines()[0]}" if message else ""
            raise GiteaError(f"HTTP {resp.status_code} from {method} {path}{suffix}")

        if not resp.content:
            return None
        try:
            return resp.json()
        except ValueError as exc:
            raise GiteaError(f"invalid JSON from {method} {path}") from exc

    def get(self, path, params=None):
        return self.request("GET", path, params=params)

    def post(self, path, payload=None, params=None):
        return self.request("POST", path, params=params, payload=payload)

    def put(self, path, payload=None):
        return self.request("PUT", path, payload=payload)

    def patch(self, path, payload=None):
        return self.request("PATCH", path, payload=payload)

    def delete(self, path, payload=None):
        return self.request("DELETE", path, payload=payload)

    def get_all(self, path, params=None, limit: int = 50, max_pages: int = 20):
        """Paginate a list endpoint (Gitea defaults to 30; we use 50)."""
        out = []
        base = dict(params or {})
        for page in range(1, max_pages + 1):
            query = dict(base, limit=limit, page=page)
            data = self.get(path, query)
            if not isinstance(data, list):
                break
            out.extend(data)
            if len(data) < limit:
                break
        return out

    def resolve_login(self) -> str:
        data = self.get("/user")
        return data.get("login", "") if isinstance(data, dict) else ""
