#!/usr/bin/env bash

set -euo pipefail

PROFILE="release"

usage() {
  cat <<'EOF' >&2
Usage:
  build_harness_binary.sh [--debug|--release]

Builds the Rust harness and copies the executable into plugins/mutagen/bin/
so an installed plugin can run without a repo-level harness/Cargo.toml.
EOF
  exit 1
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

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      PROFILE="debug"
      shift
      ;;
    --release)
      PROFILE="release"
      shift
      ;;
    --help|-h)
      usage
      ;;
    *)
      usage
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PLUGIN_ROOT/../.." && pwd)"
MANIFEST_PATH="$REPO_ROOT/harness/Cargo.toml"

if [[ ! -f "$MANIFEST_PATH" ]]; then
  printf 'harness manifest not found at %s\n' "$MANIFEST_PATH" >&2
  exit 1
fi

CARGO_BIN="$(resolve_cargo)" || {
  printf 'cargo not found on PATH\n' >&2
  exit 1
}

if [[ "$PROFILE" == "release" ]]; then
  "$CARGO_BIN" build --release --manifest-path "$MANIFEST_PATH"
  SOURCE_BINARY="$REPO_ROOT/harness/target/release/mutagen-harness"
else
  "$CARGO_BIN" build --manifest-path "$MANIFEST_PATH"
  SOURCE_BINARY="$REPO_ROOT/harness/target/debug/mutagen-harness"
fi

TARGET_BINARY="$PLUGIN_ROOT/bin/mutagen-harness"
if [[ -f "$SOURCE_BINARY" ]]; then
  :
elif [[ -f "$SOURCE_BINARY.exe" ]]; then
  SOURCE_BINARY="$SOURCE_BINARY.exe"
  TARGET_BINARY="$TARGET_BINARY.exe"
else
  printf 'built harness binary not found at %s or %s.exe\n' "$SOURCE_BINARY" "$SOURCE_BINARY" >&2
  exit 1
fi

mkdir -p "$PLUGIN_ROOT/bin"
cp "$SOURCE_BINARY" "$TARGET_BINARY"
chmod +x "$TARGET_BINARY"

printf '%s\n' "$TARGET_BINARY"
