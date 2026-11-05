---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, and manage issues on the project's Gitea server. Never close issues automatically; comment on them instead and let a human close. Determines server and repo from git remote; expects GITEA_TOKEN to already exist in the environment (sourced silently from .env, never echoed).
license: CC0-1.0
---

# gitea-issues — Gitea Issue Management

## Setup — derive server and repo from git remote

**Every** operation must start with this block to dynamically extract the Gitea
server, API base, and owner/repo from the `origin` git remote. Never hardcode
these values — they can change between repositories and environments.

```bash
source .env >/dev/null 2>&1

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"  # strip trailing .git if present

if [[ "$REMOTE" == *://* ]]; then
  # URL-style (ssh://, https://, http://): [user@]host[:port]/owner/repo
  BARE="${REMOTE#*://}"
  BARE="${BARE#*@}"        # strip user@ (e.g. git@)
  REPO="${BARE#*/}"        # owner/repo after first slash
  HOSTPART="${BARE%%/*}"   # host[:port]
else
  # scp-style (git@host:owner/repo): [user@]host:owner/repo
  BARE="${REMOTE#*@}"      # strip user@
  HOSTPART="${BARE%%:*}"   # host (before first colon)
  REPO="${BARE#*:}"        # owner/repo after colon
fi

HOST="${HOSTPART%%:*}"     # drop any :port suffix — API is on default HTTPS port
GITEA_API="https://${HOST}/api/v1"
```

After this block, the following variables are available:

- `$GITEA_API` — e.g. `https://git.ehr.gov.mo/api/v1`
- `$REPO` — owner/repo, e.g. `ehr/WeightManagement-frontend`
- `$GITEA_TOKEN` — already expected in the environment; sourced silently
  (`source .env >/dev/null 2>&1`), never printed or exposed

## Common Operations

### List all open issues

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

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get a specific issue (including pull requests — use `&type=issues` to exclude PRs)

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

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with additional filters

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

# All issues (including closed)
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues?state=all&limit=50" \
  | jq '.[] | {number, title, state, comments, updated_at}'

# Issues with pagination (Gitea default is 30 per page)
curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues?state=open&page=1&limit=50" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get comments on an issue

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

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create a new issue

**Every new issue must be created from the standard template below.** The
mandatory `## Tasks` section is a Markdown task list (`- [ ]` items): Gitea
renders it as a progress counter (e.g. `1/3`) in the issue list, and tasks are
ticked off as work lands.

#### Issue body template

Copy this skeleton into the issue body and replace every placeholder:

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
- Mark tasks `- [x]` as they complete, by PATCHing the issue body (there is no
  checkbox API). The body is the single source of progress until a human closes
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

Labels must be numeric IDs (see "List available labels"). Use an empty array
`[]` or numeric IDs like `[24]`. For `assignees`, obtain the current user's
username first (see "Get current user" below).

**Milestone field:** Use the JSON key `milestone` with a numeric milestone ID,
e.g. `"milestone": 61`. The API does **not** accept `milestone_id` — using
`milestone_id` will silently ignore the milestone.

For simple bodies without special characters, use inline JSON:

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

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"title\":\"Issue title here\",\"body\":\"Issue body / description\",\"labels\":[],\"assignees\":[\"$GITEA_USER\"]}" \
  "$GITEA_API/repos/$REPO/issues" \
  | jq '{number, title, html_url}'
```

For templated bodies (backticks, quotes, newlines), write the Markdown body to a
file and let `jq` assemble the JSON payload — no shell escaping needed:

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
  "$GITEA_API/repos/$REPO/issues" \
  | jq '{number, title, html_url}'
```

### Add a comment to an issue

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

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"body":"Comment text here"}' \
  "$GITEA_API/repos/$REPO/issues/27/comments" \
  | jq '{id, body, html_url}'
```

### Close an issue (manual only)

Never close an issue automatically — only close it when a human explicitly asks
you to. A human normally closes the issue after reviewing the work.

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

curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"closed"}' \
  "$GITEA_API/repos/$REPO/issues/27" \
  | jq '{number, title, state}'
```

### Re-open an issue

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

curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"open"}' \
  "$GITEA_API/repos/$REPO/issues/27" \
  | jq '{number, title, state}'
```

### List available labels

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

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/labels" \
  | jq '.[] | {id, name, color}'
```

### List milestones

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

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/milestones" \
  | jq '.[] | {id, title, state, open_issues, closed_issues}'
```

### Find latest milestone

Find the most recently created milestone, e.g. `v3.5.6`.

From latest issues:

```bash
source .env >/dev/null 2>&1
REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then BARE="${REMOTE#*://}"; BARE="${BARE#*@}"; REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"; else BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"; fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"

# Get milestone title from the most recent open issue that has a milestone
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues?state=open&sort=created&direction=desc&limit=10" \
  | jq -r '.[] | select(.milestone != null) | .milestone.title' | head -n1
```

From full open milestones sorted by creation date:

```bash
source .env >/dev/null 2>&1
REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then BARE="${REMOTE#*://}"; BARE="${BARE#*@}"; REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"; else BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"; fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"

curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/milestones?state=open" \
  | jq -r '.[] | "\(.created_at) \(.id) \(.title)"' | sort -r | head -n1
# Example output: 2026-08-11T12:11:16+08:00 61 v3.5.6
```

### Create an issue with milestone

Include `milestone` by numeric ID in the payload:

```bash
source .env >/dev/null 2>&1
REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then BARE="${REMOTE#*://}"; BARE="${BARE#*@}"; REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"; else BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"; fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"

cat > ./tmp/issue.json << JSONEOF
{
  "title": "Issue title",
  "body": "Issue body",
  "labels": [29],
  "assignees": ["$GITEA_USER"],
  "milestone": 61
}
JSONEOF

curl -s -X POST -H "Authorization: token $GITEA_TOKEN" -H "Content-Type: application/json" -d @./tmp/issue.json \
  "$GITEA_API/repos/$REPO/issues" | jq '{number, title, html_url, milestone: {id:.milestone.id, title:.milestone.title}}'
```

### Add / update an issue's milestone

Patch an existing issue to assign it to a milestone. Gitea accepts the numeric
ID via the `milestone` field:

```bash
source .env >/dev/null 2>&1
REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then BARE="${REMOTE#*://}"; BARE="${BARE#*@}"; REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"; else BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"; fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"

ISSUE=418
MILESTONE_ID=61

curl -s -X PATCH -H "Authorization: token $GITEA_TOKEN" -H "Content-Type: application/json" \
  -d "{\"milestone\":$MILESTONE_ID}" \
  "$GITEA_API/repos/$REPO/issues/$ISSUE" | jq '{number, title, milestone: {id:.milestone.id, title:.milestone.title}}'
```

### Get current user (for assignees)

The token's username is needed to assign issues. The preferred method is the
`/user` endpoint, but if the token lacks `read:user` scope, fall back to
extracting it from a recent issue's creator.

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

# Primary: /user endpoint (requires read:user token scope)
GITEA_USER=$(curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/user" | jq -r '.login // empty')

# Fallback: extract creator login from the most recent issue
# (the token user is the creator of issues it creates)
if [ -z "$GITEA_USER" ]; then
  GITEA_USER=$(curl -s \
    -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$REPO/issues?state=all&limit=1" \
    | jq -r '.[0].user.login // empty')
fi

echo "Current user: $GITEA_USER"
```

When creating an issue, use `$GITEA_USER` in the `assignees` array. If the
username is still unknown, create the issue without `assignees`, extract
`user.login` from the response, then PATCH the issue to add the assignee.

## Troubleshooting — common failures

These are the failure modes most likely to silently break an operation. Check
the symptom before escalating:

1. **`{"message":"invalid username, password or token"}`** — the auth header is
   wrong. Gitea expects `Authorization: token $GITEA_TOKEN` (the literal word
   `token`, **not** `Bearer`). `Bearer` returns a 401 / invalid-token error even
   with a valid token. Never substitute `Bearer` here.

2. **`{"message":"not found"}` / `404 page not found`** — usually a wrong host.
   `$GITEA_API` must be derived from the `origin` remote
   (`git remote get-url
   origin`). Do **not** assume `gitea.com` — that is a
   public instance and is almost never this project's server. If a clone used
   the wrong remote URL, fix it (`git remote set-url origin <correct-url>`)
   before running any command. Also confirm `$REPO` is `owner/repo` (lowercase,
   exact), not the bare repo name.

3. **`user redirect does not exist`** — the `$REPO` path casing/owner is wrong
   (e.g. `WeightManagement-frontend` vs `weightmanagement-frontend`). Gitea is
   case-sensitive on the repo slug. Verify the exact owner/repo from the web UI.

4. **Empty / unexpected response** — confirm `.env` was sourced silently
   (`source .env >/dev/null 2>&1`) and `$GITEA_TOKEN` is non-empty. Print the
   derived `$GITEA_API` and `$REPO` at the start of any script to catch
   derivation mistakes early. Never print or log `$GITEA_TOKEN` itself.

Quick sanity check before any write operation:

```bash
source .env >/dev/null 2>&1
REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *://* ]]; then BARE="${REMOTE#*://}"; BARE="${BARE#*@}"; REPO="${BARE#*/}"; HOSTPART="${BARE%%/*}"; else BARE="${REMOTE#*@}"; HOSTPART="${BARE%%:*}"; REPO="${BARE#*:}"; fi
HOST="${HOSTPART%%:*}"
GITEA_API="https://${HOST}/api/v1"
echo "API=$GITEA_API  REPO=$REPO"
curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" | jq '.login // .message'
```

If the last line prints your login, the server + token + auth format are all
correct and subsequent calls will work.

## Usage Notes

- Always `source .env >/dev/null 2>&1` before any curl call so `$GITEA_TOKEN` is
  available. The token is expected to already exist in `.env` — source it
  silently and never read, echo, or otherwise expose its value. No output from
  `.env` should ever reach the terminal or logs.
- Always derive `$GITEA_API` and `$REPO` from `git remote get-url origin` —
  never hardcode the server hostname or owner/repo path.
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
  name strings will return a JSON unmarshal error.
- **Complex bodies**: when the issue body contains special characters
  (backticks, quotes, parentheses, newlines), write the Markdown body to a file
  and build the payload with `jq -n --rawfile body ./tmp/issue-body.md ...`
  instead of inline `-d '...'` — see "Create a new issue".
- **New issues use the standard template**: always fill in the issue body
  template (see "Issue body template"), including the required `## Tasks`
  checklist. Empty bodies, free-form prose, or tasks without checkboxes are not
  acceptable for new issues.
- **Assignees**: `assignees` requires valid Gitea usernames (not emails or
  display names). Use the "Get current user" section above to obtain the token
  user's login dynamically. If the token lacks `read:user` scope, the `/user`
  endpoint will fail — use the fallback (extract `user.login` from a recent
  issue's creator) or create the issue without `assignees` and PATCH it
  afterward.

## Referencing issues in commit messages

When a commit relates to a Gitea issue, reference the issue in the
`#<number>` form — e.g. `#615` for issue 615. Gitea scans pushed commit
messages for `#<number>` references and automatically links the commit to the
issue (it appears in the issue's timeline as "Referenced from …", and the
commit shows the issue link). Always include such a reference when work is
tied to an issue.

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

1. Leave a comment on the issue (see "Add a comment to an issue" above)
   describing the work done — what was changed, where, and any relevant commits,
   tests, or verification.
2. Let a human review and close the issue.

Ensure the project quality gate passes before committing (`cargo test` here, per AGENTS.md).

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
  "$GITEA_API/repos/$REPO/issues/687" | jq -r '.body' > ./tmp/body.md
sed -i 's/^- \[ \] Verify: edit the duration/- [x] Verify: edit the duration/' ./tmp/body.md
sed -i '/^## Acceptance criteria/,/^## Related/{/^## Related/!d}' ./tmp/body.md

# 2. PATCH the body back
jq -n --rawfile body ./tmp/body.md '{body: $body}' > ./tmp/patch.json
curl -s -X PATCH -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" -d @./tmp/patch.json \
  "$GITEA_API/repos/$REPO/issues/687" | jq '{number, state}'
```
