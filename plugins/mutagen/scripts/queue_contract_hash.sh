#!/usr/bin/env bash
# Compute a stable fingerprint for the executable contract in slices/queue.json.
#
# Usage:
#   queue_contract_hash.sh [queue_path]

set -euo pipefail

QUEUE_PATH="${1:-slices/queue.json}"
QUEUE_CONTRACT_HASH_BASIS="execution_contract_v1"

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

hash_sha1() {
  if command -v sha1sum >/dev/null 2>&1; then
    sha1sum | awk '{print $1}'
    return 0
  fi

  if command -v sha1sum.exe >/dev/null 2>&1; then
    sha1sum.exe | awk '{print $1}'
    return 0
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 1 | awk '{print $1}'
    return 0
  fi

  if command -v shasum.exe >/dev/null 2>&1; then
    shasum.exe -a 1 | awk '{print $1}'
    return 0
  fi

  return 1
}

emit_error() {
  local reason="$1"
  local message="$2"

  "$JQ_BIN" -n \
    --arg reason "$reason" \
    --arg message "$message" \
    --arg queue "$QUEUE_PATH" \
    '{
      ok: false,
      reason: $reason,
      message: $message,
      queue: $queue
    }'
  exit 1
}

JQ_BIN="$(resolve_jq)" || {
  printf '{"ok":false,"reason":"tooling_failure","message":"jq not found on PATH"}\n'
  exit 1
}

if [[ "$QUEUE_PATH" != /* ]]; then
  QUEUE_PATH="$(pwd)/$QUEUE_PATH"
fi

if [[ ! -f "$QUEUE_PATH" ]]; then
  emit_error "queue_missing" "queue file not found"
fi

set +e
CONTRACT_JSON="$(
  "$JQ_BIN" -cS '
    {
      version,
      generated_at,
      generated_by,
      pipeline_mode,
      planning_advisories: (
        (.planning_advisories // [])
        | map({
            id,
            severity,
            summary,
            decision,
            user_response_required,
            references: (.references // []),
            affects_slices: (.affects_slices // [])
          })
      ),
      slices: (
        (.slices // [])
        | map({
            id,
            title,
            phase,
            author_agent,
            layer,
            bounded_context,
            target_loc,
            objective,
            context_to_update,
            implementation_details: (.implementation_details // []),
            review_required,
            depends_on: (.depends_on // []),
            adjacent_scope_allowed: (.adjacent_scope_allowed // []),
            write_set: (.write_set // []),
            traces_to: {
              prd: (.traces_to.prd // []),
              adr: (.traces_to.adr // []),
              ddd: (.traces_to.ddd // []),
              isc: (.traces_to.isc // []),
              dsd: (.traces_to.dsd // [])
            },
            verification_steps: {
              acceptance: (.verification_steps.acceptance // ""),
              isc_detection: (.verification_steps.isc_detection // ""),
              dsd_conformance: (.verification_steps.dsd_conformance // "")
            },
            human_check_needed: {
              required: (.human_check_needed.required // false),
              reason: (.human_check_needed.reason // "")
            }
          })
      )
    }
  ' "$QUEUE_PATH" 2>&1
)"
CONTRACT_STATUS=$?
set -e

if [[ $CONTRACT_STATUS -ne 0 ]]; then
  emit_error "queue_contract_invalid" "$CONTRACT_JSON"
fi

set +e
CONTRACT_HASH="$(printf '%s' "$CONTRACT_JSON" | hash_sha1)"
CONTRACT_HASH_STATUS=$?
set -e

if [[ $CONTRACT_HASH_STATUS -ne 0 || -z "$CONTRACT_HASH" ]]; then
  emit_error "tooling_failure" "sha1 hashing tool not found on PATH"
fi

"$JQ_BIN" -n \
  --arg queue "$QUEUE_PATH" \
  --arg basis "$QUEUE_CONTRACT_HASH_BASIS" \
  --arg algorithm "sha1" \
  --arg hash "$CONTRACT_HASH" \
  '{
    ok: true,
    queue: $queue,
    basis: $basis,
    algorithm: $algorithm,
    hash: $hash
  }'
