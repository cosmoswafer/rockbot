---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, and manage issues on the project's Gitea server. Never closes issues automatically; comments on them and lets a human close. Determines server and repo from git remote; reads token from .env.
license: CC0-1.0
---

# gitea-issues — Gitea Issue Management

## Configuration

This skill auto-detects the Gitea server and repository from the `origin` git remote — never hardcode a server or repo. Run this setup once per shell session before any API call:

```bash
# 1. Read the origin remote — the single source of truth for server and repo
GITEA_REMOTE=$(git remote get-url origin)

# 2. Normalize scp-like syntax (git@host:owner/repo.git) into ssh:// form, strip any scheme
GITEA_REMOTE=$(echo "$GITEA_REMOTE" | sed -E 's#^([^@/:]+@[^:]+):#ssh://\1/#; s#^(https?|ssh|git)://##')

# 3. Extract host (strip SSH port — the API is served over HTTPS 443) and owner/repo
GITEA_HOST=$(echo "$GITEA_REMOTE" | sed -E 's#^[^@]*@([^/:]+)(:[0-9]+)?/.*#\1#; t; s#^([^/:]+)(:[0-9]+)?/.*#\1#')
GITEA_OWNER_REPO=$(echo "$GITEA_REMOTE" | sed -E 's#^[^/]+/##; s#\.git$##')

# 4. Derive the API base and load the token
GITEA_API="https://$GITEA_HOST/api/v1"
source .env && echo "$GITEA_API/repos/$GITEA_OWNER_REPO"
```

Resulting variables (example for `ssh://git@git.tokyofy.top:33022/Atom/rockbot.git`):

- **Server**: `git.tokyofy.top` (API host derived from remote; SSH port `33022` is stripped)
- **API base**: `https://git.tokyofy.top/api/v1` (from remote)
- **Owner/Repo**: `Atom/rockbot` (from remote)
- **Token**: `$GITEA_TOKEN` — sourced from `.env` (`source .env`). Never echo, print or log its value; nothing from `.env` belongs in terminal output or logs.

If your Gitea serves HTTPS on a non-standard port, append it manually, e.g. `GITEA_API="https://$GITEA_HOST:8443/api/v1"`.

Other repos on the same server can be accessed by setting `GITEA_OWNER_REPO` explicitly, e.g. `export GITEA_OWNER_REPO=Atom/weasel`.

## Common Operations

All commands below use `$GITEA_API` and `$GITEA_OWNER_REPO` derived from the git remote as shown above.

### List all open issues

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get a specific issue (including pull requests — use `&type=issues` to exclude PRs)

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with additional filters

```bash
# All issues (including closed)
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=all&limit=50" \
  | jq '.[] | {number, title, state, comments, updated_at}'

# Issues with pagination (Gitea default is 30 per page)
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open&page=1&limit=50" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get comments on an issue

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create a new issue

**Every new issue uses the standard body template below.** The mandatory
`## Tasks` section is a Markdown task list (`- [ ]` items): Gitea renders it as
a progress counter (e.g. `1/3`) in the issue list, and tasks are ticked off as
work lands.

```markdown
## Summary

<!-- 1–3 sentences: what is wrong / what should change, and why. -->

## Tasks

- [ ] <concrete, verifiable task>
- [ ] <concrete, verifiable task>

## Related

- <!-- related issues / PRs (#123), files, screenshots, references -->
```

Template rules:

- `## Tasks` is **required** with at least one `- [ ]` item. Break work into
  small, independently verifiable steps; a task should map to a commit or
  another reviewable change.
- Mark tasks `- [x]` as they complete by PATCHing the issue body — there is no
  checkbox API. The body is the single source of progress until a human closes
  the issue.
- Never invent filler tasks; for a pure question or investigation, make the
  investigation steps the tasks.
- Headings may be translated to match the issue language (e.g. `## 待辦` for
  `## Tasks`), but the checklist syntax must stay intact.
- **Never add an `## Acceptance criteria` section.** Observable conditions
  belong in `## Tasks` as verifiable checklist items — acceptance criteria
  duplicate tasks, inflate the checkbox counter with non-work items, and the
  milestone-review flow drops the section anyway. Omit `## Related` only when it
  genuinely adds nothing.
- When the issue is driven by the [`gitea-bots`](../gitea-bots/SKILL.md)
  workflow, mirror the stage-2 plan checklist into the body with
  `sync-tasks.py <N> <plan-file>` instead of editing it by hand.

Payload notes: labels must be numeric IDs (see "List available labels") — an
empty array `[]` or IDs like `[24]`; `assignees` takes the token user's login
(see "Get current user"); the milestone JSON key is `milestone` with a numeric
ID, **not** `milestone_id` — the latter is silently ignored.

For simple bodies without special characters, use inline JSON:

```bash
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"title\":\"Issue title here\",\"body\":\"Issue body / description\",\"labels\":[],\"assignees\":[\"$GITEA_USER\"]}" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

For templated bodies (backticks, quotes, newlines), write the Markdown to a
file and let `jq` assemble the payload — no shell escaping needed:

```bash
# 1. Write the issue body (standard template, with real content)
cat > ./tmp/issue-body.md << 'MDEOF'
## Summary

Quick camera records are timestamped with the photo's EXIF date instead of the
server time, so records from devices with wrong clocks land on bogus dates.

## Tasks

- [ ] Ignore EXIF DateTimeOriginal in the quick camera flow
- [ ] Stamp new records with server submission time
- [ ] Add a regression test

## Related

- #553
MDEOF

# 2. Build the JSON payload ($GITEA_USER from "Get current user")
jq -n \
  --arg title "Quick camera: use server time, not photo EXIF" \
  --rawfile body ./tmp/issue-body.md \
  --arg user "$GITEA_USER" \
  '{title: $title, body: $body, labels: [], assignees: [$user], milestone: 61}' \
  > ./tmp/issue.json

# 3. Create the issue
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/issue.json \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

### Add a comment to an issue

```bash
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"body":"Comment text here"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27/comments" \
  | jq '{id, body, html_url}'
```

### Close an issue (manual only)

Never close an issue automatically — only when a human explicitly asks you to.
A human closes the issue after reviewing the work (see "Closing issues").

```bash
source .env && curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"closed"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state}'
```

### Re-open an issue

```bash
source .env && curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"open"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state}'
```

### List available labels

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels" \
  | jq '.[] | {id, name, color}'
```

### Issue status labels

The repo uses a shared five-label status vocabulary so issues stay scannable
whether they are tracked manually or advanced by the `gitea-bots` workflow.
The canonical source is the
[`issue-labels` skill](../issue-labels/SKILL.md).

| Canonical | Display | Colour | Meaning |
| --- | --- | --- | --- |
| `seeing` | `👀 seeing` | `1d76db` | a background agent has picked the issue up and is working on it |
| `responded` | `💬 responded` | `fbca04` | a reply, answer or plan is posted |
| `working` | `🔨 working` | `5319e7` | implementing code/tests |
| `question` | `❓ question` | `d93f0b` | blocked, waiting on human input |
| `committed` | `✅ committed` | `0e8a16` | a commit has been pushed (additive with `responded`) |

Lifecycle: `seeing` → `responded` → `working` / `question` → `responded` +
`committed`. `question` is transient; `committed` is additive. Filing an issue
writes no status label: `seeing` is a pickup marker, and only the agent that
takes the issue sets it.

Gitea's issue `PATCH` **silently ignores `labels`**. Change labels through the
dedicated endpoint with the **full** set of numeric label IDs to keep:

```bash
# 1. Resolve label IDs (names may carry a leading emoji)
source .env
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
  | jq -r '.[] | [.id, .name] | @tsv'

# 2. Replace the issue's label set (include unrelated labels you keep)
curl -s -X PUT \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"labels": [24, 31]}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/42/labels" \
  | jq '.[].name'
```

Create a missing canonical label with its exact display name, colour and
description:

```bash
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"🔨 working","color":"5319e7","description":"implementing code/tests"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels" \
  | jq '{id, name, color}'
```

For automation, the shared CLI resolves IDs and merges the set for you:

```bash
python3 .agents/skills/issue-labels/scripts/set-status.py 42 working
python3 .agents/skills/issue-labels/scripts/set-status.py --reconcile 42
```

### List milestones

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/milestones" \
  | jq '.[] | {id, title, state, open_issues, closed_issues}'
```

### Find latest milestone

```bash
source .env

# From open milestones, newest first: "<created_at> <id> <title>"
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/milestones?state=open" \
  | jq -r '.[] | "\(.created_at) \(.id) \(.title)"' | sort -r | head -n1
# Example output: 2026-08-11T12:11:16+08:00 61 v3.5.6

# Or from the most recent open issue that carries a milestone
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open&sort=created&direction=desc&limit=10" \
  | jq -r '.[] | select(.milestone != null) | .milestone.title' | head -n1
```

### Create an issue with a milestone

Pass `milestone` by numeric ID in the payload:

```bash
source .env && cat > ./tmp/issue.json << JSONEOF
{
  "title": "Issue title",
  "body": "Issue body",
  "labels": [29],
  "assignees": ["$GITEA_USER"],
  "milestone": 61
}
JSONEOF

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/issue.json \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url, milestone: {id: .milestone.id, title: .milestone.title}}'
```

### Add / update an issue's milestone

Gitea accepts the numeric ID through the `milestone` field of `PATCH`:

```bash
source .env && ISSUE=418 && MILESTONE_ID=61 && curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"milestone\":$MILESTONE_ID}" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE" \
  | jq '{number, title, milestone: {id: .milestone.id, title: .milestone.title}}'
```

### Get current user (for assignees)

`assignees` needs the token user's login. The `/user` endpoint is preferred,
but issue-only tokens (scope `write:issue`, `write:repository`) get a 403 for
missing `read:user`, so fall back to the creator of a recent issue — the token
user created the issues it filed:

```bash
source .env

GITEA_USER=$(curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/user" | jq -r '.login // empty')

if [ -z "$GITEA_USER" ]; then
  GITEA_USER=$(curl -s -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=all&limit=1" \
    | jq -r '.[0].user.login // empty')
fi

echo "Current user: $GITEA_USER"
```

If the login is still unknown, create the issue without `assignees`, read
`user.login` from the response, then `PATCH` the issue to add the assignee.

## Troubleshooting — common failures

Check these before escalating; each one breaks an operation silently:

1. **`{"message":"invalid username, password or token"}`** — wrong auth header.
   Gitea expects the literal word `token`, not `Bearer`:
   `-H "Authorization: token $GITEA_TOKEN"`. `Bearer` returns 401 / invalid
   token even for a valid token.
2. **`404 page not found` / `{"message":"not found"}`** — usually a wrong host.
   `$GITEA_API` must come from the `origin` remote, never from a guess such as
   `gitea.com` (a public instance, almost never this project's server). Fix a
   wrong remote with `git remote set-url origin <correct-url>` before anything
   else, and confirm `$GITEA_OWNER_REPO` is the exact `owner/repo`.
3. **`user redirect does not exist`** — the repo slug's casing or owner is
   wrong; Gitea is case-sensitive there. Take the path verbatim from the web UI.
4. **Empty / unexpected response** — `.env` was not sourced or `$GITEA_TOKEN` is
   empty. Echo the derived `$GITEA_API` and `$GITEA_OWNER_REPO` at the top of a
   script to catch derivation mistakes — never the token.
5. **`token does not have at least one of required scope(s)`** — the token is
   scoped to issues only. Skip `/user` and use the "Get current user" fallback;
   endpoints like `read:user` are not available.

Quick sanity check before any write:

```bash
source .env && echo "API=$GITEA_API  REPO=$GITEA_OWNER_REPO" && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO" | jq '.full_name // .message'
```

If the last line prints `owner/repo`, the server, token and auth format are all
correct and subsequent calls will work.

## Referencing issues in commit messages

Reference the issue as `#<number>` — Gitea links pushed commits carrying that
form into the issue's timeline ("Referenced from …").

- **Correct:** `#615`, `Ref: #615`, `(#615)` — the number directly follows `#`.
- **Wrong:** `Ref: 615`, `issue 615`, `# 615` — not recognised, no link created.

```text
feat: stamp quick camera records with server time (#615)
```

```text
fix: ignore EXIF DateTimeOriginal in quick camera flow

Ref: #615
```

Placement is flexible — inline in the subject or as a `Ref: #<number>` trailer;
multiple issues can be referenced in one message (`#615 #620`). Still obey the
auto-close rule below: never prefix a reference with `close`, `closes`, `fix`,
`fixes`, `fixed`, `resolve` or `resolves`.

## Closing issues

**Never close issues automatically.** A human decides when an issue is done:

- **No auto-closing keywords in commit messages** — avoid `close`, `closes`,
  `closed`, `fix`, `fixes`, `fixed` followed by an issue reference (e.g.
  `close #415`), which Gitea acts on at push time. Use the plain `#<number>`
  form instead (see "Referencing issues in commit messages").
- **No API calls to close automatically** — never `PATCH` an issue's `state` to
  `closed` as part of finishing work. "Close an issue (manual only)" exists for
  the case where a human explicitly asks you to close a specific issue.

Instead, when work on an issue is done:

1. Run the project's checks before committing (`cargo test`, `deno task check`,
   … per that repo's conventions).
2. Comment on the issue describing what changed, where, and the relevant
   commits, tests or verification.
3. Let a human review and close it.

## Milestone review — mark implemented issues ready for review

When asked to review a milestone (e.g. `v3.7.10`), go through each issue and
decide whether the codebase already implements it:

1. **Detect implementation.** Grep the files/symbols referenced in the issue
   body (`## Tasks` / `## Related`) and check
   `git log --all --oneline --grep "<issue number>"` for the implementing
   commit. Absence of the described change means not implemented.
2. **If implemented — tick every `## Tasks` item and drop the acceptance
   criteria.** `PATCH` the body (no checkbox API; the body is the single source
   of progress) so each task reads `- [x]`, and delete any
   `## Acceptance criteria` section — the human review is the acceptance check,
   and leftover unticked boxes read as unfinished work. Then comment with the
   implementing commit(s) and the verification done. A fully-ticked `## Tasks`
   means *implemented, ready for review, waiting to close*. **Never close the
   issue yourself.**
3. **If review finds problems with implemented work — create a new issue**
   (standard template) referencing the original number, and let a human decide
   what happens to the old one. Do not reopen the original or silently widen
   its scope.
4. **If not implemented — leave the tasks unticked.** The empty checkboxes say
   it; do not add "not done yet" comments.

Ticking a task in an issue body:

```bash
# 1. Fetch the current body, flip the checkbox, drop acceptance criteria
source .env && curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/687" | jq -r '.body' > ./tmp/body.md
sed -i 's/^- \[ \] Verify: edit the duration/- [x] Verify: edit the duration/' ./tmp/body.md
sed -i '/^## Acceptance criteria/,/^## Related/{/^## Related/!d}' ./tmp/body.md

# 2. PATCH the body back
jq -n --rawfile body ./tmp/body.md '{body: $body}' > ./tmp/patch.json
curl -s -X PATCH -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" -d @./tmp/patch.json \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/687" | jq '{number, state}'
```

## Usage Notes

- **Always** run the Configuration setup first to derive `GITEA_API` and
  `$GITEA_OWNER_REPO` from the git remote, then `source .env` so
  `$GITEA_TOKEN` is available. Never hardcode server or repo names, and never
  echo the token.
- If `jq` is not installed, omit the `| jq ...` pipe; the raw JSON is still
  readable.
- The issues endpoint returns pull requests too. To distinguish: PRs have a
  `pull_request` field (non-null). Use `type=issues` query param to exclude PRs
  (supported in newer Gitea versions).
- Pagination: Gitea defaults to 30 items per page. Use `&limit=50` or `&page=N`
  for more.
- **Labels require numeric IDs**, not names. List available labels with their
  IDs first (see "List available labels" above), then use the numeric `id` value
  in the `labels` array (e.g. `"labels":[24]` for the "bug" label). Using label
  name strings will return a JSON unmarshal error. Issue `PATCH` silently
  ignores `labels`; use `PUT /issues/{n}/labels` with the full set (see "Issue
  status labels").
- **Milestones take a numeric ID under the `milestone` key** — `milestone_id`
  and milestone names are silently ignored, by both `POST` and `PATCH`.
- **Complex bodies**: when the issue body contains special characters
  (backticks, quotes, parentheses, newlines), write the Markdown to a file and
  build the payload with `jq -n --rawfile body ./tmp/issue-body.md`, then pass
  it with `-d @file.json` — never inline `-d '...'` with hand-rolled escaping.
- **New issues always use the standard template**, including the required
  `## Tasks` checklist and no `## Acceptance criteria`. Empty bodies,
  free-form prose, or tasks without checkboxes are not acceptable.
- **Assignees** take valid Gitea usernames (not emails or display names). Get
  the token user's login from "Get current user"; if the token lacks
  `read:user` scope use the fallback, or create the issue unassigned and `PATCH`
  the assignee afterward.
