#!/usr/bin/env bash
# Non-interactive Claude launcher for Rust-harness-driven dispatch.
#
# The harness pipes a fully-rendered prompt on argv (or stdin) and expects
# the agent to emit its artifact on stdout, then exit. There is no human in
# the loop, so we run claude with `--print` and `--permission-mode
# bypassPermissions` so a permission prompt never stalls the pipeline.
#
# Override behaviour with:
#   MUTAGEN_CLAUDE_BIN          claude executable on PATH (default: claude)
#   MUTAGEN_CLAUDE_EXTRA_ARGS   space-separated args appended before the prompt

set -euo pipefail

claude_bin="${MUTAGEN_CLAUDE_BIN:-claude}"

extra_args=()
if [[ -n "${MUTAGEN_CLAUDE_EXTRA_ARGS:-}" ]]; then
  # shellcheck disable=SC2206
  extra_args=( ${MUTAGEN_CLAUDE_EXTRA_ARGS} )
fi

exec "$claude_bin" \
  --print \
  --permission-mode bypassPermissions \
  "${extra_args[@]}" \
  "$@"
