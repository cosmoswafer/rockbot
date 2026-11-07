---
name: gitea-bots
description: Use when a request should go through a staged Gitea-issue workflow — filing a Summary-only issue, investigating and posting an implementation plan, then implementing it via the repo's std-dev flow. Includes scripts/watch-issues.sh, a polling watcher that surfaces new issues and stage-trigger comments without ever advancing a stage itself. Every stage is human-gated; never close issues.
license: CC0-1.0
---

# gitea-bots — Staged Gitea Issue Workflow

A three-stage, human-gated workflow on top of Gitea issues:

1. **init** — file a new issue containing only a short Summary.
2. **investigate & plan** — investigate the codebase, post a plan with a task
   checklist and code sketches.
3. **implement** — run the repo's std-dev flow, commit, push, comment.

The stages are strictly sequential and each one is gated. The bot **stops after
every stage** and does nothing until a human explicitly triggers the next one.
`scripts/watch-issues.sh` only *surfaces* new issues and trigger comments; it
never creates, edits, closes, or advances anything.

## Hard rules

- **Stop after each stage.** Completing a stage is not consent for the next one.
- **Only explicit triggers advance a stage.** Silence, "ok", "thanks", "LGTM",
  a thumbs-up, an emoji, or an ambiguous "go ahead" is **not** a trigger — ask.
- **Never skip stage 2.** An `/implement` with no plan comment is a clarification
  request, not permission to start coding.
- **Never infer triggers from bot-authored text.** The bot's own plan may quote
  `/implement`; quoting is not authorization. Check the comment's author and
  position.
- **Never close issues.** Comment with what changed and let a human close.
  No auto-closing keywords (`close`, `fix`, `resolve`, …) in commits either.
- **Never guess.** If the request, plan, or implementation is unclear, stop and
  ask in chat (and/or as an issue comment) before acting.
- **Never expose the token.** Source `.env` silently, send
  `Authorization: token $GITEA_TOKEN`, never echo it, never enable `set -x` in
  snippets that touch it.

## Setup

Dependencies: `bash` 4+, `curl`, `jq`; `flock` is used when available.

```bash
chmod +x .agents/skills/gitea-bots/scripts/watch-issues.sh
```

`GITEA_TOKEN` must already be in `.env` at the repo root. Every operation starts
by deriving the API base and repository from `origin` exactly like the
`gitea-issues` skill — never hardcode host or owner/repo:

```bash
source .env >/dev/null 2>&1

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then
  BARE="${REMOTE#*://}"; BARE="${BARE#*@}"
  REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"
else
  BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"
fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"
```

After this block: `$GITEA_API`, `$REPO`, and `$GITEA_TOKEN` are available. The
snippets below assume it has already run. For labels, assignees, milestones,
and API failure modes, see the [`gitea-issues` skill](../gitea-issues/SKILL.md).

Sanity check (never prints the token):

```bash
curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" | jq '.login // .message'
```

## Watcher — `scripts/watch-issues.sh`

Polls the Gitea API for new/changed issues and new issue comments. New comment
events are filtered to the stage-trigger phrases unless all-comments mode is on.

```bash
# one-shot: poll once, print events, exit
.agents/skills/gitea-bots/scripts/watch-issues.sh check

# first run only: also list issues that already exist
.agents/skills/gitea-bots/scripts/watch-issues.sh check --report-existing

# background loop (nohup + PID file); status/stop use the same PID file
.agents/skills/gitea-bots/scripts/watch-issues.sh start --interval 60
.agents/skills/gitea-bots/scripts/watch-issues.sh status
.agents/skills/gitea-bots/scripts/watch-issues.sh stop

# foreground loop (tmux/herdr, Ctrl-C to stop)
.agents/skills/gitea-bots/scripts/watch-issues.sh loop --interval 60

# move state aside; next poll re-baselines silently
.agents/skills/gitea-bots/scripts/watch-issues.sh reset
```

Default interval is 45 s, configurable with `--interval N` or
`GITEA_BOTS_INTERVAL`. `check` refuses to run while the background loop holds
the poll lock — read the log or stop the loop first.

Sample output:

```
2026-11-08T10:05:00Z NEW ISSUE #42 "Quick camera: use server time" by alice — https://gitea.example/Atom/rockbot/issues/42
2026-11-08T10:06:12Z TRIGGER COMMENT on #42 (comment 910) by alice — https://gitea.example/Atom/rockbot/issues/42#issuecomment-910
    | /plan
2026-11-08T10:06:12Z UPDATED ISSUE #42 — changed: labels — https://gitea.example/Atom/rockbot/issues/42
```

Event tags: `NEW ISSUE`, `UPDATED ISSUE` (title/state/labels/assignees/milestone
changed, or `body/other` when only the issue body changed), `NEW COMMENT`
(all-comments mode), `TRIGGER COMMENT` (matched a stage trigger). Comment-only
updates produce comment events without a redundant issue event. A complete
comment body is printed under the event, prefixed `    | `.

### State, logs, and restarts

All runtime state lives under `./tmp/gitea-bots/` (never `/tmp`):

| Path | Purpose |
| --- | --- |
| `state.json` | per-issue `updated_at` + field snapshot + `last_comment_id`; global `last_poll` |
| `watcher.pid` | PID of the background loop (checked against `/proc/<pid>/cmdline`) |
| `watcher.log` | background-loop output (append-only) |
| `poll.lock` | `flock` guarding state writes |

- The **first** poll is a silent baseline: it records existing issues and their
  latest comment IDs without emitting events. Use `--report-existing` to list
  them instead; comments stay silent.
- Restarting never re-reports: issues dedupe by `updated_at`/field snapshot and
  comments by monotonic comment ID. A 120 s overlap absorbs clock skew.
- Incremental polls fetch issues updated since `last_poll − 120 s`, paginated
  (50/page, up to 20 pages), and fetch comments for every issue in that window.
  Comment IDs dedupe; this also catches a comment written in the same second as
  the issue update.
- If any API call fails, the loop logs `WARN`, leaves `last_poll` unadvanced,
  and retries after the interval. It never spins or crashes the loop.
- If `state.json` is corrupt or belongs to another repo, it is backed up and the
  watcher re-baselines automatically.
- `reset` moves `state.json` aside; the next poll starts a new baseline.

### Trigger matching

Comments are matched case-insensitively against this ERE (a stage-2 or stage-3
trigger from the contract below):

```
/(investigate|plan|implement)([^[:alnum:]_]|$)|(^|[^[:alnum:]_])(investigate|execute dev flow|implement the plan|plan how to implement|plan the implementation)([^[:alnum:]_]|$)
```

Plain `plan` is deliberately **not** a trigger; the explicit phrases are.
Comments authored by the token user are ignored by default so the bot cannot
trigger itself (`GITEA_BOTS_IGNORE_SELF=0` disables that, `GITEA_BOTS_SELF=<login>`
skips the `/user` lookup). If the human and the bot share a token, set
`GITEA_BOTS_IGNORE_SELF=0` or the human's triggers will be hidden.

The watcher greps the raw comment body and does not understand Markdown fences,
so a trigger quoted in a code block can still be surfaced. It is a notifier, not
an authorization mechanism — the agent must strip documentation examples and
check authorship/order before acting.

Useful environment variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `GITEA_BOTS_INTERVAL` | `45` | poll interval in seconds (min 5) |
| `GITEA_BOTS_STATE_DIR` | `./tmp/gitea-bots` | state directory |
| `GITEA_BOTS_LOG` | `$STATE_DIR/watcher.log` | background log |
| `GITEA_BOTS_ALL_COMMENTS` | `0` | `1` = report every new comment |
| `GITEA_BOTS_IGNORE_SELF` | `1` | `0` = include token-user comments |
| `GITEA_BOTS_SELF` | — | token user's login (skips `/user`) |
| `GITEA_BOTS_TRIGGER_RE` | built-in | override trigger ERE |
| `GITEA_BOTS_VERBOSE` | `0` | `1` = log poll heartbeats |

The watcher only reads: it never labels, assigns, comments, or closes. It does
not tell the agent which stage to run — the agent decides that from the issue
body, the comments, their authors, and their order.

## The three-stage contract

| Stage | Name | Trigger (all explicit) | Output | After |
| --- | --- | --- | --- | --- |
| 1 | init | chat request: "file/open/create/report an issue" | new issue with **Summary only** | **STOP** |
| 2 | investigate & plan | `/investigate`, `/plan`, `investigate`, `plan how to implement this`, `plan the implementation` | plan comment: findings, approach, task checklist, code sketches | **STOP** |
| 3 | implement | `/implement`, `execute dev flow`, `implement the plan` | code, tests, commit + push, completion comment | **STOP** |

Anything else — including `ok`, `go ahead`, `LGTM`, a thumbs-up, or a comment
that merely discusses the plan — is **not** a trigger. Ask.

### How stage state is determined

Workflow state is derived from the issue itself, never from conversation memory,
so it survives restarts:

1. Fetch the issue and **all** comments (paginated, `limit=50`):

   ```bash
   curl -s -H "Authorization: token $GITEA_TOKEN" \
     "$GITEA_API/repos/$REPO/issues/$N" \
     | jq '{number, title, state, user: .user.login, body, html_url}'
   curl -s -H "Authorization: token $GITEA_TOKEN" \
     "$GITEA_API/repos/$REPO/issues/$N/comments?limit=50&page=1" \
     | jq -r '.[] | [.id, .created_at, .user.login, (.body | gsub("\n"; " ") | .[0:120])] | @tsv'
   ```

2. Classify comments by author: comments authored by the token user (or
   containing a stage marker) are bot output; the rest are human comments.
3. Find markers:
   - stage 2 complete: `<!-- gitea-bots: stage=2 -->` at the end of the plan
     comment;
   - stage 3 complete: `<!-- gitea-bots: stage=3 -->` at the end of the
     completion comment;
   - stage 1 has **no marker** — its signature is an issue body with a
     `## Summary` section and **no** `## Tasks` section, plus no plan marker in
     the comments.

   Strip fenced code blocks (and inline code) before searching. A marker or
   trigger quoted in documentation is an example, not workflow state.
4. Current stage = the highest completed marker (0 if none).
5. Authorization for stage N requires **both**:
   - stage N-1 is complete, and
   - a **human-authored** comment created **after** the stage N-1 marker
     contains an exact stage N trigger.
   Stage 1 is authorized by the user's chat request instead.
6. Special cases:
   - `/implement` with no stage-2 marker → do **not** skip; ask whether to
     investigate and plan first.
   - a trigger inside the issue body, a bot comment, quoted text, a code block,
     or discussed-out-loud prose → not authorization.
   - multiple/conflicting triggers, or a trigger that arrives before the prior
     stage's output is visible → ask which stage to run.
   - an existing issue **not** created by this flow: if it has a Summary body it
     can serve as stage 1; otherwise ask before treating it as one.
   - a `NEW ISSUE` event for an issue this bot just created is its own stage-1
     output — do not start a new workflow on it.
7. After a restart, re-run steps 1–5. Watcher state tells you which events are
   new; it does not define workflow state.

## Stage 1 — init (default)

This is the **default** output whenever the user asks to file, open, create, or
report a new issue. Do not investigate the codebase, do not plan, do not write
tasks.

Procedure:

1. If the request is ambiguous (no clear subject or expected change), ask a
   clarifying question first.
2. Write a title and a 1–3 sentence `## Summary`. That is the entire body.
3. Create the issue using the token/label/assignee rules from `gitea-issues`:
   numeric label IDs (or `[]`), assignees from `/user`, `milestone` as a numeric
   ID only when a milestone is explicitly requested. If `/user` fails, create
   without `assignees` and PATCH the issue afterward.
4. Report the issue number and URL to the user.
5. **STOP.** Wait for an explicit stage-2 trigger.

> This deliberately overrides the `gitea-issues` rule that every new issue must
> contain `## Tasks`. In this workflow the checklist is authored at stage 2,
> after the investigation. Do **not** add Related, Tasks, a plan, acceptance
> criteria, or a hidden stage marker to the stage-1 body.

### Stage 1 — issue body template

```markdown
## Summary

<!-- 1–3 sentences: what should change / what is wrong, and why. -->
```

Nothing else belongs in the body — no `## Related`, no tasks, no plan, no
acceptance criteria, no stage marker.

### Stage 1 — create the issue

```bash
# Assumes the Setup block above ran (GITEA_API, REPO, GITEA_TOKEN).

cat > ./tmp/issue-body.md <<'MDEOF'
## Summary

Quick camera records are stamped with the photo's EXIF date instead of server
time, so devices with wrong clocks create records on bogus dates.
MDEOF

GITEA_USER=$(curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" \
  | jq -r '.login // empty')

jq -n \
  --arg title "Quick camera: use server time, not photo EXIF" \
  --rawfile body ./tmp/issue-body.md \
  --arg user "$GITEA_USER" \
  '{title: $title, body: $body, labels: [], assignees: ([$user] | map(select(. != "")))}' \
  > ./tmp/issue.json

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/issue.json \
  "$GITEA_API/repos/$REPO/issues" \
  | jq '{number, title, html_url}'
```

Then **stop**. An issue created outside this flow shows up as a `NEW ISSUE`
event in the watcher; treat it as stage 1 only after checking its body.

## Stage 2 — investigate & plan

Run only on an explicit stage-2 trigger from a human, after stage 1 is complete.
If a plan already exists, do not silently replace it — ask whether to revise it.

Procedure:

1. Re-read the issue and its comments; confirm stage 1 and that no stage-2
   marker exists. If a plan exists, ask before re-planning.
2. Investigate deeply, read-only: trace code paths, types, config, tests, and
   docs. Quote file paths and line numbers. Reproduce behaviour when cheap.
3. Post the plan as a comment using the template below. It must contain:
   - concrete findings tied to files/symbols,
   - a concrete implementation approach (not "investigate further"),
   - a Markdown task checklist (`- [ ]` items, used by Gitea's progress
     counter), and
   - example code / type sketches where useful.
4. End the comment with the stage marker `<!-- gitea-bots: stage=2 -->`.
5. Report the comment URL to the user.
6. **STOP.** No code, no branch, no commit, no edits.

If the investigation contradicts the request or reveals an unresolved choice
(schema, API, migration, UX), put the open question in the plan and **ask the
user** — do not pick a direction on their behalf.

### Stage 2 — plan comment template

````markdown
## Findings

- `path/to/file.rs:120` — current behaviour: …
- `path/to/other.rs:45` — related type/flow: …
- Root cause / gap: …

## Approach

1. Introduce/modify `TypeName` in `canonical/module.rs` …
2. Wire it through `caller.rs` …
3. Handle the failure case with `Error::Variant` …

## Tasks

- [ ] Define the boundary type(s) and their fallible constructors
- [ ] Implement the transformation in the core module
- [ ] Wire the new path into the caller / config / tool registry
- [ ] Add unit tests for the new logic
- [ ] Add a mocked end-to-end test for the new path
- [ ] Run `cargo test` and fix failures

## Code sketch

```rust
// Types first: private field + fallible constructor.
pub struct ExampleId(String);

impl TryFrom<&str> for ExampleId {
    type Error = ExampleIdError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        // validate at the boundary
        Ok(Self(raw.to_owned()))
    }
}
```

## Risks / open questions

- …

<!-- gitea-bots: stage=2 -->
````

The task list may be refined later, but every item must be concrete and
verifiable. Do not add an `## Acceptance criteria` section — the tasks are the
progress record. Post it:

```bash
# Assumes the Setup block above ran; N is the issue number.

cat > ./tmp/plan.md <<'MDEOF'
## Findings
…
<!-- gitea-bots: stage=2 -->
MDEOF

jq -n --rawfile body ./tmp/plan.md '{body: $body}' > ./tmp/comment.json
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/comment.json \
  "$GITEA_API/repos/$REPO/issues/$N/comments" \
  | jq '{id, html_url}'
```

## Stage 3 — implement

Run only on an explicit stage-3 trigger from a human, with a stage-2 plan
marker present. If the plan is stale, contradictory, or missing, **ask** —
never improvise around it.

Procedure:

1. Follow the [`std-dev` skill](../std-dev/SKILL.md) and AGENTS.md:
   - check open issues first; note the issue number for commit references;
   - probe only if an external response shape is genuinely unknown
     (artifacts under `./tmp/`);
   - map the data flow, then write the boundary/validation types **before**
     business logic;
   - implement incrementally: types → core logic → wiring;
   - add unit tests for new logic and a Wiremock-mocked end-to-end test for a
     new path;
   - run `cargo test` (the quality gate) and fix failures.
2. If implementation reveals that the plan was wrong or an ambiguity was not
   resolved, stop and ask instead of widening scope silently.
3. Commit and push per repo conventions: conventional commit describing the
   why, reference the issue as `#<N>` only (never `close`/`fix`/`resolve`
   keywords), never commit `Cargo.lock` or `config.toml`.
4. Comment on the issue with what changed: files, behaviour, tests run, commit
   hashes, and any follow-ups. End with `<!-- gitea-bots: stage=3 -->`.
5. **Do not close the issue.** Leave it for human review.
6. Release/tag only if the user explicitly asked for a release; stage-3
   consent alone does not authorize a version bump.

### Stage 3 — completion comment template

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

## Clarification rules

Stop and ask when:

- the request has no clear subject, scope, or expected behaviour;
- the trigger is ambiguous (`ok`, `go ahead`, `do it`, emoji), or arrives from
  an unexpected author/position;
- `/implement` arrives without a plan (do not skip stage 2);
- the issue is not a stage-1 issue and its body/state is unclear;
- the plan has unresolved choices (schema, API, migration, compatibility);
- implementation contradicts the plan, tests fail for reasons outside the
  change, or a dependency is missing;
- the watcher reports an event but the issue/comments cannot be fetched or the
  stage markers conflict.

Ask in chat first; when the issue is shared with humans, mirror the question as
an issue comment and wait. Do not assume silence means approval.

## Troubleshooting

### Watcher

1. **`another poll is already running`** — the background loop holds
   `./tmp/gitea-bots/poll.lock`. Read `./tmp/gitea-bots/watcher.log` or run
   `stop`, then retry `check` in the foreground.
2. **`watcher: stopped` with a stale PID** — the process exited without cleanup;
   `stop` removes the PID file. `status` also reports the last successful poll.
3. **No events on the first run** — expected: the first poll is a silent
   baseline. Use `check --report-existing` to list existing issues, or `reset`
   then `--report-existing`.
4. **A trigger comment was not surfaced** — the body must contain an exact
   trigger phrase; self-authored comments are ignored by default; check with
   `GITEA_BOTS_ALL_COMMENTS=1`. The watcher is a notifier: after seeing an
   event, re-read the issue and comments to judge authorship and stage.
5. **Duplicate events** — state was deleted/corrupted or the repo changed.
   Check `state.json` (`repo`, `last_poll`) and the `.corrupt.*` / `.repo.*`
   backups; `reset` intentionally starts over.
6. **API outage / 5xx** — the loop logs `WARN`, keeps the old `last_poll`, and
   retries after the interval; it does not spin or exit. A repo with zero
   issues returns `[]` and is handled normally.
7. **State write failed** — the loop keeps the previous state and retries; if
   it persists, check disk space and permissions on `./tmp/gitea-bots/`.
8. **Log growth** — `watcher.log` is append-only. Truncate it while stopped
   (`: > ./tmp/gitea-bots/watcher.log`) or rotate manually.

### Gitea API (same rules as gitea-issues)

1. **`invalid username, password or token`** — the header must be
   `Authorization: token $GITEA_TOKEN` (literal `token`, not `Bearer`).
2. **`not found` / 404** — wrong host or repo slug. Re-derive `$GITEA_API` and
   `$REPO` from `origin`; Gitea is case-sensitive on owner/repo.
3. **`/user` fails in `resolve_self`** — the token lacks `read:user`; the
   watcher disables self-filtering and warns. Pass `GITEA_BOTS_SELF=<login>` or
   set `GITEA_BOTS_IGNORE_SELF=0`.
4. **Empty responses** — confirm `.env` was sourced silently and
   `GITEA_TOKEN` is non-empty. Never print the token to debug.
5. **Labels/assignees in stage 1** — labels need numeric IDs, assignees need
   real usernames, milestones use the numeric `milestone` key (not
   `milestone_id`). See `gitea-issues`.

## Quick checklist

Before acting on a watcher event:

- [ ] Read the issue **and** all comments, not just the event line.
- [ ] Classify authors: human comment vs bot output.
- [ ] Verify the previous stage's marker/summary exists.
- [ ] Verify the trigger is exact, human-authored, and created after the
      previous stage.
- [ ] If anything is unclear — stop and ask.

After acting:

- [ ] Stage 1: body is Summary-only; no plan, no tasks.
- [ ] Stage 2: plan has Findings, Approach, Tasks (`- [ ]`), code sketch, and
      the `stage=2` marker.
- [ ] Stage 3: `cargo test` passes, commit/push done, issue commented with the
      `stage=3` marker, issue **not** closed.
- [ ] Stop and wait for the next explicit trigger.
