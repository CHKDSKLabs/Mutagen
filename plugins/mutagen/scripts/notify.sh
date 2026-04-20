#!/usr/bin/env bash
# Pushover notifier for mutagen halts.
#
# Usage:
#   notify.sh <event> <title> <message>
#
# Events (one of):
#   escalation         — retry budget exhausted on a slice
#   structural_fail    — Karai returned a structural conformance failure
#   scope_violation    — Traag DENY blocked a mutation
#   retry_exhausted    — alias for escalation (kept for call-site clarity)
#   traag_deny         — alias for scope_violation
#   queue_clear        — the queue ran to completion (opt-in, low priority)
#   user_interrupt     — auto-advance paused because the user sent input
#
# Config precedence (first non-empty wins):
#   1. Env: PUSHOVER_USER_KEY, PUSHOVER_APP_TOKEN
#   2. .claude/workflow.json:
#        {
#          "notifications": {
#            "pushover": {
#              "enabled": true,
#              "user_key": "...",
#              "app_token": "...",
#              "quiet_events": ["queue_clear"]
#            }
#          }
#        }
#
# Fails silently on any error. A broken notifier must never block a pipeline halt.

set -u

event="${1:-}"
title="${2:-mutagen}"
message="${3:-(no message)}"

if [ -z "$event" ]; then
  exit 0
fi

cfg_user_key=""
cfg_app_token=""
cfg_enabled="false"
cfg_quiet=""

if [ -r .claude/workflow.json ] && command -v jq >/dev/null 2>&1; then
  cfg_user_key=$(jq -r '.notifications.pushover.user_key // ""' .claude/workflow.json 2>/dev/null || echo "")
  cfg_app_token=$(jq -r '.notifications.pushover.app_token // ""' .claude/workflow.json 2>/dev/null || echo "")
  cfg_enabled=$(jq -r '(.notifications.pushover.enabled // false) | tostring' .claude/workflow.json 2>/dev/null || echo "false")
  cfg_quiet=$(jq -r '(.notifications.pushover.quiet_events // []) | join(",")' .claude/workflow.json 2>/dev/null || echo "")
fi

user_key="${PUSHOVER_USER_KEY:-$cfg_user_key}"
app_token="${PUSHOVER_APP_TOKEN:-$cfg_app_token}"

if [ -z "$user_key" ] || [ -z "$app_token" ]; then
  # No creds, no notification. Not an error — plugin works without Pushover.
  exit 0
fi

# Honor an explicit enabled:false in workflow.json even if env vars are set,
# so the user has an unambiguous kill switch.
if [ "$cfg_enabled" = "false" ] && [ -z "${PUSHOVER_USER_KEY:-}" ] && [ -z "${PUSHOVER_APP_TOKEN:-}" ]; then
  # Creds came from the file and the file says disabled — respect that.
  exit 0
fi

case ",$cfg_quiet," in
  *",$event,"*) exit 0 ;;
esac

priority=0
case "$event" in
  escalation|structural_fail|scope_violation|retry_exhausted|traag_deny) priority=1 ;;
  queue_clear|user_interrupt) priority=0 ;;
esac

if ! command -v curl >/dev/null 2>&1; then
  # No curl, no send. Quiet failure by design.
  exit 0
fi

curl -s --max-time 10 \
  --form-string "token=$app_token" \
  --form-string "user=$user_key" \
  --form-string "title=$title" \
  --form-string "message=$message" \
  --form-string "priority=$priority" \
  https://api.pushover.net/1/messages.json >/dev/null 2>&1 || true

exit 0
