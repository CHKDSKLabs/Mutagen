#!/usr/bin/env bash
# Validate a canonical slice queue using the Rust harness validator.
#
# Usage:
#   validate_queue.sh [queue_path]
#
# Exit codes:
#   0 = queue valid (warnings allowed)
#   1 = validator unavailable or runtime failure
#   2 = queue parsed but failed validation

set -euo pipefail

QUEUE_PATH="${1:-slices/queue.json}"

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

resolve_cargo() {
  if command -v cargo >/dev/null 2>&1; then
    command -v cargo
    return 0
  fi

  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    printf '%s\n' "$HOME/.cargo/bin/cargo"
    return 0
  fi

  if command -v cargo.exe >/dev/null 2>&1; then
    command -v cargo.exe
    return 0
  fi

  return 1
}

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"error":"validator_unavailable","message":"jq not found on PATH"}\n'
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  "$JQ_BIN" -n \
    --arg queue "$QUEUE_PATH" \
    --arg manifest "$MANIFEST_PATH" \
    '{
      ok: false,
      error: "validator_unavailable",
      queue: $queue,
      message: ("mutagen harness manifest not found at " + $manifest)
    }'
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  "$JQ_BIN" -n \
    --arg queue "$QUEUE_PATH" \
    '{
      ok: false,
      error: "validator_unavailable",
      queue: $queue,
      message: "cargo not found on PATH"
    }'
  exit 1
}

if [[ "$QUEUE_PATH" != /* ]]; then
  QUEUE_PATH="$(pwd)/$QUEUE_PATH"
fi

QUEUE_CONTRACT_HASH_JSON=""
set +e
queue_contract_hash_output="$(bash "$SCRIPT_DIR/queue_contract_hash.sh" "$QUEUE_PATH" 2>/dev/null)"
queue_contract_hash_status=$?
set -e

if [[ $queue_contract_hash_status -eq 0 ]] && printf '%s' "$queue_contract_hash_output" | "$JQ_BIN" empty >/dev/null 2>&1; then
  QUEUE_CONTRACT_HASH_JSON="$queue_contract_hash_output"
fi

augment_report() {
  local report_json="$1"

  if [[ -z "$QUEUE_CONTRACT_HASH_JSON" ]]; then
    printf '%s\n' "$report_json"
    return 0
  fi

  printf '%s' "$report_json" | "$JQ_BIN" -c \
    --arg queue "$QUEUE_PATH" \
    --argjson queue_contract "$QUEUE_CONTRACT_HASH_JSON" \
    '. + {
      queue: $queue,
      queue_contract_hash: ($queue_contract.hash // null),
      queue_contract_hash_basis: ($queue_contract.basis // null),
      queue_contract_hash_algorithm: ($queue_contract.algorithm // null)
    }'
}

set +e
VALIDATOR_OUTPUT="$(
  "$CARGO_BIN" run --quiet --manifest-path "$MANIFEST_PATH" -- validate-queue --queue "$QUEUE_PATH" 2>&1
)"
VALIDATOR_STATUS=$?
set -e

if [[ $VALIDATOR_STATUS -ne 0 ]]; then
  printf '%s' "$VALIDATOR_OUTPUT" | "$JQ_BIN" -Rs \
    --arg queue "$QUEUE_PATH" \
    '{
      ok: false,
      error: "validator_runtime_failure",
      queue: $queue,
      message: .
    }'
  exit 1
fi

REPORT_OK="$(
  printf '%s' "$VALIDATOR_OUTPUT" | "$JQ_BIN" -r '
    if has("ok") then
      (.ok | if . == true then "true" elif . == false then "false" else "" end)
    else
      ""
    end
  ' 2>/dev/null || true
)"

if [[ "$REPORT_OK" == "true" ]]; then
  augment_report "$VALIDATOR_OUTPUT"
  exit 0
fi

if [[ "$REPORT_OK" == "false" ]]; then
  augment_report "$VALIDATOR_OUTPUT"
  exit 2
fi

printf '%s' "$VALIDATOR_OUTPUT" | "$JQ_BIN" -Rs \
  --arg queue "$QUEUE_PATH" \
  '{
    ok: false,
    error: "validator_runtime_failure",
    queue: $queue,
    message: ("validator returned non-JSON output: " + .)
  }'
exit 1
