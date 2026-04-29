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

# Default CLAUDE_BIN to the packaged non-interactive wrapper so any agent.sh
# invocation reached from a Rust-harness dispatch (or a dev console run) does
# not stall on a permission prompt. Caller can override by exporting CLAUDE_BIN
# explicitly before invoking the harness.
if [[ -z "${CLAUDE_BIN:-}" && -x "$PLUGIN_ROOT/bin/claude-harness.sh" ]]; then
  export CLAUDE_BIN="$PLUGIN_ROOT/bin/claude-harness.sh"
fi

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

  printf 'mutagen harness unavailable: no packaged mutagen-harness binary and no harness/Cargo.toml fallback found\n' >&2
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  if HARNESS_BIN="$(resolve_path_harness_binary)"; then
    exec "$HARNESS_BIN" "$@"
  fi

  printf 'mutagen harness unavailable: cargo not found on PATH and no packaged mutagen-harness binary found\n' >&2
  exit 1
}

exec "$CARGO_BIN" run --quiet --manifest-path "$HARNESS_MANIFEST" -- "$@"
