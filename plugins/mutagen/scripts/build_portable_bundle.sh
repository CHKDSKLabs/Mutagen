#!/usr/bin/env bash

set -euo pipefail

PROFILE="release"
OUTPUT_DIR=""
BUNDLE_NAME=""

usage() {
  cat <<'EOF' >&2
Usage:
  build_portable_bundle.sh [--debug|--release] [--output-dir PATH] [--name NAME]

Builds a self-contained mutagen harness bundle for the current OS/arch.
The archive can be unpacked into another project and installed under
<project>/.mutagen/mutagen without needing the source checkout.
EOF
  exit 1
}

resolve_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    command -v sha256sum
    return 0
  fi

  if command -v shasum >/dev/null 2>&1; then
    printf 'shasum -a 256\n'
    return 0
  fi

  return 1
}

platform_id() {
  local os
  local arch

  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m | tr '[:upper:]' '[:lower:]')"

  case "$arch" in
    x86_64|amd64)
      arch="x86_64"
      ;;
    aarch64|arm64)
      arch="aarch64"
      ;;
  esac

  printf '%s-%s\n' "$os" "$arch"
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
    --output-dir)
      [[ $# -ge 2 ]] || usage
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --name)
      [[ $# -ge 2 ]] || usage
      BUNDLE_NAME="$2"
      shift 2
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

if [[ -z "$OUTPUT_DIR" ]]; then
  OUTPUT_DIR="$REPO_ROOT/dist"
fi

if [[ -z "$BUNDLE_NAME" ]]; then
  BUNDLE_NAME="mutagen-harness-$(platform_id)"
fi

mkdir -p "$OUTPUT_DIR"

if [[ "$PROFILE" == "release" ]]; then
  bash "$SCRIPT_DIR/build_harness_binary.sh" --release >/dev/null
else
  bash "$SCRIPT_DIR/build_harness_binary.sh" --debug >/dev/null
fi

STAGING_ROOT="$OUTPUT_DIR/.${BUNDLE_NAME}.staging"
BUNDLE_ROOT="$STAGING_ROOT/$BUNDLE_NAME"
rm -rf "$STAGING_ROOT"
mkdir -p "$BUNDLE_ROOT"

cp -R "$PLUGIN_ROOT/.claude-plugin" "$BUNDLE_ROOT/.claude-plugin"
cp -R "$PLUGIN_ROOT/.codex-plugin" "$BUNDLE_ROOT/.codex-plugin"
cp -R "$PLUGIN_ROOT/agents" "$BUNDLE_ROOT/agents"
cp -R "$PLUGIN_ROOT/bin" "$BUNDLE_ROOT/bin"
cp -R "$PLUGIN_ROOT/commands" "$BUNDLE_ROOT/commands"
cp -R "$PLUGIN_ROOT/guides" "$BUNDLE_ROOT/guides"
cp -R "$PLUGIN_ROOT/hooks" "$BUNDLE_ROOT/hooks"
cp -R "$PLUGIN_ROOT/scripts" "$BUNDLE_ROOT/scripts"
cp -R "$PLUGIN_ROOT/skills" "$BUNDLE_ROOT/skills"
cp -R "$PLUGIN_ROOT/templates" "$BUNDLE_ROOT/templates"
cp "$PLUGIN_ROOT/CHANGELOG.md" "$BUNDLE_ROOT/CHANGELOG.md"
cp "$PLUGIN_ROOT/README.md" "$BUNDLE_ROOT/README.md"

cat >"$BUNDLE_ROOT/PORTABLE.md" <<'EOF'
# Mutagen Harness Portable Bundle

This bundle contains the mutagen harness binary, runner shims, persona
prompts, skills, commands, hooks, guides, and templates needed to execute a
prepared mutagen queue inside another project.

## Install Into A Project

From the extracted bundle directory:

```bash
bash install.sh /path/to/project
```

This copies the bundle to:

```text
/path/to/project/.mutagen/mutagen
```

## Execute A Queue

From the target project:

```bash
export MUTAGEN_ROOT="$PWD/.mutagen/mutagen"
bash "$MUTAGEN_ROOT/scripts/harness_runtime.sh" validate-queue --queue "$PWD/slices/queue.json" > "$PWD/.mutagen/state/queue-validation.json"
bash "$MUTAGEN_ROOT/scripts/harness_runtime.sh" run-execute-next --workspace-root "$PWD" --host codex
```

Use `--host claude` when Claude Code is the execution host. `CODEX_BIN`,
`CLAUDE_BIN`, and `MUTAGEN_AGENT_LAUNCHER` are honored when set.

## Direct Binary

The packaged binary is available at:

```text
bin/mutagen-harness
```

The shell runtime prefers that binary and falls back to a source checkout only
when one is available.
EOF

cat >"$BUNDLE_ROOT/install.sh" <<'EOF'
#!/usr/bin/env bash

set -euo pipefail

PROJECT_ROOT="${1:-}"

if [[ -z "$PROJECT_ROOT" ]]; then
  printf 'usage: install.sh /path/to/project\n' >&2
  exit 2
fi

if [[ ! -d "$PROJECT_ROOT" ]]; then
  printf 'project root does not exist: %s\n' "$PROJECT_ROOT" >&2
  exit 1
fi

BUNDLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_ROOT="$PROJECT_ROOT/.mutagen/mutagen"

mkdir -p "$PROJECT_ROOT/.mutagen"
rm -rf "$TARGET_ROOT"
mkdir -p "$TARGET_ROOT"

tar -C "$BUNDLE_DIR" \
  --exclude './install.sh' \
  --exclude './PORTABLE.md' \
  -cf - . | tar -C "$TARGET_ROOT" -xf -

cp "$BUNDLE_DIR/install.sh" "$TARGET_ROOT/install.sh"
cp "$BUNDLE_DIR/PORTABLE.md" "$TARGET_ROOT/PORTABLE.md"

chmod +x "$TARGET_ROOT/scripts/"*.sh "$TARGET_ROOT/bin/"* "$TARGET_ROOT/install.sh" 2>/dev/null || true

cat <<MSG
Installed mutagen harness bundle to:
  $TARGET_ROOT

Use it from the project with:
  export MUTAGEN_ROOT="\$PWD/.mutagen/mutagen"
  bash "\$MUTAGEN_ROOT/scripts/harness_runtime.sh" run-execute-next --workspace-root "\$PWD" --host codex
MSG
EOF

chmod +x "$BUNDLE_ROOT/install.sh" "$BUNDLE_ROOT/scripts/"*.sh "$BUNDLE_ROOT/bin/"* 2>/dev/null || true

ARCHIVE_PATH="$OUTPUT_DIR/${BUNDLE_NAME}.tar.gz"
CHECKSUM_PATH="$ARCHIVE_PATH.sha256"
tar -C "$STAGING_ROOT" -czf "$ARCHIVE_PATH" "$BUNDLE_NAME"

if SHA256_BIN="$(resolve_sha256)"; then
  # shellcheck disable=SC2086
  $SHA256_BIN "$ARCHIVE_PATH" >"$CHECKSUM_PATH"
fi

rm -rf "$STAGING_ROOT"

printf '%s\n' "$ARCHIVE_PATH"
