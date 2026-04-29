#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF' >&2
Usage:
  dashboard_dev.sh --workspace-root <path> [options]

Options:
  --workspace-root <path>  Workspace to serve
  --bind <host>            Dashboard bind address
  --port <port>            Dashboard port
  --host <host-kind>       Harness host profile (default from config)
  --debug                  Build or refresh a debug binary
  --release                Build or refresh a release binary
  --build                  Force a binary rebuild before launch
  --no-build               Skip auto-build even when the binary is missing
  --help                   Show this help

Configuration precedence:
  1. CLI flags
  2. MUTAGEN_DASHBOARD_BIND / MUTAGEN_DASHBOARD_PORT / MUTAGEN_HOST_KIND / MUTAGEN_WORKSPACE_ROOT
  3. harness/config/dev.toml
  4. Script defaults
EOF
  exit 1
}

display_path() {
  printf '%s\n' "$1" | sed 's#\\#/#g'
}

trim() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s\n' "$value"
}

resolve_config_value() {
  local key="$1"
  local config_path="$2"

  [[ -f "$config_path" ]] || return 1

  awk -F '=' -v target="$key" '
    /^[[:space:]]*#/ { next }
    /^\[/ { section=$0; gsub(/[\[\]]/, "", section); next }
    index($0, "=") == 0 { next }
    {
      raw_key=$1
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", raw_key)
      value=substr($0, index($0, "=") + 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
      if (value ~ /^".*"$/) {
        sub(/^"/, "", value)
        sub(/"$/, "", value)
      }
      composite=section ? section "." raw_key : raw_key
      if (composite == target) {
        print value
        exit
      }
    }
  ' "$config_path"
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PLUGIN_ROOT/../.." && pwd)"
CONFIG_PATH="$REPO_ROOT/harness/config/dev.toml"
PROJECT_SCRIPT="$SCRIPT_DIR/project.sh"
BUILD_SCRIPT="$SCRIPT_DIR/build_harness_binary.sh"

DEFAULT_BIND="127.0.0.1"
DEFAULT_PORT="7799"
DEFAULT_HOST="stub"
DEFAULT_PROFILE="debug"

CONFIG_BIND="$(trim "$(resolve_config_value "bind" "$CONFIG_PATH" || true)")"
CONFIG_PORT="$(trim "$(resolve_config_value "port" "$CONFIG_PATH" || true)")"
CONFIG_HOST="$(trim "$(resolve_config_value "host" "$CONFIG_PATH" || true)")"
CONFIG_PROFILE="$(trim "$(resolve_config_value "build_profile" "$CONFIG_PATH" || true)")"

BIND="${MUTAGEN_DASHBOARD_BIND:-${CONFIG_BIND:-$DEFAULT_BIND}}"
PORT="${MUTAGEN_DASHBOARD_PORT:-${CONFIG_PORT:-$DEFAULT_PORT}}"
HOST_KIND="${MUTAGEN_HOST_KIND:-${CONFIG_HOST:-$DEFAULT_HOST}}"
WORKSPACE_ROOT="${MUTAGEN_WORKSPACE_ROOT:-}"
BUILD_PROFILE="${CONFIG_PROFILE:-$DEFAULT_PROFILE}"
FORCE_BUILD=0
AUTO_BUILD=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root)
      [[ $# -ge 2 ]] || usage
      WORKSPACE_ROOT="$2"
      shift 2
      ;;
    --bind)
      [[ $# -ge 2 ]] || usage
      BIND="$2"
      shift 2
      ;;
    --port)
      [[ $# -ge 2 ]] || usage
      PORT="$2"
      shift 2
      ;;
    --host)
      [[ $# -ge 2 ]] || usage
      HOST_KIND="$2"
      shift 2
      ;;
    --debug)
      BUILD_PROFILE="debug"
      shift
      ;;
    --release)
      BUILD_PROFILE="release"
      shift
      ;;
    --build)
      FORCE_BUILD=1
      shift
      ;;
    --no-build)
      AUTO_BUILD=0
      shift
      ;;
    --help|-h)
      usage
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage
      ;;
  esac
done

if [[ -z "$WORKSPACE_ROOT" ]]; then
  printf 'dashboard_dev.sh needs --workspace-root or MUTAGEN_WORKSPACE_ROOT\n' >&2
  exit 1
fi

mkdir -p "$WORKSPACE_ROOT"
WORKSPACE_ROOT="$(cd "$WORKSPACE_ROOT" && pwd)"
PROJECT_FILE="$WORKSPACE_ROOT/.mutagen/project.json"
if [[ ! -f "$PROJECT_FILE" ]]; then
  printf 'workspace is missing %s\n' "$(display_path "$PROJECT_FILE")" >&2
  printf 'dashboard will open in project setup mode.\n' >&2
fi

PACKAGED_BINARY="$PLUGIN_ROOT/bin/mutagen-harness"
PACKAGED_BINARY_EXE="$PLUGIN_ROOT/bin/mutagen-harness.exe"

should_build=0
if [[ "$FORCE_BUILD" -eq 1 ]]; then
  should_build=1
elif [[ "$AUTO_BUILD" -eq 1 && ! -x "$PACKAGED_BINARY" && ! -x "$PACKAGED_BINARY_EXE" ]]; then
  should_build=1
fi

if [[ "$should_build" -eq 1 ]]; then
  build_flag="--debug"
  if [[ "$BUILD_PROFILE" == "release" ]]; then
    build_flag="--release"
  fi

  bash "$BUILD_SCRIPT" "$build_flag" >/dev/null
fi

if [[ -x "$PACKAGED_BINARY" ]]; then
  HARNESS_BIN="$PACKAGED_BINARY"
elif [[ -x "$PACKAGED_BINARY_EXE" ]]; then
  HARNESS_BIN="$PACKAGED_BINARY_EXE"
else
  HARNESS_BIN="cargo-run-fallback"
fi

printf 'mutagen dev dashboard\n'
printf 'workspace: %s\n' "$(display_path "$WORKSPACE_ROOT")"
printf 'binary: %s\n' "$(display_path "$HARNESS_BIN")"
printf 'url: http://%s:%s/\n' "$BIND" "$PORT"
printf 'host: %s\n' "$HOST_KIND"
printf 'config: %s\n' "$(display_path "$CONFIG_PATH")"
printf 'status: starting\n'

exec bash "$PROJECT_SCRIPT" dashboard-serve \
  --workspace-root "$WORKSPACE_ROOT" \
  --bind "$BIND" \
  --port "$PORT" \
  --host "$HOST_KIND"
