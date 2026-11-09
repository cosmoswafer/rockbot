---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, and manage issues on the project's Gitea server. Never closes issues automatically; comments on them and lets a human close. Determines server and repo from the git remote; reads the token from .env.
license: CC0-1.0
---

# gitea-issues — Gitea Issue Management

## Setup — derive server, API base and owner/repo from the git remote

**Every** operation starts with this block. Never hardcode the host or the
owner/repo path: they differ per repository and per environment.

```bash
source .env >/dev/null 2>&1

GITEA_REMOTE=$(git remote get-url origin)
GITEA_REMOTE="${GITEA_REMOTE%.git}"  # strip trailing .git if present

if [[ "$GITEA_REMOTE" == *://* ]]; then
  # URL form: https://host/owner/repo or ssh://git@host:33022/owner/repo
  GITEA_BARE="${GITEA_REMOTE#*://}"; GITEA_BARE="${GITEA_BARE#*@}"
  GITEA_HOST="${GITEA_BARE%%/*}"; GITEA_OWNER_REPO="${GITEA_BARE#*/}"
else
  # scp-like form: git@host:owner/repo (no scheme, colon-separated)
  GITEA_BARE="${GITEA_REMOTE#*@}"
  GITEA_HOST="${GITEA_BARE%%:*}"; GITEA_OWNER_REPO="${GITEA_BARE#*:}"
fi

GITEA_HOST="${GITEA_HOST%%:*}"  # strip the SSH port — the API is served over HTTPS
GITEA_API="https://${GITEA_HOST}/api/v1"

echo "$GITEA_API/repos/$GITEA_OWNER_REPO"  # sanity line; never echoes the token
```

After this block:

- `$GITEA_API` — API base, e.g. `https://git.tokyofy.top/api/v1` for the remote
  `ssh://git@git.tokyofy.top:33022/Atom/rockbot.git` (SSH port `33022` dropped)
- `$GITEA_OWNER_REPO` — `owner/repo`, e.g. `Atom/rockbot`
- `$GITEA_TOKEN` — already in `.env`; sourced silently, never printed, logged or
  echoed

An SSH remote's port belongs to `git` only and is stripped above because the
API is served over HTTPS on 443. If your Gitea serves HTTPS on a non-standard
port, append it manually: `GITEA_API="https://${GITEA_HOST}:8443/api/v1"`.

Other repos on the same server are reachable by overriding the path, e.g.
`GITEA_OWNER_REPO=Atom/weasel`.

All snippets below assume this block has already run in the same shell.

## Common Operations

### List all open issues

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get a specific issue (pull requests included — use `&type=issues` to exclude PRs)

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with additional filters

```bash
# All issues (including closed)
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=all&limit=50" \
  | jq '.[] | {number, title, state, comments, updated_at}'

# Issues with pagination (Gitea default is 30 per page)
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open&page=1&limit=50" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get comments on an issue

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create a new issue

**Every new issue is created from the standard body template below.** The
mandatory `## Tasks` section is a Markdown task list (`- [ ]` items): Gitea
renders it as a progress counter (e.g. `1/3`) in the issue list, and tasks are
ticked off as work lands.

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

- `## Tasks` is **required** and needs at least one `- [ ]` item. Break work
  into small, independently verifiable steps; a task should map to a commit or
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
  duplicate tasks, inflate Gitea's checkbox progress counter with non-work
  items, and the milestone-review flow drops the section anyway. Omit
  `## Related` only when it genuinely adds nothing.
- When an issue is driven by a planning workflow (for example the `gitea-bots`
  skill, where it exists), mirror the plan checklist into the body with that
  tooling (`sync-tasks.py <N> <plan-file>`) instead of editing the body by hand.

Payload notes:

- **Labels** must be numeric IDs (see "List available labels") — an empty array
  `[]` or IDs like `[24]`.
- **Assignees** take the token user's login — see "Get current user".
- **Milestone** uses the JSON key `milestone` with a numeric ID, e.g.
  `"milestone": 61`. The API does **not** accept `milestone_id` — using it
  silently drops the milestone.

For simple bodies without special characters, use inline JSON:

```bash
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"title\":\"Issue title here\",\"body\":\"Issue body / description\",\"labels\":[],\"assignees\":[\"$GITEA_USER\"]}" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

For templated bodies (backticks, quotes, newlines), write the Markdown to a
file and let `jq` assemble the JSON payload — no shell escaping needed:

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
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/issue.json \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

### Add a comment to an issue

```bash
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"body":"Comment text here"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27/comments" \
  | jq '{id, body, html_url}'
```

### Close an issue (manual only)

Never close an issue automatically — only when a human explicitly asks you to.
A human normally closes the issue after reviewing the work (see
"Closing issues").

```bash
curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"closed"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state}'
```

### Re-open an issue

```bash
curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"open"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state}'
```

### List available labels

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels" \
  | jq '.[] | {id, name, color}'
```

### Issue status labels

Repos that track agent work share a five-label status vocabulary so issues stay
scannable whether they are handled manually or by a background workflow. The
canonical source is the `issue-labels` skill where the repo carries one, at
`.agents/skills/issue-labels/SKILL.md`; repos without it still follow the same
vocabulary by hand.

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

Where the repo has the shared CLI, it resolves IDs and merges the set for you:

```bash
python3 .agents/skills/issue-labels/scripts/set-status.py 42 working
python3 .agents/skills/issue-labels/scripts/set-status.py --reconcile 42
```

### List milestones

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/milestones" \
  | jq '.[] | {id, title, state, open_issues, closed_issues}'
```

### Find the latest milestone

```bash
# Open milestones, newest first: "<created_at> <id> <title>"
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/milestones?state=open" \
  | jq -r '.[] | "\(.created_at) \(.id) \(.title)"' | sort -r | head -n1
# Example output: 2026-08-11T12:11:16+08:00 61 v3.5.6

# Or the milestone of the most recent open issue that has one
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open&sort=created&direction=desc&limit=10" \
  | jq -r '.[] | select(.milestone != null) | .milestone.title' | head -n1
```

### Create an issue with a milestone

Pass `milestone` by numeric ID in the payload:

```bash
cat > ./tmp/issue.json << JSONEOF
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
ISSUE=418
MILESTONE_ID=61

curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"milestone\":$MILESTONE_ID}" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE" \
  | jq '{number, title, milestone: {id: .milestone.id, title: .milestone.title}}'
```

### Get current user (for assignees)

`assignees` needs the token user's login. The `/user` endpoint is preferred,
but issue-only tokens (scope `write:issue`, `write:repository`) get a 403 for a
missing `read:user`, so fall back to the creator of a recent issue — the token
user filed the issues it created:

```bash
# Primary: /user endpoint (requires the read:user token scope)
GITEA_USER=$(curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/user" | jq -r '.login // empty')

# Fallback: creator login of the most recent issue
if [ -z "$GITEA_USER" ]; then
  GITEA_USER=$(curl -s \
    -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=all&limit=1" \
    | jq -r '.[0].user.login // empty')
fi

echo "Current user: $GITEA_USER"
```

If the login is still unknown, create the issue without `assignees`, read
`user.login` from the response, then PATCH the issue to add the assignee.

## Troubleshooting — common failures

These failure modes break operations silently. Check the symptom before
escalating:

1. **`{"message":"invalid username, password or token"}`** — the auth header is
   wrong. Gitea expects `Authorization: token $GITEA_TOKEN` (the literal word
   `token`, **not** `Bearer`). `Bearer` returns 401 / invalid-token even with a
   valid token. Never substitute `Bearer` here.

2. **`{"message":"not found"}` / `404 page not found`** — usually a wrong host.
   `$GITEA_API` must be derived from the `origin` remote
   (`git remote get-url origin`). Do **not** assume `gitea.com` — that is a
   public instance and is almost never this project's server. If the clone used
   the wrong remote URL, fix it first
   (`git remote set-url origin <correct-url>`). Also confirm
   `$GITEA_OWNER_REPO` is `owner/repo`, not the bare repo name.

3. **`user redirect does not exist`** — the `$GITEA_OWNER_REPO` path's
   casing or owner is wrong (e.g. `WeightManagement-frontend` vs
   `weightmanagement-frontend`). Gitea is case-sensitive on the repo slug; take
   it verbatim from the web UI.

4. **`token does not have at least one of required scope(s)`** — the token is
   scoped to issues/repository only, so `read:user` endpoints such as `/user`
   return 403. Use the fallback in "Get current user"; the repository and issue
   endpoints still work.

5. **Empty / unexpected response** — confirm `.env` was sourced silently
   (`source .env >/dev/null 2>&1`) and `$GITEA_TOKEN` is non-empty. Print the
   derived `$GITEA_API` and `$GITEA_OWNER_REPO` at the start of any script to
   catch derivation mistakes early. Never print or log `$GITEA_TOKEN` itself.

Quick sanity check before any write operation:

```bash
echo "API=$GITEA_API  REPO=$GITEA_OWNER_REPO"
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO" | jq '.full_name // .message'
```

If the last line prints `owner/repo`, the server, token and auth format are all
correct and subsequent calls will work.

## Referencing issues in commit messages

When a commit relates to a Gitea issue, reference it in the `#<number>` form —
e.g. `#615` for issue 615. Gitea scans pushed commit messages for `#<number>`
references and links the commit to the issue (it appears in the issue's
timeline as "Referenced from …", and the commit shows the issue link). Always
include such a reference when work is tied to an issue.

- **Correct:** `#615`, `Ref: #615`, `(#615)` — the number must directly follow
  `#`, with no space.
- **Wrong:** `Ref: 615`, `issue 615`, `# 615` — Gitea does not recognize these
  as issue references, so no link is created.

Example commit messages:

```text
feat: stamp quick camera records with server time (#615)
```

```text
fix: ignore EXIF DateTimeOriginal in quick camera flow

Ref: #615
```

Placement is flexible — inline in the subject (often as `(#615)`) or on its own
`Ref: #<number>` trailer line in the body. Multiple issues can be referenced in
one message (`#615 #620`).

Still obey the auto-close rule below: never prefix the reference with `close`,
`closes`, `fixed`, `fix`, `resolves`, etc.

## Closing issues

**Never close issues automatically.** A human decides when an issue is done — do
not close it yourself, and do not trigger Gitea's auto-close behavior:

- **No auto-closing keywords in commit messages** — avoid `close`, `closes`,
  `closed`, `fix`, `fixes`, `fixed` followed by an issue reference (e.g.
  `close #415`), which Gitea interprets as auto-close on push. Reference work
  with the plain `#<number>` form instead (e.g. `Ref: #615`) — see
  "Referencing issues in commit messages" above.
- **No API calls to close automatically** — never PATCH an issue's `state` to
  `closed` as part of finishing work. The manual close API is available in
  "Close an issue (manual only)" above, but use it only when a human explicitly
  asks you to close a specific issue.

Instead, when work on an issue is done:

1. Run the project's own checks before committing — its test/build task
   (`cargo test`, `deno task check`, `npm test`, …) as defined in that repo's
   conventions.
2. Leave a comment on the issue (see "Add a comment to an issue" above)
   describing the work done — what was changed, where, and any relevant commits,
   tests, or verification.
3. Let a human review and close the issue.

## Milestone review — mark implemented issues as ready for review

When asked to review a milestone (e.g. `v3.7.10`), go through each issue and
determine whether it has been implemented in the codebase:

1. **Detect implementation.** Grep the files/symbols referenced in the issue
   body (`## Tasks` / `## Related`) and check
   `git log --all --oneline --grep "<issue number>"` for the implementing
   commit. Absence of the described code/change means not implemented.

2. **If implemented — tick all `## Tasks` and drop the acceptance criteria.**
   PATCH the issue body (there is no checkbox API; the body is the single source
   of progress) so every task reads `- [x]`, and remove the
   `## Acceptance criteria` section entirely — the human review itself is the
   acceptance check, and leftover unticked boxes would misread as unfinished
   work. Then leave a comment pointing at the implementing commit(s) and the
   code-level verification done. A fully-ticked Tasks section means:
   _implemented, ready for review, waiting to close_. **Never close the issue
   yourself** (see "Closing issues" above) — a human closes it after review.

3. **If review finds problems with implemented work — create a new issue.** Do
   not reopen the original or silently widen its scope. Create a new issue
   (standard template, see "Create a new issue") that references the original
   issue number, and let a human decide what to do with the old one.

4. **If not implemented — leave tasks unticked.** Say nothing beyond what the
   empty checkboxes already convey; do not add "not done yet" comments.

Quick reference for ticking a task in an issue body:

```bash
# 1. Fetch the current body, flip the task's checkbox, drop acceptance criteria
curl -s -H "Authorization: token $GITEA_TOKEN" \
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

- **Always** run the Setup block first to derive `$GITEA_API` and
  `$GITEA_OWNER_REPO` from the git remote, and source `.env` silently so
  `$GITEA_TOKEN` is available. Never hardcode server or repo names; never read,
  echo or log the token.
- If `jq` is not installed, omit the `| jq ...` pipe; the raw JSON is still
  readable.
- The issues endpoint returns pull requests too. To distinguish: PRs have a
  `pull_request` field (non-null). Use the `type=issues` query param to exclude
  PRs (supported in newer Gitea versions).
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
  (backticks, quotes, parentheses, newlines), write the Markdown body to a file
  and build the payload with `jq -n --rawfile body ./tmp/issue-body.md` instead
  of inline `-d '...'` — see "Create a new issue".
- **New issues always use the standard template**, including the required
  `## Tasks` checklist and no `## Acceptance criteria`. Empty bodies, free-form
  prose, or tasks without checkboxes are not acceptable.
- **Assignees** take valid Gitea usernames (not emails or display names). Use
  "Get current user" to obtain the token user's login; if the token lacks the
  `read:user` scope the `/user` endpoint fails, so use the fallback, or create
  the issue without `assignees` and PATCH it afterward.
