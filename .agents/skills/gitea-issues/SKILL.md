---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, close, and manage issues on the project's Gitea server. Determines server and repo from git remote; reads token from .env.
license: CC0-1.0
---

# gitea-issues — Gitea Issue Management

## Setup — derive server and repo from git remote

**Every** operation must start with this block to dynamically extract the Gitea
server, API base, and owner/repo from the `origin` git remote. Never hardcode
these values — they can change between repositories and environments.

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"  # strip trailing .git if present

if [[ "$REMOTE" == *@* ]]; then
  # SSH format: git@host:owner/repo
  HOST="${REMOTE%%:*}"
  HOST="${HOST##*@}"
  REPO="${REMOTE#*:}"
else
  # HTTPS format: https://host/owner/repo
  HOST="${REMOTE#*://}"
  HOST="${HOST%%/*}"
  REPO="${REMOTE#*://*/}"
fi

GITEA_API="https://${HOST}/api/v1"
```

After this block, the following variables are available:

- `$GITEA_API` — e.g. `https://gitea.com/api/v1`
- `$REPO` — owner/repo, e.g. `ehr/WeightManagement-frontend`
- `$GITEA_TOKEN` — sourced from `.env`

## Common Operations

### List all open issues

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get a specific issue (including pull requests — use `&type=issues` to exclude PRs)

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with additional filters

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
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
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create a new issue

Labels must be numeric IDs (see "List available labels"). Use an empty array
`[]` or numeric IDs like `[24]`. For `assignees`, obtain the current user's
username first (see "Get current user" below).

For simple bodies without special characters, use inline JSON:

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"title\":\"Issue title here\",\"body\":\"Issue body / description\",\"labels\":[],\"assignees\":[\"$GITEA_USER\"]}" \
  "$GITEA_API/repos/$REPO/issues" \
  | jq '{number, title, html_url}'
```

For complex bodies (backticks, quotes, newlines), write the JSON payload to a
temp file:

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

# 1. Write JSON to a temp file (use $GITEA_USER for assignees)
cat > ./tmp/issue.json << JSONEOF
{
  "title": "Issue title",
  "body": "Body with \`backticks\`, \"quotes\", and\nnewlines",
  "labels": [24],
  "assignees": ["$GITEA_USER"]
}
JSONEOF

# 2. Create the issue
curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @./tmp/issue.json \
  "$GITEA_API/repos/$REPO/issues" \
  | jq '{number, title, html_url}'
```

### Add a comment to an issue

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"body":"Comment text here"}' \
  "$GITEA_API/repos/$REPO/issues/27/comments" \
  | jq '{id, body, html_url}'
```

### Close an issue

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
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
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
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
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/labels" \
  | jq '.[] | {id, name, color}'
```

### List milestones

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
GITEA_API="https://${HOST}/api/v1"

curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$REPO/milestones" \
  | jq '.[] | {id, title, state, open_issues, closed_issues}'
```

### Get current user (for assignees)

The token's username is needed to assign issues. The preferred method is the
`/user` endpoint, but if the token lacks `read:user` scope, fall back to
extracting it from a recent issue's creator.

```bash
source .env

REMOTE=$(git remote get-url origin)
REMOTE="${REMOTE%.git}"
if [[ "$REMOTE" == *@* ]]; then
  HOST="${REMOTE%%:*}"; HOST="${HOST##*@}"; REPO="${REMOTE#*:}"
else
  HOST="${REMOTE#*://}"; HOST="${HOST%%/*}"; REPO="${REMOTE#*://*/}"
fi
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

## Usage Notes

- Always `source .env` before any curl call so `$GITEA_TOKEN` is available.
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
  (backticks, quotes, parentheses, newlines), write the JSON payload to a temp
  file first and pass it with `-d @file.json` instead of inline `-d '...'` to
  avoid shell escaping issues.
- **Assignees**: `assignees` requires valid Gitea usernames (not emails or
  display names). Use the "Get current user" section above to obtain the token
  user's login dynamically. If the token lacks `read:user` scope, the `/user`
  endpoint will fail — use the fallback (extract `user.login` from a recent
  issue's creator) or create the issue without `assignees` and PATCH it
  afterward.
