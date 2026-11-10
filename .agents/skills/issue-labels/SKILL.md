---
name: issue-labels
description: The shared Gitea issue status label set (👀 seeing, 💬 responded, 🔨 working, ❓ question, ✅ committed) with curl recipes to create, apply, reconcile and migrate labels by hand. Use when labelling Gitea issues from OpenCode or a terminal, marking an issue as picked up before working on it, tracking progress without gitea-bots, or repairing drifted labels. Works alongside the gitea-issues skill and needs no Python.
license: CC0-1.0
compatibility: Requires git, curl and jq. Reads GITEA_TOKEN from .env and the repo from the origin remote. No Python, no gitea-bots dependency.
---

# issue-labels — shared Gitea issue status labels

One small status vocabulary for Gitea issues. The same five labels are used
when a human or OpenCode works an issue by hand with the `gitea-issues` skill,
and when the `gitea-bots` watcher handles it automatically. This skill is
**self-contained**: every operation below uses only `git`, `curl` and `jq`.
The `gitea-bots` skill is not needed and nothing here depends on it.

## The five labels

Each label is one word with a leading emoji in its **display name**. The bare
token is the canonical status name; the emoji is a display prefix that the
recipes strip when matching repo labels.

| Canonical   | Display        | Colour          | Meaning                                                         |
| ----------- | -------------- | --------------- | --------------------------------------------------------------- |
| `seeing`    | `👀 seeing`    | `1d76db` blue   | a background agent has picked the issue up and is working on it |
| `responded` | `💬 responded` | `fbca04` yellow | a reply, answer or plan is posted; waiting on the human         |
| `working`   | `🔨 working`   | `5319e7` purple | implementing code/tests                                         |
| `question`  | `❓ question`  | `d93f0b` orange | blocked, waiting on human input                                 |
| `committed` | `✅ committed` | `0e8a16` green  | a commit has been pushed                                        |

Gitea stores colours as 6-digit hex **without** the leading `#`.

### Lifecycle

```
seeing → responded → working / question → responded + committed
```

- `committed` is **additive**: an issue keeps `responded` as well.
- `question` is **transient**: it can appear at any point and returns to the
  previous label once answered.
- `working` and `question` are **sticky** while implementation is in flight, so
  a restart never downgrades in-progress work.
- `seeing` is a **pickup marker**, not a filing marker. Filing an issue writes
  no label. Only the agent that actually takes the issue writes `seeing`, and
  it stays while the issue has no plan yet.

## Setup — derive the server, API base and owner/repo from the git remote

Run this block once per shell, from the repository root. Never hardcode the
host or the `owner/repo` path.

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

GITEA_HOST="${GITEA_HOST%%:*}"
GITEA_API="https://${GITEA_HOST}/api/v1"

echo "API=$GITEA_API  REPO=$GITEA_OWNER_REPO"
```

`$GITEA_TOKEN` comes from `.env`. Never print, log or echo it. The SSH port in
a remote belongs to `git`; the API uses HTTPS on port 443.

## Diagnostics — run this before changing labels

This read-only check prints what is wrong and what to do next. It never prints
the token.

```bash
echo "== issue-labels diagnostics =="

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
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
  | jq -r 'if type == "array" then "PASS  labels readable: \(length) label(s)" else "FAIL  labels: \(.message // "unexpected response")" end'
```

If every line says `PASS`, the token, API base and repository are correct.
The label recipes below will work.

## Create the missing canonical labels

Run this once per repository. It is idempotent: existing labels are left alone.

```bash
ensure_label() {
  local name="$1" color="$2" description="$3" id code
  id=$(curl -s -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
    | jq -r --arg n "$name" '.[] | select(.name == $n) | .id' | head -n1)
  if [ -n "$id" ]; then
    echo "OK    $name (id $id)"
    return 0
  fi
  code=$(curl -s -o /dev/null -w '%{http_code}' -X POST \
    -H "Authorization: token $GITEA_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$(jq -n --arg n "$name" --arg c "$color" --arg d "$description" \
      '{name: $n, color: $c, description: $d}')" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/labels")
  if [ "$code" = "201" ]; then
    echo "NEW   $name"
  elif [ "$code" -ge 500 ]; then
    echo "WARN  $name rejected with HTTP $code (emoji-hostile server)"
    echo "      Retry with the bare name: $4"
  else
    echo "FAIL  $name: HTTP $code"
  fi
}

ensure_label "👀 seeing"    "1d76db" "a background agent has picked the issue up and is working on it" "seeing"
ensure_label "💬 responded" "fbca04" "a reply, answer or plan is posted; waiting on the human"          "responded"
ensure_label "🔨 working"   "5319e7" "implementing code/tests"                                          "working"
ensure_label "❓ question"  "d93f0b" "blocked, waiting on human input"                                  "question"
ensure_label "✅ committed" "0e8a16" "a commit has been pushed"                                         "committed"
```

List the result:

```bash
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
  | jq -r '.[] | [.id, .name, .color] | @tsv'
```

## Applying a status — the full-set rule

Gitea's issue `PATCH` **silently ignores `labels`**. Do not send labels there.
Change labels through the dedicated endpoint. `PUT /issues/{n}/labels` replaces
the whole set, so send the **full** set of numeric IDs to keep: unrelated labels
plus the status labels you want.

### Step by step

```bash
ISSUE=42

# 1. Resolve the display names to numeric IDs
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100" \
  | jq -r '.[] | [.id, .name] | @tsv'

# 2. Read the issue's current labels
curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE/labels" \
  | jq -r '.[] | [.id, .name] | @tsv'

# 3. Write the full set (example: keep unrelated labels 3 and 7,
#    add "👀 seeing" with id 24)
curl -s -X PUT \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"labels": [3, 7, 24]}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE/labels" \
  | jq -r '.[].name'
```

Label names are strings only in the response. The `labels` array in the request
takes **numeric IDs**; name strings return a JSON unmarshal error.

### Helper — set one or more statuses safely

This function keeps unrelated labels, strips any old status labels, and writes
the full set. Define it once per shell after the Setup block.

```bash
status_label_name() {
  case "$1" in
    seeing)    echo "👀 seeing" ;;
    responded) echo "💬 responded" ;;
    working)   echo "🔨 working" ;;
    question)  echo "❓ question" ;;
    committed) echo "✅ committed" ;;
    *) return 1 ;;
  esac
}

set_status() {
  local issue="$1"; shift
  local -a wanted=()
  local status name repo_labels issue_labels keep_json want_names want_ids payload

  for status in "$@"; do
    case "$status" in
      none) continue ;;
    esac
    name=$(status_label_name "$status") || {
      echo "unknown status: $status (use seeing|responded|working|question|committed|none)" >&2
      return 2
    }
    wanted+=("$name")
  done

  repo_labels=$(curl -s -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/labels?limit=100")
  issue_labels=$(curl -s -H "Authorization: token $GITEA_TOKEN" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$issue/labels")

  # Keep everything that is not a status label (including legacy names).
  keep_json=$(jq -c '[.[] | select(.name | test("(seeing|responded|working|question|committed|inspect|plan|implement|questions|done)$") | not) | .id]' <<<"$issue_labels")
  if [ ${#wanted[@]} -eq 0 ]; then
    want_names='[]'
  else
    want_names=$(printf '%s\n' "${wanted[@]}" | jq -R . | jq -s -c .)
  fi
  want_ids=$(jq -c --argjson names "$want_names" \
    '[.[] | select(.name as $n | $names | index($n)) | .id]' <<<"$repo_labels")

  if [ "$(jq 'length' <<<"$want_ids")" -ne "${#wanted[@]}" ]; then
    echo "one or more status labels do not exist in this repo;" >&2
    echo "run the 'Create the missing canonical labels' block first." >&2
    return 3
  fi

  payload=$(jq -n --argjson keep "$keep_json" --argjson want "$want_ids" \
    '{labels: ($keep + $want)}')

  curl -s -X PUT \
    -H "Authorization: token $GITEA_TOKEN" \
    -H "Content-Type: application/json" \
    -d "$payload" \
    "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$issue/labels" \
    | jq -r '.[].name'
}
```

Clear every status label with `set_status 42 none` (the empty `wanted` array
keeps only unrelated labels).

### Worked example — pick up issue 42 and mark it `seeing`

Run "Create the missing canonical labels" once in this repo first. Then, when
OpenCode is asked to work on issue 42, mark the pickup before investigating:

```bash
set_status 42 seeing
```

Expected output:

```text
👀 seeing
```

Then, as the work moves:

```bash
set_status 42 responded              # after posting an answer or a plan
set_status 42 working                # implementation has started
set_status 42 responded committed    # commit pushed; issue stays open
set_status 42 question               # blocked, waiting on a human
```

Filing a **new** issue writes no label. `seeing` is only for an issue that an
agent or person has actually picked up. Never close an issue from this skill;
leave it for a human to review and close.

## Manual workflow with the gitea-issues skill

Both skills are independent. Use `gitea-issues` for the issue, comment and
milestone API, and this skill for the status label:

| Moment | Label |
| --- | --- |
| an issue is filed | none |
| an agent picks the issue up | `seeing` |
| a reply, answer or plan is posted | `responded` |
| implementation has started | `working` |
| a commit is pushed | `responded` + `committed` |
| an open question blocks the work | `question` |
| work is ready for review | leave the label; a human closes |

The board then reads the same whether a human, OpenCode or `gitea-bots` did the
work. `gitea-bots` applies these labels automatically when it is installed; no
step here needs it.

## Deriving the label from the issue body

When in doubt, derive the set from the issue instead of guessing. Strip fenced
and inline code first, then inspect the body:

| Issue state | Derived labels |
| --- | --- |
| `## Summary` only, no plan | none (`seeing` is preserved if already present) |
| a plan or `## Tasks` checklist in the body | `responded` |
| a stage-3 completion marker (gitea-bots only) | `responded` + `committed` |
| a stage-2 plan marker (gitea-bots only) | `responded` |
| anything else | none |

Sticky rules: `committed` stays once present; `working` and `question` survive
a reread while there is no completion marker; `seeing` stays while the issue is
unplanned. Reconciliation never invents `seeing` from a `## Summary` body and
never erases a real pickup marker.

To reconcile by hand, read the body and comments, pick the row above, then call
`set_status` with the derived set:

```bash
ISSUE=42

curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE" \
  | jq -r '.body'

curl -s -H "Authorization: token $GITEA_TOKEN" \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/issues/$ISSUE/comments?limit=50" \
  | jq -r '.[].body'

# Example: the body now has a plan or a ## Tasks checklist
set_status "$ISSUE" responded
```

## Legacy names and migration

Older repos may still carry these names. They are accepted as input and are
stripped from an issue when a new status set is applied:

| Old input / label | Resolves to |
| --- | --- |
| `inspect` | `seeing` |
| `plan` | `responded` |
| `implement` | `responded` + `committed` |
| `questions` | `question` |
| `done` | `responded` + `committed` |

Rename a label in place so its ID and existing assignments survive:

```bash
LABEL_ID=18
curl -s -X PATCH \
  -H "Authorization: token $GITEA_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"👀 seeing","color":"1d76db","description":"a background agent has picked the issue up and is working on it"}' \
  "$GITEA_API/repos/$GITEA_OWNER_REPO/labels/$LABEL_ID" \
  | jq '{id, name, color}'
```

Create `question` if the repo never had it. Delete leftover old labels by hand
from the web UI after checking that no issue still uses them.

## Troubleshooting

1. **`{"message":"invalid username, password or token"}` / HTTP 401** — the auth
   header is wrong. Gitea expects `Authorization: token $GITEA_TOKEN`. Use the
   literal word `token`, **not** `Bearer`. `Bearer` fails even with a valid
   token.

2. **HTTP 404 / `not found`** — the host or `owner/repo` is wrong. Derive both
   from `git remote get-url origin`; do not assume `gitea.com`. The repo slug is
   case-sensitive (`WeightManagement-frontend` is not
   `weightmanagement-frontend`).

3. **HTTP 403** — the token lacks access to this repository, or the endpoint
   needs a scope the token does not have. Check the token in the Gitea web UI.

4. **Labels do not change after an issue `PATCH`** — Gitea silently ignores
   `labels` there. Use `PUT /issues/{n}/labels` with the full numeric-ID set.

5. **`json: cannot unmarshal string into Go struct field ... of type int64`** —
   you sent label names. Send the numeric label IDs from `GET /labels`.

6. **HTTP 5xx when creating a label with an emoji** — a Gitea whose label store
   cannot hold 4-byte characters may reject `👀 seeing`. Retry with the bare
   name (`seeing`) and keep the canonical colour and description. Read and write
   whichever spelling the repo actually stores.

7. **`jq: command not found`** — drop the `| jq ...` pipe; the raw JSON is still
   readable. Parsing IDs by hand is slower, so install `jq` when possible.

8. **Empty output** — source `.env` silently, confirm `$GITEA_TOKEN` is
   non-empty, and print `$GITEA_API` and `$GITEA_OWNER_REPO` to catch a bad
   remote. Never print the token.

## Usage notes

- **Never close an issue** from this skill. Add the label and a comment; a human
  closes after review.
- **`seeing` means picked up**, not filed. Filing writes no status label.
- **Always write the full label set.** `PUT /issues/{n}/labels` replaces, so
  collect unrelated labels before adding a status.
- **One vocabulary everywhere.** Use these five names and colours even when
  editing labels by hand; do not invent per-repo status names.
- **The token is a secret.** Source `.env` silently and never echo, log or paste
  its value.
