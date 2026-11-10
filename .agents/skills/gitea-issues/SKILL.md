---
name: gitea-issues
description: Use when working with Gitea issues: list, create, comment on and manage issues on the project's Gitea server. Includes curl diagnostics for the token, .env, API base, owner/repo and server access. The skill never closes issues on its own. It comments and lets a human close. It derives the server and repo from the git remote and reads the token from .env.
license: CC0-1.0
---

# gitea-issues — Gitea issue management

## Setup — derive the server, API base and owner/repo from the git remote

**Every** operation starts with this block. Never hardcode the host or the
owner/repo path. They change per repository and per environment.

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

After this block you have:

- `$GITEA_API` — API base. For the remote
  `ssh://git@git.tokyofy.top:33022/Atom/rockbot.git` it is
  `https://git.tokyofy.top/api/v1`. The SSH port `33022` is dropped.
- `$GITEA_OWNER_REPO` — `owner/repo`, e.g. `Atom/rockbot`.
- `$GITEA_TOKEN` — already in `.env`. Source it silently. Never print, log or
  echo it.

The port in an SSH remote belongs to `git` only. The block drops it because
the API uses HTTPS on port 443. If your Gitea serves HTTPS on another port,
add the port yourself:
`GITEA_API="https://${GITEA_HOST}:8443/api/v1"`.

To reach another repo on the same server, override the path:
`GITEA_OWNER_REPO=Atom/weasel`.

Run the block in the same shell as the snippets below.

## Diagnostics — run this before any write

This read-only check prints what is wrong and what to do next. It never prints
the token.

```bash
echo "== gitea-issues diagnostics =="

for tool in git curl jq; do
  if command -v "$tool" >/dev/null 2>&1; then
    echo "PASS  $tool: $(command -v "$tool")"
  else
    echo "FAIL  $tool is missing — install it before using this skill"
  fi
done

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

echo "INFO  API=$GITEA_API  REPO=$GITEA_OWNER_REPO"

HTTP=$(curl -s -o /dev/null -w '%{http_code}' \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO")

case "$HTTP" in
  200) echo "PASS  repo access: HTTP 200" ;;
  401) echo "FAIL  HTTP 401 — token rejected; use the literal word 'token' in the header, not 'Bearer'" ;;
  403) echo "FAIL  HTTP 403 — token lacks repository access or the needed scope" ;;
  404) echo "FAIL  HTTP 404 — wrong host or owner/repo; check the origin remote" ;;
  000) echo "FAIL  no response — DNS, TLS or network problem reaching $GITEA_HOST" ;;
  *)   echo "FAIL  repo access: HTTP $HTTP" ;;
esac

curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO" \
  | jq -r 'if .full_name then "PASS  repository: \(.full_name)" else "FAIL  repository: \(.message // "unexpected response")" end'

# Optional: confirms the token user (needs read:user; a 403 here is not fatal)
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/user" \
  | jq -r 'if .login then "PASS  token user: \(.login)" else "WARN  token user: \(.message // "unavailable")" end'
```

If every line says `PASS`, the server, token, auth header and owner/repo are
correct. A `WARN` on the last line only means the token lacks `read:user`; the
issue endpoints still work. If any line says `FAIL`, fix it before writing.

## Common operations

### List all open issues

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get one issue

Gitea returns pull requests from this endpoint too. Add `&type=issues` to leave
them out.

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with more filters

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

### Get the comments on an issue

```bash
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create an issue

**Create every new issue from the standard template below.** The `## Tasks`
section is required. It is a Markdown task list (`- [ ]` items). Gitea renders
it as a progress counter (e.g. `1/3`) in the issue list. Tick tasks as work
lands.

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

- `## Tasks` is **required**. Add at least one `- [ ]` item. Break work into
  small steps you can verify on their own. One task is one commit or one other
  reviewable change.
- Tick a task with `- [x]` when it is done. Edit the issue body to do this;
  there is no checkbox API. The body is the only progress record until a human
  closes the issue.
- Do not invent filler tasks. For a pure question or investigation, make the
  investigation steps the tasks.
- You may translate the headings to match the issue language (e.g. `## 待辦`
  for `## Tasks`). Keep the checklist syntax intact.
- **Never add an `## Acceptance criteria` section.** Put observable conditions
  in `## Tasks` as checklist items you can verify. Acceptance criteria copy the
  tasks. They add non-work items to the Gitea progress counter. The milestone
  review drops the section anyway. Omit `## Related` only when it adds nothing.
- When a planning workflow drives the issue (for example the `gitea-bots`
  skill, where it exists), copy the plan checklist into the body with that tool
  (`sync-tasks.py <N> <plan-file>`). Do not edit the body by hand.

Payload notes:

- **Assignees** take the token user's login. See "Get the current user".
- **Milestone** uses the JSON key `milestone` with a numeric ID, e.g.
  `"milestone": 61`. The API does **not** accept `milestone_id`. If you use it,
  the milestone is dropped without an error.

For simple bodies without special characters, use inline JSON:

```bash
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"title\":\"Issue title here\",\"body\":\"Issue body / description\",\"assignees\":[\"$GITEA_USER\"]}" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

For templated bodies (backticks, quotes, newlines), write the Markdown to a
file. Let `jq` build the JSON payload. You need no shell escaping:

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

# 2. Build the JSON payload ($GITEA_USER from "Get the current user")
jq -n \
  --arg title "Quick camera: use server time, not photo EXIF" \
  --rawfile body ./tmp/issue-body.md \
  --arg user "$GITEA_USER" \
  '{title: $title, body: $body, assignees: [$user], milestone: 61}' \
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

Never close an issue automatically. Close it only when a human explicitly asks
you to. A human normally closes the issue after reviewing the work. See
"Closing issues".

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

### Add or update an issue's milestone

Gitea takes the numeric ID through the `milestone` field of `PATCH`:

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

### Get the current user (for assignees)

`assignees` needs the token user's login. Prefer the `/user` endpoint.
Issue-only tokens (scope `write:issue`, `write:repository`) get a 403 when they
lack `read:user`. In that case, fall back to the creator of a recent issue. The
token user filed the issues it created:

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

If you still do not know the login, create the issue without `assignees`. Read
`user.login` from the response. Then PATCH the issue to add the assignee.

## Troubleshooting — common failures

These failures break operations without an error. Check the symptom before you
escalate:

1. **`{"message":"invalid username, password or token"}`** — the auth header is
   wrong. Gitea expects `Authorization: token $GITEA_TOKEN`. Use the literal
   word `token`, **not** `Bearer`. `Bearer` returns 401 / invalid-token even
   with a valid token. Never substitute `Bearer` here.

2. **`{"message":"not found"}` / `404 page not found`** — usually a wrong host.
   `$GITEA_API` must come from the `origin` remote
   (`git remote get-url origin`). Do **not** assume `gitea.com`. That is a
   public instance and is almost never this project's server. If the clone used
   the wrong remote URL, fix it first
   (`git remote set-url origin <correct-url>`). Also confirm that
   `$GITEA_OWNER_REPO` is `owner/repo`, not the bare repo name.

3. **`user redirect does not exist`** — the casing or the owner in
   `$GITEA_OWNER_REPO` is wrong (e.g. `WeightManagement-frontend` vs
   `weightmanagement-frontend`). Gitea is case-sensitive on the repo slug. Copy
   it verbatim from the web UI.

4. **`token does not have at least one of required scope(s)`** — the token is
   scoped to issues/repository only, so `read:user` endpoints such as `/user`
   return 403. Use the fallback in "Get the current user". The repository and
   issue endpoints still work.

5. **Empty or unexpected response** — confirm that `.env` was sourced silently
   (`source .env >/dev/null 2>&1`) and that `$GITEA_TOKEN` is non-empty. Print
   the derived `$GITEA_API` and `$GITEA_OWNER_REPO` at the start of any script
   to catch derivation mistakes early. Never print or log `$GITEA_TOKEN`
   itself.

Run this sanity check before any write operation:

```bash
echo "API=$GITEA_API  REPO=$GITEA_OWNER_REPO"
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO" | jq '.full_name // .message'
```

If the last line prints `owner/repo`, the server, the token and the auth format
are all correct. Later calls will work.

## Referencing issues in commit messages

When a commit relates to a Gitea issue, reference it as `#<number>`, e.g. `#615`
for issue 615. Gitea scans pushed commit messages for `#<number>` and links the
commit to the issue. The commit appears in the issue timeline as
"Referenced from …". The commit also shows the issue link. Always add the
reference when work is tied to an issue.

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

Place it where you like: inline in the subject (often `(#615)`) or on its own
`Ref: #<number>` line in the body. One message can reference many issues
(`#615 #620`).

Obey the auto-close rule below. Never put `close`, `closes`, `fixed`, `fix` or
`resolves` before a reference.

## Closing issues

**Never close issues automatically.** A human decides when an issue is done. Do
not close it yourself. Do not trigger Gitea's auto-close behavior:

- **No auto-closing keywords in commit messages** — do not use `close`,
  `closes`, `closed`, `fix`, `fixes` or `fixed` before an issue reference
  (e.g. `close #415`). Gitea reads that as auto-close on push. Reference the
  work with the plain `#<number>` form instead (e.g. `Ref: #615`). See
  "Referencing issues in commit messages".
- **No API calls to close automatically** — never PATCH an issue's `state` to
  `closed` as part of finishing work. The manual close API is available in
  "Close an issue (manual only)". Use it only when a human explicitly asks you
  to close a specific issue.

Instead, when the work is done:

1. Run the project's own checks before you commit. Use its test or build task
   (`cargo test`, `deno task check`, `npm test`, …) as defined in that repo's
   conventions.
2. Add a comment to the issue (see "Add a comment to an issue"). Describe the
   work done: what changed, where, and any relevant commits, tests or
   verification.
3. Let a human review and close the issue.

## Milestone review — mark implemented issues as ready for review

When a human asks you to review a milestone (e.g. `v3.7.10`), check each issue.
Decide whether the codebase implements it:

1. **Check for the implementation.** Grep the files and symbols named in the
   issue body (`## Tasks` / `## Related`). Search for the implementing commit
   with `git log --all --oneline --grep "<issue number>"`. If the code or
   change is missing, the issue is not implemented.

2. **If implemented — tick all `## Tasks` and drop the acceptance criteria.**
   PATCH the issue body (there is no checkbox API; the body is the only progress
   record) so every task reads `- [x]`. Remove the `## Acceptance criteria`
   section entirely. The human review itself is the acceptance check. Leftover
   unticked boxes would read as unfinished work. Then add a comment that points
   at the implementing commit(s) and the code-level verification done. A
   fully-ticked Tasks section means: _implemented, ready for review, waiting to
   close_. **Never close the issue yourself** (see "Closing issues"). A human
   closes it after review.

3. **If the review finds problems with implemented work — create a new issue.**
   Do not reopen the original. Do not widen its scope without a word. Create a
   new issue (standard template, see "Create an issue") that references the
   original issue number. Let a human decide what to do with the old one.

4. **If not implemented — leave the tasks unticked.** Say nothing beyond what
   the empty checkboxes already show. Do not add "not done yet" comments.

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

## Usage notes

- **Always** run the Setup block first. It derives `$GITEA_API` and
  `$GITEA_OWNER_REPO` from the git remote and sources `.env` silently, so
  `$GITEA_TOKEN` is available. Never hardcode server or repo names. Never read,
  echo or log the token.
- Write issues, comments and plans in plain language: common words, short
  sentences, active voice, one word per meaning.
- If `jq` is not installed, drop the `| jq ...` pipe. The raw JSON is still
  readable.
- The issues endpoint returns pull requests too. To tell them apart: a PR has a
  non-null `pull_request` field. Use the `type=issues` query param to exclude
  PRs (supported in newer Gitea versions).
- Pagination: Gitea defaults to 30 items per page. Use `&limit=50` or `&page=N`
  for more.
- **Milestones take a numeric ID under the `milestone` key**. `milestone_id`
  and milestone names are silently ignored, by both `POST` and `PATCH`.
- **Complex bodies**: when the issue body holds special characters (backticks,
  quotes, parentheses, newlines), write the Markdown body to a file and build
  the payload with `jq -n --rawfile body ./tmp/issue-body.md`. Do not use
  inline `-d '...'`. See "Create an issue".
- **New issues always use the standard template**, including the required
  `## Tasks` checklist and no `## Acceptance criteria`. Do not file empty
  bodies, free-form prose, or tasks without checkboxes.
- **Assignees** take valid Gitea usernames, not emails or display names. Use
  "Get the current user" to obtain the token user's login. If the token lacks
  the `read:user` scope, the `/user` endpoint fails. Use the fallback, or create
  the issue without `assignees` and PATCH it afterward.
