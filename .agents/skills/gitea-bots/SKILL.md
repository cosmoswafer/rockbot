---
name: gitea-bots
description: Use when working with the project's Gitea issue watcher — polling for new issues and comments, reporting them from tmp/gitea-bots/pending.md, answering a human reply, and doing the requested issue work by following AGENTS.md. Also use to diagnose the setup — Python 3.10+ and requests, the sibling gitea-issues and issue-labels skills, .env and GITEA_TOKEN, origin derivation, and Gitea server access. The skill owns the watcher, event store, notification prompts and issue plumbing only; it defines no command vocabulary and no separate development flow.
license: CC0-1.0
---

# gitea-bots — Gitea issue watcher & notification plumbing

This skill is deliberately thin. It owns the **plumbing**:

- `scripts/watch-issues.py` — poll Gitea for issues and comments, record events
  in `./tmp/gitea-bots/`, and wake the agent.
- `set-status.py` / `sync-tasks.py` — issue status labels and the body task
  checklist.
- The notification, worker and comment templates, and the hidden stage markers
  that keep plan/completion records from being duplicated.

It does **not** define a command language and it does **not** override the
agent harness. A human reply that at-mentions the bot is the request; what the
reply asks for is worked out from the issue and the repo's
[`AGENTS.md`](../../../AGENTS.md) (build/lint/test commands, the development
flow, version bumping, commit conventions, and so on). There are no slash
commands or prose trigger phrases — wording like "plan" or "implement" only
counts because it is what the human asked for, not because the skill parses it.

Each request is handled once. Post the answer (a plan, a completion note, a
reply), then **stop** and wait for the next human reply. Never continue into
more work on your own.

## Hard rules

- **The bot never writes an at-mention (#136).** The bot and human may share one
  Gitea account, so the author login cannot distinguish them. Any at-mention of
  the bot login outside code spans is therefore a **human reply** and must be
  surfaced/answered. The bot must never put an at-mention in its comments,
  plans, completion notes or notification prompts (otherwise it would answer
  itself).
- **Never close issues.** Comment with what changed and let a human close. No
  auto-closing keywords (`close`, `fix`, `resolve`, …) in commits either.
- **Never guess.** If the request, plan, or implementation is unclear, ask in
  chat (and/or as an issue comment) before acting. Do not widen scope.
- **Never expose the token.** Source `.env` silently, send
  `Authorization: token $GITEA_TOKEN`, never echo it, and never enable `set -x`
  in snippets that touch it.
- **Quoted text is not a request.** At-mentions and commands quoted in fenced,
  inline or indented code, or appearing in bot output (a stage marker), are
  examples, not authorization.
- **A completed stage is not consent for the next one.** A plan comment does
  not authorize implementation; a human reply must ask for it.
- **Set `👀 seeing` first when you pick an issue up.** Before investigating,
  run `python3 .agents/skills/gitea-bots/scripts/set-status.py <N> seeing`
  (idempotent). Do not rely on the notifier: a loop started before the pickup
  code merged, a failed label write, or disabled workers all leave the issue
  unlabelled. Never skip this because the board still shows an older label.

## Setup

**Canonical implementation: Python.** The watcher and helpers are
`watch-issues.py`, `set-status.py`, `sync-tasks.py`, `probe-live.py`,
`probe-live-write.py` and `probe-e2e.py`, backed
by the `scripts/gitea_bots/` package. The status vocabulary and HTTP client are
vendored in `scripts/issue_labels/`, so the tooling needs no sibling skill and
no install step. They require Python 3.10+ and `requests` —
the only non-stdlib dependency. No `curl`, `jq`, `flock`, `awk` or `stdbuf` is
needed by the tooling itself: HTTP uses `requests`, JSON uses `json`, locking
uses `fcntl`, and the background loop is line-buffered with `python -u`. Run
them with `python3 <script> <command>` (no `chmod +x` needed).

The retired bash implementations (`watch-issues.sh`, `set-status.sh`,
`sync-tasks.sh`) and their black-box tests were removed in #155. The first
Python run on an old state directory still imports the pre-SQLite flat files
(`state.json`, `events.jsonl`, `pending/`, `pending-ack/`, `ack/`) once and
archives them under `legacy-json-<ts>/`. That compatibility path is scheduled
for removal on **2027-01-01**, or sooner once every known
`GITEA_BOTS_STATE_DIR` has `meta.imported_from_files=1` and no flat files
remain. See
[`scripts/gitea_bots/README.md`](scripts/gitea_bots/README.md) for the
architecture and `python3 scripts/run-tests.py` for the Python test-suite.

The **live probe** is an opt-in, read-only check of the real server's API
shapes. Run `python3 scripts/probe-live.py` directly (the script run is the
opt-in) for a per-scenario PASS/FAIL report; `-n/--issues N` bounds the
comments/timeline reads and `--verbose` adds progress output. The historical
unittest gate `GITEA_BOTS_LIVE=1 python3 scripts/run-tests.py` still works as a
thin adapter over the same probe. The probe refuses every non-GET request and
uses `tmp/gitea-bots-live-probe/`, never the live watcher's
`tmp/gitea-bots/` state.

The **write-data probe** (`probe-live-write.py`) and the **end-to-end probe**
(`probe-e2e.py`) are the opt-in write-side checks. The write probe exercises
create → comment → label → assign → close → delete on one dummy issue. The e2e
probe creates controlled dummy issues, runs the **real watcher** against them
(NEW ISSUE / AUTO PLAN / DIRECTED COMMENT / REASSIGNED), asserts the emitted
events, `state.db` transitions, idempotence and the real status labels, then
deletes every fixture and fails loudly on a leftover. **Stop the production
watcher first** — the e2e harness refuses while `watcher.pid` points at a live
process, because the live loop would consume the dummies. It uses
`tmp/gitea-bots-live-e2e/` plus a sibling lock, never `tmp/gitea-bots/`.
`GITEA_BOTS_LIVE_WRITE=1` / `GITEA_BOTS_LIVE_E2E=1` are the unittest gates.
Deletion is admin-gated: a non-admin token gets the same loud `403` fallback
that clears labels/assignees, keeps the dummy closed and reports the leftover
URL. The e2e report lands under `tmp/artifacts/gitea-bots-e2e-<ts>.log` (plus a
structured `.json` sibling).

**In-session launch:** launch `loop` as a **background shell job of the current
OpenCode session** (rather than a detached `start`), e.g.
`python3 .agents/skills/gitea-bots/scripts/watch-issues.py loop --interval 20 >> ./tmp/gitea-bots/watcher.log 2>&1`.
Both `start` and `loop` inherit `$OPENCODE_SESSION_ID` from the launching
environment; the model is resolved from that session for pinned `fresh`
notifications (#137) and `shared`-mode delivery still reaches it when opted in.
`stop` / `status` keep working through `watcher.pid`. The single-watcher /
`poll.lock` rules are unchanged either way.

`GITEA_TOKEN` must already be in `.env` at the repo root. The agent-side API
snippets below derive the API base and repository from `origin` exactly like
the `gitea-issues` skill (using `curl` + `jq`) — never hardcode host or
owner/repo. The Python tooling derives both itself from `origin` and does not
need this block:

```bash
source .env >/dev/null 2>&1

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then
  BARE="${REMOTE#*://}"; BARE="${BARE#*@}"
  GITEA_OWNER_REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"
else
  BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; GITEA_OWNER_REPO="${BARE#*:}"
fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"
```

After this block: `$GITEA_API`, `$GITEA_OWNER_REPO`, and `$GITEA_TOKEN` are
available. The snippets below assume it has already run. For the issue API,
assignees, milestones and API failure modes, see the
[`gitea-issues` skill](../gitea-issues/SKILL.md). For the status label
vocabulary and curl recipes, see the
[`issue-labels` skill](../issue-labels/SKILL.md). The Python tooling reads
`.env` and `origin` itself; this block is only for the curl examples below.

Sanity check (never prints the token):

```bash
curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" | jq '.login // .message'
```

## Diagnostics — check the toolchain, skills, token and server

Run this from the repository root before starting the watcher or debugging
silence. It prints `PASS` / `WARN` / `FAIL` for each requirement and ends with
next steps. It never prints the token. Run the Setup block above first.

```bash
echo "== gitea-bots diagnostics =="

SKILLS_DIR=".agents/skills"
if [ ! -f "$SKILLS_DIR/gitea-bots/SKILL.md" ]; then
  SKILLS_DIR="skills"
fi
BOTS_DIR="$SKILLS_DIR/gitea-bots/scripts"

# 1. Toolchain
if command -v git >/dev/null 2>&1; then
  echo "PASS  git: $(command -v git)"
else
  echo "FAIL  git is missing — the skills derive the repo from the origin remote"
fi

if python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)' 2>/dev/null; then
  echo "PASS  Python $(python3 -c 'import platform; print(platform.python_version())') (needs 3.10+)"
else
  echo "FAIL  Python 3.10+ is required — install it and re-run"
fi

if python3 -c 'import requests' 2>/dev/null; then
  echo "PASS  requests installed (the only non-stdlib dependency)"
else
  echo "FAIL  requests is missing — run: python3 -m pip install requests"
fi

if command -v curl >/dev/null 2>&1; then
  echo "PASS  curl: $(command -v curl)"
else
  echo "WARN  curl missing — the Python tooling still works; curl examples will not"
fi
if command -v jq >/dev/null 2>&1; then
  echo "PASS  jq: $(command -v jq)"
else
  echo "WARN  jq missing — drop the '| jq' pipes in the curl examples"
fi
if command -v opencode >/dev/null 2>&1; then
  echo "PASS  opencode: $(command -v opencode)"
else
  echo "WARN  opencode missing — events still queue in pending.md, but no notification"
fi

# 2. Skill layout (gitea-issues and issue-labels are independent helpers)
for skill in gitea-bots gitea-issues issue-labels; do
  if [ -f "$SKILLS_DIR/$skill/SKILL.md" ]; then
    echo "PASS  skill found: $SKILLS_DIR/$skill/SKILL.md"
  else
    echo "WARN  skill missing: $SKILLS_DIR/$skill/SKILL.md (install it or run from the qoder-skills checkout)"
  fi
done

for script in watch-issues.py set-status.py sync-tasks.py; do
  if [ -f "$BOTS_DIR/$script" ]; then
    echo "PASS  script found: $BOTS_DIR/$script"
  else
    echo "FAIL  script missing: $BOTS_DIR/$script"
  fi
done

if [ -d "$BOTS_DIR/issue_labels" ]; then
  echo "PASS  vendored issue_labels package found"
else
  echo "FAIL  vendored issue_labels package missing — reinstall the gitea-bots skill"
fi

# 3. Remote and token
echo "INFO  origin: $(git remote get-url origin 2>/dev/null || echo '(no origin remote)')"
echo "INFO  API=$GITEA_API  REPO=$GITEA_OWNER_REPO"
if [ -n "${GITEA_OWNER_REPO:-}" ]; then
  echo "PASS  owner/repo derived: $GITEA_OWNER_REPO"
else
  echo "FAIL  could not derive owner/repo from origin"
fi
if [ -f .env ]; then
  echo "PASS  .env found at $PWD/.env"
else
  echo "FAIL  no .env at $PWD — create it with GITEA_TOKEN=..."
fi
if [ -n "${GITEA_TOKEN:-}" ]; then
  echo "PASS  GITEA_TOKEN set (${#GITEA_TOKEN} characters; value hidden)"
else
  echo "FAIL  GITEA_TOKEN is empty — add GITEA_TOKEN=<token> to .env"
fi

# 4. Server access
HTTP=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO")
case "$HTTP" in
  200) echo "PASS  Gitea repo access: HTTP 200" ;;
  401) echo "FAIL  HTTP 401 — token rejected; use the literal word 'token', not 'Bearer'" ;;
  403) echo "FAIL  HTTP 403 — token lacks repository access or the needed scope" ;;
  404) echo "FAIL  HTTP 404 — wrong host or owner/repo; check the origin remote" ;;
  000) echo "FAIL  no response — DNS, TLS or network problem reaching $GITEA_HOST" ;;
  *)   echo "FAIL  Gitea repo access: HTTP $HTTP" ;;
esac
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO" \
  | jq -r 'if .full_name then "PASS  repository: \(.full_name)" else "FAIL  repository: \(.message // "unexpected response")" end' 2>/dev/null
curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" \
  | jq -r 'if .login then "PASS  token user: \(.login)" else "WARN  token user: \(.message // "unavailable") (read:user scope missing is not fatal)" end' 2>/dev/null

# 5. Local watcher state (optional; WARN means it has not run here yet)
echo "INFO  watcher state: ./tmp/gitea-bots/"
if [ -f "$BOTS_DIR/watch-issues.py" ]; then
  python3 "$BOTS_DIR/watch-issues.py" status 2>&1 | sed 's/^/      /'
fi
if [ -f ./tmp/gitea-bots/watcher.log ]; then
  echo "INFO  last watcher log lines:"
  tail -n 5 ./tmp/gitea-bots/watcher.log | sed 's/^/      /'
else
  echo "WARN  no watcher.log yet — start the watcher when you want background polling"
fi

echo
echo "Next steps for a FAIL:"
echo "  missing Python 3.10+ / requests -> install them, then re-run this block"
echo "  missing skill or script          -> install the skill under $SKILLS_DIR/"
echo "  HTTP 401                         -> Authorization: token \$GITEA_TOKEN (never Bearer)"
echo "  HTTP 403 / 404                   -> check the token scope, origin remote and owner/repo casing"
echo "  no watcher yet                   -> optional; see 'Watcher' below to start it"
```

## Watcher — `scripts/watch-issues.py`

Polls the Gitea API for new/changed issues and new issue comments. By default
it surfaces every new comment that at-mentions the bot login (a human reply);
all-comments mode surfaces everything.

```bash
# All commands use the Python entry point (stdlib + requests only).
# one-shot: poll once, print events, exit (silent baseline by default)
python3 .agents/skills/gitea-bots/scripts/watch-issues.py check

# one-shot: also report existing open issues on a fresh baseline
python3 .agents/skills/gitea-bots/scripts/watch-issues.py check --report-existing

# background loop (nohup + PID file); reports existing open issues on first
# start after a fresh baseline, then listens for changes
python3 .agents/skills/gitea-bots/scripts/watch-issues.py start --interval 20
python3 .agents/skills/gitea-bots/scripts/watch-issues.py status
python3 .agents/skills/gitea-bots/scripts/watch-issues.py stop

# foreground loop (tmux/herdr, Ctrl-C to stop); same baseline-reporting default
python3 .agents/skills/gitea-bots/scripts/watch-issues.py loop --interval 20
# add --no-report-existing to skip the baseline report, or --report-existing-all
# to include closed issues in that report

# read / acknowledge pending events (what the agent consumes)
python3 .agents/skills/gitea-bots/scripts/watch-issues.py inbox
python3 .agents/skills/gitea-bots/scripts/watch-issues.py ack

# move state aside; next poll re-baselines and can report existing issues again
python3 .agents/skills/gitea-bots/scripts/watch-issues.py reset

# list open Summary-only plan-eligible issues with no plan
python3 .agents/skills/gitea-bots/scripts/watch-issues.py scan-unplanned

# helpers (agent-side)
python3 .agents/skills/gitea-bots/scripts/set-status.py --reconcile '<N>'
python3 .agents/skills/gitea-bots/scripts/sync-tasks.py '<N>' '<plan-file>'
```

Default interval is 20 s, configurable with `--interval N` or
`GITEA_BOTS_INTERVAL`. `check` refuses to run while the background loop holds
the poll lock — read the log or stop the loop first.

Sample output:

```
2026-11-08T10:05:00Z NEW ISSUE #42 "Quick camera: use server time" by alice — https://gitea.example/Atom/rockbot/issues/42
2026-11-08T10:06:12Z DIRECTED COMMENT on #42 (comment 910) by alice — https://gitea.example/Atom/rockbot/issues/42#issuecomment-910
    | the bot login asked for a status update
2026-11-08T10:06:12Z DIRECTED COMMENT on #43 (issue body) by alice — https://gitea.example/Atom/rockbot/issues/43
    | the bot login was edited into the issue body
2026-11-08T10:06:12Z UPDATED ISSUE #42 — changed: labels — https://gitea.example/Atom/rockbot/issues/42
```

Event tags: `NEW ISSUE`, `UPDATED ISSUE` (title/state/labels/assignees/milestone
changed, or `body/other` when only the issue body changed), `NEW COMMENT`
(all-comments mode), `DIRECTED COMMENT` (a human reply that at-mentioned the
bot login — in a comment, `(comment <id>)`, or in the issue body itself,
`(issue body)` — answer it), `AUTO PLAN` (plan-eligible Summary-only issue with
no plan; authorizes investigation and planning once), `REASSIGNED` (issue
re-assigned to the token user after a plan exists — ask before re-planning).
Comment-only updates produce comment events without a redundant issue event. A
complete comment or mention paragraph is printed under the event, prefixed
`    | `.

### State, logs, and restarts

**Single-watcher rule (#118, #119):** run exactly one `watch-issues.py` poller
(`loop`/`start`) per repo/state directory. `start`/`loop` hold `poll.lock`
while polling and release it before spawning notification sessions; notification
children must never run `watch-issues.py` and must not inherit the poll lock. Do
not run concurrent `check`/`loop`/`start` (or `scan-unplanned` as a competing
poller) against the same `GITEA_BOTS_STATE_DIR` — only that loop may poll, read
timeline/comments for state, and deliver events.

All runtime state lives under `./tmp/gitea-bots/` (never `/tmp`):

| Path | Purpose |
| --- | --- |
| `state.db` | authoritative local SQLite store (WAL): `meta` (`repo`, `api`, `last_poll`, schema `version`), `issues` (per-issue `updated_at` + field snapshot + `last_comment_id` + `auto_plan_at` + `last_assignee_event_id` + `body_mention_sha`), `batches` + `events` (pending/acked history), `tasks` (the `## Tasks` mirror), `polls` (one row per poll) |
| `pending.md` | generated view of unacknowledged batches (`inbox` / `ack`) |
| `legacy-json-<ts>/` | one-shot archive of the pre-SQLite flat files after migration (compatibility path retired 2027-01-01) |
| `watcher.pid` | PID of the background loop (checked against `/proc/<pid>/cmdline`) |
| `watcher.meta` | startup identity: `pid`, `started_at`, informational `script_commit` / `script_mtime`, and a `fingerprint` (code mtimes + effective `GITEA_BOTS_*` values). The loop warns when the fingerprint drifts; `status` diffs against it |
| `watcher.log` | background-loop output (append-only) |
| `notify.log` | pinned spawned sessions (`fresh` notifications / reply workers) and their JSON output |
| `poll.lock` | `flock` guarding state writes |
| `pending.lock` | `flock` guarding the generated `pending.md` view |
| `scratch/` | per-issue scratch (dumps, plans, comment payloads); prunable at any time |

`state.db` is created on first use. The pre-SQLite flat files (`state.json`,
`events.jsonl`, `pending/`, `pending-ack/`, `ack/`) are imported exactly once
inside a single transaction and moved into `legacy-json-<ts>/`; this is the only
compatibility read left, and it is removed at the 2027-01-01 condition above.

Everything else under `./tmp/` is disposable local scratch. Prune it with the
repo-level `python3 scripts/prune_tmp.py` (dry run by default; `--apply` to
delete; `--browsers` to include browser profiles), which never touches live
state and gates `legacy-json-<ts>/` on `meta.imported_from_files=1`. Stop the
watcher and let reply workers finish before pruning its directory. See
[`AGENTS.md`](../../../AGENTS.md#temp-directory) for the retention policy.

Useful direct queries (`sqlite3` CLI is not a runtime dependency):

```bash
sqlite3 ./tmp/gitea-bots/state.db \
  "SELECT number, title, state, last_comment_id FROM issues WHERE assignees LIKE '%<login>%';"
sqlite3 ./tmp/gitea-bots/state.db \
  "SELECT id, created_at, event_count FROM batches WHERE acked_at IS NULL ORDER BY created_at;"
sqlite3 ./tmp/gitea-bots/state.db \
  "SELECT at, line FROM events ORDER BY id DESC LIMIT 20;"
sqlite3 ./tmp/gitea-bots/state.db \
  "SELECT at, scanned, emitted, ok, error FROM polls ORDER BY id DESC LIMIT 20;"
sqlite3 ./tmp/gitea-bots/state.db \
  "SELECT issue, position, done, text FROM tasks ORDER BY issue, position;"
```

`status` prints the DB path plus tracked-issue, pending-batch, event, and poll
counts. `pending.md` stays the agent/human-facing view; `events.jsonl` and the
batch files are no longer written — they exist only as a one-shot migration
source.

- The **first** poll is a baseline: it records existing issues and their latest
  comment IDs. `start`/`loop` report existing **open** issues on that fresh
  baseline by default (tagged with state, e.g. `[open]`), then continue polling
  for changes. Plan-eligible Summary-only issues with no plan marker emit
  `AUTO PLAN` instead of `NEW ISSUE` on that baseline/report (#118), so a fresh
  start queues plans immediately. `check` keeps the baseline silent unless
  `--report-existing` is passed. Use `--no-report-existing` to suppress the
  report, or `--report-existing-all` to include closed issues; comments stay
  silent.
- On every `start`/`loop`, before the first poll, the watcher reconciles the
  status label of **every open issue assigned to the token user** (plus any open
  issue already carrying a status label) — `GITEA_BOTS_START_RECONCILE`, default
  on. It derives the expected label from the workflow markers through the
  in-process `gitea_bots.status` reconcile (equivalent to
  `scripts/set-status.py --reconcile`) and repairs drift. Unrelated labels are
  preserved, unrelated backlog issues are left untouched, the operation is
  idempotent, and a failure is logged as `WARN` without stopping the loop. This
  is the watcher's **only** write. The log line is `startup label reconcile:
  checked N issue(s), updated M, failed K`. Set `GITEA_BOTS_START_RECONCILE=0`
  to disable it.
- **Startup catch-up (#118):** after the first successful poll, `start`/`loop`
  scan the full paginated open set (not limited by `last_poll − 120 s`) and emit
  `AUTO PLAN` once for every open plan-eligible, Summary-only issue with no
  plan marker / no `## Tasks`. This catches issues that predate the watcher,
  arrived while it was down, or fell outside the incremental window. Dedupe is
  by `auto_plan_at` in the `issues` table; `scan-unplanned` uses the same
  predicate and should report the same set.
- **Body-only plans (#121, #128):** an issue whose plan was written straight
  into the body (`## Tasks`, no plan marker comment) is an anomaly. It is *not*
  an AUTO PLAN candidate (the checklist already exists), but `--reconcile`
  **does** derive `responded` for it so the status label is visible and sticky
  across startup reconcile. The startup/catch-up scan and `scan-unplanned`
  still log a `WARN` because the plan has no marker, and
  `set-status.py --reconcile` prints an informational `NOTE`. The marker
  comment is still recommended to make the plan canonical for marker checks.
- **Re-assignment (#119):** for tracked issues the watcher polls
  `/issues/{n}/timeline` (paginated, non-fatal) and detects `type=assignees`
  events that add `$SELF_LOGIN` with `id > last_assignee_event_id`, including a
  net-unchanged unassign → assign cycle. It emits `AUTO PLAN` for unplanned
  Summary-only issues (once, via `auto_plan_at`) and `REASSIGNED` for issues
  that already have a plan / `## Tasks` / a plan marker — the agent must ask
  before replacing a plan, and re-assignment never authorizes implementation.
  `last_assignee_event_id` advances only after events are published
  (publish-before-advance).
- Restarting never re-reports: issues dedupe by `updated_at`/field snapshot and
  comments by monotonic comment ID. A 120 s overlap absorbs clock skew. A
  restart with existing state does not re-run the baseline report.
- Incremental polls fetch issues updated since `last_poll − 120 s`, paginated
  (50/page, up to 20 pages), and fetch comments for every issue in that window.
  Comment IDs dedupe; this also catches a comment written in the same second as
  the issue update. Timeline events are deduped by `last_assignee_event_id`.
- If any API call fails, the loop logs `WARN`, leaves `last_poll` unadvanced,
  and retries after the interval. It never spins or crashes the loop.
- If `state.db` is corrupt or belongs to another repo, it is renamed
  (`state.db.corrupt.<ts>` / `state.db.repo.<ts>`) and the watcher re-baselines.
- `reset` moves `state.db` (and its WAL sidecars) aside; the next poll starts a
  new baseline.
- Events are committed to `batches` + `events` and rendered into `pending.md`
  until `ack` runs; `ack` sets `batches.acked_at` (it does not delete the
  history). `events` rows are never trimmed by `ack`.
- Publishing happens **before** `last_poll` advances: the batch + event rows
  commit first, then the issue cursors and poll watermark in a second
  transaction. If a write fails, the poll keeps the old watermark and retries
  next tick — events are never consumed without being delivered.
- Editing an issue body bumps `updated_at`, so the next poll emits
  `UPDATED ISSUE — changed: body/other`. When that edit was the bot's own
  `sync-tasks.py` (or a human's whitespace tweak), the event is noise: report it
  and move on. An edit that puts an at-mention into the body is the exception:
  it emits `DIRECTED COMMENT on #N (issue body)` instead of the noise line, so
  the request spawns a reply worker just like a comment (deduped by
  `body_mention_sha`, a digest of the mention paragraphs only).
- `meta.version` in `state.db` carries the event schema version. When it is
  older than the script's event schema, the next poll runs a **catch-up scan**
  over the last `GITEA_BOTS_CATCHUP_MINUTES` (default 60) and re-surfaces recent
  directed mentions — comment and issue-body — as `DIRECTED COMMENT (catch-up)`.
  This recovers events a previous version recorded in state but had no way to
  deliver.
- The background loop runs under `python -u` (unbuffered stdio), so
  `watcher.log` is line-buffered and survives a kill.

### Human replies, AUTO PLAN and directed mentions

**Shared-login mode (#136).** The skill is designed to run with the **bot and the
human on the same Gitea account**. The author login therefore cannot
distinguish human from bot. The invariant is instead:

- **The bot never writes an at-mention.** Any at-mention of the bot login
  outside code spans is a **human reply**, regardless of the author account.
  The watcher surfaces it as `DIRECTED COMMENT` (from a comment or from the
  issue body) and the agent must answer it.
- **Loop avoidance** comes from that invariant plus two exemptions: bot output
  (comments carrying a stage marker) and at-mentions quoted inside fenced,
  inline or indented code are never human replies. Plain comments with no
  at-mention stay dropped by default — use `GITEA_BOTS_ALL_COMMENTS=1` to
  surface them.
- `GITEA_BOTS_SHARED_LOGIN=0` restores the login-equality guard for deployments
  where the bot has its own account.

**Directed at-mentions** require an answer on the issue and in chat. The reply
is the request: read the issue and all its comments, work out what was asked,
and do it following `AGENTS.md`. It authorizes only the work it actually asks
for — no more. Requests to "review" or "close it if already implemented" are
answered the same way: report status and evidence and refuse auto-close. If the
work is already recorded, say so plainly and point at the plan or completion
comment; do not duplicate it.

**AUTO PLAN** is a state predicate, not a command: an open, plan-eligible,
`## Summary`-only issue with no plan. Plan-eligible means assigned to the token
user — an unassigned issue is never auto-planned, even when the token user
authored it, because in shared-login mode every issue is self-authored (#150).
The event authorizes investigation and planning only; it never authorizes
implementation. Emitted at most once per issue.
`AUTO_PLAN_WORKERS` spawns one pinned planning worker for the batch's queued
set; the prompt re-verifies the issue and links an existing plan instead of
duplicating it.

**Re-assignment** is the normal way a human hands an issue over: assigning it
to the token user is what makes it plan-eligible. Re-assignment after a plan
exists emits `REASSIGNED`: ask before replacing the plan. Re-assignment never
authorizes implementation.

`GITEA_BOTS_SELF=<login>` pins the token user's login for AUTO PLAN /
`scan-unplanned` identity and at-mention detection without a `/user` lookup.

The watcher strips fenced, inline and indented code before matching, so an
at-mention quoted in a code block is an example, not a human reply. It is a
notifier, not an authorization mechanism — the agent must check the marker, the
at-mention and the order before acting.

Useful environment variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `GITEA_BOTS_INTERVAL` | `20` | poll interval in seconds (min 5) |
| `GITEA_BOTS_CATCHUP_MINUTES` | `60` | catch-up window after a state-schema upgrade |
| `GITEA_BOTS_STATE_DIR` | `./tmp/gitea-bots` | state directory |
| `GITEA_BOTS_LOG` | `$STATE_DIR/watcher.log` | background log |
| `GITEA_BOTS_ALL_COMMENTS` | `0` | `1` = report every new comment |
| `GITEA_BOTS_AUTO_PLAN` | `1` | `0` = never auto-emit `AUTO PLAN`. When enabled, `start`/`loop` run a startup scan and polling emits `AUTO PLAN` at most once for an open plan-eligible Summary-only issue with no plan marker — including first-seen or a later assignment update. Plan-eligible means assigned to the token user; an unassigned issue never counts (#150). |
| `GITEA_BOTS_AUTO_PLAN_WORKERS` | `1` | `1` = spawn one pinned planning worker per poll for the batch's `AUTO PLAN` issues. Gated on `GITEA_BOTS_AUTO_PLAN`; `0` leaves `AUTO PLAN` as a notification the interactive session must act on. |
| `GITEA_BOTS_START_RECONCILE` | `1` | `0` = do not reconcile status labels on startup. |
| `GITEA_BOTS_SELF` | — | token user's login (skips `/user`; used for AUTO PLAN / `scan-unplanned` identity) |
| `GITEA_BOTS_NOTIFY` | `1` | `1` = wake the agent when a background poll emits events |
| `GITEA_BOTS_NOTIFY_SESSION_MODE` | `fresh` | `fresh` = spawn a pinned archived session per event batch (#137, cheap minimal context); `shared` = deliver back into the launching session (same model, #131) |
| `GITEA_BOTS_NOTIFY_SESSION` | `$OPENCODE_SESSION_ID` | session used by `shared` mode |
| `GITEA_BOTS_MODEL` | — | explicit `provider/model[#variant]` override/fallback for pinning spawned sessions |
| `GITEA_BOTS_SPAWN_WORKERS` | `1` | `0` = do not spawn pinned reply workers for human replies |
| `GITEA_BOTS_ARCHIVE_WORKERS` | `0` | `0` = reply/AUTO PLAN workers are non-archived and visible in the TUI session picker; `1` = hide them as before. The routine batch reporter is always archived. Needs a watcher restart. (#147) |
| `GITEA_BOTS_MAX_CONCURRENCY` | `8` | bound on concurrent per-issue comment/timeline reads and per-issue worker dispatch (clamped to `>= 1`) |
| `GITEA_BOTS_SHARED_LOGIN` | `1` | `1` = bot and human share one account (any at-mention is a human reply); `0` = dedicated bot account (login-equality guard) |
| `GITEA_BOTS_VERBOSE` | `0` | `1` = log poll heartbeats |

The watcher only reads events, apart from in-process status-label writes: the
startup reconcile and the pickup `seeing` label applied before a dispatched
worker starts (#144). It never assigns, comments, or closes, and it does not
decide what work a reply asks for — that is the agent's job.

### Being notified — `inbox` / `ack`

A background poll (`start`/`loop`) is invisible to the chat, so the watcher has a
delivery path for it:

1. Every surfaced event is committed to the `batches` / `events` tables in
   `./tmp/gitea-bots/state.db` and rendered into `./tmp/gitea-bots/pending.md`.
2. After a poll publishes events (before `last_poll` advances) and the poll lock
   is released, the watcher wakes the agent as a **post-lock, best-effort**
   step. Notification never holds `poll.lock` and never blocks the next poll.
   **#137 — cheap default delivery:** the default **`fresh`** mode spawns a
   fresh, short-lived `opencode run --archived --auto --format json --title …
   --model <model>` session per batch, carrying only the bounded notification
   prompt (`pending.md` pointer + batch id). A long-lived interactive session is
   therefore never re-sent its accumulated history for a tiny event batch. The
   model is still resolved from `GITEA_BOTS_MODEL` or the launching session
   (`GITEA_BOTS_NOTIFY_SESSION`, default `$OPENCODE_SESSION_ID`) via
   `opencode api get /api/session/<id>`; it is logged as
   `notification model: <provider/model>`. If no model can be resolved, **no
   detached session is spawned** and the events stay safely in `pending.md`.
   `GITEA_BOTS_NOTIFY_SESSION_MODE=shared` is the explicit opt-in: a bounded
   synthetic message (`delivery: queue`) is posted back into the launching
   session (#131 same-model delivery) instead of spawning one. Either way, the
   spawned/asked session reports the events and then runs `ack`; its JSON/id
   lands in `notify.log`.
3. **Pinned reply workers (#131/#136/#138/#134):** when a batch contains a
   human reply that at-mentions the bot login, the watcher spawns **one** fresh
   background session per replying issue, pinned to the resolved model
   (`--model`) and carrying only that issue's event block. Workers are visible
   in the TUI by default; set `GITEA_BOTS_ARCHIVE_WORKERS=1` to hide them. Workers are
   dispatched concurrently, bounded by `GITEA_BOTS_MAX_CONCURRENCY`. The
   worker's prompt is generic: it reads the issue and comments, works out what
   the human asked for, and follows `AGENTS.md`; it never names a stage. An
   `AUTO PLAN` event is the other authorization shape: the watcher spawns
   **one** additional pinned worker for the whole queued set from that poll
   (issues already covered by a human reply are skipped), with a prompt that
   re-verifies the issue and links an existing plan instead of duplicating it.
   `AUTO PLAN` never authorizes implementation.
   `GITEA_BOTS_AUTO_PLAN_WORKERS=0` disables that worker (gated on
   `GITEA_BOTS_AUTO_PLAN`). The other notification session (the fresh batch
   session, or the interactive session in `shared` mode) is told (delegate
   prompt) to **only report the events and `ack`** — it must not do the work
   itself or spawn a second worker. Comments with no at-mention, bot output
   (stage markers), quoted/fenced at-mentions, and an unknown bot identity never
   spawn a worker. Set `GITEA_BOTS_SPAWN_WORKERS=0` to disable all worker
   spawns; the notification session then answers the reply itself.
4. Either way the message points at `pending.md` and restates the hard rules:
   report, do not invent work, never close issues. The agent runs `inbox`,
   reads the events, reports them, and then runs `ack`. `state.db`
   (`batches` + `events`) remains the authoritative record; notification is only
   a wake-up pointer and authorizes nothing on its own.

Rules for the agent on a `gitea-bots:` notification:

- Run `inbox` first; the message is a pointer, not the payload.
- Report the events to the user in chat (issue numbers, titles, URLs, replies).
- `ack` **only after** reporting, so nothing is lost if the turn is interrupted.
- The notification itself authorizes nothing, but a **human reply in the batch
  does**. In shared-login mode the human and bot share one account, so a reply
  counts when the comment at-mentions the bot login (code-stripped, not bot
  output); otherwise the author must differ from the token user. When the batch
  contains such a reply, a pinned background worker session is spawned (#131/
  #136). In the notification session: report + `ack` only, do **not** do the
  work yourself and do not spawn a second worker. (When
  `GITEA_BOTS_SPAWN_WORKERS=0` no worker is spawned, so the notification
  session answers the reply itself — never ask for a second confirmation in
  chat.)
- An **`AUTO PLAN` event authorizes investigation and planning only**. With
  `GITEA_BOTS_AUTO_PLAN_WORKERS=1` (default) a pinned worker is spawned for the
  queued set and the notification session only reports + `ack`s; with the
  toggle off (or `GITEA_BOTS_SPAWN_WORKERS=0`) the notification session itself
  fetches each issue, re-checks that it is open, plan-eligible, `## Summary`-only
  with no plan, and posts a plan. An existing plan is linked and queried, never
  duplicated. Never start implementing from an AUTO PLAN.
- A `DIRECTED COMMENT` (a human reply that at-mentioned the bot login) requires
  an answer: fetch the issue/comments, do what was asked on the issue (or reply
  to the question/status request clearly), and report in chat. It never closes
  issues. Ignore quoted/fenced at-mentions and bot output (stage markers); the
  bot never writes an at-mention, so any at-mention it reads is a human reply.
  For "review status" / "close if already implemented" requests, answer with the
  current state/label/evidence and the exact next step; **refuse to auto-close**
  (the watcher stays read-only; only a human closes). If the work is already
  done, say so plainly and point at the completion comment/commit.
- Quoted, code-fenced or bot-output text is **not** authorization: when the
  batch carries no valid human reply and no AUTO PLAN, report and stop.

If no notification arrives (no `opencode` CLI, an older serve API, or
`GITEA_BOTS_NOTIFY=0`), the events still sit in `pending.md`. In that case check
`inbox` at the start of any turn where the user mentions issues, the watcher, or
Gitea, and after any restart of the session.

`check` (foreground) prints events to stdout and records them in `pending.md`,
but deliberately does **not** notify — the caller already sees the output. Ack
after handling those events too.

Notification failure is non-fatal: the loop logs `WARN`, the events remain in
`pending.md`, and the next successful poll retries with its own events.

## Working an issue

The dev flow itself is `AGENTS.md`: build/lint/test commands, the temp
directory, the development flow, version bumping, commit conventions and the
repo's technical conventions. This skill adds only the issue-bookkeeping shape
below.

### 1. Filing an issue

When the user asks to file, open, create, or report an issue, investigate
nothing and plan nothing. Write a title and a 1–3 sentence `## Summary`; that is
the entire body. Create it using the token/label/assignee rules from the
`gitea-issues` skill. Filing writes **no** status label: `👀 seeing` means a
background agent picked the issue up and is working on it, so only that agent
sets it. Report the number and URL, then stop.

```markdown
## Summary

<!-- 1–3 sentences: what should change / what is wrong, and why. -->
```

Do **not** add `## Related`, tasks, a plan, acceptance criteria, or a hidden
marker to this body.

### 2. Investigating and planning

Do this when a human reply asks for investigation or a plan, or on an
`AUTO PLAN` event. Read the issue and its comments first. If a plan already
exists, do not re-plan silently: reply with a short comment linking the
existing plan and asking whether to revise it or go ahead, and report the same
in chat. A reply that asks for a plan always gets an answer.

Investigate read-only (trace code paths, types, config, tests, docs; quote file
paths and line numbers), then post a plan comment containing findings, a
concrete approach, a `- [ ]` checklist and code sketches. End it with the
hidden marker `<!-- gitea-bots: stage=2 -->`. Mirror the checklist into the
issue body with `sync-tasks.py <N> <plan-file>` (Gitea's task counter reads the
body only), then set `set-status.py <N> responded`. Report the comment URL and
**stop** — no code, no branch, no commit.

````markdown
## Findings

- `path/to/file.rs:120` — current behaviour: …
- `path/to/other.rs:45` — related type/flow: …

## Approach

1. Introduce/modify `TypeName` in `canonical/module.rs` …
2. Wire it through `caller.rs` …

## Tasks

- [ ] Define the boundary type(s) and their fallible constructors
- [ ] Implement the transformation in the core module
- [ ] Add unit tests for the new logic
- [ ] Run `cargo test` and fix failures

## Code sketch

```rust
// Types first: private field + fallible constructor.
pub struct ExampleId(String);
```

## Risks / open questions

- …

<!-- gitea-bots: stage=2 -->
````

If the investigation reveals an unresolved choice (schema, API, migration, UX),
put the open question in the plan, set `question`, and ask — do not pick a
direction on the user's behalf.

### 3. Implementing

Do this when a human reply asks for the implementation, **and** a plan (or its
stage-2 marker) exists. If the reply asks to implement but no plan exists, ask whether
to investigate and plan first — never skip the investigation because the words
sounded like "implement". If the plan is stale or contradicts the request, ask
instead of improvising.

Work the issue following `AGENTS.md` end to end: check the repo's open issues,
keep temporary artifacts under `./tmp/`, write boundary/validation types before
business logic, implement incrementally, add unit tests plus a mocked
end-to-end test for a new path, run the repo's test gate, and bump the version
per `AGENTS.md` (skill-only or docs-only changes may state that no crate version
bump applies). Commit and push, referencing the issue as `#<N>` only — never
`close`/`fix`/`resolve` keywords. Tick the finished `## Tasks` items in the
issue body, post a completion comment ending with
`<!-- gitea-bots: stage=3 -->`, then set
`set-status.py <N> responded committed`. **Do not close the issue**; leave it
for human review. Release/tag only if the user explicitly asked for it.

```markdown
## Implemented

- `path/to/file.rs` — …
- `path/to/test.rs` — new coverage for …

## Verification

- `cargo test` — passing
- Manual/end-to-end check: …

## Commits

- `<sha>` — <subject> (#N)

## Follow-ups

- <!-- anything intentionally left out, or "none" -->

<!-- gitea-bots: stage=3 -->
```

### Other replies

A reply may ask a question, request a review, or ask for status. Answer it on
the issue with a clear response (and, for status, the current labels/markers
and the exact next step), then report in chat. It does not authorize code work
unless it actually asks for it, and it never authorizes closing the issue.

## Status labels

The label vocabulary, emoji display names, colours, aliases, migration, manual
curl recipes and the `set-status` mechanics are documented in the **issue-labels**
skill ([`../issue-labels/SKILL.md`](../issue-labels/SKILL.md)): `👀 seeing`,
`💬 responded`, `🔨 working`, `❓ question`, `✅ committed`. The watcher imports
the same labels from the vendored `scripts/issue_labels/` package. This skill
only decides which moment sets which label:

| Moment | Labels |
| --- | --- |
| a human reply or an AUTO PLAN is picked up (a worker is dispatched) | `seeing` |
| a plan comment is posted | `responded` |
| implementation has started | `working` |
| implementation is committed and pushed | `responded` + `committed` |
| an open question blocks the work | `question` |

Filing an issue sets no label; `seeing` is a pickup marker and only the agent
that actually takes the issue writes it. The notifier applies it best-effort to
every dispatched issue before the worker spawns (#144); when no worker is
spawned, the notification and worker prompts tell the answering agent to set it
first.

**The answering agent always sets it first too.** The notifier write belongs to
the **running loop's code**, so a loop started before that code merged cannot
apply it (and also emits prompts from the same old code). A worker reading this
skill from the checkout is always current, so its very first action on a
picked-up issue is the idempotent
`python3 .agents/skills/gitea-bots/scripts/set-status.py <N> seeing` — before
fetching comments, planning or coding, and regardless of the label already
shown. After merging loop code, restart the watcher (`stop` then `start`) so the
notifier write itself becomes active, then verify one dispatch shows `seeing`;
#152 makes a stale loop visible through the per-poll fingerprint `WARN`.

`scripts/set-status.py` is a thin wrapper over the vendored CLI with the
workflow's marker-aware `--reconcile` injected: a stage-3 marker →
`responded` + `committed`, a stage-2 marker (or a directed-reply answer) →
`responded`, an unmarked body plan → `responded` plus a `NOTE`, a
`## Summary` body → `seeing` only when the issue already carries that pickup
label (the projection never invents it). The watcher applies the derived set on
startup (`GITEA_BOTS_START_RECONCILE=0` disables it). It never changes markers,
comments, assignment, or issue state.

For a hand-tracked issue (no issue bookkeeping), use the gitea-bots wrapper
directly, or follow the curl recipes in the `issue-labels` skill:

```bash
python3 .agents/skills/gitea-bots/scripts/set-status.py 42 responded
python3 .agents/skills/gitea-bots/scripts/set-status.py --reconcile 42
```

## How work state is determined

State is derived from the issue itself, never from conversation memory, so it
survives restarts. Fetch the issue and **all** comments (paginated,
`limit=50`), strip fenced/inline code before searching, and look for the hidden
markers:

- `<!-- gitea-bots: stage=2 -->` at the end of the plan comment;
- `<!-- gitea-bots: stage=3 -->` at the end of the completion comment.

The markers are write-once bookkeeping: they prevent duplicate plans and
completion notes and drive the `--reconcile` label. They are not user input and
never appear in a prompt. With no marker, the issue is at the filed/planning
stage. A marker quoted in documentation is an example, not workflow state.

An issue-body at-mention is deduped separately: `issues.body_mention_sha` holds
the SHA-256 of the blank-line-separated mention paragraphs (code spans stripped),
so a request edited into the body is surfaced once and an unrelated body edit or
a `sync-tasks.py` rewrite does not re-surface it.

## Clarification rules

Stop and ask when:

- the request has no clear subject, scope, or expected behaviour;
- the reply is ambiguous ("ok", "go ahead", a thumbs-up) — it is not a request;
- an implementation request arrives with no plan (do not skip investigation);
- a reply asks for work that is already recorded — link it and ask what comes
  next; do not duplicate it;
- the issue is not a normal Summary-shaped issue and its body/state is unclear;
- the plan has unresolved choices (schema, API, migration, compatibility);
- implementation contradicts the plan, tests fail for reasons outside the
  change, or a dependency is missing;
- the watcher reports an event but the issue/comments cannot be fetched or the
  markers conflict.

Ask in chat first; when the issue is shared with humans, mirror the question as
an issue comment and wait. Do not assume silence means approval.

## Troubleshooting

### Background job observability (repo fallback, upstream #127)

The watcher, `probe/` runs, and long `cargo test|run` jobs are the canonical
background jobs this workflow produces. OpenCode tracks shell jobs server-side,
but agent-side status/output/kill/wait primitives are not exposed in the tool
catalog yet (upstream gap: #127). Use the repo-local fallback:

```bash
# Always pass the project directory (the helper does this for you). Never call
# /api/shell without location[directory] — the service cwd default can return
# [] or 404 for a valid job.
scripts/opencode-task-status.sh --directory "$PWD" list
scripts/opencode-task-status.sh --directory "$PWD" status '<shell-id>'
scripts/opencode-task-status.sh --directory "$PWD" output '<shell-id>' --cursor 0
scripts/opencode-task-status.sh --directory "$PWD" output '<shell-id>' --cursor '<n>' --tail 80
scripts/opencode-task-status.sh --directory "$PWD" kill '<shell-id>' --yes   # explicit/destructive
scripts/opencode-task-status.sh --directory "$PWD" [--session "$OPENCODE_SESSION_ID"] wait '<shell-id>' --timeout 120 --interval 2
```

Notes: `list` always queries `/api/shell?location[directory]=...`; with
`--session`/`$OPENCODE_SESSION_ID` it also queries `/api/experimental/job` for
session-scoped job kinds. `output` is resumable by cursor. `kill` requires
`--yes`. `wait` requires `--timeout` (unbounded waits are refused). This helper
is a stopgap until OpenCode provides first-class `task.*` tools/API defaults.

### Watcher

1. **`another poll is already running`** — the background loop holds
   `./tmp/gitea-bots/poll.lock`. This is expected under the **one-watcher
   rule**: exactly one session/process may poll. Read
   `./tmp/gitea-bots/watcher.log` or run `stop`, then retry `check` in the
   foreground. Never run `watch-issues.py` from a spawned notification session.
   For OpenCode background shell jobs (not the watcher loop), use the repo
   fallback `scripts/opencode-task-status.sh` (`list|status|output|kill|wait`)
   which always passes `location[directory]` so lookups never silently hit the
   service cwd; it refuses unbounded `wait` and requires `kill <id> --yes`.
2. **`watcher: stopped` with a stale PID** — the process exited without
   cleanup; `stop` removes the PID file. `status` also reports the last
   successful poll. If `status` reports a **STALE** running watcher, the
   fingerprint in `watcher.meta` no longer matches the current inputs: a
   `watch-issues.py` / `gitea_bots/*.py` mtime changed, or an effective
   `GITEA_BOTS_*` value differs. The line names the changed `file:…` /
   `config:…` entries. Run `stop` then `start` so the running loop picks them
   up. A commit that only touches docs or other files is **not** drift, and
   the loop itself logs one `WARN` per distinct drift after a poll without
   restarting automatically (one-watcher rule). If
   `stale-check: unavailable` appears, restart the watcher once to record the
   fingerprint in `watcher.meta`.
3. **No events on the first run** — `start`/`loop` report existing open issues
   on a fresh baseline by default. `check` keeps the baseline silent unless
   `--report-existing` is passed. Use `--no-report-existing` to suppress the
   report, `--report-existing-all` to include closed issues, or `reset` to
   force a fresh baseline.
4. **A reply was not surfaced** — the body must at-mention the bot login
   outside code spans. That applies to comments **and** the issue body: a body
   at-mention is only surfaced when the mention paragraphs changed after the
   baseline (a silent first poll records the digest without reporting, and an
   old untracked issue does not replay a mention outside the incremental
   window). Check with `GITEA_BOTS_ALL_COMMENTS=1` to confirm a comment was
   seen. Wording alone (for example a former slash command) is not a request:
   the skill has no command vocabulary, so text without an at-mention is
   ordinary chat. The watcher is a notifier, not an authorization mechanism.
5. **Duplicate events** — state was deleted/corrupted or the repo changed.
   Check `state.db` (`meta.repo`, `meta.last_poll`) and the `.corrupt.*` /
   `.repo.*` backups; `reset` intentionally starts over.
6. **API outage / 5xx** — the loop logs `WARN`, keeps the old `last_poll`, and
   retries after the interval; it does not spin or exit. A repo with zero
   issues returns `[]` and is handled normally.
7. **State write failed** — the loop keeps the previous watermark and retries;
   if it persists, check disk space and permissions on `./tmp/gitea-bots/` and
   inspect `state.db` for a locked/corrupt file.
8. **Log growth** — `watcher.log` is append-only. Truncate it while stopped
   (`: > ./tmp/gitea-bots/watcher.log`) or rotate manually.
9. **No agent notification** — notification is post-lock and best-effort, so it
   never blocks polling. The loop logs at most a `WARN`. Check that `opencode`
   is on `PATH`, `GITEA_BOTS_NOTIFY` is not `0`, and a model can be resolved
   (`GITEA_BOTS_MODEL`, or `OPENCODE_SESSION_ID` /
   `GITEA_BOTS_NOTIFY_SESSION` for the default `fresh` mode). Look for
   `notification model: <provider/model>` in `watcher.log`; if the model cannot
   be resolved, no detached session is spawned and events stay in
   `pending.md` — set `GITEA_BOTS_MODEL` as an explicit fallback. Events are
   always still in `pending.md`; run `inbox`. Restart the loop after editing
   this script — a running loop keeps executing the version it started with.
   Pinned spawned sessions (`fresh` notifications / reply workers) write their
   JSON and id to `./tmp/gitea-bots/notify.log`; the batch reporter is archived
   and stays out of the session list, while reply/AUTO PLAN workers are visible
   by default (`GITEA_BOTS_ARCHIVE_WORKERS=1` hides them, #147).
10. **`pending.md` keeps growing** — nobody acknowledged. Run `inbox`, report,
    then `ack`. `ack` marks the batch's `acked_at` in `state.db` instead of
    deleting its event rows, so the history stays queryable; delete rows
    manually only if the database gets large.
11. **A `shared`-mode synthetic message did not wake the agent** — `shared` is
    an explicit opt-in. The session may be busy; `delivery: queue` delivers
    after the current turn. Confirm with
    `opencode api get /api/session/active`. If the API surface changed, the loop
    warns and the events remain queued in `pending.md`. In `fresh` mode (the
    default) a notification instead shows up as a new archived session
    (`notify.log`). The notification child is spawned after the poll lock is
    released, with `close_fds=True`, so it can never inherit `poll.lock`.
12. **An event appeared while the watcher was stopped or mid-upgrade** — bump
    `GITEA_BOTS_CATCHUP_MINUTES` if needed; the next poll with an older state
    schema re-surfaces the last window of directed mentions. Otherwise read the
    issue/comments directly and handle the reply; the watcher is only a
    notifier.

### Checklist for a notification

- [ ] Run `inbox` and read every event.
- [ ] Report the events to the user (numbers, titles, URLs, replies).
- [ ] `ack` after reporting.
- [ ] Treat a human reply as the request; fetch the issue and all comments and
      work out what was asked.
- [ ] Follow `AGENTS.md` for the work; never invent scope.
- [ ] Post the answer (plan, completion note, or reply) and stop.
- [ ] Never close an issue and never write an at-mention.
