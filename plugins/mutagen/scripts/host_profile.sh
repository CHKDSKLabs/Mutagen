#!/usr/bin/env bash

set -euo pipefail

WORKFLOW_CONFIG_PATH=".claude/workflow.json"
HOST_KIND="codex"

usage() {
  cat <<'EOF' >&2
Usage:
  host_profile.sh [--workflow-config PATH] [--host HOST]
EOF
  exit 1
}

resolve_jq() {
  if command -v jq >/dev/null 2>&1; then
    command -v jq
    return 0
  fi

  if command -v jq.exe >/dev/null 2>&1; then
    command -v jq.exe
    return 0
  fi

  return 1
}


absolute_path() {
  local path="$1"

  if [[ "$path" == /* ]]; then
    printf '%s\n' "$path"
    return 0
  fi

  printf '%s/%s\n' "$(pwd)" "$path"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workflow-config)
      [[ $# -ge 2 ]] || usage
      WORKFLOW_CONFIG_PATH="$2"
      shift 2
      ;;
    --host)
      [[ $# -ge 2 ]] || usage
      HOST_KIND="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      usage
      ;;
  esac
done

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKFLOW_CONFIG_PATH="$(absolute_path "$WORKFLOW_CONFIG_PATH")"

set +e
PROFILE_OUTPUT="$(
  bash "$SCRIPT_DIR/harness_runtime.sh" \
    host-profile \
    --workflow-config "$WORKFLOW_CONFIG_PATH" \
    --host "$HOST_KIND" \
    2>&1
)"
PROFILE_STATUS=$?
set -e

if [[ $PROFILE_STATUS -ne 0 ]]; then
  printf '%s' "$PROFILE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "host_profile_runtime_failure",
      workflow_config: $workflow_config,
      host: $host,
      message: .
    }'
  exit 1
fi

if ! printf '%s' "$PROFILE_OUTPUT" | "$JQ_BIN" empty >/dev/null 2>&1; then
  printf '%s' "$PROFILE_OUTPUT" | "$JQ_BIN" -Rs \
    --arg workflow_config "$WORKFLOW_CONFIG_PATH" \
    --arg host "$HOST_KIND" \
    '{
      ok: false,
      reason: "host_profile_runtime_failure",
      workflow_config: $workflow_config,
      host: $host,
      message: ("host-profile returned non-JSON output: " + .)
    }'
  exit 1
fi

printf '%s\n' "$PROFILE_OUTPUT"
