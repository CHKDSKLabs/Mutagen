#!/usr/bin/env bash

# Auto-provisioner for the mutagen-harness binary. No Rust toolchain required —
# pulls the matching per-target asset off the GitHub Release, sha256-verifies it,
# and drops the executable at plugins/mutagen/bin/. Idempotent: a matching
# binary that's already on disk short-circuits the whole thing.

set -euo pipefail

QUIET=0
FORCE=0

usage() {
  cat <<'EOF' >&2
Usage:
  fetch_harness_binary.sh [--quiet] [--force]

Detects the host triple, reads the plugin version from plugins/mutagen/.claude-plugin/plugin.json,
downloads the matching mutagen-harness archive + .sha256 from the GitHub Release,
verifies the checksum, and extracts the binary into plugins/mutagen/bin/.

Env overrides:
  MUTAGEN_HARNESS_RELEASE_BASE_URL  Base URL (default: GitHub Releases for CHKDSKLabs/Mutagen)
  MUTAGEN_HARNESS_FORCE_TRIPLE      Skip uname detection and use this triple verbatim
EOF
  exit 1
}

log() { [[ "$QUIET" -eq 1 ]] || printf '[fetch-harness] %s\n' "$*" >&2; }
die() { printf '[fetch-harness] error: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --quiet) QUIET=1; shift ;;
    --force) FORCE=1; shift ;;
    --help|-h) usage ;;
    *) usage ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MANIFEST="$PLUGIN_ROOT/.claude-plugin/plugin.json"
BIN_DIR="$PLUGIN_ROOT/bin"

[[ -f "$MANIFEST" ]] || die "plugin manifest not found at $MANIFEST"

VERSION="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$MANIFEST" | head -n1)"
[[ -n "$VERSION" ]] || die "could not parse version from $MANIFEST"

detect_triple() {
  if [[ -n "${MUTAGEN_HARNESS_FORCE_TRIPLE:-}" ]]; then
    printf '%s\n' "$MUTAGEN_HARNESS_FORCE_TRIPLE"
    return 0
  fi

  local sys arch
  sys="$(uname -s 2>/dev/null || echo unknown)"
  arch="$(uname -m 2>/dev/null || echo unknown)"

  case "$sys" in
    Linux)
      case "$arch" in
        x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) return 1 ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64|aarch64) echo "aarch64-apple-darwin" ;;
        *) return 1 ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      case "$arch" in
        x86_64|amd64) echo "x86_64-pc-windows-msvc" ;;
        *) return 1 ;;
      esac
      ;;
    *) return 1 ;;
  esac
}

TRIPLE="$(detect_triple)" || die "unsupported host: $(uname -sm). Set MUTAGEN_HARNESS_FORCE_TRIPLE or build from source."

case "$TRIPLE" in
  *windows*) ARCHIVE_EXT="zip"; BINARY_NAME="mutagen-harness.exe" ;;
  *)         ARCHIVE_EXT="tar.gz"; BINARY_NAME="mutagen-harness" ;;
esac

TARGET_BIN="$BIN_DIR/$BINARY_NAME"
VERSION_STAMP="$BIN_DIR/.harness-version"

if [[ "$FORCE" -ne 1 && -x "$TARGET_BIN" && -f "$VERSION_STAMP" ]]; then
  if [[ "$(cat "$VERSION_STAMP" 2>/dev/null)" == "$VERSION-$TRIPLE" ]]; then
    log "binary already present at $TARGET_BIN (v$VERSION, $TRIPLE)"
    exit 0
  fi
fi

BASE_URL="${MUTAGEN_HARNESS_RELEASE_BASE_URL:-https://github.com/CHKDSKLabs/Mutagen/releases/download}"
ASSET="mutagen-harness-v${VERSION}-${TRIPLE}.${ARCHIVE_EXT}"
ASSET_URL="${BASE_URL}/v${VERSION}/${ASSET}"
SHA_URL="${ASSET_URL}.sha256"

command -v curl >/dev/null 2>&1 || die "curl is required but not on PATH"
[[ "$ARCHIVE_EXT" == "tar.gz" ]] && { command -v tar >/dev/null 2>&1 || die "tar is required for this platform"; }
[[ "$ARCHIVE_EXT" == "zip"   ]] && { command -v unzip >/dev/null 2>&1 || die "unzip is required for this platform"; }

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t mutagen-harness)"
trap 'rm -rf "$TMP_DIR"' EXIT

log "fetching $ASSET_URL"
curl -fsSL --retry 3 --retry-delay 2 -o "$TMP_DIR/$ASSET" "$ASSET_URL" \
  || die "download failed: $ASSET_URL"
curl -fsSL --retry 3 --retry-delay 2 -o "$TMP_DIR/$ASSET.sha256" "$SHA_URL" \
  || die "checksum download failed: $SHA_URL"

EXPECTED_SHA="$(awk '{print $1; exit}' "$TMP_DIR/$ASSET.sha256")"
[[ -n "$EXPECTED_SHA" ]] || die "could not parse expected sha256 from $SHA_URL"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA="$(sha256sum "$TMP_DIR/$ASSET" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA="$(shasum -a 256 "$TMP_DIR/$ASSET" | awk '{print $1}')"
else
  die "neither sha256sum nor shasum available — refusing to install unverified binary"
fi

[[ "$ACTUAL_SHA" == "$EXPECTED_SHA" ]] \
  || die "checksum mismatch for $ASSET (expected $EXPECTED_SHA, got $ACTUAL_SHA)"
log "checksum verified ($EXPECTED_SHA)"

EXTRACT_DIR="$TMP_DIR/extract"
mkdir -p "$EXTRACT_DIR"

if [[ "$ARCHIVE_EXT" == "tar.gz" ]]; then
  tar -xzf "$TMP_DIR/$ASSET" -C "$EXTRACT_DIR"
else
  unzip -q "$TMP_DIR/$ASSET" -d "$EXTRACT_DIR"
fi

# The release matrix nests the binary in a directory named after the asset stem.
SRC_BIN="$(find "$EXTRACT_DIR" -type f -name "$BINARY_NAME" | head -n1)"
[[ -n "$SRC_BIN" && -f "$SRC_BIN" ]] || die "extracted archive did not contain $BINARY_NAME"

mkdir -p "$BIN_DIR"
cp "$SRC_BIN" "$TARGET_BIN"
chmod +x "$TARGET_BIN" 2>/dev/null || true
printf '%s-%s\n' "$VERSION" "$TRIPLE" > "$VERSION_STAMP"

log "installed $TARGET_BIN (v$VERSION, $TRIPLE)"
