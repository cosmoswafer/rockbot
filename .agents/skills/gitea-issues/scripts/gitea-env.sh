#!/usr/bin/env bash
# gitea-env.sh — one line of setup for the curl recipes in this skill.
#
#   source .agents/skills/gitea-issues/scripts/gitea-env.sh
#
# Every shell call starts fresh, so sourcing this file inside the same call as
# the curl replaces the whole derivation block: it reads .env silently, derives
# GITEA_API and GITEA_OWNER_REPO from the git remote, defines the request
# helpers below, and prints one sanity line. The token is never echoed.
#
# Overrides (export before sourcing, or set in .env):
#   GITEA_REMOTE_NAME   git remote to read          (default: origin)
#   GITEA_OWNER_REPO    skip derivation and use this owner/repo
#   GITEA_API           skip derivation and use this API base
#   GITEA_ENV_FILE      read the token from this file instead of .env
#   GITEA_QUIET=1       do not print the sanity line
#   GITEA_STRICT=1      return non-zero when the token or remote is unusable
#
# Needs: git, curl. jq is optional (the helpers do not require it).

# --- keep the token out of any trace log -----------------------------------
_gitea_saved_xtrace=0
case "$-" in
*x*) _gitea_saved_xtrace=1 ;;
esac
set +x

# --- token -------------------------------------------------------------------
if [ -n "${GITEA_ENV_FILE:-}" ] && [ -f "$GITEA_ENV_FILE" ]; then
  # shellcheck disable=SC1090
  source "$GITEA_ENV_FILE" >/dev/null 2>&1
else
  [ -f ./.env ] && source ./.env >/dev/null 2>&1
  _gitea_root=$(git rev-parse --show-toplevel 2>/dev/null)
  [ -n "$_gitea_root" ] && [ -f "$_gitea_root/.env" ] \
    && source "$_gitea_root/.env" >/dev/null 2>&1
fi

# --- remote -> API base + owner/repo ----------------------------------------
_gitea_remote_name="${GITEA_REMOTE_NAME:-origin}"
if [ -z "${GITEA_API:-}" ] || [ -z "${GITEA_OWNER_REPO:-}" ]; then
  GITEA_REMOTE_URL=$(git remote get-url "$_gitea_remote_name" 2>/dev/null)
  GITEA_REMOTE_URL="${GITEA_REMOTE_URL%.git}"

  if [ -z "${GITEA_OWNER_REPO:-}" ]; then
    case "$GITEA_REMOTE_URL" in
      *://*)
        _bare=${GITEA_REMOTE_URL#*://}
        _bare=${_bare#*@}
        GITEA_HOST=${_bare%%/*}
        GITEA_OWNER_REPO=${_bare#*/}
        ;;
      *:*)
        _bare=${GITEA_REMOTE_URL#*@}
        GITEA_HOST=${_bare%%:*}
        GITEA_OWNER_REPO=${_bare#*:}
        ;;
      *)
        GITEA_HOST=""
        GITEA_OWNER_REPO=""
        ;;
    esac
  fi

  if [ -z "${GITEA_API:-}" ]; then
    GITEA_HOST=${GITEA_HOST%%:*}  # the SSH port belongs to git, not the API
    GITEA_API="https://${GITEA_HOST}/api/v1"
  fi
fi

# --- helpers -----------------------------------------------------------------
gitea_url() { printf '%s/repos/%s%s' "$GITEA_API" "$GITEA_OWNER_REPO" "$1"; }

gitea_call() { # gitea_call METHOD SUFFIX [JSON]
  local method="$1" suffix="$2" json="${3:-}" _xtrace=0
  case "$-" in *x*) _xtrace=1 ;; esac
  set +x
  local -a args=(-sS -X "$method"
    -H "Authorization: token ${GITEA_TOKEN:-}"
    -H "Accept: application/json")
  [ -n "$json" ] && args+=(-H "Content-Type: application/json" --data "$json")
  curl "${args[@]}" "$(gitea_url "$suffix")${GITEA_QUERY:+?$GITEA_QUERY}"
  local code=$?
  [ "$_xtrace" = 1 ] && set -x
  return $code
}

gitea_get()  { gitea_call GET    "$1"; }
gitea_post() { gitea_call POST   "$1" "$2"; }
gitea_put()  { gitea_call PUT    "$1" "$2"; }
gitea_patch(){ gitea_call PATCH  "$1" "$2"; }
gitea_del()  { gitea_call DELETE "$1" "${2:-}"; }

gitea_code() { # gitea_code METHOD SUFFIX [JSON] -> HTTP status only
  local method="$1" suffix="$2" json="${3:-}" _xtrace=0
  case "$-" in *x*) _xtrace=1 ;; esac
  set +x
  local -a args=(-sS -o /dev/null -w '%{http_code}' -X "$method"
    -H "Authorization: token ${GITEA_TOKEN:-}")
  [ -n "$json" ] && args+=(-H "Content-Type: application/json" --data "$json")
  curl "${args[@]}" "$(gitea_url "$suffix")${GITEA_QUERY:+?$GITEA_QUERY}"
  local code=$?
  [ "$_xtrace" = 1 ] && set -x
  return $code
}

gitea_selfcheck() {
  echo "API=$GITEA_API  REPO=$GITEA_OWNER_REPO  REMOTE=$_gitea_remote_name"
  if [ -z "${GITEA_TOKEN:-}" ]; then
    echo "FAIL  GITEA_TOKEN is empty — add it to .env (it is never printed)"
  elif ! command -v curl >/dev/null 2>&1; then
    echo "FAIL  curl is missing"
  elif ! command -v git >/dev/null 2>&1; then
    echo "FAIL  git is missing — GITEA_API/GITEA_OWNER_REPO cannot be derived"
  else
    local code
    code=$(gitea_code GET "")
    case "$code" in
      200) echo "PASS  repo access: HTTP 200" ;;
      401) echo "FAIL  HTTP 401 — bad token, or 'Bearer' instead of 'token'" ;;
      403) echo "FAIL  HTTP 403 — token has no access to this repository" ;;
      404) echo "FAIL  HTTP 404 — wrong host or owner/repo (case-sensitive)" ;;
      000) echo "FAIL  no response — DNS, TLS or network reaching the host" ;;
      *)   echo "FAIL  repo access: HTTP $code" ;;
    esac
  fi
}

gitea_login() { # the token user, with the recent-issue fallback
  local login
  login=$(curl -sS -H "Authorization: token ${GITEA_TOKEN:-}" "$GITEA_API/user" \
    | { command -v jq >/dev/null 2>&1 && jq -r '.login // empty'; } || true)
  if [ -z "$login" ]; then
    login=$(GITEA_QUERY="state=all&limit=1" gitea_get "/issues" \
      | { command -v jq >/dev/null 2>&1 && jq -r '.[0].user.login // empty'; } \
      || true)
  fi
  printf '%s\n' "$login"
}

# --- report ------------------------------------------------------------------
if [ "${GITEA_QUIET:-}" != "1" ]; then
  echo "GITEA_API=$GITEA_API"
  echo "GITEA_OWNER_REPO=$GITEA_OWNER_REPO"
  if [ -n "${GITEA_TOKEN:-}" ]; then
    echo "GITEA_TOKEN=set (${#GITEA_TOKEN} characters, value hidden)"
  else
    echo "GITEA_TOKEN=empty — add GITEA_TOKEN=<token> to .env"
  fi
fi

_gitea_status=0
if [ -z "${GITEA_TOKEN:-}" ]; then _gitea_status=1; fi
[ -z "${GITEA_OWNER_REPO:-}" ] && _gitea_status=1

if [ "$_gitea_saved_xtrace" = 1 ]; then
  set -x
fi
unset _gitea_saved_xtrace _gitea_root _gitea_host 2>/dev/null || true

if [ "${GITEA_STRICT:-}" = "1" ] && [ "$_gitea_status" != 0 ]; then
  return 1
fi
return 0
