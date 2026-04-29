#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export MUTAGEN_ROOT="${MUTAGEN_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"

exec bash "$SCRIPT_DIR/harness_runtime.sh" run-execute-next "$@"
