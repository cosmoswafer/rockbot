"""The only code in the skill that touches the network.

Standard library only: :mod:`urllib.request` replaces ``requests``/``curl``, so
there is nothing to install. A transport is injected (any callable
``(method, url, headers, body) -> Response``), which is what makes the whole
CLI testable offline and ``--dry-run`` possible without special-casing.

Auth format matters and is fixed here: Gitea expects
``Authorization: token <value>`` — the literal word ``token``, never
``Bearer``. The token is never included in an exception message.
"""

from __future__ import annotations

import json
import ssl
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass

from .env import Settings

# Each status code gets a hint that names the likely cause and the fix, because
# these failures otherwise read as an unexplained "not found".
HINTS = {
    401: (
        "token rejected. Use `Authorization: token <value>` (the literal word "
        "token, not Bearer), or the token expired. `Bearer` fails even with a "
        "valid token."
    ),
    403: (
        "forbidden. Either the token has no access to this repo, or it lacks "
        "the scope this endpoint needs (e.g. `read:user` for `/user`). "
        "Private repos also answer 404, so check the repo slug too."
    ),
    404: (
        "not found. Usually a wrong host or `owner/repo`: both come from the "
        "`origin` remote, and Gitea repo slugs are case-sensitive. A missing "
        "token on a private repo also looks like 404."
    ),
    405: "method not allowed — wrong endpoint shape for this Gitea version.",
    409: (
        "conflict — the resource already exists (e.g. a label with that name, "
        "or a duplicate comment)."
    ),
    422: (
        "validation failed — the payload is missing a required field or a "
        "value is out of range. Check label IDs (numeric), milestone IDs "
        "(numeric, under the key `milestone`) and title/body lengths."
    ),
}


class GiteaError(Exception):
    """A network failure, a non-2xx response, or undecodable JSON."""

    def __init__(self, message: str, status: int | None = None, hint: str = ""):
        super().__init__(message)
        self.status = status
        self.hint = hint or HINTS.get(status or 0, "")

    def report(self) -> str:
        """One or two lines: what happened, then what to do about it."""
        if self.hint:
            return f"{self}\n  hint: {self.hint}"
        return str(self)


@dataclass
class Response:
    status: int
    body: bytes = b""


class UrllibTransport:
    """Minimal HTTP transport over :mod:`urllib.request`."""

    def __init__(self, timeout: int = 25, insecure: bool = False):
        self.timeout = timeout
        self.insecure = insecure

    def _ssl_context(self, url: str):
        """An unverified context only when the caller opted into ``--insecure``."""
        if url.startswith("https://") and self.insecure:
            return ssl._create_unverified_context()
        return None

    def __call__(self, method, url, headers, body):
        request = urllib.request.Request(url, data=body, headers=headers,
                                        method=method)
        try:
            with urllib.request.urlopen(
                request, timeout=self.timeout,
                context=self._ssl_context(url),
            ) as response:
                return Response(response.status, response.read())
        except urllib.error.HTTPError as error:  # a real response, non-2xx
            try:
                payload = error.read()
            except Exception:  # pragma: no cover - unreadable error body
                payload = b""
            return Response(error.code, payload)
        except urllib.error.URLError as error:
            raise GiteaError(
                f"cannot reach {urllib.parse.urlsplit(url).netloc}: "
                f"{error.reason}"
            ) from error


class GiteaClient:
    """Thin client for the Gitea REST API (``/api/v1``)."""

    def __init__(self, settings: Settings, transport=None):
        self.settings = settings
        self.api_base = (settings.api_base or "").rstrip("/")
        self.token = settings.token or ""
        if transport is None:
            transport = UrllibTransport(
                timeout=settings.timeout, insecure=settings.insecure
            )
        self.transport = transport

    # ---------------------------------------------------------------- paths
    def repo_path(self, suffix: str = "") -> str:
        """``/repos/owner/repo`` plus an optional suffix, joined by one slash."""
        base = f"/repos/{self.settings.repo}"
        return base if not suffix else f"{base}/{suffix.lstrip('/')}"

    def url(self, path: str, params: dict | None = None) -> str:
        url = f"{self.api_base}{path if path.startswith('/') else '/' + path}"
        query = _clean_params(params)
        return f"{url}?{urllib.parse.urlencode(query)}" if query else url

    def headers(self, has_body: bool) -> dict:
        headers = {
            "Authorization": f"token {self.token}",
            "Accept": "application/json",
            "User-Agent": "gitea-issues-skill",
        }
        if has_body:
            headers["Content-Type"] = "application/json"
        return headers

    # ------------------------------------------------------------- requests
    def request(self, method: str, path: str, params=None, payload=None):
        """One API call, decoded. Raises :class:`GiteaError` on any failure."""
        if not self.token:
            raise GiteaError(
                "no token loaded — refusing to call the API. Add GITEA_TOKEN "
                "to .env; an unauthenticated request would fail as 401/404."
            )
        url = self.url(path, params)
        body = None
        if payload is not None:
            body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        response = self.transport(method, url, self.headers(body is not None),
                                 body)
        return self._decode(method, path, response)

    def _decode(self, method: str, path: str, response: Response):
        text = (response.body or b"").decode("utf-8", "replace").strip()
        data = None
        if text:
            try:
                data = json.loads(text)
            except ValueError:
                data = None

        if not 200 <= response.status < 300:
            message = ""
            if isinstance(data, dict):
                message = str(data.get("message") or data.get("error") or "")
            elif text:
                message = text.splitlines()[0]
            suffix = f" — {message}" if message else ""
            raise GiteaError(
                f"HTTP {response.status} from {method} {path}{suffix}",
                status=response.status,
            )

        if data is None:
            return None
        return data

    def get(self, path, params=None):
        return self.request("GET", path, params=params)

    def post(self, path, payload=None, params=None):
        return self.request("POST", path, params=params, payload=payload)

    def put(self, path, payload=None, params=None):
        return self.request("PUT", path, params=params, payload=payload)

    def patch(self, path, payload=None, params=None):
        return self.request("PATCH", path, params=params, payload=payload)

    def delete(self, path, payload=None, params=None):
        return self.request("DELETE", path, params=params, payload=payload)

    # ----------------------------------------------------------- pagination
    def get_all(self, path, params=None, limit: int = 50, max_pages: int = 20):
        """Collect every page of a list endpoint (Gitea's default is 30)."""
        out: list = []
        base = dict(params or {})
        for page in range(1, max_pages + 1):
            batch = self.get(path, dict(base, limit=limit, page=page))
            if not isinstance(batch, list):
                break
            out.extend(batch)
            if len(batch) < limit:
                break
        return out


def _clean_params(params: dict | None) -> dict:
    """Drop ``None``/empty values so the query string stays clean."""
    if not params:
        return {}
    return {
        key: value
        for key, value in params.items()
        if value is not None and str(value) != ""
    }


def is_pull_request(item) -> bool:
    """Gitea's issue endpoints return pull requests too — this tells them apart."""
    if not isinstance(item, dict):
        return False
    return item.get("pull_request") is not None or bool(item.get("is_pull"))


def current_login(client: GiteaClient) -> tuple[str, str]:
    """Resolve the token user's login, with a documented fallback.

    Returns ``(login, source)`` where source is ``/user``, ``fallback-issue`` or
    ``""``. Issue-scoped tokens (no ``read:user``) get a 403 from ``/user``; the
    creator of the most recent issue is then used, because the token user filed
    the issues it created.
    """
    try:
        data = client.get("/user")
        login = (data or {}).get("login") or ""
        if login:
            return login, "/user"
    except GiteaError as error:
        if error.status != 403:
            raise
    try:
        recent = client.get(client.repo_path("issues"),
                            {"state": "all", "limit": 1}) or []
        if recent:
            login = ((recent[0] or {}).get("user") or {}).get("login") or ""
            if login:
                return login, "fallback-issue"
    except GiteaError:
        pass
    return "", ""
