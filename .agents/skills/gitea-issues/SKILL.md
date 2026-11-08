---
name: gitea-issues
description: Use when working with Gitea issues — list, create, comment, close, and manage issues on the project's Gitea server. Determines server and repo from git remote; reads token from .env.
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
- **Token**: `$GITEA_TOKEN` — sourced from `.env` (`source .env`)

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

Labels must be numeric IDs (see "List available labels"). Use an empty array `[]` or numeric IDs like `[24]`.

For simple bodies without special characters, use inline JSON:

```bash
source .env && curl -s -X POST \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"title":"Issue title here","body":"Issue body / description","labels":[],"assignees":["saru"]}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues" \
  | jq '{number, title, html_url}'
```

For complex bodies (backticks, quotes, newlines), write the JSON payload to a temp file:

```bash
# 1. Write JSON to a temp file
cat > ./tmp/issue.json << 'JSONEOF'
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

### Close an issue

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

## Usage Notes

- **Always** run the Configuration setup first to derive `GITEA_API` and `GITEA_OWNER_REPO` from the git remote, then `source .env` so `$GITEA_TOKEN` is available. Never hardcode server or repo names.
- If `jq` is not installed, omit the `| jq ...` pipe; the raw JSON is still readable.
- The issues endpoint returns pull requests too. To distinguish: PRs have a `pull_request` field (non-null). Use `type=issues` query param to exclude PRs (supported in newer Gitea versions).
- Pagination: Gitea defaults to 30 items per page. Use `&limit=50` or `&page=N` for more.
- **Labels require numeric IDs**, not names. List available labels with their IDs first (see "List available labels" above), then use the numeric `id` value in the `labels` array (e.g. `"labels":[24]` for the "bug" label). Using label name strings will return a JSON unmarshal error. Issue `PATCH` silently ignores `labels`; use `PUT /issues/{n}/labels` with the full set (see "Issue status labels").
- **Complex bodies**: when the issue body contains special characters (backticks, quotes, parentheses, newlines), write the JSON payload to a temp file first and pass it with `-d @file.json` instead of inline `-d '...'` to avoid shell escaping issues.
