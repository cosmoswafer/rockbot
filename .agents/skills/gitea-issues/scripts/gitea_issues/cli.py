"""Argument parsing and the composition root for the gitea-issues CLI.

Wiring happens here and only here: settings are resolved, one client is built,
and each handler gets explicit arguments and returns an exit code. Domain
decisions (what a valid body looks like, how label sets merge, which milestone
a reference names) live in the sibling modules, not here.

Every invocation re-derives the remote and re-reads ``.env`` by itself, which
is what removes the classic footgun of a token that only lives in one shell.

Exit codes: ``0`` success, ``1`` configuration/API/domain failure, ``2`` bad
usage (argparse), ``3`` a policy guard was hit (closing without ``--yes``).
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from dataclasses import replace
from pathlib import Path

from . import issues as issue_ops
from . import labels as label_ops
from . import milestones as milestone_ops
from . import render, tasks as task_ops, template
from .client import GiteaClient, GiteaError, current_login
from .env import ConfigError, load_settings

PROGRAM = "gitea.py"
POLICY_EXIT = 3
OFFLINE_COMMANDS = ("template",)
WRITE_METHODS = ("POST", "PUT", "PATCH", "DELETE")

DESCRIPTION = """\
Gitea issue toolkit (stdlib Python only). The server, the owner/repo and the
token are derived from the git remote and .env — nothing is hardcoded, and the
token is never printed. Issues are never closed on your own initiative:
`close` refuses to run without --yes, which stands for an explicit human
request.
"""

EPILOG = """\
examples:
  gitea.py doctor
  gitea.py list --milestone v3.7.10 --label working
  gitea.py view 687 --comments
  gitea.py create --title "Quick camera: use server time" \\
      --summary "Records inherit the photo's EXIF date." \\
      --task "Ignore EXIF in the quick camera flow" \\
      --task "Stamp records with server time" \\
      --milestone v3.7.11 --related 615
  gitea.py comment 687 --body-file ./tmp/note.md
  gitea.py tasks 687 tick all --drop "Acceptance criteria"
  gitea.py label status 687 responded committed
  gitea.py milestone report v3.7.10
  gitea.py api GET repos/OWNER/REPO/issues/687/timeline
"""


# ---------------------------------------------------------------------------
# dry-run client
# ---------------------------------------------------------------------------
class DryRunClient:
    """Runs reads for real, records writes, and answers them with canned data.

    Handlers take the same code path they would take live, so the printed plan
    is honest — and no write can leak out by accident.
    """

    def __init__(self, inner: GiteaClient, printer: render.Printer):
        self.inner = inner
        self.printer = printer
        self.api_base = inner.api_base
        self.settings = inner.settings
        self.planned: list[str] = []

    def __getattr__(self, name):
        return getattr(self.inner, name)

    def request(self, method: str, path: str, params=None, payload=None):
        method = method.upper()
        if method not in WRITE_METHODS:
            return self.inner.request(method, path, params=params,
                                      payload=payload)
        body = json.dumps(payload, ensure_ascii=False) if payload is not None else ""
        line = f"DRY-RUN {method} {path}" + (f" {body}" if body else "")
        self.planned.append(line)
        print(line, file=sys.stderr)
        return _canned(method, path, payload)

    def get(self, path, params=None):
        return self.inner.get(path, params)

    def get_all(self, path, params=None, limit=50, max_pages=20):
        return self.inner.get_all(path, params, limit=limit, max_pages=max_pages)

    def post(self, path, payload=None, params=None):
        return self.request("POST", path, params=params, payload=payload)

    def put(self, path, payload=None, params=None):
        return self.request("PUT", path, params=params, payload=payload)

    def patch(self, path, payload=None, params=None):
        return self.request("PATCH", path, params=params, payload=payload)

    def delete(self, path, payload=None, params=None):
        return self.request("DELETE", path, params=params, payload=payload)


def _canned(method: str, path: str, payload):
    """A believable response shape so handlers can format results under --dry-run."""
    data = dict(payload) if isinstance(payload, dict) else {}
    if method == "PUT" and path.rstrip("/").endswith("labels"):
        return [{"id": label_id} for label_id in data.get("labels") or []]
    labels = [
        item if isinstance(item, dict) else {"id": item}
        for item in (data.get("labels") or [])
    ]
    return {
        "number": 0,
        "id": 0,
        "title": data.get("title", ""),
        "name": data.get("name", ""),
        "state": data.get("state", "open"),
        "body": data.get("body", ""),
        "html_url": "",
        "labels": labels,
        "milestone": {"id": data.get("milestone")} if "milestone" in data else None,
        "assignees": [{"login": login} for login in data.get("assignees") or []],
    }


# ---------------------------------------------------------------------------
# parser
# ---------------------------------------------------------------------------
def _add_global(parser: argparse.ArgumentParser) -> None:
    group = parser.add_argument_group("connection and output")
    group.add_argument("--repo", metavar="OWNER/REPO",
                       help="override the owner/repo derived from the remote")
    group.add_argument("--remote", metavar="NAME",
                       help="git remote to derive from (default: origin)")
    group.add_argument("--api", metavar="URL",
                       help="override the API base, e.g. https://host:8443/api/v1")
    group.add_argument("--env-file", metavar="PATH",
                       help="read the token from this file instead of .env")
    group.add_argument("--timeout", type=int, metavar="SECONDS",
                       help="HTTP timeout (default: 25)")
    group.add_argument("--insecure", action="store_true",
                       help="skip TLS verification (self-signed servers only)")
    group.add_argument("--dry-run", action="store_true",
                       help="print the writes instead of sending them")
    group.add_argument("--json", action="store_true",
                       help="machine-readable result on stdout")
    group.add_argument("-q", "--quiet", action="store_true",
                       help="no prose, just the essentials")


def _body_args(parser: argparse.ArgumentParser, inline: str = "--body") -> None:
    parser.add_argument(inline, help="text; pass - to read stdin")
    parser.add_argument("--body-file", metavar="PATH", help="text from a file")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog=PROGRAM, description=DESCRIPTION, epilog=EPILOG,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    _add_global(parser)
    sub = parser.add_subparsers(dest="command", metavar="COMMAND")

    sub.add_parser("doctor", help="read-only setup and access checks")
    sub.add_parser("me", help="print the token user's login (with fallback)")

    tpl = sub.add_parser("template", help="print the standard issue body template")
    tpl.add_argument("--out", metavar="PATH", help="write the template to a file")
    tpl.add_argument("--force", action="store_true", help="overwrite an existing file")

    listing = sub.add_parser("list", aliases=["ls"], help="list issues with filters")
    listing.add_argument("--state", choices=issue_ops.STATES, default="open")
    listing.add_argument("--label", action="append", metavar="NAME",
                         help="only issues with this label (repeatable)")
    listing.add_argument("--milestone", metavar="REF", help="milestone title or ID")
    listing.add_argument("--assignee", metavar="LOGIN")
    listing.add_argument("--author", metavar="LOGIN")
    listing.add_argument("--search", metavar="TEXT", help="server-side text search")
    listing.add_argument("--include-pulls", action="store_true",
                         help="keep pull requests in the result")
    listing.add_argument("--limit", type=int, default=30)
    listing.add_argument("--all", action="store_true", help="walk every page")
    listing.add_argument("--columns", metavar="K,K",
                         help="columns: " + ",".join(issue_ops.DEFAULT_COLUMNS)
                         + ",assignees,author,url,created")

    view = sub.add_parser("view", help="show one issue")
    view.add_argument("number", type=int)
    view.add_argument("--comments", action="store_true", help="include the thread")
    view.add_argument("--limit", type=int, default=50, help="max comments")

    create = sub.add_parser("create", help="create an issue (template-checked)")
    create.add_argument("--title", required=True)
    _body_args(create)
    create.add_argument("--summary", help="Summary paragraph of a generated body")
    create.add_argument("--task", action="append", metavar="TEXT",
                        help="add a `- [ ]` task (repeatable)")
    create.add_argument("--related", action="append", metavar="N",
                        help="reference issues/PRs under ## Related (repeatable)")
    create.add_argument("--assign", action="append", metavar="LOGIN")
    create.add_argument("--assign-me", action="store_true",
                        help="assign the token user")
    create.add_argument("--milestone", metavar="REF",
                        help="milestone title or ID — resolved to a numeric ID")
    create.add_argument("--create-milestone", action="store_true",
                        help="create the milestone when nothing matches")
    create.add_argument("--due", metavar="YYYY-MM-DD",
                        help="due date for a milestone created here")
    create.add_argument("--label", action="append", metavar="NAME")
    create.add_argument("--create-labels", action="store_true",
                        help="create missing labels instead of failing")
    create.add_argument("--force", action="store_true",
                        help="file a body that breaks the template rules")

    edit = sub.add_parser("edit", help="edit fields, body, labels or assignees")
    edit.add_argument("number", type=int)
    edit.add_argument("--title")
    _body_args(edit)
    edit.add_argument("--milestone", metavar="REF")
    edit.add_argument("--create-milestone", action="store_true")
    edit.add_argument("--due", metavar="YYYY-MM-DD")
    edit.add_argument("--no-milestone", action="store_true",
                      help="clear the milestone")
    edit.add_argument("--assign", action="append", metavar="LOGIN")
    edit.add_argument("--unassign", action="append", metavar="LOGIN")
    edit.add_argument("--label-add", action="append", metavar="NAME")
    edit.add_argument("--label-remove", action="append", metavar="NAME")
    edit.add_argument("--create-labels", action="store_true")
    edit.add_argument("--related", action="append", metavar="N")
    edit.add_argument("--force", action="store_true")

    comment = sub.add_parser("comment", help="post a comment on an issue")
    comment.add_argument("number", type=int)
    _body_args(comment)
    comment.add_argument("--label", action="append", metavar="NAME",
                         help="also add these labels")
    comment.add_argument("--status", action="append", metavar="TOKEN",
                         help="also apply status labels (seeing|responded|…)")

    comments = sub.add_parser("comments", help="print an issue's comments")
    comments.add_argument("number", type=int)
    comments.add_argument("--limit", type=int, default=50)

    task_cmd = sub.add_parser("tasks", help="show or tick the body checklist")
    task_cmd.add_argument("number", type=int)
    task_cmd.add_argument("action", nargs="?", default="show",
                          choices=("show", "tick", "untick", "drop"),
                          help="what to do (default: show)")
    task_cmd.add_argument("targets", nargs="*", metavar="TARGET",
                          help="'all', '3', '1,4', '2-5', or a unique phrase")
    task_cmd.add_argument("--drop", action="append", metavar="HEADING",
                          dest="drop_headings",
                          help="delete a section, e.g. 'Acceptance criteria'")

    label_cmd = sub.add_parser("label", help="labels: repo, issue, and status")
    label_sub = label_cmd.add_subparsers(dest="action", metavar="ACTION")
    label_sub.add_parser("list", help="labels defined in the repo")
    shown = label_sub.add_parser("show", help="labels on one issue")
    shown.add_argument("number", type=int)
    created = label_sub.add_parser("create", help="create a repo label (idempotent)")
    created.add_argument("name")
    created.add_argument("--color", default=label_ops.DEFAULT_COLOR)
    created.add_argument("--description", default="")
    added = label_sub.add_parser("add", help="add labels, keeping the others")
    added.add_argument("number", type=int)
    added.add_argument("names", nargs="+")
    added.add_argument("--create", action="store_true")
    removed = label_sub.add_parser("remove", help="remove labels")
    removed.add_argument("number", type=int)
    removed.add_argument("names", nargs="+")
    setter = label_sub.add_parser("set", help="replace the whole label set")
    setter.add_argument("number", type=int)
    setter.add_argument("names", nargs="*")
    setter.add_argument("--create", action="store_true")
    status = label_sub.add_parser("status", help="apply the shared status vocabulary")
    status.add_argument("number", type=int)
    status.add_argument("tokens", nargs="+",
                        help="seeing|responded|working|question|committed|none")
    label_sub.add_parser("bootstrap", help="create the five canonical status labels")
    cleared = label_sub.add_parser("clear", help="strip every status label")
    cleared.add_argument("number", type=int)

    ms_cmd = sub.add_parser("milestone", help="milestones: list, create, assign, review")
    ms_sub = ms_cmd.add_subparsers(dest="action", metavar="ACTION")
    ms_list = ms_sub.add_parser("list", help="milestones with issue counts")
    ms_list.add_argument("--state", choices=("open", "closed", "all"), default="open")
    ms_sub.add_parser("latest", help="the newest open milestone")
    ms_create = ms_sub.add_parser("create", help="create a milestone (idempotent)")
    ms_create.add_argument("title")
    ms_create.add_argument("--due", metavar="YYYY-MM-DD")
    ms_create.add_argument("--description", default="")
    ms_set = ms_sub.add_parser("set", help="assign a milestone to an issue")
    ms_set.add_argument("number", type=int)
    ms_set.add_argument("ref", help="milestone title, fragment, or ID")
    ms_set.add_argument("--create-milestone", action="store_true")
    ms_set.add_argument("--due", metavar="YYYY-MM-DD")
    ms_unset = ms_sub.add_parser("unset", help="clear an issue's milestone")
    ms_unset.add_argument("number", type=int)
    ms_issues = ms_sub.add_parser("issues", help="issues in a milestone")
    ms_issues.add_argument("ref")
    ms_issues.add_argument("--state", choices=issue_ops.STATES, default="open")
    ms_report = ms_sub.add_parser("report", help="per-issue checklist progress")
    ms_report.add_argument("ref")
    ms_report.add_argument("--state", choices=issue_ops.STATES, default="open")
    ms_state = ms_sub.add_parser("state", help="open or close a milestone")
    ms_state.add_argument("ref")
    ms_state.add_argument("state", choices=("open", "closed"))

    close = sub.add_parser("close", help="close an issue — a human must have asked")
    close.add_argument("number", type=int)
    close.add_argument("--yes", action="store_true",
                       help="confirm that a human explicitly asked for this close")
    close.add_argument("--note", help="post this comment first")
    close.add_argument("--note-file", metavar="PATH")
    close.add_argument("--status", action="append", metavar="TOKEN")

    reopen = sub.add_parser("reopen", help="reopen an issue")
    reopen.add_argument("number", type=int)
    reopen.add_argument("--note", help="post this comment first")
    reopen.add_argument("--note-file", metavar="PATH")

    api = sub.add_parser("api", help="raw endpoint escape hatch (path omits /api/v1)")
    api.add_argument("method", nargs="?", default="GET")
    api.add_argument("path", nargs="?",
                     help="e.g. repos/OWNER/REPO/issues/687/timeline")
    api.add_argument("--data", metavar="JSON", help="inline JSON payload")
    api.add_argument("--input", metavar="PATH", help="JSON payload from a file")
    api.add_argument("--param", action="append", metavar="K=V",
                     help="query parameter (repeatable)")

    return parser


# ---------------------------------------------------------------------------
# shared helpers
# ---------------------------------------------------------------------------
class UsageError(Exception):
    """A handler-level input problem: a message plus exit 1."""


def _file_text(path: str) -> str:
    try:
        return Path(path).expanduser().read_text(encoding="utf-8")
    except OSError as error:
        raise UsageError(f"cannot read {path}: {error}") from error


def read_text(args, inline_attr: str = "body", file_attr: str = "body_file",
              required: bool = True) -> str:
    """Text from ``--<inline>``/``--<inline> -``/``--<attr>-file``."""
    path = getattr(args, file_attr, None)
    if path:
        return _file_text(path)
    inline = getattr(args, inline_attr, None)
    if inline == "-":
        return sys.stdin.read()
    if inline is not None:
        return inline
    if required:
        raise UsageError(
            "no text given — pass "
            f"--{inline_attr.replace('_','-')} TEXT, "
            f"--{inline_attr.replace('_','-')} - (stdin), or "
            f"--{file_attr.replace('_','-')} PATH."
        )
    return ""


def build_body(args) -> str:
    """A compliant body from ``--summary``/``--task``/``--related``, or a given one."""
    explicit = read_text(args, required=False)
    if explicit.strip():
        return explicit
    task_lines = list(args.task or [])
    summary = (args.summary or "").strip()
    if not summary and not task_lines:
        raise UsageError(
            "nothing to file: pass --body/--body-file, or --summary with at "
            "least one --task (`template` prints the standard shape)."
        )
    parts = ["## Summary", "", summary or "(fill in the summary)", "",
             "## Tasks", ""]
    parts += [f"- [ ] {line.strip()}" for line in task_lines] or \
             ["- [ ] <add a concrete, verifiable task>"]
    related = [str(ref).lstrip("#") for ref in (args.related or [])]
    if related:
        parts += ["", "## Related", ""] + [f"- #{ref}" for ref in related]
    return "\n".join(parts) + "\n"


def resolve_login(client) -> str:
    try:
        login, _ = current_login(client)
        return login
    except GiteaError:
        return ""


def milestone_for(client, args):
    ref = getattr(args, "milestone", None)
    if not ref:
        return None
    return milestone_ops.resolve(
        client, ref, create=bool(getattr(args, "create_milestone", False)),
        due=getattr(args, "due", "") or "",
    )


def _issue_table(rows, columns) -> str:
    if not rows:
        return "no issues matched"
    lines = []
    for row in rows:
        cells = []
        for column in columns:
            value = row.get(column)
            if isinstance(value, list):
                value = ", ".join(str(item) for item in value)
            cells.append(_cell(column, value))
        lines.append(cells)
    return render.table(lines)


def _cell(column: str, value) -> str:
    if column == "number":
        return f"#{value}"
    if column == "title":
        return render.clip(value, 80)
    if value in (None, ""):
        return "-"
    return str(value)


# ---------------------------------------------------------------------------
# handlers
# ---------------------------------------------------------------------------
def cmd_template(client, args, out, settings) -> int:
    text = template.ISSUE_TEMPLATE
    if args.out:
        path = Path(args.out).expanduser()
        if path.exists() and not args.force:
            raise UsageError(f"{path} already exists — pass --force to overwrite.")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        out.note(f"wrote {path}")
        return 0
    print(text, end="")
    return 0


def cmd_doctor(client, args, out, settings) -> int:
    """Read-only checks: runtime, .env, remote, token, access. Prints PASS/FAIL."""
    checks: list[dict] = []

    def add(status: str, message: str) -> None:
        checks.append({"status": status, "message": message})

    def ok(condition: bool, good: str, bad: str) -> None:
        add("PASS" if condition else "FAIL", good if condition else bad)

    ok(sys.version_info >= (3, 8),
       f"python {sys.version.split()[0]} at {sys.executable}",
       f"python {sys.version.split()[0]} is too old — 3.8 or newer is needed")
    git_path = shutil.which("git")
    add("PASS" if git_path else "WARN",
        f"git: {git_path}" if git_path else
        "git is missing — remote derivation cannot work; pass --repo and --api")
    add("INFO", f"curl: {shutil.which('curl') or 'missing (only the bash recipes need it)'}")

    summary = settings.summary()
    add("PASS" if summary["env_files"] else "WARN",
        "env file: " + (", ".join(summary["env_files"])
                        or "no .env found — GITEA_TOKEN must be exported"))
    ok(bool(summary["token_chars"]),
       f"GITEA_TOKEN: set ({summary['token_chars']} characters, value hidden)",
       "GITEA_TOKEN is empty — add `GITEA_TOKEN=<token>` to .env")
    add("INFO", f"remote: {summary['remote']} {summary['remote_url'] or '(none)'}")
    add("INFO", f"api:    {summary['api_base'] or '(not derived)'}")
    add("INFO", f"repo:   {summary['repo'] or '(not derived)'}")
    add("INFO", f"timeout: {summary['timeout']}s  tls verify: "
                f"{'off (--insecure)' if summary['insecure'] else 'on'}")

    probes = (
        ("repo", lambda: client.get(client.repo_path())),
        ("labels", lambda: client.get_all(client.repo_path("labels"), limit=100)),
        ("milestones",
         lambda: client.get_all(client.repo_path("milestones"), {"state": "open"})),
    )
    for name, probe in probes:
        try:
            data = probe()
            if name == "repo":
                full_name = str((data or {}).get("full_name") or "")
                ok(full_name.replace(" ", "") != "",
                   f"repo access: {full_name or 'ok'}",
                   "repo access returned no full_name — check the owner/repo casing")
            else:
                add("PASS", f"{name} readable: {len(data or [])} item(s)")
        except GiteaError as error:
            add("FAIL", f"{name}: {error}")
            if error.hint:
                add("FAIL", f"  hint: {error.hint}")

    try:
        login, source = current_login(client)
    except GiteaError as error:
        login, source = "", str(error)
    if login and source == "/user":
        add("PASS", f"token user: {login}")
    elif login:
        add("WARN", f"token user: {login} (inferred from the newest issue — "
                    "the token has no read:user scope)")
    else:
        add("WARN", "token user unknown — create issues without assignees "
                    "and set them later")

    failed = any(item["status"] == "FAIL" for item in checks)
    out.emit_either(
        {"ok": not failed, "settings": summary, "checks": checks},
        "== gitea-issues diagnostics ==\n"
        + "\n".join(f"{item['status']:<5} {item['message']}" for item in checks)
        + ("" if not failed else
           "\n\nFix every FAIL before writing to the tracker."),
    )
    return 1 if failed else 0


def cmd_me(client, args, out, settings) -> int:
    login, source = current_login(client)
    if not login:
        raise UsageError(
            "could not resolve a login (no read:user scope and no issue to "
            "infer from). Create the issue without assignees and set them later."
        )
    out.emit_either(
        {"login": login, "source": source},
        f"{login}  ({'from /user' if source == '/user' else 'inferred from the newest issue'})",
    )
    return 0


def cmd_list(client, args, out, settings) -> int:
    columns = [column.strip() for column in (args.columns or "").split(",")
               if column.strip()] or list(issue_ops.DEFAULT_COLUMNS)
    rows = issue_ops.list_issues(
        client, state=args.state, labels=args.label, milestone=args.milestone,
        assignee=args.assignee or "", author=args.author or "",
        search=args.search or "", includes_pulls=args.include_pulls,
        limit=args.limit, all_pages=args.all,
    )
    out.emit_either({"count": len(rows), "issues": rows},
                    _issue_table(rows, columns))
    return 0


def cmd_view(client, args, out, settings) -> int:
    issue = issue_ops.fetch_issue(client, args.number)
    row = issue_ops.issue_row(issue, settings.repo_url)
    comments = issue_ops.fetch_comments(client, args.number, args.limit) \
        if args.comments else []
    out.emit_either(
        {"issue": row, "body": issue.get("body") or "", "comments": comments},
        issue_ops.render_issue(row, issue.get("body") or "", comments),
    )
    return 0


def cmd_create(client, args, out, settings) -> int:
    body = build_body(args)
    problems = template.validate_issue_body(body)
    if problems:
        if not args.force:
            raise UsageError(
                "this body would break the issue template:\n  - "
                + "\n  - ".join(problems)
                + "\nFix the body, or pass --force to file it anyway."
            )
        out.note("warning: filed despite template problems: "
                 + "; ".join(problems))

    assignees = list(args.assign or [])
    if args.assign_me:
        login = resolve_login(client)
        if not login:
            raise UsageError("--assign-me needs a login — see `me`.")
        if login not in assignees:
            assignees.append(login)

    milestone = milestone_for(client, args)
    label_ids = (label_ops.resolve_label_ids(client, args.label,
                                            create=args.create_labels)["ids"]
                 if args.label else [])

    payload = issue_ops.create_payload(
        args.title, body, assignees=assignees,
        milestone_id=(milestone or {}).get("id"), label_ids=label_ids,
    )
    created = issue_ops.create_issue(client, payload, expected_label_ids=label_ids)
    row = issue_ops.issue_row(created, settings.repo_url)
    number = row["number"] or 0
    placeholder = not number  # a dry run has no real number yet
    url = "" if placeholder else (row["url"] or settings.issues_url(number))
    progress = task_ops.progress(task_ops.parse_tasks(body))
    milestone_title = (milestone or {}).get("title") or ""

    out.emit_either(
        {"number": number, "title": args.title, "state": row["state"],
         "milestone": row["milestone"] or milestone_title,
         "labels": row["labels"] or list(args.label or []),
         "assignees": row["assignees"] or assignees,
         "tasks": progress, "html_url": url, "dry_run": placeholder},
        "\n".join(filter(None, [
            ("(dry run) would create — " if placeholder else f"#{number} created — ")
            + args.title,
            url,
            f"milestone {milestone_title}" if milestone_title else "",
            "labels " + ", ".join(args.label) if args.label else "",
            "assignees " + ", ".join(assignees) if assignees else "",
            "" if progress == "0/0" else
            f"tasks {progress} — tick them with `tasks {number} tick all`"
            if not placeholder else f"tasks {progress}",
        ])),
    )
    return 0


def cmd_edit(client, args, out, settings) -> int:
    issue = issue_ops.fetch_issue(client, args.number)
    fields: dict = {}
    if args.title:
        fields["title"] = args.title

    body = read_text(args, required=False)
    if body.strip():
        problems = template.validate_issue_body(body)
        if problems and not args.force:
            raise UsageError(
                "the new body breaks the template:\n  - "
                + "\n  - ".join(problems) + "\n(pass --force to override)"
            )
    if body.strip() or args.related:
        base = body if body.strip() else (issue.get("body") or "")
        fields["body"] = issue_ops.append_related(base, args.related)

    milestone = None
    if args.no_milestone:
        fields["milestone"] = 0
    else:
        milestone = milestone_for(client, args)
        if milestone:
            fields["milestone"] = milestone["id"]

    current = [item.get("login") for item in issue.get("assignees") or []]
    wanted = issue_ops.merge_assignees(current, args.assign, args.unassign)
    if args.assign or args.unassign:
        if sorted(wanted) != sorted(current):
            fields["assignees"] = wanted
        else:
            out.note("assignees already set that way")

    if not fields and not args.label_add and not args.label_remove:
        raise UsageError("nothing to change — no field was given.")

    updated = issue_ops.patch_issue(client, args.number, **fields) if fields else issue
    row = issue_ops.issue_row(updated or issue, settings.repo_url)
    if not row["number"]:  # a dry run echoes the payload, not the stored issue
        row["number"] = args.number

    reported: list[str] = []
    if "title" in fields:
        row["title"] = fields["title"]
        reported.append(f"title: {fields['title']}")
    if "body" in fields:
        checklist = task_ops.parse_tasks(fields["body"])
        reported.append(f"body: {len(fields['body'])} characters"
                        + (f", tasks {task_ops.progress(checklist)}" if checklist else ""))
    if "milestone" in fields:
        milestone_title = (milestone or {}).get("title") or ""
        row["milestone"] = milestone_title
        reported.append("milestone: " + (milestone_title or "(cleared)"))
    if "assignees" in fields:
        row["assignees"] = fields["assignees"]
        reported.append("assignees: " + (", ".join(fields["assignees"]) or "(none)"))

    if args.label_add or args.label_remove:
        applied = _move_labels(client, args.number, args.label_add,
                              args.label_remove, create=args.create_labels)
        names = [str(label.get("name") or "") for label in applied
                 if label.get("name")]
        parts = []
        if args.label_add:
            parts.append("added " + ", ".join(args.label_add))
        if args.label_remove:
            parts.append("removed " + ", ".join(args.label_remove))
        line = "labels: " + "; ".join(parts)
        if names:
            line += " — full set now: " + ", ".join(names)
            row["labels"] = names
        reported.append(line)

    out.emit_either(
        {"number": row["number"], "changed": sorted(fields), "title": row["title"],
         "state": row["state"], "milestone": row["milestone"],
         "labels": row["labels"], "assignees": row["assignees"]},
        "\n".join(filter(None, [
            f"#{row['number']} updated"
            + (f" — {row['title']}" if row["title"] else ""),
            "\n".join(reported),
        ])),
    )
    return 0


def _move_labels(client, number, add, remove, create=False):
    current = label_ops.issue_labels(client, number)
    add_ids = label_ops.resolve_label_ids(client, add or [], create=create)["ids"]
    remove_ids = label_ops.resolve_label_ids(client, remove or [])["ids"]
    return label_ops.apply_issue_labels(
        client, number, label_ops.merge_labels(current, add_ids, remove_ids)
    )


def cmd_comment(client, args, out, settings) -> int:
    body = read_text(args)
    posted = issue_ops.post_comment(client, args.number, body) or {}
    if args.label:
        _move_labels(client, args.number, args.label, [], create=True)
    if args.status:
        label_ops.set_status(client, args.number, args.status)
    url = posted.get("html_url") or (
        f"{settings.repo_url}/issues/{args.number}#issuecomment-{posted.get('id')}"
    )
    out.emit_either(posted, f"comment {posted.get('id')} posted on "
                            f"#{args.number}\n{url}")
    return 0


def cmd_comments(client, args, out, settings) -> int:
    items = issue_ops.fetch_comments(client, args.number, args.limit)
    lines = []
    for item in items:
        author = (item.get("user") or {}).get("login") or "?"
        created = str(item.get("created_at") or "")[:16]
        lines += [f"--- comment {item.get('id')} by {author} ({created})",
                  str(item.get("body") or "").rstrip(), ""]
    out.emit_either({"count": len(items), "comments": items},
                    "\n".join(lines).rstrip() or f"#{args.number} has no comments")
    return 0


def cmd_tasks(client, args, out, settings) -> int:
    issue = issue_ops.fetch_issue(client, args.number)
    body = issue.get("body") or ""
    before = task_ops.parse_tasks(body)

    if args.action == "show":
        payload = {
            "number": args.number,
            "progress": task_ops.progress(before),
            "tasks": [{"index": item.index, "done": item.done, "text": item.text}
                      for item in before],
        }
        text = (f"#{args.number} {render.clip(issue.get('title'), 70)}\n"
                + task_ops.render_checklist(before))
        if not before:
            text += "\n\nThe body has no checklist — see the template (`template`)."
        out.emit_either(payload, text)
        return 0

    new_body = body
    touched: list[task_ops.Task] = []
    dropped: list[str] = []
    if args.action in ("tick", "untick"):
        new_body, touched = task_ops.edit_checkboxes(
            new_body, args.targets or ["all"], done=args.action == "tick"
        )
    for heading in list(args.drop_headings or []) + (
        list(args.targets) if args.action == "drop" else []
    ):
        new_body, removed = task_ops.drop_section(new_body, heading)
        if removed:
            dropped.append(heading)

    if new_body == body:
        raise UsageError(
            "the body is unchanged — nothing matched, or the tasks were "
            "already in that state."
        )
    issue_ops.set_body(client, args.number, new_body)
    after = task_ops.parse_tasks(new_body)
    out.emit_either(
        {"number": args.number, "progress": task_ops.progress(after),
         "touched": sorted({item.index for item in touched}),
         "dropped_sections": dropped},
        f"#{args.number} tasks {task_ops.progress(before)} -> "
        f"{task_ops.progress(after)}"
        + ("\n  touched: " + ", ".join(str(item.index) for item in touched)
           if touched else "")
        + ("\n  dropped section(s): " + ", ".join(dropped) if dropped else "")
        + "\n" + task_ops.render_checklist(after),
    )
    return 0


def cmd_label(client, args, out, settings) -> int:
    action = args.action
    if action == "list":
        items = label_ops.fetch_labels(client)
        rows = [[str(item.get("id")), render.clip(item.get("name"), 40),
                 str(item.get("color") or "")] for item in items]
        out.emit_either(
            {"count": len(items), "labels": items},
            render.table(rows, ["id", "label", "color"]) if rows
            else "this repo defines no labels yet",
        )
        return 0
    if action == "bootstrap":
        result = label_ops.ensure_labels(
            client,
            [(status.display, status.color, status.description)
             for status in label_ops.STATUS_VOCABULARY],
            dry_run=args.dry_run,
        )
        out.emit_either(result, _label_change_text(result, "status labels"))
        return 0
    if action == "create":
        result = label_ops.ensure_labels(
            client, [(args.name, args.color, args.description)],
            dry_run=args.dry_run,
        )
        out.emit_either(result, _label_change_text(result, args.name))
        return 0
    if action == "show":
        items = label_ops.issue_labels(client, args.number)
        names = [str(item.get("name") or "") for item in items]
        out.emit_either(
            {"number": args.number, "labels": items},
            f"#{args.number} labels: " + (", ".join(names) or "(none)"),
        )
        return 0
    if action in ("status", "clear"):
        tokens = args.tokens if action == "status" else ["none"]
        result = label_ops.set_status(client, args.number, tokens,
                                      dry_run=args.dry_run)
        out.emit_either(result, _status_text(result))
        return 0
    if action in ("add", "remove"):
        add, remove = (args.names, []) if action == "add" else ([], args.names)
        applied = _move_labels(client, args.number, add, remove,
                              create=getattr(args, "create", False))
        names = ", ".join(str(item.get("name") or "") for item in applied)
        out.emit_either(applied, f"#{args.number} labels: {names or '(none)'}")
        return 0
    if action == "set":
        ids = label_ops.resolve_label_ids(client, args.names or [],
                                          create=args.create)["ids"]
        applied = label_ops.apply_issue_labels(client, args.number, ids)
        names = ", ".join(str(item.get("name") or "") for item in applied)
        out.emit_either(applied,
                        f"#{args.number} labels set to: {names or '(none)'}")
        return 0
    raise UsageError(f"unknown label action {action!r}")


def _label_change_text(result: dict, subject: str) -> str:
    created = result.get("created") or []
    existed = result.get("existed") or []
    parts = []
    if created:
        parts.append(f"{'would create' if result.get('dry_run') else 'created'}: "
                     f"{', '.join(created)}")
    if existed:
        parts.append(f"already present: {', '.join(existed)}")
    return f"{subject} — " + ("; ".join(parts) if parts else "no change")


def _status_text(result: dict) -> str:
    before = ", ".join(result.get("before") or []) or "(none)"
    wanted = result.get("wanted") or []
    target = ", ".join(wanted) if wanted else "(status cleared)"
    if result.get("dry_run"):
        return f"would set status to {target}\n  was: {before}"
    applied = ", ".join(result.get("applied") or []) or "(none)"
    created = result.get("created") or []
    return (f"status: {applied}\n  was: {before}"
            + (f"\n  created label(s): {', '.join(created)}" if created else ""))


def cmd_milestone(client, args, out, settings) -> int:
    action = args.action
    if action == "list":
        items = milestone_ops.summary_rows(
            milestone_ops.list_milestones(client, args.state))
        rows = [[str(row["id"]), render.clip(row["title"], 40), row["state"],
                 f"{row['open']}/{row['closed']}", str(row["due"])[:10]]
                for row in items]
        out.emit_either(
            {"milestones": items},
            render.table(rows, ["id", "title", "state", "open/closed", "due"])
            if rows else f"no {args.state} milestones",
        )
        return 0
    if action == "latest":
        item = milestone_ops.latest(client)
        out.emit_either(
            item,
            f"{item.get('id')}  {item.get('title')}  open "
            f"{item.get('open_issues')}/{item.get('closed_issues')}",
        )
        return 0
    if action == "create":
        item, created = milestone_ops.create_milestone(
            client, args.title, due=args.due or "", description=args.description
        )
        out.emit_either(
            item,
            f"milestone {args.title!r} "
            + ("created" if created else "already exists")
            + (f" (id {item.get('id')})" if item.get("id") else ""),
        )
        return 0
    if action == "set":
        item = milestone_ops.resolve(client, args.ref,
                                    create=args.create_milestone,
                                    due=args.due or "")
        updated = milestone_ops.assign(client, args.number, item["id"])
        row = issue_ops.issue_row(updated or {}, settings.repo_url)
        out.emit_either(
            updated,
            f"#{args.number} milestone -> "
            f"{row.get('milestone') or item.get('title')} (id {item['id']})",
        )
        return 0
    if action == "unset":
        updated = milestone_ops.assign(client, args.number, 0)
        out.emit_either(updated, f"#{args.number} milestone cleared")
        return 0
    if action == "state":
        item = milestone_ops.resolve(client, args.ref)
        updated = milestone_ops.set_state(client, item["id"], args.state)
        out.emit_either(updated,
                        f"milestone {item.get('title')} is now {args.state}")
        return 0
    if action in ("issues", "report"):
        item = milestone_ops.resolve(client, args.ref)
        rows = issue_ops.list_issues(client, state=args.state,
                                    milestone=item.get("title"), all_pages=True)
        if action == "issues":
            out.emit_either(
                {"milestone": milestone_ops.summary_rows([item])[0],
                 "count": len(rows), "issues": rows},
                _issue_table(rows, list(issue_ops.DEFAULT_COLUMNS)),
            )
            return 0
        out.emit_either(
            {"milestone": milestone_ops.summary_rows([item])[0], "issues": rows},
            _milestone_report(item, rows),
        )
        return 0
    raise UsageError(f"unknown milestone action {action!r}")


def _all_ticked(progress: str) -> bool:
    done, _, total = str(progress).partition("/")
    return bool(total) and done == total


def _milestone_report(item, rows) -> str:
    head = (f"milestone {item.get('title')} (id {item.get('id')}) — "
            f"{len(rows)} issue(s)")
    if not rows:
        return head + "\n(no issues in this milestone)"
    table_rows = [
        [f"{row['number']}", row["state"], row["tasks"] or "-",
         render.clip(row["title"], 60), ", ".join(row["labels"])]
        for row in rows
    ]
    untidy = [row for row in rows if row["tasks"] and not _all_ticked(row["tasks"])]
    return "\n".join([
        head,
        render.table(table_rows, ["#", "state", "tasks", "title", "labels"]),
        "",
        f"checklist not fully ticked: {len(untidy)} of {len(rows)}",
        "a fully ticked body means implemented and ready for a human to close;",
        "an empty one means not implemented — do not tick it, and do not close it.",
    ])


def cmd_close(client, args, out, settings) -> int:
    if not args.yes:
        render.error_stream(
            f"refusing to close #{args.number}: closing is a human decision.\n"
            "  Post a comment describing the work instead (`comment`), then let\n"
            "  a human review and close it. Re-run with --yes only when a human\n"
            "  explicitly asked you to close this issue in this conversation."
        )
        return POLICY_EXIT
    note = read_text(args, "note", "note_file", required=False)
    if note:
        issue_ops.post_comment(client, args.number, note)
    updated = issue_ops.set_state(client, args.number, "closed")
    if args.status:
        label_ops.set_status(client, args.number, args.status)
    out.emit_either(updated, f"#{args.number} closed (by explicit request)")
    return 0


def cmd_reopen(client, args, out, settings) -> int:
    note = read_text(args, "note", "note_file", required=False)
    if note:
        issue_ops.post_comment(client, args.number, note)
    updated = issue_ops.set_state(client, args.number, "open")
    out.emit_either(updated, f"#{args.number} reopened")
    return 0


def cmd_api(client, args, out, settings) -> int:
    method = (args.method or "GET").upper()
    path = args.path or ""
    if not path:
        raise UsageError(
            "give a path after /api/v1, e.g. repos/OWNER/REPO/issues/687/timeline"
        )
    if not path.startswith("/"):
        path = "/" + path
    payload = None
    if args.data:
        try:
            payload = json.loads(args.data)
        except ValueError as error:
            raise UsageError(f"--data is not valid JSON: {error}") from error
    elif args.input:
        try:
            payload = json.loads(_file_text(args.input))
        except ValueError as error:
            raise UsageError(f"{args.input} is not valid JSON: {error}") from error
    params = {}
    for item in args.param or []:
        key, _, value = item.partition("=")
        params[key] = value
    result = client.request(method, path, params=params or None, payload=payload)
    print(render.json_dump(result) if result is not None else "(empty response)")
    return 0


HANDLERS = {
    "doctor": cmd_doctor,
    "me": cmd_me,
    "template": cmd_template,
    "list": cmd_list,
    "ls": cmd_list,
    "view": cmd_view,
    "create": cmd_create,
    "edit": cmd_edit,
    "comment": cmd_comment,
    "comments": cmd_comments,
    "tasks": cmd_tasks,
    "label": cmd_label,
    "milestone": cmd_milestone,
    "close": cmd_close,
    "reopen": cmd_reopen,
    "api": cmd_api,
}


# ---------------------------------------------------------------------------
# entry point
# ---------------------------------------------------------------------------
def main(argv=None, *, transport=None, settings=None, out=None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if not args.command:
        parser.print_help()
        return 2
    out = out or render.Printer(as_json=args.json, quiet=args.quiet)

    try:
        if settings is None:
            settings = load_settings(
                repo=args.repo, remote=args.remote, api=args.api,
                timeout=args.timeout, insecure=args.insecure,
                env_file=args.env_file,
            )
        else:  # injected config (tests): explicit flags still win
            api_base = settings.api_base
            if args.api:
                api_base = args.api.rstrip("/")
                if not api_base.endswith("/api/v1"):
                    api_base = f"{api_base}/api/v1"
            settings = replace(
                settings,
                repo=args.repo or settings.repo,
                api_base=api_base,
                timeout=args.timeout or settings.timeout,
                insecure=bool(args.insecure) or settings.insecure,
            )
        if args.command in OFFLINE_COMMANDS:
            return HANDLERS[args.command](None, args, out, settings)

        settings.require()
        client = GiteaClient(settings)
        if transport is not None:
            client.transport = transport
        if args.dry_run:
            client = DryRunClient(client, out)
        code = HANDLERS[args.command](client, args, out, settings)
        if isinstance(client, DryRunClient):
            if client.planned:
                out.note(f"(dry run: {len(client.planned)} write(s) planned, "
                         "nothing was sent)")
            else:
                out.note("(dry run: this command only reads — nothing to send)")
        return code
    except UsageError as error:
        render.error_stream(str(error))
        return 1
    except (ConfigError, GiteaError, label_ops.LabelError,
            milestone_ops.MilestoneError, task_ops.TaskSelection,
            LookupError, ValueError, RuntimeError) as error:
        render.error_stream(_describe(error))
        return 1
    except BrokenPipeError:  # piping into head or less is normal use
        return 0
    except KeyboardInterrupt:
        render.error_stream("interrupted")
        return 130


def _describe(error) -> str:
    if isinstance(error, GiteaError):
        return error.report()
    text = str(error)
    if "\n" in text:  # ConfigError already spells out one fix per line
        return text
    return f"{type(error).__name__}: {text}"


def run() -> None:
    sys.exit(main())
