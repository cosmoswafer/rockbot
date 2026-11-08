#!/usr/bin/env bash
# shellcheck shell=bash
#
# opencode-task-status.sh — repo-local fallback for OpenCode background job
# observability (upstream: Atom/openrouter-image-runner#127).
#
# The OpenCode server already tracks background shell jobs via /api/shell and
# /api/experimental/job, but the agent tool catalog does not expose
# status/output/kill/wait, and /api/shell defaults to the service cwd when
# location[directory] is omitted. This helper ALWAYS passes location[directory]
# (and sessionID where supported) so a lookup never silently queries the wrong
# directory.
#
# Read-only by default. `kill` is explicit and destructive.
#
# Usage:
#   opencode-task-status.sh [--directory DIR] [--session ID] list [--kind K] [--status S]
#   opencode-task-status.sh [--directory DIR] status <shell-id>
#   opencode-task-status.sh [--directory DIR] output <shell-id> [--cursor N] [--tail LINES]
#   opencode-task-status.sh [--directory DIR] kill <shell-id> --yes
#   opencode-task-status.sh [--directory DIR] wait <shell-id> --timeout SECONDS [--interval S] [--cursor N]
#
# Environment:
#   OPENCODE_TASK_DIR        default directory (fallback: $PWD)
#   OPENCODE_SESSION_ID      default session id for /api/experimental/job
#   OPENCODE_TASK_API_CMD    override API transport for tests/mocks, e.g.
#                            "bash ./mock-opencode api" (called as: <cmd> get|delete <path>)
#
# Never prints secrets. Never uses /tmp for artifacts.

set -euo pipefail

die() { printf 'opencode-task-status: %s\n' "$*" >&2; exit 1; }
warn() { printf 'opencode-task-status: WARN: %s\n' "$*" >&2; }
have() { command -v "$1" >/dev/null 2>&1; }

usage() {
  cat >&2 <<'EOF'
Usage:
  opencode-task-status.sh [global opts] <command> [args]

Global options:
  --directory DIR   Project/session directory for location[directory]. Default:
                    $OPENCODE_TASK_DIR, else $PWD. Always passed to /api/shell.
  --session ID      Session ID for /api/experimental/job. Default:
                    $OPENCODE_SESSION_ID (if set).
  -h, --help        Show this help.

Commands:
  list [--kind shell|subagent|image|all] [--status S]
                    Query /api/shell with location[directory]. With --session and
                    a non-shell kind scope, also queries /api/experimental/job.
  status <id>       GET /api/shell/{id} with location[directory].
  output <id> [--cursor N] [--tail LINES]
                    GET /api/shell/{id}/output (resumable by absolute byte cursor).
                    --tail keeps only the last N lines client-side.
  kill <id> --yes   DELETE /api/shell/{id}. Destructive/explicit; requires --yes.
  wait <id> --timeout SECONDS [--interval S] [--cursor N]
                    Poll until exited/timeout/killed or local timeout. Unbounded
                    waits are refused.

Diagnostics name the queried directory and session. A /api/shell call without
location[directory] is never made.
Upstream: OpenCode issue #127 (agent-facing task primitives + sane defaults).
EOF
  exit 2
}

urlencode() {
  local s="${1-}"
  if have jq; then
    printf '%s' "$s" | jq -sRr @uri
    return
  fi
  if have python3; then
    python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$s"
    return
  fi
  local out="" c i
  for ((i=0; i<${#s}; i++)); do
    c="${s:i:1}"
    case "$c" in
      [a-zA-Z0-9.~_/-]) out+="$c" ;;
      *) printf -v out '%s%%%02X' "$out" "'$c" ;;
    esac
  done
  printf '%s' "$out"
}

DIR="${OPENCODE_TASK_DIR:-$PWD}"
SESSION="${OPENCODE_SESSION_ID:-}"
if [[ -n "${OPENCODE_TASK_API_CMD:-}" ]]; then
  # shellcheck disable=SC2206
  API_CMD=(${OPENCODE_TASK_API_CMD})
else
  API_CMD=(opencode api)
fi

while (($#)); do
  case "${1:-}" in
    --directory) (($# >= 2)) || usage; DIR="$2"; shift 2 ;;
    --session) (($# >= 2)) || usage; SESSION="$2"; shift 2 ;;
    -h|--help) usage ;;
    --) shift; break ;;
    -*) die "unknown option: $1" ;;
    *) break ;;
  esac
done

(($# >= 1)) || usage
CMD="$1"; shift

have jq || die "jq is required"
if [[ -z "${OPENCODE_TASK_API_CMD:-}" ]]; then
  have opencode || die "opencode CLI not found on PATH (set OPENCODE_TASK_API_CMD for tests/mocks)"
fi

if have realpath; then
  DIR="$(realpath -m -- "$DIR" 2>/dev/null || printf '%s' "$DIR")"
fi
DIR_ENC="$(urlencode "$DIR")"

api_call() {
  local method="$1" path="$2"
  "${API_CMD[@]}" "$method" "$path"
}
api_get() { api_call get "$1"; }
api_delete() { api_call delete "$1"; }

shell_list_path() { printf '/api/shell?location[directory]=%s' "$DIR_ENC"; }
shell_item_path() { printf '/api/shell/%s?location[directory]=%s' "$(urlencode "$1")" "$DIR_ENC"; }
shell_output_path() { printf '/api/shell/%s/output?location[directory]=%s&cursor=%s' "$(urlencode "$1")" "$DIR_ENC" "$(urlencode "$2")"; }

queried() { printf 'queried location[directory]=%s%s' "$DIR" "${SESSION:+ sessionID=$SESSION}"; }

is_not_found() {
  printf '%s' "$1" | jq -e '._tag == "ShellNotFoundError" or .name == "NotFoundError" or .error == "not_found"' >/dev/null 2>&1
}

report_miss() {
  local id="$1" ctx="$2"
  die "$ctx: shell '$id' not found or not visible ($(queried)). /api/shell without location[directory] defaults to the service cwd and can falsely report missing jobs."
}

cmd_list() {
  local kind="all" status_filter=""
  while (($#)); do
    case "$1" in
      --kind) (($#>=2))||usage; kind="$2"; shift 2 ;;
      --status) (($#>=2))||usage; status_filter="$2"; shift 2 ;;
      *) die "list: unknown arg: $1" ;;
    esac
  done
  case "$kind" in shell|subagent|image|all) ;; *) die "list: --kind must be shell|subagent|image|all" ;; esac

  local shell_json
  if ! shell_json="$(api_get "$(shell_list_path)")"; then
    die "list: failed to query /api/shell ($(queried))"
  fi
  if [[ -n "$status_filter" ]]; then
    shell_json="$(printf '%s' "$shell_json" | jq --arg s "$status_filter" 'if type=="object" and has("data") then .data |= map(select(.status==$s)) | . else . end')"
  fi
  printf '%s\n' "$shell_json"

  if [[ -n "$SESSION" && ( "$kind" == "all" || "$kind" != "shell" ) ]]; then
    local job_path="/api/experimental/job?sessionID=$(urlencode "$SESSION")&descendants=true"
    [[ "$kind" != "all" ]] && job_path+="&kind=$(urlencode "$kind")"
    [[ -n "$status_filter" ]] && job_path+="&status=$(urlencode "$status_filter")"
    local jobs_json
    if jobs_json="$(api_get "$job_path" 2>/dev/null)"; then
      printf '%s\n' "$jobs_json"
    else
      warn "could not query $job_path; shell list above still used $(queried)"
    fi
  fi
}

cmd_status() {
  (($# >= 1)) || usage
  local id="$1" json
  if ! json="$(api_get "$(shell_item_path "$id")" 2>/dev/null)"; then
    report_miss "$id" "status"
  fi
  if is_not_found "$json"; then report_miss "$id" "status"; fi
  printf '%s\n' "$json"
}

cmd_output() {
  (($# >= 1)) || usage
  local id="$1"; shift
  local cursor="0" tail_lines=""
  while (($#)); do
    case "$1" in
      --cursor) (($#>=2))||usage; cursor="$2"; shift 2 ;;
      --tail) (($#>=2))||usage; tail_lines="$2"; shift 2 ;;
      [0-9]*) cursor="$1"; shift ;;
      *) die "output: unknown arg: $1" ;;
    esac
  done
  [[ "$cursor" =~ ^[0-9]+$ ]] || die "output: --cursor must be a non-negative integer"
  [[ -z "$tail_lines" || "$tail_lines" =~ ^[0-9]+$ ]] || die "output: --tail must be a non-negative integer"

  local json
  if ! json="$(api_get "$(shell_output_path "$id" "$cursor")" 2>/dev/null)"; then
    report_miss "$id" "output"
  fi
  if is_not_found "$json"; then report_miss "$id" "output"; fi
  if [[ -n "$tail_lines" ]]; then
    printf '%s' "$json" | jq --argjson n "$tail_lines" '
      .output = ((.output // "") | split("\n") | if length <= $n then . else .[-$n:] end | join("\n"))
    '
  else
    printf '%s\n' "$json"
  fi
}

cmd_kill() {
  (($# >= 1)) || usage
  local id="$1"; shift
  local confirmed=0
  while (($#)); do
    case "$1" in
      --yes) confirmed=1; shift ;;
      *) die "kill: unknown arg: $1 (kill is destructive; pass --yes)" ;;
    esac
  done
  (( confirmed == 1 )) || die "kill is destructive and terminates the process group via the API; re-run as: kill $id --yes ($(queried))"
  local json
  if ! json="$(api_delete "$(shell_item_path "$id")" 2>/dev/null)"; then
    report_miss "$id" "kill"
  fi
  printf '%s\n' "$json"
}

cmd_wait() {
  (($# >= 1)) || usage
  local id="$1"; shift
  local timeout_s="" interval_s="2" cursor="0"
  while (($#)); do
    case "$1" in
      --timeout) (($#>=2))||usage; timeout_s="$2"; shift 2 ;;
      --interval) (($#>=2))||usage; interval_s="$2"; shift 2 ;;
      --cursor) (($#>=2))||usage; cursor="$2"; shift 2 ;;
      *) die "wait: unknown arg: $1" ;;
    esac
  done
  [[ -n "$timeout_s" ]] || die "wait: --timeout SECONDS is required; unbounded wait is refused (use status/output, or upstream task.wait)"
  [[ "$timeout_s" =~ ^[0-9]+$ && "$interval_s" =~ ^[0-9]+$ && "$cursor" =~ ^[0-9]+$ ]] || die "wait: --timeout/--interval/--cursor must be non-negative integers"
  (( timeout_s > 0 )) || die "wait: --timeout must be > 0; unbounded wait is refused"

  local deadline=$(( $(date +%s) + timeout_s ))
  local status_json st exit_code out_json
  while :; do
    if ! status_json="$(api_get "$(shell_item_path "$id")" 2>/dev/null)"; then
      report_miss "$id" "wait"
    fi
    st="$(printf '%s' "$status_json" | jq -r '.status // .data.status // "unknown"' 2>/dev/null || printf 'unknown')"
    case "$st" in
      exited|timeout|killed|failed|done|completed)
        exit_code="$(printf '%s' "$status_json" | jq -r '.exitCode // .exit_code // .code // empty' 2>/dev/null || true)"
        if out_json="$(api_get "$(shell_output_path "$id" "$cursor")" 2>/dev/null)"; then
          jq -n --arg id "$id" --arg status "$st" --arg exit "${exit_code}" --argjson status_json "$status_json" --argjson output "$out_json" \
            '{id:$id, status:$status, exit:(if $exit=="" then null else ($exit|tonumber? // $exit) end), status_detail:$status_json, output_tail:$output}'
        else
          jq -n --arg id "$id" --arg status "$st" --arg exit "${exit_code}" --argjson status_json "$status_json" \
            '{id:$id, status:$status, exit:(if $exit=="" then null else ($exit|tonumber? // $exit) end), status_detail:$status_json, output_tail:null}'
        fi
        return 0
        ;;
    esac
    if (( $(date +%s) >= deadline )); then
      if out_json="$(api_get "$(shell_output_path "$id" "$cursor")" 2>/dev/null)"; then
        jq -n --arg id "$id" --argjson status_json "$status_json" --argjson output "$out_json" --argjson timeout "$timeout_s" \
          '{id:$id, status:"wait-timeout", waited_seconds:$timeout, status_detail:$status_json, output_tail:$output}'
      else
        jq -n --arg id "$id" --argjson status_json "$status_json" --argjson timeout "$timeout_s" \
          '{id:$id, status:"wait-timeout", waited_seconds:$timeout, status_detail:$status_json, output_tail:null}'
      fi
      return 124
    fi
    sleep "$interval_s"
  done
}

case "$CMD" in
  list) cmd_list "$@" ;;
  status) cmd_status "$@" ;;
  output) cmd_output "$@" ;;
  kill) cmd_kill "$@" ;;
  wait) cmd_wait "$@" ;;
  help|-h|--help) usage ;;
  *) die "unknown command: $CMD (try --help)" ;;
esac
