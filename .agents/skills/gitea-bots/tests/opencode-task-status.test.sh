#!/usr/bin/env bash
# tests/opencode-task-status.test.sh — black-box tests for
# scripts/opencode-task-status.sh (repo-local fallback, upstream #127).
#
# Mocks `opencode api` via OPENCODE_TASK_API_CMD and records requests. No live
# OpenCode service or network access required.
#
#   bash .agents/skills/gitea-bots/tests/opencode-task-status.test.sh

set -euo pipefail

TEST_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="$(git -C "$TEST_DIR" rev-parse --show-toplevel)"
HELPER="$REPO_ROOT/scripts/opencode-task-status.sh"

WORK="$(mktemp -d "$REPO_ROOT/tmp/opencode-task-status-test.XXXXXX")"
trap 'rm -rf -- "$WORK"' EXIT

mkdir -p "$WORK/bin"
CALLS="$WORK/calls.log"
RESP="$WORK/response.json"
printf '{"data":[]}\n' >"$RESP"
: >"$CALLS"

cat >"$WORK/bin/mock-opencode" <<'EOS'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${MOCK_CALLS:?}"
# args: api get|delete <path>
method="${2:-}"
path="${3:-}"
case "$method $path" in
  get\ /api/shell/*/output*|get\ /api/shell/*output*)
    cat "${MOCK_RESP_OUTPUT:-${MOCK_RESP:?}}"
    ;;
  get\ /api/shell*|get\ /api/experimental/job*)
    cat "${MOCK_RESP:?}"
    ;;
  delete\ /api/shell/*|delete\ /api/shell*)
    cat "${MOCK_RESP_DELETE:-${MOCK_RESP:?}}"
    ;;
  *)
    printf '{"error":"unexpected %s %s"}\n' "$method" "$path"
    exit 1
    ;;
esac
EOS
chmod +x "$WORK/bin/mock-opencode"

export MOCK_CALLS="$CALLS" MOCK_RESP="$RESP"
API_CMD="bash $WORK/bin/mock-opencode api"

fail=0
check() {
  if [[ "$1" == "$2" ]]; then printf 'ok: %s\n' "$3"; else printf 'FAIL: %s — want %q, got %q\n' "$3" "$1" "$2"; fail=1; fi
}
check_contains() {
  if printf '%s' "$1" | grep -Fq -- "$2"; then printf 'ok: %s\n' "$3"; else printf 'FAIL: %s — missing %q in:\n%s\n' "$3" "$2" "$1"; fail=1; fi
}
check_not_contains() {
  if printf '%s' "$1" | grep -Fq -- "$2"; then printf 'FAIL: %s — unexpectedly contains %q\n' "$3" "$2"; fail=1; else printf 'ok: %s\n' "$3"; fi
}
calls() { cat "$CALLS"; }
reset_calls() { : >"$CALLS"; }

# 1. list always passes location[directory] (never service cwd default).
reset_calls
printf '{"data":[]}\n' >"$RESP"
OPENCODE_TASK_API_CMD="$API_CMD" bash "$HELPER" --directory /repo/root list >/dev/null
check_contains "$(calls)" 'get /api/shell?location[directory]=%2Frepo%2Froot' "list passes encoded location[directory]"
check_not_contains "$(calls)" 'get /api/shell$' "list never calls /api/shell without location"

# 2. status passes location and reports queried context on miss.
reset_calls
printf '{"_tag":"ShellNotFoundError","id":"sh_missing"}\n' >"$RESP"
set +e
err="$(OPENCODE_TASK_API_CMD="$API_CMD" OPENCODE_SESSION_ID=ses_test bash "$HELPER" --directory /repo/root status sh_missing 2>&1 >/dev/null)"
rc=$?
set -e
if (( rc != 0 )); then printf 'ok: %s\n' "status exits non-zero on missing job"; else printf 'FAIL: status should fail on missing job\n'; fail=1; fi
check_contains "$err" 'queried location[directory]=/repo/root' "status miss names queried directory"
check_contains "$err" 'sessionID=ses_test' "status miss names queried session"
check_contains "$(calls)" 'location[directory]=%2Frepo%2Froot' "status request includes location"

# 3. output is resumable by cursor and supports --tail.
reset_calls
jq -n '{"output":"a\nb\nc\nd\n","cursor":8,"size":8,"truncated":false}' >"$RESP"
out="$(OPENCODE_TASK_API_CMD="$API_CMD" bash "$HELPER" --directory /repo/root output sh_123 --cursor 4 --tail 2)"
check_contains "$(calls)" '/api/shell/sh_123/output?location[directory]=%2Frepo%2Froot&cursor=4' "output passes location and cursor"
check_contains "$out" '"truncated"' "output preserves metadata with --tail"
# last 2 lines of "a\nb\nc\nd\n" split => ["a","b","c","d",""] -> last 2 => ["d",""]
check_contains "$(printf '%s' "$out" | jq -r '.output')" $'d' "output --tail keeps tail lines"

# 4. kill is explicit/destructive and requires --yes.
reset_calls
printf '{"ok":true}\n' >"$RESP"
set +e
err="$(OPENCODE_TASK_API_CMD="$API_CMD" bash "$HELPER" --directory /repo/root kill sh_123 2>&1 >/dev/null)"
rc=$?
set -e
if (( rc != 0 )); then printf 'ok: %s\n' "kill without --yes is refused"; else printf 'FAIL: kill without --yes should fail\n'; fail=1; fi
check_contains "$err" '--yes' "kill refusal mentions --yes"
check_not_contains "$(calls)" 'delete /api/shell' "kill without --yes makes no API call"

reset_calls
OPENCODE_TASK_API_CMD="$API_CMD" bash "$HELPER" --directory /repo/root kill sh_123 --yes >/dev/null
check_contains "$(calls)" 'delete /api/shell/sh_123?location[directory]=%2Frepo%2Froot' "kill with --yes calls DELETE with location"

# 5. wait refuses unbounded/missing timeout.
reset_calls
set +e
err="$(OPENCODE_TASK_API_CMD="$API_CMD" bash "$HELPER" --directory /repo/root wait sh_123 2>&1 >/dev/null)"
rc=$?
set -e
if (( rc != 0 )); then printf 'ok: %s\n' "wait without --timeout is refused"; else printf 'FAIL: wait without --timeout should fail\n'; fail=1; fi
check_contains "$err" 'unbounded wait is refused' "wait refusal explains bounded requirement"
check_not_contains "$(calls)" 'get /api/shell' "wait without --timeout makes no API call"

# 6. experimental jobs use session scoping when provided.
reset_calls
printf '{"data":[]}\n' >"$RESP"
OPENCODE_TASK_API_CMD="$API_CMD" OPENCODE_SESSION_ID=ses_abc bash "$HELPER" --directory /repo/root --session ses_abc list >/dev/null
check_contains "$(calls)" '/api/experimental/job?sessionID=ses_abc' "list queries experimental jobs with sessionID when session is set"

if (( fail != 0 )); then
  printf '\nopencode-task-status tests: FAILED\n'
  exit 1
fi
printf '\nopencode-task-status tests: all passed\n'
