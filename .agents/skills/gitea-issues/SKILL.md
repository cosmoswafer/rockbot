---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, close, and manage issues on the project's Gitea server. Determines server and repo from git remote; reads token from .env.
license: CC0-1.0
---

# gitea-issues — Gitea Issue Management

## Configuration

This skill auto-detects the Gitea server and repository from the `origin` git remote (`ssh://git@git.tokyofy.top:33022/Atom/rockbot.git`):

- **Server**: `git.tokyofy.top`
- **API base**: `https://git.tokyofy.top/api/v1`
- **Owner/Repo**: `Atom/rockbot`
- **Token**: `$GITEA_TOKEN` — sourced from `.env` (`source .env`)

Available repos via the same server:
- `Atom/rockbot`
- `Atom/weasel`
- `ReLab/Ideas`

## Common Operations

### List all open issues

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get a specific issue (including pull requests — use `&type=issues` to exclude PRs)

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues/27" \
  | jq '{number, title, state, body, comments, html_url}'
```

### List with additional filters

```bash
# All issues (including closed)
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues?state=all&limit=50" \
  | jq '.[] | {number, title, state, comments, updated_at}'

# Issues with pagination (Gitea default is 30 per page)
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues?state=open&page=1&limit=50" \
  | jq '.[] | {number, title, state, comments, created_at}'
```

### Get comments on an issue

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues/27/comments" \
  | jq '.[] | {id, body, user: .user.login, created_at}'
```

### Create a new issue

Labels must be numeric IDs (see "List available labels"). Use an empty array `[]` or numeric IDs like `[24]`.

For simple bodies without special characters, use inline JSON:

```bash
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"title":"Issue title here","body":"Issue body / description","labels":[],"assignees":["saru"]}' \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues" \
  | jq '{number, title, html_url}'
```

For complex bodies (backticks, quotes, newlines), write the JSON payload to a temp file:

```bash
# 1. Write JSON to a temp file
cat > /tmp/issue.json << 'JSONEOF'
{
  "title": "Issue title",
  "body": "Body with `backticks`, \"quotes\", and\nnewlines",
  "labels": [24]
}
JSONEOF

# 2. Create the issue
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d @/tmp/issue.json \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues" \
  | jq '{number, title, html_url}'
```

### Add a comment to an issue

```bash
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"body":"Comment text here"}' \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues/27/comments" \
  | jq '{id, body, html_url}'
```

### Close an issue

```bash
source .env && curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"closed"}' \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues/27" \
  | jq '{number, title, state}'
```

### Re-open an issue

```bash
source .env && curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"state":"open"}' \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/issues/27" \
  | jq '{number, title, state}'
```

### List available labels

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/labels" \
  | jq '.[] | {id, name, color}'
```

### List milestones

```bash
source .env && curl -s \
  -H "Authorization: token $GITEA_TOKEN" \
  "https://git.tokyofy.top/api/v1/repos/Atom/rockbot/milestones" \
  | jq '.[] | {id, title, state, open_issues, closed_issues}'
```

## Usage Notes

- Always `source .env` before any curl call so `$GITEA_TOKEN` is available.
- If `jq` is not installed, omit the `| jq ...` pipe; the raw JSON is still readable.
- The issues endpoint returns pull requests too. To distinguish: PRs have a `pull_request` field (non-null). Use `type=issues` query param to exclude PRs (supported in newer Gitea versions).
- Pagination: Gitea defaults to 30 items per page. Use `&limit=50` or `&page=N` for more.
- **Labels require numeric IDs**, not names. List available labels with their IDs first (see "List available labels" above), then use the numeric `id` value in the `labels` array (e.g. `"labels":[24]` for the "bug" label). Using label name strings will return a JSON unmarshal error.
- **Complex bodies**: when the issue body contains special characters (backticks, quotes, parentheses, newlines), write the JSON payload to a temp file first and pass it with `-d @file.json` instead of inline `-d '...'` to avoid shell escaping issues.
