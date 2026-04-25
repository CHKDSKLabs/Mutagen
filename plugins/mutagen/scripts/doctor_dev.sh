#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF' >&2
Usage:
  doctor_dev.sh --workspace-root <path>

Checks the local dev deployment prerequisites for the harness dashboard and
then runs the project doctor against the selected workspace.
EOF
  exit 1
}

display_path() {
  printf '%s\n' "$1" | sed 's#\\#/#g'
}

resolve_tool() {
  local executable="$1"

  if command -v "$executable" >/dev/null 2>&1; then
    command -v "$executable"
    return 0
  fi

  return 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$PLUGIN_ROOT/../.." && pwd)"
PROJECT_SCRIPT="$SCRIPT_DIR/project.sh"
BUILD_SCRIPT="$SCRIPT_DIR/build_harness_binary.sh"

WORKSPACE_ROOT="${MUTAGEN_WORKSPACE_ROOT:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --workspace-root)
      [[ $# -ge 2 ]] || usage
      WORKSPACE_ROOT="$2"
      shift 2
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
  printf 'doctor_dev.sh needs --workspace-root or MUTAGEN_WORKSPACE_ROOT\n' >&2
  exit 1
fi

WORKSPACE_ROOT="$(cd "$WORKSPACE_ROOT" && pwd)"
PROJECT_FILE="$WORKSPACE_ROOT/.mutagen/project.json"

printf 'mutagen dev doctor\n'
printf 'workspace: %s\n' "$(display_path "$WORKSPACE_ROOT")"
printf '\n'

for executable in bash jq git cargo rustc; do
  if tool_path="$(resolve_tool "$executable")"; then
    printf '[ok]   %-5s %s\n' "$executable" "$(display_path "$tool_path")"
  else
    printf '[miss] %-5s not found on PATH\n' "$executable"
  fi
done

if [[ -x "$PLUGIN_ROOT/bin/mutagen-harness" || -x "$PLUGIN_ROOT/bin/mutagen-harness.exe" ]]; then
  printf '[ok]   binary packaged harness binary is present\n'
else
  printf '[warn] binary packaged harness binary is missing; building a dev one now\n'
  bash "$BUILD_SCRIPT" --debug >/dev/null
  printf '[ok]   binary built plugins/mutagen/bin/mutagen-harness\n'
fi

if [[ -f "$PROJECT_FILE" ]]; then
  printf '[ok]   workspace %s\n' "$(display_path "$PROJECT_FILE")"
else
  printf '[miss] workspace %s\n' "$(display_path "$PROJECT_FILE")"
  printf '\n'
  printf 'run bash plugins/mutagen/scripts/project.sh init before asking the dashboard to be helpful.\n' >&2
  exit 1
fi

printf '\n'
bash "$PROJECT_SCRIPT" doctor --workspace-root "$WORKSPACE_ROOT"
