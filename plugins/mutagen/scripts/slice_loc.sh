#!/usr/bin/env bash
# Measure net-new LOC for a slice from git, filtered to the slice's declared
# write scope plus any retry-only adjacent scope. Emits a single line of JSON:
#
#   {"slice":"L2-Orders-003","added":214,"deleted":37,"net":177,"target":250,"over_target_pct":0}
#
# Usage:
#   slice_loc.sh <slice_id>
#
# Contract:
#   - Reads the slice from slices/queue.json.
#   - Reads the start-of-slice commit marker from .mutagen/state/slice-start-ref/<slice_id>
#     (written by execute-next.md Stage 1 before the author dispatch). If the marker
#     is absent, falls back to comparing against HEAD's first parent — good enough
#     to avoid a hard fail, but the caller should treat the number as advisory.
#   - Path filter = union of the slice's `write_set` (authoritative) + the
#     slice's optional `adjacent_scope_allowed` globs.
#   - If `write_set` is absent because the queue came from an older slicer,
#     the script falls back to the legacy author-agent table below.
#   - Outputs JSON on stdout. Silent on stderr unless we genuinely cannot compute.
#
# The 120%-of-target hard gate lives in the orchestrator (execute-next.md); this
# script only reports the numbers.

set -euo pipefail

slice_id="${1:-}"
if [ -z "$slice_id" ]; then
  echo '{"error":"missing slice_id"}'
  exit 1
fi

resolve_jq() {
  if command -v jq >/dev/null 2>&1; then
    command -v jq
    return 0
  fi

  if command -v jq.exe >/dev/null 2>&1; then
    command -v jq.exe
    return 0
  fi

  return 1
}

JQ_BIN="$(resolve_jq)" || {
  echo '{"error":"jq not installed"}'
  exit 1
}

if ! command -v git >/dev/null 2>&1; then
  echo '{"error":"git not installed"}'
  exit 1
fi

if [ ! -r slices/queue.json ]; then
  echo '{"error":"slices/queue.json not found"}'
  exit 1
fi
queue="slices/queue.json"

slice_json=$("$JQ_BIN" -c --arg id "$slice_id" '.slices[] | select(.id == $id)' "$queue" 2>/dev/null)
if [ -z "$slice_json" ] || [ "$slice_json" = "null" ]; then
  echo "{\"error\":\"slice $slice_id not found in $queue\"}"
  exit 1
fi

author_agent=$(printf '%s' "$slice_json" | "$JQ_BIN" -r '.author_agent // ""')
target_loc=$(printf '%s' "$slice_json" | "$JQ_BIN" -r '.target_loc // 0')
write_globs=$(printf '%s' "$slice_json" | "$JQ_BIN" -r '(.write_set // []) | .[]' 2>/dev/null || true)
adjacent_globs=$(printf '%s' "$slice_json" | "$JQ_BIN" -r '(.adjacent_scope_allowed // []) | .[]' 2>/dev/null || true)

legacy_author_globs() {
  case "$1" in
    Bebop)
      printf '%s\n' \
        'src/**' 'app/**' 'api/**' 'components/**' 'pages/**' 'tests/**' 'styles/**' 'public/**'
      ;;
    Baxter)
      # Older Baxter slices never had a safe derived write set. Use the full
      # repo as an upper bound instead of pretending certainty.
      printf '%s\n' ':(top)*'
      ;;
    Chaplin)
      printf '%s\n' \
        'migrations/**' 'schema/**' 'db/**' 'prisma/**' 'src/models/**' \
        'src/queries/**' 'src/repositories/**' 'seeds/**' 'tests/db/**' 'tests/migrations/**'
      ;;
    Metalhead)
      printf '%s\n' \
        'observability/**' 'dashboards/**' 'alerts/**' 'slo/**' 'runbooks/alerts/**' \
        'src/instrumentation/**' 'src/tracing/**' 'src/logging/**' 'src/metrics/**' \
        'src/telemetry/**' 'tests/observability/**'
      ;;
    Splinter)
      printf '%s\n' \
        'docs/api/**' 'docs/onboarding/**' 'docs/guides/**' 'docs/how-to/**' \
        'docs/architecture/**' 'docs/migration/**' 'docs/glossary.md' 'runbooks/ops/**' \
        'README.md' 'CONTRIBUTING.md' 'CHANGELOG.md'
      ;;
    Tatsu)
      printf '%s\n' 'src/security/**' 'src/auth/**' 'middleware/**' 'policies/**' 'tests/security/**'
      ;;
    Krang)
      printf '%s\n' \
        '.github/workflows/**' 'fly.toml' 'wrangler.toml' 'Dockerfile' 'docker-compose.*' \
        'infrastructure/**' 'terraform/**' 'migrations/**' '.env.example'
      ;;
    *)
      return 1
      ;;
  esac
}

scope_globs="$write_globs"
if [ -z "${scope_globs//[$'\n\r\t ']}" ]; then
  scope_globs="$(legacy_author_globs "$author_agent")" || {
    echo "{\"error\":\"slice $slice_id is missing write_set and has unknown author_agent '$author_agent'\"}"
    exit 1
  }
fi

ref_file=".mutagen/state/slice-start-ref/$slice_id"
base_ref=""
if [ -r "$ref_file" ]; then
  base_ref=$(tr -d '[:space:]' < "$ref_file")
fi

# Resolve the comparison base. Greenfield repos (no commits yet) and freshly
# `git init`-ed projects have no HEAD^, so `git diff HEAD^` reports `added: 0`
# even when real new files exist. Walk a fallback chain instead:
#   1. Saved start-of-slice ref (if it actually resolves).
#   2. HEAD^ (normal case: prior commit on the branch).
#   3. Empty-tree object (greenfield: every tracked file counts as added).
#
# Mode is reported back so the caller knows whether the LOC delta is being
# measured against a real base or against the empty tree.
empty_tree="$(git hash-object -t tree /dev/null 2>/dev/null || echo '4b825dc642cb6eb9a060e54bf8d69288fbee4904')"
base_mode="saved"

if [ -n "$base_ref" ]; then
  if ! git rev-parse --verify --quiet "$base_ref^{commit}" >/dev/null 2>&1 \
     && ! git rev-parse --verify --quiet "$base_ref^{tree}" >/dev/null 2>&1; then
    base_ref=""
  fi
fi

if [ -z "$base_ref" ]; then
  if git rev-parse --verify --quiet 'HEAD^{commit}' >/dev/null 2>&1; then
    if git rev-parse --verify --quiet 'HEAD^^{commit}' >/dev/null 2>&1; then
      base_ref="HEAD^"
      base_mode="head_parent"
    else
      # Single-commit repo: HEAD^ does not resolve. Compare against empty tree
      # so the initial commit shows up as added LOC instead of zero.
      base_ref="$empty_tree"
      base_mode="empty_tree"
    fi
  else
    base_ref="$empty_tree"
    base_mode="empty_tree"
  fi
fi

# Build pathspec list for `git diff`. Include both declared write_set globs and
# any retry-only adjacent-scope globs.
pathspecs=()
while IFS= read -r g; do
  [ -z "$g" ] && continue
  if [[ "$g" == :\(* ]]; then
    pathspecs+=("$g")
  else
    pathspecs+=(":(glob)$g")
  fi
done <<< "$scope_globs"
while IFS= read -r g; do
  [ -z "$g" ] && continue
  if [[ "$g" == :\(* ]]; then
    pathspecs+=("$g")
  else
    pathspecs+=(":(glob)$g")
  fi
done <<< "$adjacent_globs"

# `git diff --numstat` is easier to parse than `--stat`: tab-separated added\tdeleted\tpath.
diff_out=$(git diff --numstat "$base_ref" -- "${pathspecs[@]}" 2>/dev/null || true)

# When the base is the empty tree we also need to count files that exist on
# disk but were never staged (greenfield slice that wrote new files without
# committing them yet). Walk those via `git diff --no-index` against /dev/null
# so the script reports a real LOC count instead of `added: 0`.
added=0
deleted=0
while IFS=$'\t' read -r a d _; do
  [ -z "$a" ] && continue
  # Binary files show '-' for both; skip them.
  [ "$a" = "-" ] && continue
  added=$((added + a))
  deleted=$((deleted + d))
done <<< "$diff_out"

if [ "$base_mode" = "empty_tree" ]; then
  untracked=$(git ls-files --others --exclude-standard -- "${pathspecs[@]}" 2>/dev/null || true)
  while IFS= read -r path; do
    [ -z "$path" ] && continue
    [ -f "$path" ] || continue
    lines=$(wc -l < "$path" 2>/dev/null || echo 0)
    added=$((added + lines))
  done <<< "$untracked"
fi

net=$((added - deleted))

over_pct=0
if [ "$target_loc" -gt 0 ]; then
  over_pct=$(( (net * 100) / target_loc ))
fi

printf '{"slice":"%s","base_ref":"%s","base_mode":"%s","added":%d,"deleted":%d,"net":%d,"target":%d,"over_target_pct":%d}\n' \
  "$slice_id" "$base_ref" "$base_mode" "$added" "$deleted" "$net" "$target_loc" "$over_pct"
