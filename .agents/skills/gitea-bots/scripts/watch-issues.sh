#!/usr/bin/env bash
# shellcheck shell=bash
#
# watch-issues.sh — poll the project's Gitea repo for new issues and for issue
# comments that look like gitea-bots stage triggers.
#
# This script only *surfaces* events. It never creates, labels, assigns,
# comments on, or closes anything, and it never advances a workflow stage.
#
# Usage:
#   watch-issues.sh check [--interval N] [--report-existing]
#   watch-issues.sh loop  [--interval N] [--report-existing]
#   watch-issues.sh start [--interval N] [--report-existing]
#   watch-issues.sh status
#   watch-issues.sh stop
#   watch-issues.sh reset
#
# Runtime state lives under ./tmp/gitea-bots/ (never /tmp):
#   state.json    — seen issues / comments / last successful poll
#   watcher.pid   — PID of the background loop
#   watcher.log   — background loop output
#   poll.lock     — flock guarding state writes
#
# Environment:
#   GITEA_BOTS_INTERVAL     poll interval in seconds (default 45, minimum 5)
#   GITEA_BOTS_STATE_DIR    override ./tmp/gitea-bots
#   GITEA_BOTS_LOG          override $STATE_DIR/watcher.log
#   GITEA_BOTS_ROOT         override the detected repo root
#   GITEA_BOTS_ALL_COMMENTS 1 = report every new comment, not only triggers
#   GITEA_BOTS_IGNORE_SELF  default 1 = skip comments authored by the token user
#   GITEA_BOTS_SELF         token user's login (skips the /user lookup)
#   GITEA_BOTS_TRIGGER_RE   override the trigger ERE
#   GITEA_BOTS_VERBOSE      1 = log poll heartbeats
#
# GITEA_TOKEN is read from .env (sourced silently) and is never printed.

set -euo pipefail

SCRIPT_PATH="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)/$(basename -- "${BASH_SOURCE[0]}")"

if [[ -n "${GITEA_BOTS_ROOT:-}" ]]; then
  REPO_ROOT="$(cd -- "$GITEA_BOTS_ROOT" >/dev/null 2>&1 && pwd)" || REPO_ROOT=""
else
  REPO_ROOT="$(git -C "$(dirname -- "$SCRIPT_PATH")" rev-parse --show-toplevel 2>/dev/null)" || REPO_ROOT=""
  if [[ -z "$REPO_ROOT" ]]; then
    REPO_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")/../../../.." >/dev/null 2>&1 && pwd)" || REPO_ROOT=""
  fi
fi

if [[ -z "$REPO_ROOT" ]]; then
  printf 'WARN cannot determine the repo root — set GITEA_BOTS_ROOT\n' >&2
  exit 1
fi

STATE_DIR="${GITEA_BOTS_STATE_DIR:-$REPO_ROOT/tmp/gitea-bots}"
STATE_FILE="$STATE_DIR/state.json"
PID_FILE="$STATE_DIR/watcher.pid"
LOCK_FILE="$STATE_DIR/poll.lock"
LOG_FILE="${GITEA_BOTS_LOG:-$STATE_DIR/watcher.log}"
DEFAULT_INTERVAL="${GITEA_BOTS_INTERVAL:-45}"

TRIGGER_RE="${GITEA_BOTS_TRIGGER_RE:-/(investigate|plan|implement)([^[:alnum:]_]|$)|(^|[^[:alnum:]_])(investigate|execute dev flow|implement the plan|plan how to implement|plan the implementation)([^[:alnum:]_]|$)}"

REPO=""
GITEA_API=""
GITEA_TOKEN="${GITEA_TOKEN:-}"
SELF_LOGIN="${GITEA_BOTS_SELF:-}"
INTERVAL="$DEFAULT_INTERVAL"
WORK=""
WINDOW_START=""
WINDOW_EPOCH=0
POLL_SCANNED=0
POLL_EVENTS=0
POLL_HAD_ERROR=0
LOCK_FD_OPEN=0

# ---------------------------------------------------------------------------
# Output / usage
# ---------------------------------------------------------------------------

now_utc() { date -u +%Y-%m-%dT%H:%M:%SZ; }

log() { printf '%s %s\n' "$(now_utc)" "$*"; }

warn() { printf '%s WARN %s\n' "$(now_utc)" "$*" >&2; }

die() {
  warn "$*"
  exit 1
}

verbose() {
  if [[ "${GITEA_BOTS_VERBOSE:-0}" == "1" ]]; then
    log "$*"
  fi
}

usage() {
  cat <<'USAGE'
Poll the project's Gitea repo for new issues and stage-trigger comments.

Usage:
  watch-issues.sh check  [--interval N] [--report-existing]
      Poll once, print events, exit. Refuses to run while the background
      watcher holds the poll lock.
  watch-issues.sh loop   [--interval N] [--report-existing]
      Run the poll loop in the foreground (Ctrl-C / SIGTERM stops it).
  watch-issues.sh start  [--interval N] [--report-existing]
      Start the loop in the background (nohup), writing a PID file.
  watch-issues.sh status
      Show whether the loop is running, plus state-file stats.
  watch-issues.sh stop
      Stop the background loop (SIGTERM, then SIGKILL if needed).
  watch-issues.sh reset
      Move state.json aside; the next poll re-baselines silently.

Options:
  -i, --interval N     Poll interval in seconds (default: GITEA_BOTS_INTERVAL or 45)
      --report-existing
                       On the first (baseline) poll, emit NEW ISSUE events for
                       issues that already exist instead of recording them
                       silently.
  -h, --help           Show this help.

Environment:
  GITEA_BOTS_INTERVAL, GITEA_BOTS_STATE_DIR, GITEA_BOTS_LOG, GITEA_BOTS_ROOT,
  GITEA_BOTS_ALL_COMMENTS, GITEA_BOTS_IGNORE_SELF, GITEA_BOTS_SELF,
  GITEA_BOTS_TRIGGER_RE, GITEA_BOTS_VERBOSE

State: ./tmp/gitea-bots/ (state.json, watcher.pid, watcher.log, poll.lock)
Token: GITEA_TOKEN is sourced silently from .env and never printed.
USAGE
}

# ---------------------------------------------------------------------------
# Environment / remote derivation
# ---------------------------------------------------------------------------

require_tools() {
  local tool
  for tool in curl jq git date; do
    command -v "$tool" >/dev/null 2>&1 || die "missing required tool: $tool"
  done
}

load_env() {
  if [[ -f "$REPO_ROOT/.env" ]]; then
    # shellcheck disable=SC1091
    source "$REPO_ROOT/.env" >/dev/null 2>&1 || true
  fi
  GITEA_TOKEN="${GITEA_TOKEN:-}"
}

# Derive $REPO and $GITEA_API from `origin`, exactly like the gitea-issues
# skill: never hardcode host or owner/repo, and drop any :port suffix.
derive_remote() {
  local remote bare hostpart host
  remote="$(git remote get-url origin 2>/dev/null)" || die "no git 'origin' remote"

  remote="${remote%.git}" # strip trailing .git if present

  if [[ "$remote" == *://* ]]; then
    # URL-style (ssh://, https://, http://): [user@]host[:port]/owner/repo
    bare="${remote#*://}"
    bare="${bare#*@}"      # strip user@ (e.g. git@)
    REPO="${bare#*/}"      # owner/repo after first slash
    hostpart="${bare%%/*}" # host[:port]
  else
    # scp-style (git@host:owner/repo): [user@]host:owner/repo
    bare="${remote#*@}"      # strip user@
    hostpart="${bare%%:*}"   # host (before first colon)
    REPO="${bare#*:}"        # owner/repo after colon
  fi

  host="${hostpart%%:*}" # drop any :port suffix — API is on default HTTPS port

  [[ -n "$host" ]] || die "cannot derive Gitea host from origin: $remote"
  [[ "$REPO" == */* ]] || die "cannot derive owner/repo from origin: $remote"

  GITEA_API="https://${host}/api/v1"
  [[ -n "$GITEA_TOKEN" ]] || die "GITEA_TOKEN is empty — set it in .env (it is never printed)"
}

resolve_self() {
  SELF_LOGIN="${GITEA_BOTS_SELF:-${SELF_LOGIN:-}}"
  [[ -n "$SELF_LOGIN" ]] && return 0
  [[ "${GITEA_BOTS_IGNORE_SELF:-1}" == "1" ]] || return 0

  local response login=""
  if response="$(http_get "$GITEA_API/user" 2>/dev/null)"; then
    login="$(jq -r '.login // empty' <<<"$response" 2>/dev/null)" || login=""
  fi
  if [[ -n "$login" ]]; then
    SELF_LOGIN="$login"
  else
    warn "could not resolve the token user; self-comment filtering is disabled"
  fi
}

# ---------------------------------------------------------------------------
# HTTP / state helpers
# ---------------------------------------------------------------------------

ensure_state_dir() {
  mkdir -p -- "$STATE_DIR"
}

# http_get URL — prints the response body, fails on network or non-2xx errors.
http_get() {
  local url="$1" body code
  body="$(mktemp "$STATE_DIR/.http.XXXXXX")" || return 1
  if ! code="$(curl -sS -m 25 -o "$body" -w '%{http_code}' \
    -H "Authorization: token $GITEA_TOKEN" \
    -H 'Accept: application/json' \
    "$url")"; then
    warn "network error while calling $url"
    rm -f -- "$body"
    return 1
  fi
  if [[ "$code" != 2* ]]; then
    warn "HTTP $code from $url — $(jq -r '.message // .error // empty' "$body" 2>/dev/null | head -n1 || true)"
    rm -f -- "$body"
    return 1
  fi
  cat -- "$body"
  rm -f -- "$body"
}

acquire_lock() {
  ensure_state_dir
  if ! command -v flock >/dev/null 2>&1; then
    warn "flock not available — running without a poll lock"
    return 0
  fi
  exec 9>"$LOCK_FILE"
  if flock -n 9; then
    LOCK_FD_OPEN=1
    return 0
  fi
  exec 9>&-
  return 1
}

release_lock() {
  if (( LOCK_FD_OPEN == 1 )); then
    exec 9>&-
    LOCK_FD_OPEN=0
  fi
}

new_state_json() {
  jq -n \
    --arg repo "$REPO" \
    --arg api "$GITEA_API" \
    --arg now "$(now_utc)" \
    '{version: 1, repo: $repo, api: $api, initialized_at: $now, last_poll: null, issues: {}}'
}

ensure_state() {
  if [[ -f "$STATE_FILE" ]]; then
    if ! jq -e 'type == "object" and (.issues | type == "object")' "$STATE_FILE" >/dev/null 2>&1; then
      warn "state file is unreadable or corrupt — re-baselining: $STATE_FILE"
      mv -- "$STATE_FILE" "$STATE_FILE.corrupt.$(date -u +%Y%m%dT%H%M%SZ)" 2>/dev/null || true
      new_state_json > "$STATE_FILE"
      return 0
    fi
    local state_repo=""
    state_repo="$(jq -r '.repo // empty' "$STATE_FILE" 2>/dev/null)" || state_repo=""
    if [[ -n "$state_repo" && "$state_repo" != "$REPO" ]]; then
      warn "state belongs to repo '$state_repo' — re-baselining for '$REPO'"
      mv -- "$STATE_FILE" "$STATE_FILE.repo.$(date -u +%Y%m%dT%H%M%SZ)" 2>/dev/null || true
      new_state_json > "$STATE_FILE"
    fi
  else
    new_state_json > "$STATE_FILE"
  fi
}

shift_iso() {
  local iso="$1" delta="$2" epoch
  epoch="$(date -u -d "$iso" +%s 2>/dev/null)" || return 1
  date -u -d "@$((epoch + delta))" +%Y-%m-%dT%H:%M:%SZ
}

timestamp_in_window() {
  local ts="${1:-}" epoch
  [[ -n "${WINDOW_START:-}" ]] || return 1
  (( WINDOW_EPOCH > 0 )) || return 0
  epoch="$(date -u -d "$ts" +%s 2>/dev/null)" || return 0
  (( epoch >= WINDOW_EPOCH ))
}

# ---------------------------------------------------------------------------
# Gitea reads (read-only; the watcher never mutates the issue tracker)
# ---------------------------------------------------------------------------

fetch_issues_baseline() {
  # 50 most recently updated issues, PRs filtered out later.
  http_get "$GITEA_API/repos/$REPO/issues?state=all&sort=updated&direction=desc&limit=50&page=1"
}

fetch_issues_since() {
  local since="$1" encoded page=1 limit=50 n f
  local -a files=()
  encoded="$(jq -rn --arg s "$since" '$s | @uri')"

  while (( page <= 20 )); do
    f="$WORK/issues.page.$page.json"
    if ! http_get "$GITEA_API/repos/$REPO/issues?state=all&sort=updated&direction=asc&limit=$limit&since=$encoded&page=$page" > "$f"; then
      return 1
    fi
    files+=("$f")
    n="$(jq 'length' "$f" 2>/dev/null)" || return 1
    if (( n < limit )); then
      break
    fi
    page=$((page + 1))
  done

  jq -s 'add // []' "${files[@]}"
}

fetch_comments() {
  local num="$1" page=1 limit=50 n f
  local -a files=()

  while (( page <= 20 )); do
    f="$WORK/comments.$num.$page.json"
    if ! http_get "$GITEA_API/repos/$REPO/issues/$num/comments?limit=$limit&page=$page" > "$f"; then
      return 1
    fi
    files+=("$f")
    n="$(jq 'length' "$f" 2>/dev/null)" || return 1
    if (( n < limit )); then
      break
    fi
    page=$((page + 1))
  done

  jq -s 'add // [] | sort_by(.id)' "${files[@]}"
}

# ---------------------------------------------------------------------------
# Polling
# ---------------------------------------------------------------------------

add_update() {
  local number="$1" updated_at="$2" title="$3" state="$4" labels="$5"
  local assignees="$6" milestone="$7" last_comment_id="$8"

  jq -nc \
    --argjson number "$number" \
    --arg updated_at "$updated_at" \
    --arg title "$title" \
    --arg state "$state" \
    --arg labels "$labels" \
    --arg assignees "$assignees" \
    --arg milestone "$milestone" \
    --argjson last_comment_id "$last_comment_id" \
    '{number: $number, updated_at: $updated_at, title: $title, state: $state,
      labels: $labels, assignees: $assignees, milestone: $milestone,
      last_comment_id: $last_comment_id}' \
    >> "$WORK/updates.jsonl"
}

# process_issue BASELINE REPORT_EXISTING NUMBER UPDATED CREATED TITLE AUTHOR
#               STATE LABELS ASSIGNEES MILESTONE URL
process_issue() {
  local baseline="$1" report_existing="$2" num="$3" updated="$4" created="$5"
  local title="$6" author="$7" state="$8" labels="$9"
  local assignees="${10}" milestone="${11}" url="${12}"

  [[ "$num" =~ ^[0-9]+$ ]] || {
    warn "unexpected issue number in API response: '$num'"
    return 1
  }

  local prev="" prev_updated="" prev_title="" prev_state="" prev_labels=""
  local prev_assignees="" prev_milestone="" prev_last_id=0

  prev="$(jq -c --arg n "$num" '.issues[$n] // empty' "$STATE_FILE")" || return 1
  if [[ -n "$prev" ]]; then
    IFS=$'\x1f' read -r prev_updated prev_title prev_state prev_labels prev_assignees prev_milestone prev_last_id \
      <<<"$(jq -r '[.updated_at // "", .title // "", .state // "",
                    .labels // "", .assignees // "", .milestone // "",
                    ((.last_comment_id // 0) | tostring)] | join("\u001f")' <<<"$prev")" || true
    [[ -n "$prev_last_id" ]] || prev_last_id=0
  fi

  local comments_json
  if ! comments_json="$(fetch_comments "$num")"; then
    warn "comment fetch failed for issue #$num — will retry next poll"
    return 1
  fi

  local last_id
  last_id="$(jq -r 'if length == 0 then 0 else (map(.id) | max) end' <<<"$comments_json" 2>/dev/null)" || {
    warn "could not parse comments for issue #$num"
    return 1
  }
  [[ "$last_id" =~ ^[0-9]+$ ]] || last_id=0
  if [[ -n "$prev" && "$prev_last_id" =~ ^[0-9]+$ ]] && (( last_id < prev_last_id )); then
    last_id="$prev_last_id"
  fi

  # --- collect comments that are new for this issue ------------------------
  # Filters run before event emission so a comment-only update does not also
  # produce a redundant "body/other" issue event.
  local -a new_comments=()
  local c cid ccreated cauthor cbody curl_url
  if (( baseline == 0 )); then
    while IFS= read -r c; do
      [[ -n "$c" ]] || continue

      cid="$(jq -r '.id' <<<"$c")"
      ccreated="$(jq -r '.created_at // ""' <<<"$c")"

      # Known issue: comment IDs are monotonic, so ID comparison is exact.
      if [[ -n "$prev" && "$cid" =~ ^[0-9]+$ && "$prev_last_id" =~ ^[0-9]+$ ]] \
        && (( cid <= prev_last_id )); then
        continue
      fi
      # Untracked issue: only surface comments created inside the poll window.
      if [[ -z "$prev" ]] && ! timestamp_in_window "$ccreated"; then
        continue
      fi
      new_comments+=("$c")
    done < <(jq -c '.[]' <<<"$comments_json" 2>/dev/null || true)
  fi

  # --- issue-level event ---------------------------------------------------
  if (( baseline == 1 )); then
    if (( report_existing == 1 )); then
      log "NEW ISSUE #$num \"$title\" by $author — $url"
      POLL_EVENTS=$((POLL_EVENTS + 1))
    fi
  elif [[ -n "$prev" ]]; then
    local changed=""
    if [[ "$title" != "$prev_title" ]]; then changed+="title "; fi
    if [[ "$state" != "$prev_state" ]]; then changed+="state "; fi
    if [[ "$labels" != "$prev_labels" ]]; then changed+="labels "; fi
    if [[ "$assignees" != "$prev_assignees" ]]; then changed+="assignees "; fi
    if [[ "$milestone" != "$prev_milestone" ]]; then changed+="milestone "; fi
    if [[ -n "$changed" ]]; then
      log "UPDATED ISSUE #$num — changed: ${changed% } — $url"
      POLL_EVENTS=$((POLL_EVENTS + 1))
    elif [[ "$updated" != "$prev_updated" ]] && (( ${#new_comments[@]} == 0 )); then
      # updated_at moved but no tracked field or new comment explains it
      # (typically an issue-body edit).
      log "UPDATED ISSUE #$num — changed: body/other — $url"
      POLL_EVENTS=$((POLL_EVENTS + 1))
    fi
  else
    # First time this issue is seen after baseline (e.g. it sat outside the
    # baseline window). Surface it, but do not claim it was just created.
    if timestamp_in_window "$created"; then
      log "NEW ISSUE #$num \"$title\" by $author — $url"
    else
      log "UPDATED ISSUE #$num \"$title\" (first seen — previously untracked) — $url"
    fi
    POLL_EVENTS=$((POLL_EVENTS + 1))
  fi

  # --- comment-level events ------------------------------------------------
  if (( ${#new_comments[@]} > 0 )); then
    for c in "${new_comments[@]}"; do
      cid="$(jq -r '.id' <<<"$c")"
      cauthor="$(jq -r '.user.login // "unknown"' <<<"$c")"
      cbody="$(jq -r '.body // ""' <<<"$c")"
      curl_url="$(jq -r '.html_url // ""' <<<"$c")"

      # Do not let the bot's own plan/implementation comments trigger it.
      if [[ "${GITEA_BOTS_IGNORE_SELF:-1}" == "1" && -n "${SELF_LOGIN:-}" && "$cauthor" == "$SELF_LOGIN" ]]; then
        continue
      fi

      if [[ "${GITEA_BOTS_ALL_COMMENTS:-0}" == "1" ]]; then
        log "NEW COMMENT on #$num (comment $cid) by $cauthor — ${curl_url:-$url}"
        printf '%s\n' "$cbody" | sed 's/^/    | /'
        POLL_EVENTS=$((POLL_EVENTS + 1))
      elif grep -Eiq -- "$TRIGGER_RE" <<<"$cbody"; then
        log "TRIGGER COMMENT on #$num (comment $cid) by $cauthor — ${curl_url:-$url}"
        printf '%s\n' "$cbody" | sed 's/^/    | /'
        POLL_EVENTS=$((POLL_EVENTS + 1))
      fi
    done
  fi

  add_update "$num" "$updated" "$title" "$state" "$labels" "$assignees" "$milestone" "$last_id"
}

poll_body() {
  local report_existing="${1:-0}"
  local now baseline last_poll since="" scanned
  local num updated created title author state labels assignees milestone url

  now="$(now_utc)"
  ensure_state
  baseline="$(jq -r 'if (.last_poll // null) == null then 1 else 0 end' "$STATE_FILE")"

  WORK="$STATE_DIR/work"
  rm -rf -- "$WORK"
  mkdir -p -- "$WORK"
  : > "$WORK/updates.jsonl"

  if (( baseline == 1 )); then
    if ! fetch_issues_baseline > "$WORK/issues.json"; then
      warn "baseline issue fetch failed (api: $GITEA_API, repo: $REPO)"
      return 1
    fi
    verbose "baseline poll: recording existing issues without events"
  else
    last_poll="$(jq -r '.last_poll' "$STATE_FILE")"
    # 120 s overlap absorbs clock skew and mid-poll writes; state dedupes.
    if ! since="$(shift_iso "$last_poll" -120)"; then
      warn "state last_poll is invalid ('$last_poll') — re-baselining"
      baseline=1
      since=""
      if ! fetch_issues_baseline > "$WORK/issues.json"; then
        warn "baseline issue fetch failed (api: $GITEA_API, repo: $REPO)"
        return 1
      fi
    elif ! fetch_issues_since "$since" > "$WORK/issues.json"; then
      warn "incremental issue fetch failed (since: $since)"
      return 1
    fi
  fi

  WINDOW_START="$since"
  WINDOW_EPOCH=0
  if [[ -n "$since" ]]; then
    WINDOW_EPOCH="$(date -u -d "$since" +%s 2>/dev/null)" || WINDOW_EPOCH=0
  fi

  if ! jq -r '
      .[] | select(.pull_request == null) |
      [
        (.number | tostring),
        (.updated_at // ""),
        (.created_at // ""),
        ((.title // "") | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " ")),
        ((.user.login // "unknown") | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " ")),
        (.state // ""),
        (([.labels[]?.name] | sort | join(",")) | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " ")),
        (([.assignees[]?.login] | sort | join(",")) | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " ")),
        (((.milestone.title // "")) | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " ")),
        ((.html_url // "") | gsub("[\\t\\r\\n]+"; " ") | gsub("\u001f"; " "))
      ] | join("\u001f")
    ' "$WORK/issues.json" > "$WORK/issues.tsv"; then
    warn "could not parse the issue list from the API"
    return 1
  fi

  scanned="$(wc -l < "$WORK/issues.tsv" | tr -d ' ')"
  POLL_SCANNED="$scanned"

  while IFS=$'\x1f' read -r num updated created title author state labels assignees milestone url; do
    [[ -n "$num" ]] || continue
    process_issue "$baseline" "$report_existing" "$num" "$updated" "$created" \
      "$title" "$author" "$state" "$labels" "$assignees" "$milestone" "$url" \
      || POLL_HAD_ERROR=1
  done < "$WORK/issues.tsv"

  # Advance last_poll only when every issue was processed; partial failures
  # are retried next poll (already-stored issues dedupe by updated_at/ID).
  local advance=1
  if (( baseline == 0 && POLL_HAD_ERROR != 0 )); then
    advance=0
  fi

  if ! jq --arg now "$now" --argjson advance "$advance" --slurpfile u "$WORK/updates.jsonl" '
      .issues += ($u | map({key: (.number | tostring), value: (. | del(.number))}) | from_entries)
      | (if $advance == 1 then .last_poll = $now else . end)
    ' "$STATE_FILE" > "$WORK/state.new"; then
    warn "failed to update state file: $STATE_FILE"
    return 1
  fi
  mv -- "$WORK/state.new" "$STATE_FILE"

  return 0
}

poll_once() {
  local report_existing="${1:-0}" rc=0

  POLL_SCANNED=0
  POLL_EVENTS=0
  POLL_HAD_ERROR=0

  if ! acquire_lock; then
    warn "another poll is already running (lock: $LOCK_FILE)"
    return 1
  fi
  if ! poll_body "$report_existing"; then
    rc=1
  fi
  release_lock
  return "$rc"
}

# ---------------------------------------------------------------------------
# Commands
# ---------------------------------------------------------------------------

cmd_check() {
  local report_existing="${1:-0}"
  if poll_once "$report_existing"; then
    log "check complete — repo $REPO, scanned $POLL_SCANNED issue(s), emitted $POLL_EVENTS event(s)"
    return 0
  fi
  warn "check failed — see the warnings above"
  return 1
}

cmd_loop() {
  local report_existing="${1:-0}"
  ensure_state_dir
  printf '%s\n' "$$" > "$PID_FILE"
  trap remove_pid EXIT
  trap 'exit 0' INT TERM

  log "watcher started (pid $$, repo $REPO, api $GITEA_API, interval ${INTERVAL}s)"

  while :; do
    if poll_once "$report_existing"; then
      if [[ "${GITEA_BOTS_VERBOSE:-0}" == "1" ]]; then
        log "poll ok — scanned $POLL_SCANNED issue(s), emitted $POLL_EVENTS event(s)"
      fi
    else
      warn "poll failed — retrying in ${INTERVAL}s"
    fi
    sleep "$INTERVAL" &
    wait $! 2>/dev/null || true
  done
}

# Guard against PID reuse: only treat a live PID as the watcher when its
# command line references this script. Reads one specific /proc entry — it
# never scans all processes.
pid_is_watcher() {
  local pid="${1:-}"
  [[ -n "$pid" ]] || return 1
  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  if [[ -r "/proc/$pid/cmdline" ]]; then
    local cmdline=""
    cmdline="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)" || return 1
    [[ "$cmdline" == *watch-issues.sh* ]]
    return $?
  fi
  return 0
}

cmd_start() {
  local report_existing="${1:-0}" pid=""
  ensure_state_dir
  mkdir -p -- "$(dirname -- "$LOG_FILE")"

  if [[ -f "$PID_FILE" ]]; then
    pid="$(<"$PID_FILE")"
  fi
  if pid_is_watcher "$pid"; then
    die "watcher already running (pid $pid) — run '$SCRIPT_PATH stop' first"
  fi
  rm -f -- "$PID_FILE"

  local -a args=(loop --interval "$INTERVAL")
  if (( report_existing == 1 )); then
    args+=(--report-existing)
  fi

  nohup bash "$SCRIPT_PATH" "${args[@]}" < /dev/null >> "$LOG_FILE" 2>&1 &
  local new_pid=$!
  printf '%s\n' "$new_pid" > "$PID_FILE"

  log "watcher started (pid $new_pid, interval ${INTERVAL}s) — log: $LOG_FILE"
}

cmd_status() {
  local pid="" running=0
  if [[ -f "$PID_FILE" ]]; then
    pid="$(<"$PID_FILE")"
  fi
  if pid_is_watcher "$pid"; then
    running=1
  fi

  if (( running == 1 )); then
    echo "watcher: running (pid $pid)"
  else
    echo "watcher: stopped"
    if [[ -n "$pid" ]]; then
      echo "  stale pid file: $PID_FILE (run 'stop' to clear)"
    fi
  fi

  echo "state: $STATE_FILE"
  if [[ -f "$STATE_FILE" ]]; then
    jq -r '"repo: \(.repo // "?")\napi: \(.api // "?")\nlast_poll: \(.last_poll // "never (baseline pending)")\ntracked issues: \(.issues | length)"' "$STATE_FILE" 2>/dev/null || echo "  (state file unreadable)"
  else
    echo "  (not initialized — first poll will baseline)"
  fi
  echo "log: $LOG_FILE"
}

cmd_stop() {
  local pid=""
  if [[ -f "$PID_FILE" ]]; then
    pid="$(<"$PID_FILE")"
  fi
  if [[ -z "$pid" ]]; then
    echo "watcher: not running (no pid file)"
    rm -f -- "$PID_FILE"
    return 0
  fi
  if ! pid_is_watcher "$pid"; then
    echo "watcher: not running (stale pid $pid)"
    rm -f -- "$PID_FILE"
    return 0
  fi

  kill -TERM "$pid" 2>/dev/null || true
  local i
  for i in $(seq 1 50); do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  if kill -0 "$pid" 2>/dev/null; then
    warn "watcher (pid $pid) ignored SIGTERM — sending SIGKILL"
    kill -KILL "$pid" 2>/dev/null || true
  fi
  rm -f -- "$PID_FILE"
  echo "watcher: stopped (pid $pid)"
}

cmd_reset() {
  ensure_state_dir
  if ! acquire_lock; then
    warn "a poll is in progress — stop the watcher or retry in a moment"
    return 1
  fi
  if [[ -f "$STATE_FILE" ]]; then
    local backup
    backup="$STATE_FILE.bak.$(date -u +%Y%m%dT%H%M%SZ)"
    mv -- "$STATE_FILE" "$backup"
    release_lock
    echo "state cleared — next poll re-baselines (backup: $backup)"
  else
    release_lock
    echo "state already clear — next poll will baseline"
  fi
}

remove_pid() {
  local pid=""
  if [[ -f "$PID_FILE" ]]; then
    pid="$(<"$PID_FILE")"
  fi
  if [[ "$pid" == "$$" ]]; then
    rm -f -- "$PID_FILE"
  fi
}

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

main() {
  local cmd="${1:-}"
  if (( $# > 0 )); then
    shift
  fi

  INTERVAL="$DEFAULT_INTERVAL"
  local report_existing=0

  while (( $# > 0 )); do
    case "$1" in
      -i | --interval)
        if (( $# < 2 )); then
          die "--interval requires a value"
        fi
        INTERVAL="$2"
        shift 2
        ;;
      --report-existing)
        report_existing=1
        shift
        ;;
      -h | --help)
        usage
        return 0
        ;;
      *)
        die "unknown option: $1 (try --help)"
        ;;
    esac
  done

  case "$cmd" in
    "" | help | -h | --help)
      usage
      return 0
      ;;
  esac

  require_tools

  case "$cmd" in
    status)
      cmd_status
      return 0
      ;;
    stop)
      cmd_stop
      return 0
      ;;
    reset)
      cmd_reset
      return 0
      ;;
  esac

  if ! [[ "$INTERVAL" =~ ^[0-9]+$ ]] || (( INTERVAL < 5 )); then
    die "interval must be an integer >= 5 seconds (got: $INTERVAL)"
  fi

  cd -- "$REPO_ROOT" || die "cannot change directory to repo root: $REPO_ROOT"
  load_env
  derive_remote
  ensure_state_dir
  resolve_self

  case "$cmd" in
    check | once)
      cmd_check "$report_existing"
      ;;
    loop | run)
      cmd_loop "$report_existing"
      ;;
    start)
      cmd_start "$report_existing"
      ;;
    *)
      die "unknown command: $cmd (try --help)"
      ;;
  esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
