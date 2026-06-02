#!/usr/bin/env bash
# CodexPro regression — agent.sh must invoke the codex host (and any
# custom MUTAGEN_AGENT_LAUNCHER) with stdin closed by default. This was
# the root cause of the "Reading additional input from stdin..." hang
# inside non-TTY Codex shells.
#
# Run manually:
#   bash plugins/mutagen/tests/agent_sh_stdin.sh
# Exits 0 on success, 1 on failure.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
AGENT_SH="$PLUGIN_ROOT/bin/agent.sh"

if [[ ! -x "$AGENT_SH" ]]; then
  echo "agent_sh_stdin: agent.sh missing or not executable at $AGENT_SH" >&2
  exit 1
fi

# Disposable workspace so the synthesized persona doesn't collide with the
# real plugins/mutagen/agents/ tree.
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

mkdir -p "$WORK_DIR/agents"
cat >"$WORK_DIR/agents/Probe.md" <<'EOF'
---
name: Probe
---

# Probe persona
EOF

LAUNCHER="$WORK_DIR/launcher.sh"
CAPTURE="$WORK_DIR/capture.txt"

cat >"$LAUNCHER" <<'EOF'
#!/usr/bin/env bash
# args: <host> <persona> <profile> <framing>
# Drain whatever stdin we got. If it's /dev/null, cat reads zero bytes and
# exits immediately. If stdin is inherited from a real source, we get the
# bytes the caller fed in.
data=$(cat 2>/dev/null || true)
if [[ -z "$data" ]]; then
  printf 'STDIN_EMPTY\n'
else
  printf 'STDIN_HAS: %s\n' "$data"
fi
EOF
chmod +x "$LAUNCHER"

# Default branch: launcher should see /dev/null on stdin even though the
# caller (this test) has a live, inheritable stdin attached.
MUTAGEN_ROOT="$WORK_DIR" \
MUTAGEN_AGENT_LAUNCHER="$LAUNCHER" \
  bash "$AGENT_SH" --host codex Probe "probe task" <<<"this should NOT reach the launcher" >"$CAPTURE" 2>&1

if ! grep -q "STDIN_EMPTY" "$CAPTURE"; then
  echo "FAIL: default launcher branch should receive /dev/null on stdin." >&2
  echo "captured:" >&2
  cat "$CAPTURE" >&2
  exit 1
fi

# Opt-in branch: launcher should see whatever stdin we feed.
KEEP_CAPTURE="$WORK_DIR/keep.txt"
MUTAGEN_ROOT="$WORK_DIR" \
MUTAGEN_AGENT_LAUNCHER="$LAUNCHER" \
MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN=1 \
  bash "$AGENT_SH" --host codex Probe "probe task" <<<"yo from stdin" >"$KEEP_CAPTURE" 2>&1

if ! grep -q "STDIN_HAS: yo from stdin" "$KEEP_CAPTURE"; then
  echo "FAIL: opt-in branch should preserve inherited stdin." >&2
  echo "captured:" >&2
  cat "$KEEP_CAPTURE" >&2
  exit 1
fi

echo "agent_sh_stdin: OK"
exit 0
