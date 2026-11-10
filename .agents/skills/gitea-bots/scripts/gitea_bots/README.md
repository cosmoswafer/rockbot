# gitea_bots — Python watcher tooling

A dependency-light, high-cohesion / low-coupling Python implementation of the
gitea-bots watcher tooling. This is the **only** implementation: the deprecated
bash scripts and their black-box tests were removed in #155. Use the
`watch-issues.py`, `set-status.py`, `sync-tasks.py`, `probe-live.py`,
`probe-live-write.py` and `probe-e2e.py` entry points below.

The Python tooling uses the SQLite store `state.db`. A state directory created
before #135 still has the flat files (`state.json`, `events.jsonl`, `pending/`,
`pending-ack/`, `ack/`); the first Python run imports them once and archives
them under `legacy-json-<ts>/`. That one-shot compatibility path is removed on
**2027-01-01**, or sooner once every known `GITEA_BOTS_STATE_DIR` has
`meta.imported_from_files=1` and no flat files remain
([details](#legacy-surface-audit-155)). SKILL.md points at the Python entry
points throughout.

## Requirements

- Python 3.10+
- [`requests`](https://pypi.org/project/requests/) — the **only** non-stdlib
  dependency.
- `git` on `PATH` (used only to derive the API host / `owner/repo` from
  `origin`). No `curl`, `jq`, `flock`, `awk`, or `stdbuf` is needed: HTTP uses
  `requests`, JSON uses the stdlib `json` module, file locks use `fcntl`, and the
  background loop is line-buffered via `python -u`.

## Entry points

| Entry point | Purpose |
| --- | --- |
| `watch-issues.py` | Poll for issues / comments / human replies / mentions; `check`, `loop`, `start`, `stop`, `status`, `inbox`, `ack`, `reset`, `scan-unplanned` |
| `set-status.py` | Apply or `--reconcile` the derived status label |
| `sync-tasks.py` | Mirror a plan's `## Tasks` checklist into the issue body |
| `probe-live.py` | Opt-in, read-only probe of the real Gitea API shapes (`-n/--issues N`, `--verbose`) |
| `probe-live-write.py` | Opt-in write-data probe: one dummy issue through create → comment → label → assign → close → delete, with guaranteed cleanup |
| `probe-e2e.py` | Opt-in end-to-end probe: dummy issues → real watcher events/state/labels → guaranteed cleanup (`--verbose`) |

Run them with `python3`, e.g.:

```bash
python3 scripts/watch-issues.py check
python3 scripts/watch-issues.py loop --interval 20
python3 scripts/watch-issues.py start
python3 scripts/watch-issues.py inbox
python3 scripts/watch-issues.py ack
python3 scripts/watch-issues.py status
python3 scripts/watch-issues.py stop
python3 scripts/set-status.py --reconcile 42
python3 scripts/sync-tasks.py 42 ./tmp/plan.md
python3 scripts/probe-live.py
python3 scripts/probe-live.py --issues 5 --verbose
python3 scripts/probe-live-write.py
python3 scripts/probe-e2e.py        # stop the watcher first
```

All environment variables from `SKILL.md` are honoured (`GITEA_TOKEN` from
`.env`, `GITEA_BOTS_*`). `scripts/watch-issues.py --help` lists the commands and
options.

## Architecture (cohesion / coupling)

Wiring happens **once**, in `cli.py`; every other module has a single concern
and depends only on small, explicit interfaces. Pure decision logic has no I/O
at all, which is why it is unit-testable with a fake client and no network.

```
cli.py  (composition root: builds Config -> components -> command handlers)
  |
  +-- config.py    env / .env / repo root / remote derivation / paths / identity
  +-- identity.py  runtime fingerprint: module mtimes + effective config (pure)
  +-- clock.py     UTC helpers (pure)
  +-- rules.py     mentions, markers, task-list helpers (pure); re-exports
  |                the shared label vocabulary/body helpers
  +-- models.py    normalized Issue value object (one API-shape boundary)
  +-- errors.py    re-exports GiteaError + BotFatal
  +-- log.py       injectable structured logger
  +-- locks.py     fcntl file locks
  |
  +-- client.py    re-exports issue_labels.gitea.GiteaClient
  +-- db.py        SQLite schema/connection, event+poll+tasks persistence,
  |                one-shot legacy-file migration
  +-- state.py     meta + issue cursors over the DB; atomic poll commits
  +-- inbox.py     batches + events over the DB, pending.md view, notify text
  +-- notify.py    opencode notification (fresh / shared, post-lock)
  +-- status.py    shared StatusManager + the workflow marker-aware derive
  +-- tasks.py     sync-tasks
  +-- watcher.py   polling + command verbs (writes only status labels)
  +-- live_probe.py  opt-in read-only real-server scenarios (no write paths)
  +-- write_probe.py opt-in write lifecycle on one dummy issue
  +-- e2e_probe.py   opt-in dummy issues -> real watcher -> state/labels -> cleanup
```

Key design points:

- `Config` is one value object injected into everything; `GiteaClient` is the
  sole network boundary and is duck-typed, so tests inject a `FakeClient`.
- The watcher's only remote writes are **in-process** status labels: the
  startup reconcile and the pickup `seeing` label applied before a dispatched
  worker starts (#144), both through `StatusManager`, instead of shelling out to
  a bash/jq/curl helper — that is what keeps the whole package dependency-free.
- Issue reads (comments, timeline) are prefetched with a bounded
  `ThreadPoolExecutor` (`GITEA_BOTS_MAX_CONCURRENCY`, default 8); classification,
  event emission and every `state.db` / `pending.md` write stay serial in the
  main thread (#138). `GiteaClient` keeps one `requests.Session` per thread
  unless a session is injected (tests).
- `requests` is imported in exactly one module
  (`issue_labels/gitea.py`, re-exported by `client.py`). If it is missing, the
  entry points print a clear error instead of a traceback.
- The store is one stdlib `sqlite3` database (WAL, `busy_timeout`, foreign keys,
  `PRAGMA user_version`). `meta` / `issues` replace the old `state.json`;
  `batches` / `events` replace the batch files + `events.jsonl`; `tasks` mirrors
  `sync-tasks.py`; `polls` records one row per poll. `pending.md` is generated.
  `migrate_from_files()` imports the legacy files once in a single transaction
  and archives them under `legacy-json-<ts>/`. Schema v2 adds
  `issues.body_mention_sha`; `ensure_schema()` applies it to an existing
  database with an idempotent `ALTER TABLE`.

## Behavioural notes

- There is **no command vocabulary** (#134). A comment is a request only when
  a human at-mentions the bot login (shared-login mode) or the author differs
  from the token user (dedicated-bot mode); what the reply asks for is read
  from the issue and the repo's `AGENTS.md`, not from slash commands. Since
  #185 the same rule applies to an at-mention edited into the **issue body**:
  it emits `DIRECTED COMMENT on #N (issue body)` and is deduped by
  `issues.body_mention_sha`, a digest of the mention paragraphs only.
- `.env` precedence: `cwd/.env` < repo-root `.env` < process environment. A
  real exported `GITEA_TOKEN` is never clobbered by a stale `.env` value.
- Notification children are spawned with `close_fds=True` and `start_new_session`
  **after** the poll lock is released, so a long-lived `opencode` session can
  never inherit `poll.lock`.

## Runtime identity and config drift (#152)

A loop loads `Config` and its modules once at startup, so a merged default or a
`GITEA_BOTS_*` change stays inactive until a restart. `identity.py` captures a
`Fingerprint` of the inputs that actually matter, and `watcher.meta` stores it
alongside `pid` / `started_at`, the resolved notification `model` (from
`GITEA_BOTS_MODEL` or the launching session; empty when unresolved), and
informational `script_commit` / `script_mtime`:

- `files`: nanosecond mtimes of `watch-issues.py` and every `gitea_bots/*.py`,
  keyed by relative path;
- `config`: effective `Config` behaviour values, with the token, the raw
  `env`, `repo`/`api_base`, `self_login`, `script_path` and the per-invocation
  `notify_session` excluded.

Every `Config` field must be classified in `FINGERPRINT_CONFIG_FIELDS` or
`FINGERPRINT_EXCLUDED_FIELDS`; a unit test fails otherwise. `loop` re-checks
after every poll and logs one `WARN` per distinct drift (the labels name the
changed `file:…` / `config:…`), then stays quiet until the inputs change again.
It never restarts itself (one-watcher rule / `poll.lock`). `status` diffs the
same values, so a doc-only commit no longer reports `STALE` while an env-only
change does; a meta without a fingerprint reports `stale-check: unavailable`.
`status` also prints `notification model:`: the meta value, else
`GITEA_BOTS_MODEL`, else the running process's inherited session, else the last
model line in `watcher.log`, else `unresolved`.

## Status labels (#133)

The vocabulary, emoji display names, colours, aliases, migration and the label
manager are documented in the **issue-labels** skill and vendored here as
`scripts/issue_labels/`: `👀 seeing`, `💬 responded`, `🔨 working`,
`❓ question`, `✅ committed`. The label skill is curl-only, so this package is
what the watcher actually imports.

- `gitea_bots/__init__.py` puts the sibling `scripts/` directory on `sys.path`
  so the vendored package resolves with no install step
  (`ISSUE_LABELS_SCRIPTS` overrides it for an out-of-tree checkout).
- `rules.py`, `client.py`, `errors.py` and `config.py` re-export the moved
  names, so `watcher.py`, `models.py` and the tests keep their imports.
- `status.py` is a thin `StatusManager` subclass that injects the workflow's
  marker-aware derive; the shared manager owns apply/reconcile/migrate.
- `scripts/set-status.py` forwards to the vendored CLI with that derive, so all
  documented `set-status` commands keep working (marker-aware `--reconcile`).
  Manual label work through the `issue-labels` skill uses plain `curl`; this
  wrapper is the workflow-aware path.

The workflow-specific label mapping stays documented in `SKILL.md`.
`derive_status()` there still takes `(body, comments, labels)`; the shared
`issue_labels.labels.derive_status()` takes marker booleans.

`seeing` is a **pickup marker**, not a filing marker: filing an issue writes no
label, and the projection never invents `seeing` from a `## Summary` body. It is
only preserved while the issue is unplanned, so startup reconciliation cannot
erase the label of a worker that actually took the issue. The shared manager
also repairs a repo label's drifted colour/description in place (the canonical
`👀 seeing` description is "a background agent has picked the issue up and is
working on it").

The notifier write runs in the **running loop's code**, so a loop started before
that code merged cannot apply it. Every answering agent must therefore set the
label itself as its **first action** on a picked-up issue:
`python3 .agents/skills/gitea-bots/scripts/set-status.py <N> seeing` (idempotent,
works from the checkout even while the loop is stale). After merging loop code,
restart the watcher (`stop` then `start`) and verify one dispatch shows
`👀 seeing`; the `status` / per-poll fingerprint checks from
[#152](#runtime-identity-and-config-drift-152) surface a stale loop instead of
letting it stay silent.

## Notification architecture (#131 / #129 / #137)

The old `fresh` default spawned detached `opencode run` sessions with **no
`--model`**; those inherited the OpenCode default (`opencode/iceman-free`) and
failed with a free-tier 429 instead of using the launching session's model
(`opencode-go/hy4`). The Python implementation fixes the cause:

- **`fresh` is the default** (`GITEA_BOTS_NOTIFY_SESSION_MODE=fresh`, #137).
  Each event batch is handled by a fresh, short-lived, minimal-context
  `opencode run --archived` session carrying only the bounded notification
  prompt. The long-lived interactive session is never re-sent its accumulated
  history for a tiny batch, which keeps token usage and cost predictable.
- **`shared` is an explicit opt-in** (`GITEA_BOTS_NOTIFY_SESSION_MODE=shared`).
  Events are delivered back into the launching session
  (`GITEA_BOTS_NOTIFY_SESSION`, default `$OPENCODE_SESSION_ID`) via a bounded
  `delivery: queue` synthetic message, so they run on **the model the user
  already selected** — same-model by construction, no detached notification
  session needed.
- **Model resolution** (`notify.py`): `GITEA_BOTS_MODEL` override, else
  `opencode api get /api/session/<id>` → `data.model` →
  `provider/model[#variant]`. The resolved model is logged as
  `notification model: …` in `watcher.log`. If it cannot be resolved, **no
  model-less detached session is spawned** — the events stay safely in
  `pending.md`.
- **Pinned reply workers (#134)**: when a batch carries a **human reply** (see
  shared-login mode below), the watcher spawns **one fresh background session
  per replying issue** with `--model <resolved>`, each carrying only its
  own event block. Workers are non-archived (visible in the TUI session picker)
  by default; `GITEA_BOTS_ARCHIVE_WORKERS=1` restores the old hidden behaviour
  (#147). The batch reporter always stays archived, so a poll loop cannot flood
  the picker. Changing the flag needs a watcher restart. The worker's prompt is generic: read the issue and comments,
  work out what the human is asking for, then follow `AGENTS.md` — no stage
  names and no command grammar. Dispatch is concurrent and bounded by
  `GITEA_BOTS_MAX_CONCURRENCY` (#138). Every other notification session (the
  fresh batch session, or the interactive session in `shared` mode) gets a
  delegate prompt: **report + `ack` only**, never do the work itself and never
  spawn a second worker. Disable with `GITEA_BOTS_SPAWN_WORKERS=0`.
- **AUTO PLAN workers (#140)**: an `AUTO PLAN` event authorizes investigation
  and planning only, so the notifier spawns **one** extra pinned worker per
  poll for that batch's queued issue set (issues already covered by a human
  reply are dropped, so one issue never gets two workers). Its prompt
  re-verifies the issue is still open/plan-eligible/Summary-only with no plan,
  and links an existing plan instead of posting a duplicate. Disable with
  `GITEA_BOTS_AUTO_PLAN_WORKERS=0` (gated on `GITEA_BOTS_AUTO_PLAN`). When no
  worker is spawned (toggle off, spawn workers off, or no model), the
  notification prompt says the reporter itself must investigate and post a
  plan.
- **`fresh` sessions are always pinned** via `--model`; a model-less `fresh`
  spawn is refused.

`state.db` (`batches` + `events`) remains the authoritative event record;
`pending.md` is only its generated view. Notification is still just a wake-up
pointer and authorizes nothing on its own.

## Shared-login mode (#136)

The skill is designed for the **bot and human sharing one Gitea account**
(`saru` here), so the author login cannot distinguish them. The pure rule core
(`rules.py`) therefore uses a different invariant:

- **The bot never writes an at-mention** — not in comments, plans, completion
  notes or notification prompts (`inbox.py` prompts and SKILL.md templates use
  "at-mention of the bot login" wording, never a literal at-login).
- **Any at-mention of the bot login outside code spans is a human reply**,
  regardless of author. `is_directed_mention` / `classify_comment` surface it
  as `DIRECTED COMMENT` and the agent answers it. The reply is the request: the
  skill names no stage and defines no command vocabulary (#134).
- **Loop avoidance** comes from that invariant plus `strip_code_spans` (fenced,
  inline, indented code) and `is_bot_output` (stage markers): quoted at-mentions
  and bot output are never human replies. Plain self-authored comments with no
  at-mention stay dropped by default (`GITEA_BOTS_ALL_COMMENTS=1` surfaces them).
- `human_reply_blocks` (#134, formerly `human_trigger_blocks`) reads the
  `    | ` comment-body lines already present in the per-poll `events_text`, so
  per-issue worker prompts work without changing the durable `events` row
  format.
- `GITEA_BOTS_SHARED_LOGIN` (default `1`) selects this mode; `=0` restores the
  old login-equality guard for deployments with a dedicated bot account.
  `GITEA_BOTS_SELF` is unchanged (it only pins the token user's login).

## Legacy surface audit (#155)

The #135 SQLite migration left a ring of pre-SQLite surface behind. Each item
was grep-checked for live references before removal; this table records the
decision so it does not have to be re-litigated.

| Item | Decision | Reason |
| --- | --- | --- |
| `scripts/watch-issues.sh`, `set-status.sh`, `sync-tasks.sh` | removed | frozen since #132; nothing outside their own headers and the retired tests referenced them |
| `tests/watcher-behavior.test.sh`, `tests/set-status.test.sh` | removed | black-box tests for the removed bash scripts |
| `tests/notify-text.test.sh` | removed | sourced `watch-issues.sh` and asserted its old trigger grammar; the live prompt is guarded by `scripts/tests/test_notify.py` |
| `tests/opencode-task-status.test.sh` | kept | still guards the live `scripts/opencode-task-status.sh` helper |
| `Inbox.pending_dir` / `ack_dir` / `events_log` | removed | dead attributes; the one-shot import reads `state_dir` directly |
| `Database.migrate_from_files()` and its `_import_*` / `_archive_legacy` helpers | kept | still the only way to adopt a pre-#135 state directory (removal condition below) |
| `Inbox.migrate_legacy()` | kept | adopts a pre-batch-store `pending.md` for the same migration window |
| `legacy-json-<ts>/` archive | kept | migration evidence; safe to prune once every state dir is migrated |
| `pending.ack.*.md` snapshots and per-issue `*.json` dumps in `tmp/gitea-bots/` | prune manually | gitignored debugging leftovers; the tooling never reads them again |

### Migration removal condition

`migrate_from_files()` (and `migrate_legacy()`) are retired on **2027-01-01**,
or sooner once every known `GITEA_BOTS_STATE_DIR` has
`meta.imported_from_files=1` and no `state.json` / `events.jsonl` / `pending/` /
`pending-ack/` / `ack/` remains. At that point remove the `_import_*` and
`_archive_legacy` helpers, the `migrate_legacy()` adoption path, the `legacy`
column on `batches`, and the matching tests.

### Safe prune of local leftovers

`tmp/` is gitignored and local-only. The repo-level
`scripts/prune_tmp.py` owns the whole directory: it is a **dry run by
default**, refuses to remove live watcher state, and gates the archived flat
files behind the #155 migration condition. Preview, then apply:

```bash
python3 scripts/prune_tmp.py                     # safe categories, preview
python3 scripts/prune_tmp.py --apply             # delete scratch + artifacts
python3 scripts/prune_tmp.py --browsers --apply  # also drop browser profiles
python3 scripts/prune_tmp.py --truncate-logs --apply
```

`--truncate-logs` empties `watcher.log` / `notify.log` (never deletes them) and
refuses while `watcher.pid` points at a live process. Stop the watcher before
touching its state directory:

```bash
python3 .agents/skills/gitea-bots/scripts/watch-issues.py stop
python3 scripts/prune_tmp.py --apply
python3 .agents/skills/gitea-bots/scripts/watch-issues.py start
```

Retention rules (also summarised in `AGENTS.md`):

| Path | Retention |
| --- | --- |
| `tmp/gitea-bots/state.db`, `state.db-wal`, `state.db-shm` | live store — never prune |
| `tmp/gitea-bots/pending.md`, `poll.lock`, `pending.lock`, `watcher.pid`, `watcher.meta` | live — never prune |
| `tmp/gitea-bots/watcher.log`, `notify.log` | append-only logs — truncate only while stopped |
| `tmp/gitea-bots/legacy-json-*` | prune only once `meta.imported_from_files=1` and no flat state remains |
| `tmp/gitea-bots/pending.ack.*.md`, per-issue `*.json` / `*.md` / `*.jsonl` / `*.log`, helper dirs | scratch — prune any time |
| `tmp/gitea-bots/scratch/` | new per-issue scratch — prune any time |
| `tmp/artifacts/`, `tmp/verify/`, `tmp/site/`, `tmp/shots/`, `tmp/bria/` | generated output — prune any time |
| `tmp/browsers/`, legacy `chrome-*` / `firefox-*` profiles | opt-in (`--browsers`); deleting loses cookies/localStorage only |

New per-issue scratch belongs in `tmp/gitea-bots/scratch/`; screenshots, logs
and dumps belong in `tmp/artifacts/`. Flat files at either root are legacy and
are pruned as scratch. Do not run the prune while reply workers are writing.

`legacy-json-*` eligibility can be checked directly:

```bash
python3 - <<'PY'
import sqlite3

con = sqlite3.connect("./tmp/gitea-bots/state.db")
print(dict(con.execute(
    "select key, value from meta where key = 'imported_from_files'"
)))
PY
```


## Tests

Zero extra dependencies — standard-library `unittest` only:

```bash
python3 scripts/run-tests.py
# or
python3 -m unittest discover -s scripts/tests -t scripts
```

The suite covers the pure rule core, config/remote derivation, SQLite state
recovery and atomic poll commits, the one-shot legacy-file migration (state,
pending/acked batches, `events.jsonl`, rollback and re-open), the pending inbox
(including the "generated view must not resurrect itself" regression), status
reconcile/apply, task sync and the `tasks` mirror, the #152 runtime fingerprint
(field classification, JSON round-trips, named file/key diffs, warn-once drift,
commit-only status changes not stale, token absent from meta), end-to-end
watcher behaviour
(baseline reporting, AUTO PLAN dedup, directed mentions, no trigger vocabulary,
timeline re-assignment, catch-up) against an in-memory fake client, the #131
notification path (model resolution, shared-by-default delivery, pinned reply
workers, refusal of model-less spawns), the #136 shared-login mode (at-mention =
human reply, code-span stripping, bot-output exemption, dedicated-bot
back-compat, worker spawning for shared-login replies), the #138 concurrency
work (per-issue reply blocks, barrier-proven concurrent prefetch, bounded pools,
no lost events on a failed fetch, per-issue worker dispatch), the #140 AUTO PLAN
equivalence (anchored parser, dedupe/sort, reply-covered issues skipped, one
batch worker, both disable toggles, reporter fallback), and the #150
assignment-only plan-eligibility predicate shared by polling, catch-up and
`scan-unplanned`, the #185 issue-body mention path (paragraph extraction,
digest stability, watcher emission/dedupe/suppression, notifier worker
spawning, v1→v2 column migration), and the end-to-end probe (baseline →
fixtures → expected events → idempotence → REASSIGNED → pickup label → label
reconcile → cleanup, plus the live-watcher and overlapping-lock refusals)
against the same fake.

### Live probe test (opt-in, read-only)

The unit suite uses `FakeClient`, so it cannot catch a real Gitea API shape
change. The probe lives in `gitea_bots/live_probe.py` and covers the endpoints
the documented trigger→response scenarios depend on — issue list +
`## Summary` / `## Tasks` / stage-2 checks (AUTO PLAN), per-issue comments +
at-mention classification (DIRECTED COMMENT), `/issues/{n}/timeline` assignee
events (REASSIGNED), and repository labels — then runs the same watcher
predicates on the data it reads.

Run it directly against the real server (running the script is the opt-in):

```bash
python3 scripts/probe-live.py
python3 scripts/probe-live.py --issues 5 --verbose
```

It prints a per-scenario PASS/FAIL report and exits non-zero on any failure.
`-n/--issues N` bounds the comments/timeline reads (default 10).

The historical unittest gate still works as a thin adapter over the same probe
(`tests/test_live_gitea.py`), so the offline suite stays network-free:

```bash
GITEA_BOTS_LIVE=1 python3 scripts/run-tests.py
```

The probe is strictly read-only: `ReadOnlyClient` rejects every non-GET
request, and the notifier/status stubs refuse to write, so a regression fails
the probe instead of commenting, labelling, assigning or closing anything. Its
state lives in `tmp/gitea-bots-live-probe/`, never the live watcher's
`tmp/gitea-bots/` state. Offline coverage for the report assembly and the
scenario pass/fail propagation is `tests/test_live_probe.py`.

### Live write-data probe (opt-in, creates one dummy issue)

The read-only probe cannot prove the write endpoints, so
`gitea_bots/write_probe.py` runs the full lifecycle against **one dummy issue
it creates itself**: create → comment → label → assign → close → delete, each
step verified with a fresh `GET`.

Run it directly (running the script is the opt-in):

```bash
python3 scripts/probe-live-write.py
python3 scripts/probe-live-write.py --verbose
```

or as the unittest adapter, which keeps the historical opt-in gate so the
offline suite never creates issues:

```bash
GITEA_BOTS_LIVE_WRITE=1 python3 scripts/run-tests.py
```

It prints a per-step PASS/FAIL report and exits non-zero on any failure. Safety
model:

- every mutating call goes through the path of the issue number this run
  created, and the created issue is re-read (title marker checked) before the
  first mutation — an existing issue is never touched;
- the dummy body deliberately has no `## Summary` or `## Tasks`, so a live
  watcher can never emit AUTO PLAN or REASSIGNED for it; stop the loop anyway
  for a noise-free run;
- a `FileLock` in `tmp/gitea-bots-live-write-probe/` refuses overlapping runs
  and keeps the probe away from `tmp/gitea-bots/`;
- cleanup runs in `finally`: the dummy is deleted (and a temporary label, if
  one had to be created, is removed). Deletion is admin-gated: on a non-admin
  deployment the `403` fallback clears labels/assignees, keeps the dummy
  closed and fails the probe loudly with its URL rather than leaving a silent
  leftover.

Offline coverage for the happy path, mid-run cleanup, the `403` fallback, the
target guard and a label-less repo is `tests/test_write_probe.py`.

### Live end-to-end probe (opt-in, creates and deletes dummy issues)

The read and write probes prove the API, but neither runs the watcher. The
documented trigger→response scenarios were only covered offline against
`FakeClient`. `gitea_bots/e2e_probe.py` closes that layer: it creates
controlled dummy issues on the real server, lets the **real watcher** poll and
classify them, asserts the emitted events, `state.db` transitions, durable
batches and real status labels, checks idempotence and deletes everything
afterwards.

Run it directly (running the script is the opt-in):

```bash
# The production watcher would consume the dummy issues: stop it first.
python3 scripts/watch-issues.py stop
python3 scripts/probe-e2e.py
python3 scripts/watch-issues.py start
```

or as the unittest adapter, which keeps the opt-in gate so the offline suite
never creates issues:

```bash
GITEA_BOTS_LIVE_E2E=1 python3 scripts/run-tests.py
```

Fixtures are one run id `[probe] gitea-bots e2e <ts>` and cover:

| Case | Shape | Expected |
| --- | --- | --- |
| A | open, assigned, `## Summary` only | AUTO PLAN |
| B | open, unassigned, `## Summary` only | no AUTO PLAN (#150) |
| C | open, assigned, no `## Summary` | no AUTO PLAN (#151) |
| D | open, assigned, `## Summary` + `## Tasks` | no AUTO PLAN; REASSIGNED after unassign → assign |
| E | human comment at-mentioning the bot login (shared-login mode) | DIRECTED COMMENT |
| F | bot output (stage-3 marker) without an at-mention | no DIRECTED COMMENT |
| H | closed issue | tracked as closed, never planned |
| I | unassigned Summary-only body with an at-mention paragraph | DIRECTED COMMENT (issue body), once (#185) |

The run sequence is baseline (silent) → fixtures → first incremental poll →
second poll (idempotence) → unassign/assign (REASSIGNED) → pickup-label case →
`set-status --reconcile` on every fixture → cleanup. The pickup case
uses a recording `Notifier`, so `👀 seeing` is asserted on A, E and I **without
starting any `opencode` process**. Reports are written under
`tmp/artifacts/gitea-bots-e2e-<ts>.log` (plus a structured `.json` sibling).

Safety model:

- the probe refuses to start while the production `watcher.pid` points at a
  live `watch-issues.py` process; its message says to run `watch-issues.py stop`
  first;
- its state lives in `tmp/gitea-bots-live-e2e/` and a sibling
  `tmp/gitea-bots-live-e2e.lock` refuses overlapping runs — never the live
  watcher's `tmp/gitea-bots/`;
- every fixture is created by this run, re-read (title marker verified) before
  its first mutation, and every mutating issue path goes through
  `_issue_path()`, which refuses an uncreated number;
- notifications and worker spawns are disabled for the polls; the optional
  pickup case records dispatches instead of spawning;
- cleanup runs in `finally`: each dummy is deleted and a `GET` must answer
  `404`; temporary labels created during the run are removed; a leftover scan
  fails the probe loudly. Delete is admin-gated, so a non-admin token gets the
  same `403` fallback as the write probe (labels/assignees cleared, issue kept
  closed, leftover URL reported).

Assertions are scoped to the run's issue numbers (never a bare substring), so
unrelated `UPDATED ISSUE` noise from another human touching the repo mid-run is
tolerated. Production state (`tmp/gitea-bots/state.db`, pending, `watcher.pid`)
is never written by the probe.

Offline coverage for the sequence, event parser, guards, mid-run cleanup, the
`403` fallback and artifact writing is `tests/test_e2e_probe.py`.

