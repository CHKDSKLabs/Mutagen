#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF' >&2
Usage:
  harness_runtime.sh <harness-subcommand> [args...]

Resolution order:
  1. MUTAGEN_HARNESS_BIN, if set and executable
  2. plugins/mutagen/bin/mutagen-harness(.exe), if packaged
  3. cargo run against a source checkout harness/Cargo.toml
  4. mutagen-harness on PATH
EOF
  exit 1
}

display_path() {
  printf '%s\n' "$1" | sed 's#\\#/#g'
}

resolve_cargo() {
  if command -v cargo >/dev/null 2>&1; then
    command -v cargo
    return 0
  fi

  if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    printf '%s\n' "$HOME/.cargo/bin/cargo"
    return 0
  fi

  if command -v cargo.exe >/dev/null 2>&1; then
    command -v cargo.exe
    return 0
  fi

  return 1
}

resolve_harness_binary() {
  local plugin_root="$1"

  if [[ -n "${MUTAGEN_HARNESS_BIN:-}" ]]; then
    if [[ -x "$MUTAGEN_HARNESS_BIN" ]]; then
      printf '%s\n' "$MUTAGEN_HARNESS_BIN"
      return 0
    fi

    printf 'MUTAGEN_HARNESS_BIN is set but not executable: %s\n' "$(display_path "$MUTAGEN_HARNESS_BIN")" >&2
    return 2
  fi

  if [[ -x "$plugin_root/bin/mutagen-harness" ]]; then
    printf '%s\n' "$plugin_root/bin/mutagen-harness"
    return 0
  fi

  if [[ -x "$plugin_root/bin/mutagen-harness.exe" ]]; then
    printf '%s\n' "$plugin_root/bin/mutagen-harness.exe"
    return 0
  fi

  return 1
}

resolve_path_harness_binary() {
  if command -v mutagen-harness >/dev/null 2>&1; then
    command -v mutagen-harness
    return 0
  fi

  return 1
}

resolve_harness_manifest() {
  local plugin_root="$1"
  local repo_root

  if [[ -n "${MUTAGEN_HARNESS_MANIFEST:-}" && -f "$MUTAGEN_HARNESS_MANIFEST" ]]; then
    printf '%s\n' "$MUTAGEN_HARNESS_MANIFEST"
    return 0
  fi

  if [[ -f "$plugin_root/harness/Cargo.toml" ]]; then
    printf '%s\n' "$plugin_root/harness/Cargo.toml"
    return 0
  fi

  repo_root="$(cd "$plugin_root/../.." && pwd)"
  if [[ -f "$repo_root/harness/Cargo.toml" ]]; then
    printf '%s\n' "$repo_root/harness/Cargo.toml"
    return 0
  fi

  return 1
}

if [[ $# -lt 1 ]]; then
  usage
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# CodexPro contract — operator diagnostics. Opt-in via
# MUTAGEN_HARNESS_DIAGNOSTICS=1 so we don't drown stderr on normal runs (the
# harness produces structured JSON on stdout that callers parse; stderr should
# stay quiet by default). When enabled, print the resolved invocation surface
# plus the queue contract hash recorded by the last validator run.
if [[ "${MUTAGEN_HARNESS_DIAGNOSTICS:-0}" == "1" ]]; then
  diag_host="(unspecified)"
  for ((i=1; i<=$#; i++)); do
    if [[ "${!i}" == "--host" ]]; then
      next=$((i+1))
      diag_host="${!next:-(missing-value)}"
      break
    fi
  done
  diag_subcommand="${1:-(none)}"
  diag_queue_validation=".mutagen/state/queue-validation.json"
  diag_hash="(no queue-validation payload)"
  if [[ -f "$diag_queue_validation" ]]; then
    if command -v jq >/dev/null 2>&1; then
      diag_hash="$(jq -r '.queue_contract_hash // "(field absent)"' "$diag_queue_validation" 2>/dev/null || echo "(jq parse failed)")"
    else
      diag_hash="(jq unavailable — cannot extract)"
    fi
  fi
  printf 'mutagen-harness: host=%s · subcommand=%s · args=[%s] · queue_contract_hash=%s\n' \
    "$diag_host" "$diag_subcommand" "$*" "$diag_hash" >&2
fi

# Default CLAUDE_BIN to the packaged non-interactive wrapper so any agent.sh
# invocation reached from a Rust-harness dispatch (or a dev console run) does
# not stall on a permission prompt. Caller can override by exporting CLAUDE_BIN
# explicitly before invoking the harness.
if [[ -z "${CLAUDE_BIN:-}" && -x "$PLUGIN_ROOT/bin/claude-harness.sh" ]]; then
  export CLAUDE_BIN="$PLUGIN_ROOT/bin/claude-harness.sh"
fi

try_fetch_harness_binary() {
  local plugin_root="$1"
  local fetch_script="$plugin_root/scripts/fetch_harness_binary.sh"

  [[ -f "$fetch_script" ]] || return 1
  [[ "${MUTAGEN_NO_AUTOFETCH:-0}" -eq 1 ]] && return 1

  printf 'mutagen-harness binary missing — attempting auto-fetch from GitHub Release...\n' >&2
  if bash "$fetch_script" --quiet >&2; then
    return 0
  fi
  return 1
}

set +e
HARNESS_BIN="$(resolve_harness_binary "$PLUGIN_ROOT")"
HARNESS_BIN_STATUS=$?
set -e

case "$HARNESS_BIN_STATUS" in
  0)
    exec "$HARNESS_BIN" "$@"
    ;;
  2)
    exit 1
    ;;
esac

if ! HARNESS_MANIFEST="$(resolve_harness_manifest "$PLUGIN_ROOT")"; then
  if HARNESS_BIN="$(resolve_path_harness_binary)"; then
    exec "$HARNESS_BIN" "$@"
  fi

  if try_fetch_harness_binary "$PLUGIN_ROOT"; then
    if HARNESS_BIN="$(resolve_harness_binary "$PLUGIN_ROOT")"; then
      exec "$HARNESS_BIN" "$@"
    fi
  fi

  printf 'mutagen harness unavailable: no packaged mutagen-harness binary, no harness/Cargo.toml fallback, and auto-fetch could not provision one.\n' >&2
  printf 'Try one of:\n' >&2
  printf '  - bash %s/scripts/fetch_harness_binary.sh   # download from GitHub Release\n' "$(display_path "$PLUGIN_ROOT")" >&2
  printf '  - export MUTAGEN_HARNESS_BIN=/path/to/mutagen-harness   # for offline / air-gapped installs\n' >&2
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  if HARNESS_BIN="$(resolve_path_harness_binary)"; then
    exec "$HARNESS_BIN" "$@"
  fi

  if try_fetch_harness_binary "$PLUGIN_ROOT"; then
    if HARNESS_BIN="$(resolve_harness_binary "$PLUGIN_ROOT")"; then
      exec "$HARNESS_BIN" "$@"
    fi
  fi

  printf 'mutagen harness unavailable: cargo not found on PATH, no packaged mutagen-harness binary, and auto-fetch could not provision one.\n' >&2
  printf 'Try one of:\n' >&2
  printf '  - bash %s/scripts/fetch_harness_binary.sh   # download from GitHub Release (no Rust toolchain needed)\n' "$(display_path "$PLUGIN_ROOT")" >&2
  printf '  - export MUTAGEN_HARNESS_BIN=/path/to/mutagen-harness   # for offline / air-gapped installs\n' >&2
  exit 1
}

exec "$CARGO_BIN" run --quiet --bin mutagen-harness --manifest-path "$HARNESS_MANIFEST" -- "$@"
