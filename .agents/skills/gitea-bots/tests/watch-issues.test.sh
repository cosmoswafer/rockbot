#!/usr/bin/env bash
# shellcheck shell=bash
#
# watch-issues.test.sh — offline functional tests for
# scripts/watch-issues.sh.
#
# The suite sources the watcher, replaces its HTTP layer with fixture files,
# and calls poll_once / cmd_* directly. No network, no Gitea server, no token.
#
# Usage:
#   .agents/skills/gitea-bots/tests/watch-issues.test.sh
#
# Runtime scratch space lives in ./tmp/gitea-bots-test/ (never /tmp).
# Every scenario starts from a fresh state directory; the first failure exits
# non-zero with the offending poll's output.

set -euo pipefail

TEST_PATH="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)/$(basename -- "${BASH_SOURCE[0]}")"
SKILL_DIR="$(dirname -- "$(dirname -- "$TEST_PATH")")"
WATCH="$SKILL_DIR/scripts/watch-issues.sh"

REPO_ROOT="$(git -C "$SKILL_DIR" rev-parse --show-toplevel 2>/dev/null)" || REPO_ROOT=""
if [[ -z "$REPO_ROOT" ]]; then
  REPO_ROOT="$(cd -- "$SKILL_DIR/../../.." >/dev/null 2>&1 && pwd)" || {
    echo "FAIL: cannot determine the repo root" >&2
    exit 1
  }
fi

TEST_DIR="$REPO_ROOT/tmp/gitea-bots-test"
FIXTURES="$TEST_DIR/fixtures"
OUT="$TEST_DIR/last-poll.out"

rm -rf -- "$TEST_DIR"
mkdir -p -- "$FIXTURES"

# The watcher sources GITEA_TOKEN from .env; make sure no real token leaks in.
GITEA_TOKEN=""
export GITEA_BOTS_ROOT="$REPO_ROOT"
export GITEA_BOTS_STATE_DIR="$TEST_DIR/state"
export GITEA_BOTS_IGNORE_SELF=1
export GITEA_BOTS_ALL_COMMENTS=0
unset GITEA_BOTS_VERBOSE 2>/dev/null || true

# shellcheck disable=SC1090
source "$WATCH"

REPO="Atom/rockbot"
GITEA_API="https://example.test/api/v1"
SELF_LOGIN=""
HTTP_FAIL_RE=""

# ---------------------------------------------------------------------------
# Fixture server and builders
# ---------------------------------------------------------------------------

# Stand-in for the watcher's curl wrapper. Serves $FIXTURES/issues.json for
# issue lists and $FIXTURES/comments-<N>.json for issue comments. Set
# HTTP_FAIL_RE to an ERE to simulate an API/network failure for matching URLs.
http_get() {
  local url="$1"
  if [[ -n "$HTTP_FAIL_RE" && "$url" =~ $HTTP_FAIL_RE ]]; then
    return 1
  fi
  if [[ "$url" =~ /issues/([0-9]+)/comments ]]; then
    cat -- "$FIXTURES/comments-${BASH_REMATCH[1]}.json"
  elif [[ "$url" == *"/issues?"* ]]; then
    cat -- "$FIXTURES/issues.json"
  elif [[ "$url" == *"/user" ]]; then
    printf '{"login":"botuser"}'
  else
    return 22
  fi
}

issue() { # number updated created title [state] [labels_json] [assignees_json] [milestone_json] [pull_request_json]
  jq -n \
    --argjson n "$1" --arg u "$2" --arg c "$3" --arg t "$4" \
    --arg st "${5:-open}" \
    --argjson labels "${6:-[]}" \
    --argjson assignees "${7:-[]}" \
    --argjson milestone "${8:-null}" \
    --argjson pr "${9:-null}" \
    '{
      number: $n, updated_at: $u, created_at: $c, title: $t,
      user: {login: "alice"}, state: $st, labels: $labels,
      assignees: $assignees, milestone: $milestone,
      html_url: "https://example.test/Atom/rockbot/issues/\($n)",
      pull_request: $pr, comments: 0
    }'
}

comment() { # id body author created
  jq -n \
    --argjson id "$1" --arg b "$2" --arg a "$3" --arg c "$4" \
    '{
      id: $id, body: $b, user: {login: $a}, created_at: $c,
      html_url: "https://example.test/Atom/rockbot/issues/1#issuecomment-\($id)"
    }'
}

issues_json() { jq -s '.'; }

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  if [[ -f "$OUT" ]]; then
    sed 's/^/  | /' "$OUT" >&2
  fi
  exit 1
}

pass() { printf 'ok: %s\n' "$*"; }

run_poll() { # [report_existing]
  local report_existing="${1:-0}" rc=0
  poll_once "$report_existing" > "$OUT" 2>&1 || rc=$?
  return "$rc"
}

expect_out() { grep -Fq -- "$1" "$OUT" || fail "expected output to contain: $1"; }
expect_no_out() { ! grep -Fq -- "$1" "$OUT" || fail "expected output NOT to contain: $1"; }
expect_no_events() {
  ! grep -Eq 'NEW ISSUE|UPDATED ISSUE|TRIGGER COMMENT|NEW COMMENT' "$OUT" \
    || fail "expected no events"
}

state_field() { jq -r "$1" "$STATE_FILE"; }

# Fresh, isolated state directory for a scenario group.
use_state() { # name
  STATE_DIR="$TEST_DIR/$1"
  STATE_FILE="$STATE_DIR/state.json"
  PID_FILE="$STATE_DIR/watcher.pid"
  LOCK_FILE="$STATE_DIR/poll.lock"
  rm -rf -- "$STATE_DIR"
  mkdir -p -- "$STATE_DIR"
}

# ---------------------------------------------------------------------------
# Baseline and issue-field events
# ---------------------------------------------------------------------------

use_state state

issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "First issue" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"

run_poll || fail "baseline poll failed"
expect_no_events
[[ "$(state_field '.issues | length')" == "1" ]] || fail "baseline did not record the issue"
[[ "$(state_field '.issues["1"].last_comment_id')" == "0" ]] || fail "bad baseline comment id"
[[ "$(state_field '.last_poll')" != "null" ]] || fail "baseline did not set last_poll"
pass "baseline records silently"

run_poll || fail "second poll failed"
expect_no_events
pass "repeat poll is idempotent"

# Field changes are reported one at a time, with updated_at advancing.
issue 1 "2026-11-08T10:01:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  | issues_json > "$FIXTURES/issues.json"
run_poll || fail "title-change poll failed"
expect_out 'UPDATED ISSUE #1 — changed: title'
pass "title change emitted"

issue 1 "2026-11-08T10:02:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  open '[{"name":"bug"}]' '[]' null | issues_json > "$FIXTURES/issues.json"
run_poll || fail "labels-change poll failed"
expect_out 'UPDATED ISSUE #1 — changed: labels'
pass "label change emitted"

issue 1 "2026-11-08T10:03:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  open '[{"name":"bug"}]' '[{"login":"bob"}]' null | issues_json > "$FIXTURES/issues.json"
run_poll || fail "assignee-change poll failed"
expect_out 'UPDATED ISSUE #1 — changed: assignees'
pass "assignee change emitted"

issue 1 "2026-11-08T10:04:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  open '[{"name":"bug"}]' '[{"login":"bob"}]' '{"title":"v1"}' | issues_json > "$FIXTURES/issues.json"
run_poll || fail "milestone-change poll failed"
expect_out 'UPDATED ISSUE #1 — changed: milestone'
pass "milestone change emitted"

issue 1 "2026-11-08T10:05:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  closed '[{"name":"bug"}]' '[{"login":"bob"}]' '{"title":"v1"}' | issues_json > "$FIXTURES/issues.json"
run_poll || fail "state-change poll failed"
expect_out 'UPDATED ISSUE #1 — changed: state'
pass "state change emitted"

# updated_at moves but no tracked field and no new comment explains it.
issue 1 "2026-11-08T10:06:00Z" "2026-11-08T09:00:00Z" "First issue renamed" \
  closed '[{"name":"bug"}]' '[{"login":"bob"}]' '{"title":"v1"}' | issues_json > "$FIXTURES/issues.json"
run_poll || fail "body-only poll failed"
expect_out 'UPDATED ISSUE #1 — changed: body/other'
pass "body-only change emitted"

# ---------------------------------------------------------------------------
# New issues and comment-only updates
# ---------------------------------------------------------------------------

use_state events
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "First issue" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"
run_poll || fail "events baseline failed"

{
  issue 1 "2026-11-08T10:05:00Z" "2026-11-08T09:00:00Z" "First issue"
  issue 2 "2026-11-08T10:05:00Z" "2026-11-08T10:04:00Z" "Second issue"
} | issues_json > "$FIXTURES/issues.json"
comment 10 "/plan" bob "2026-11-08T10:05:00Z" | issues_json > "$FIXTURES/comments-1.json"
echo '[]' > "$FIXTURES/comments-2.json"

run_poll || fail "new-issue poll failed"
expect_out 'TRIGGER COMMENT on #1 (comment 10) by bob'
expect_out 'NEW ISSUE #2 "Second issue"'
[[ "$(state_field '.issues["1"].last_comment_id')" == "10" ]] || fail "comment id not stored"
pass "trigger comment and new issue emitted"

# A comment created in the same second as updated_at is still detected, and
# does not also produce a spurious issue-level event.
issue 1 "2026-11-08T10:05:00Z" "2026-11-08T09:00:00Z" "First issue" \
  | issues_json > "$FIXTURES/issues.json"
comment 11 "/implement" bob "2026-11-08T10:05:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll || fail "same-second poll failed"
expect_out 'TRIGGER COMMENT on #1 (comment 11) by bob'
expect_no_out 'UPDATED ISSUE'
pass "same-second comment detected without issue noise"

# `--report-existing` lists issues on the very first poll and still records
# their comments silently.
use_state report-existing
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Existing issue" \
  | issues_json > "$FIXTURES/issues.json"
comment 20 "/plan" bob "2026-11-08T09:30:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll 1 || fail "report-existing poll failed"
expect_out 'NEW ISSUE #1 "Existing issue"'
expect_no_out 'TRIGGER COMMENT'
[[ "$(state_field '.issues["1"].last_comment_id')" == "20" ]] \
  || fail "baseline comment id not recorded in report-existing mode"
pass "report-existing lists issues but not comments"

# ---------------------------------------------------------------------------
# Trigger matching
# ---------------------------------------------------------------------------

use_state triggers
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Trigger target" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"
run_poll || fail "trigger baseline failed"

trigger_case() { # id body expect(yes|no)
  local id="$1" body="$2" expect="$3"
  comment "$id" "$body" bob "2026-11-08T10:00:00Z" | issues_json > "$FIXTURES/comments-1.json"
  run_poll || fail "trigger poll failed for: $body"
  if [[ "$expect" == "yes" ]]; then
    expect_out "TRIGGER COMMENT on #1 (comment $id) by bob"
  else
    expect_no_out 'TRIGGER COMMENT'
  fi
}

trigger_case 101 "/plan" yes
trigger_case 102 "/implement" yes
trigger_case 103 "/investigate" yes
trigger_case 104 "/Plan" yes
trigger_case 105 "please investigate this" yes
trigger_case 106 "plan how to implement this" yes
trigger_case 107 "Plan the implementation" yes
trigger_case 108 "execute dev flow" yes
trigger_case 109 "plan" no
trigger_case 110 "planned" no
trigger_case 111 "implementation details below" no
trigger_case 112 "/implementing" no
trigger_case 113 "ok" no
trigger_case 114 "LGTM" no
trigger_case 115 "👍" no
pass "trigger phrases match exactly as documented"

# ---------------------------------------------------------------------------
# Author filtering and all-comments mode
# ---------------------------------------------------------------------------

use_state self
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Self filter" \
  | issues_json > "$FIXTURES/issues.json"
comment 30 "/implement" botuser "2026-11-08T10:00:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll || fail "self-filter baseline failed"

SELF_LOGIN="botuser"
issue 1 "2026-11-08T10:01:00Z" "2026-11-08T09:00:00Z" "Self filter" \
  | issues_json > "$FIXTURES/issues.json"
comment 31 "/implement" botuser "2026-11-08T10:01:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll || fail "self-comment poll failed"
expect_no_out 'TRIGGER COMMENT'
[[ "$(state_field '.issues["1"].last_comment_id')" == "31" ]] \
  || fail "self comment not consumed"
pass "self-authored trigger ignored but consumed"

GITEA_BOTS_IGNORE_SELF=0
issue 1 "2026-11-08T10:02:00Z" "2026-11-08T09:00:00Z" "Self filter" \
  | issues_json > "$FIXTURES/issues.json"
comment 32 "/implement" botuser "2026-11-08T10:02:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll || fail "ignore-self=0 poll failed"
expect_out 'TRIGGER COMMENT on #1 (comment 32) by botuser'
GITEA_BOTS_IGNORE_SELF=1
SELF_LOGIN=""
pass "GITEA_BOTS_IGNORE_SELF=0 surfaces self comments"

use_state all-comments
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Chatter" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"
run_poll || fail "all-comments baseline failed"

GITEA_BOTS_ALL_COMMENTS=1
issue 1 "2026-11-08T10:01:00Z" "2026-11-08T09:00:00Z" "Chatter" \
  | issues_json > "$FIXTURES/issues.json"
comment 40 "just chatting" bob "2026-11-08T10:01:00Z" | issues_json > "$FIXTURES/comments-1.json"
run_poll || fail "all-comments poll failed"
expect_out 'NEW COMMENT on #1 (comment 40) by bob'
GITEA_BOTS_ALL_COMMENTS=0
pass "GITEA_BOTS_ALL_COMMENTS=1 reports every new comment"

# ---------------------------------------------------------------------------
# Pull requests and API failures
# ---------------------------------------------------------------------------

use_state pull-requests
issue 50 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "A PR" \
  open '[]' '[]' null '{"url":"https://example.test/Atom/rockbot/pulls/50"}' \
  | issues_json > "$FIXTURES/issues.json"
run_poll || fail "pull-request poll failed"
expect_no_events
[[ "$POLL_SCANNED" == "0" ]] || fail "pull request was not filtered (scanned $POLL_SCANNED)"
[[ "$(state_field '.issues | length')" == "0" ]] || fail "pull request was stored"
pass "pull requests are filtered out"

use_state api-failure
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Failure target" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"
run_poll || fail "api-failure baseline failed"
last_poll_before="$(state_field '.last_poll')"

HTTP_FAIL_RE='/issues\?'
run_poll && fail "failing API call unexpectedly succeeded"
HTTP_FAIL_RE=""
[[ "$(state_field '.last_poll')" == "$last_poll_before" ]] \
  || fail "last_poll advanced despite a failed poll"
pass "failed poll leaves last_poll unadvanced"

# ---------------------------------------------------------------------------
# State repair
# ---------------------------------------------------------------------------

use_state empty-repo
echo '[]' > "$FIXTURES/issues.json"
run_poll || fail "empty-repo poll failed"
expect_no_events
[[ "$(state_field '.issues | length')" == "0" ]] || fail "empty repo state wrong"
pass "empty repo handled"

use_state bad-last-poll
printf '%s\n' '{"version":1,"repo":"Atom/rockbot","initialized_at":"2026-11-08T09:00:00Z","last_poll":"not-a-date","issues":{}}' \
  > "$STATE_FILE"
echo '[]' > "$FIXTURES/issues.json"
run_poll || fail "bad-last-poll repair failed"
[[ "$(state_field '.last_poll')" =~ ^[0-9]{4}- ]] || fail "invalid last_poll was not repaired"
pass "invalid last_poll re-baselined"

use_state corrupt-state
printf '%s\n' 'not json at all' > "$STATE_FILE"
echo '[]' > "$FIXTURES/issues.json"
run_poll || fail "corrupt-state repair failed"
compgen -G "$STATE_FILE.corrupt.*" >/dev/null || fail "corrupt state was not backed up"
pass "corrupt state.json backed up and re-baselined"

use_state repo-mismatch
printf '%s\n' '{"version":1,"repo":"Other/repo","initialized_at":"2026-11-08T09:00:00Z","last_poll":"2026-11-08T09:00:00Z","issues":{}}' \
  > "$STATE_FILE"
echo '[]' > "$FIXTURES/issues.json"
run_poll || fail "repo-mismatch repair failed"
compgen -G "$STATE_FILE.repo.*" >/dev/null || fail "foreign state was not backed up"
expect_no_events
pass "state for another repo re-baselined"

# ---------------------------------------------------------------------------
# Commands (reset / status / stop) without background processes
# ---------------------------------------------------------------------------

use_state commands
issue 1 "2026-11-08T10:00:00Z" "2026-11-08T09:00:00Z" "Commands" \
  | issues_json > "$FIXTURES/issues.json"
echo '[]' > "$FIXTURES/comments-1.json"
run_poll || fail "commands baseline failed"

reset_out="$(cmd_reset)"
[[ "$reset_out" == *"state cleared"* ]] || fail "reset did not report clearing state"
[[ ! -f "$STATE_FILE" ]] || fail "reset left state.json in place"
compgen -G "$STATE_FILE.bak.*" >/dev/null || fail "reset did not back up state"
pass "reset moves state aside"

printf '%s\n' '999999' > "$PID_FILE"
status_out="$(cmd_status)"
[[ "$status_out" == *"watcher: stopped"* ]] || fail "status did not report stopped"
[[ "$status_out" == *"stale pid file"* ]] || fail "status did not flag the stale pid"
[[ "$status_out" == *"not initialized"* ]] || fail "status did not report the pending baseline"
stop_out="$(cmd_stop)"
[[ "$stop_out" == *"not running (stale pid 999999)"* ]] || fail "stop did not report the stale pid"
[[ ! -f "$PID_FILE" ]] || fail "stop left the stale pid file behind"
pass "status/stop handle a stale pid file"

printf '\nALL TESTS PASSED\n'
