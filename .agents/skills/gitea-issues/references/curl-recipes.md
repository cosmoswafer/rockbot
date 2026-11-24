# bash + curl recipes (the no-Python path)

Use these when Python is unavailable, or when a one-off request does not deserve
a command. The CLI in `scripts/gitea.py` is the preferred path: it derives
everything and never needs the setup block repeated by hand.

## Setup — source the helper instead of pasting the block

Every shell call starts fresh, so the whole setup has to happen in the _same_
call as the request. One line does it:

```bash
source .agents/skills/gitea-issues/scripts/gitea-env.sh
```

It sources `.env` silently, derives `GITEA_API` and `GITEA_OWNER_REPO` from the
git remote, prints one sanity line, and defines:

| Helper                                   | Meaning                                    |
| ---------------------------------------- | ------------------------------------------ |
| `gitea_get "/issues?state=open"`         | GET on a repo path                         |
| `gitea_post "/issues" "$JSON"`           | POST with a JSON body                      |
| `gitea_patch "/issues/687" "$JSON"`      | PATCH                                      |
| `gitea_put "/issues/687/labels" "$JSON"` | PUT                                        |
| `gitea_del "/labels/7"`                  | DELETE                                     |
| `gitea_code GET "/issues"`               | HTTP status code only                      |
| `gitea_selfcheck`                        | print `API=… REPO=…` and the access status |
| `gitea_login`                            | token user, with the newest-issue fallback |

Overrides: `GITEA_REMOTE_NAME`, `GITEA_OWNER_REPO`, `GITEA_API`,
`GITEA_ENV_FILE`, `GITEA_QUIET=1`, `GITEA_STRICT=1` (return non-zero when the
token or remote is unusable). The helpers never echo the token, and they disable
`xtrace` around the `curl` call so `set -x` cannot leak it.

Equivalent manual derivation, if you cannot source the file:

```bash
source .env >/dev/null 2>&1

GITEA_REMOTE=$(git remote get-url origin)
GITEA_REMOTE="${GITEA_REMOTE%.git}"

if [[ "$GITEA_REMOTE" == *://* ]]; then
  GITEA_BARE="${GITEA_REMOTE#*://}"; GITEA_BARE="${GITEA_BARE#*@}"
  GITEA_HOST="${GITEA_BARE%%/*}"; GITEA_OWNER_REPO="${GITEA_BARE#*/}"
else
  GITEA_BARE="${GITEA_REMOTE#*@}"
  GITEA_HOST="${GITEA_BARE%%:*}"; GITEA_OWNER_REPO="${GITEA_BARE#*:}"
fi

GITEA_HOST="${GITEA_HOST%%:*}"   # the SSH port belongs to git, not the API
GITEA_API="https://${GITEA_HOST}/api/v1"

echo "$GITEA_API/repos/$GITEA_OWNER_REPO"   # sanity line; never echoes the token
```

## Diagnostics before any write

```bash
echo "== gitea-issues diagnostics =="

for tool in git curl jq; do
  command -v "$tool" >/dev/null 2>&1 \
    && echo "PASS  $tool: $(command -v "$tool")" \
    || echo "FAIL  $tool is missing"
done

[ -f .env ] && echo "PASS  .env found at $PWD/.env" \
  || echo "FAIL  no .env at $PWD — create it with GITEA_TOKEN=..."

if [ -n "${GITEA_TOKEN:-}" ]; then
  echo "PASS  GITEA_TOKEN set (${#GITEA_TOKEN} characters; value hidden)"
else
  echo "FAIL  GITEA_TOKEN is empty — add GITEA_TOKEN=<token> to .env"
fi

echo "INFO  API=$GITEA_API  REPO=$GITEA_OWNER_REPO"

HTTP=$(gitea_code GET "")
case "$HTTP" in
  200) echo "PASS  repo access: HTTP 200" ;;
  401) echo "FAIL  HTTP 401 — use the literal word 'token', not 'Bearer'" ;;
  403) echo "FAIL  HTTP 403 — token lacks repository access or the scope" ;;
  404) echo "FAIL  HTTP 404 — wrong host or owner/repo; check origin" ;;
  000) echo "FAIL  no response — DNS, TLS or network reaching the host" ;;
  *)   echo "FAIL  repo access: HTTP $HTTP" ;;
esac

gitea_get "" | jq -r 'if .full_name then "PASS  repository: \(.full_name)"
  else "FAIL  repository: \(.message // "unexpected response")" end'

gitea_get "/labels?limit=100" | jq -r \
  'if type == "array" then "PASS  labels readable: \(length)"
   else "FAIL  labels: \(.message // "unexpected response")" end'
```

`PASS` on every line means the server, token, auth format and `owner/repo` are
right. If anything says `FAIL`, fix it before writing.

## Reading

```bash
# open issues
gitea_get "/issues?state=open" \
  | jq '.[] | {number, title, state, comments, created_at}'

# everything, including closed
gitea_get "/issues?state=all&limit=50" | jq '.[] | {number, title, state, updated_at}'

# a page of 50
GITEA_QUERY="state=open&page=2&limit=50" gitea_get "/issues" | jq '.[].number'

# one issue (this endpoint returns pull requests too — a PR has a
# non-null .pull_request; add type=issues to leave them out)
gitea_get "/issues/27" | jq '{number, title, state, body, comments, html_url}'

# comments
gitea_get "/issues/27/comments" | jq '.[] | {id, body, user: .user.login, created_at}'

# labels on an issue, and repo labels
gitea_get "/issues/27/labels" | jq '.[] | {id, name}'
gitea_get "/labels?limit=100" | jq '.[] | [.id, .name, .color] | @tsv'

# milestones
gitea_get "/milestones" | jq '.[] | {id, title, state, open_issues, closed_issues}'

# the newest open milestone: "<created_at> <id> <title>"
gitea_get "/milestones?state=open" \
  | jq -r '.[] | "\(.created_at) \(.id) \(.title)"' | sort -r | head -n1

# the milestone of the most recent open issue that has one
gitea_get "/issues?state=open&sort=created&direction=desc&limit=10" \
  | jq -r '.[] | select(.milestone != null) | .milestone.title' | head -n1
```

## The token user (for assignees)

`assignees` takes login names. `/user` is the direct answer, but it needs the
`read:user` scope; an issue-only token gets a 403 there. The helper tries both,
falling back to the creator of a recent issue — the token user filed the issues
it created:

```bash
GITEA_USER=$(gitea_login)
echo "Current user: $GITEA_USER"

# what it does, spelled out:
curl -s -H "Authorization: token $GITEA_TOKEN" "$GITEA_API/user" \
  | jq -r '.login // empty'                                   # primary
gitea_get "/issues?state=all&limit=1" | jq -r '.[0].user.login // empty'  # fallback
```

Still unknown? Create the issue without `assignees`, read `user.login` from the
response, then PATCH the issue to add the assignee.

## Creating an issue

Write the templated body to a file and let `jq` build the payload — no shell
escaping is needed for backticks, quotes or newlines:

```bash
mkdir -p ./tmp
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

jq -n \
  --arg title "Quick camera: use server time, not photo EXIF" \
  --rawfile body ./tmp/issue-body.md \
  --arg user "$GITEA_USER" \
  '{title: $title, body: $body, assignees: [$user], milestone: 61}' \
  > ./tmp/issue.json

gitea_post "/issues" "$(cat ./tmp/issue.json)" | jq '{number, title, html_url}'
```

For a short body, inline JSON is enough:

```bash
gitea_post "/issues" "{\"title\":\"Issue title here\",\"body\":\"Issue body\"}" \
  | jq '{number, title, html_url}'
```

Payload rules:

- `milestone` is the numeric ID. `milestone_id` is ignored without an error.
- `labels` is an array of numeric label IDs and is ignored by older servers —
  after creating, write them through the labels endpoint (see below).
- Never invent filler tasks; for an investigation, the investigation steps are
  the tasks. Never add `## Acceptance criteria`.

## Commenting

```bash
gitea_post "/issues/27/comments" '{"body":"Comment text here"}' | jq '{id, html_url}'

# a long comment from a file
jq -n --rawfile body ./tmp/note.md '{body: $body}' > ./tmp/comment.json
gitea_post "/issues/27/comments" "$(cat ./tmp/comment.json)" | jq '{id, html_url}'
```

## Editing an issue

```bash
# title / body / milestone
gitea_patch "/issues/418" '{"title":"New title"}' | jq '{number, title}'
gitea_patch "/issues/418" '{"milestone":61}' | jq '{number, milestone: .milestone.title}'
gitea_patch "/issues/418" '{"milestone":0}' | jq '.milestone'   # 0 clears it

# the body is the only progress record: fetch, edit, write back
gitea_get "/issues/687" | jq -r '.body' > ./tmp/body.md
sed -i 's/^- \[ \] Verify: edit the duration/- [x] Verify: edit the duration/' ./tmp/body.md
sed -i '/^## Acceptance criteria/,/^## Related/{/^## Related/!d}' ./tmp/body.md
jq -n --rawfile body ./tmp/body.md '{body: $body}' > ./tmp/patch.json
gitea_patch "/issues/687" "$(cat ./tmp/patch.json)" | jq '{number, state}'
```

## Labels

An issue `PATCH` silently ignores `labels`. Use the labels endpoint with the
**full** set of numeric IDs — `PUT` replaces:

```bash
# 1. current labels on the issue
gitea_get "/issues/42/labels" | jq -r '.[] | [.id, .name] | @tsv'

# 2. write the set to keep (unrelated ids + the status ids you want)
gitea_put "/issues/42/labels" '{"labels": [3, 7, 24]}' | jq -r '.[].name'
```

Create a label idempotently (colours are 6 hex digits, no `#`):

```bash
ensure_label() { # name color description
  local id
  id=$(gitea_get "/labels?limit=100" | jq -r --arg n "$1" \
        '.[] | select(.name == $n) | .id' | head -n1)
  if [ -n "$id" ]; then echo "OK    $1 (id $id)"; return 0; fi
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    -H "Authorization: token $GITEA_TOKEN" -H "Content-Type: application/json" \
    -d "$(jq -n --arg n "$1" --arg c "$2" --arg d "$3" \
          '{name: $n, color: $c, description: $d}')" \
    "$(gitea_url /labels)")
  case "$code" in
    201) echo "NEW   $1" ;;
    5*)  echo "WARN  $1 rejected with HTTP $code — retry with the bare name" ;;
    *)   echo "FAIL  $1: HTTP $code" ;;
  esac
}

ensure_label "👀 seeing"    1d76db "picked the issue up; working on it"
ensure_label "💬 responded" fbca04 "a reply, answer or plan is posted"
ensure_label "🔨 working"   5319e7 "implementing code/tests"
ensure_label "❓ question"  d93f0b "blocked, waiting on human input"
ensure_label "✅ committed" 0e8a16 "a commit has been pushed"
```

`gitea.py label bootstrap` does the same and keeps unrelated labels when it
writes a status set.

## State changes

Closing is a human decision — use these only on an explicit request.

```bash
gitea_patch "/issues/27" '{"state":"closed"}' | jq '{number, title, state}'
gitea_patch "/issues/27" '{"state":"open"}'   | jq '{number, title, state}'
```

## Milestones

```bash
gitea_post "/milestones" '{"title":"v3.7.11","description":"spring cleanup","due_date":"2026-12-31"}' \
  | jq '{id, title, state}'

gitea_patch "/milestones/87" '{"state":"closed"}' | jq '{id, title, state}'
```

## Raw endpoints

Anything the CLI does not wrap:

```bash
curl -sS -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/687/timeline" | jq '.[].type'
```

Or through the CLI, which derives the base for you:

```bash
python3 .agents/skills/gitea-issues/scripts/gitea.py \
  api GET repos/$GITEA_OWNER_REPO/issues/687/timeline
```
