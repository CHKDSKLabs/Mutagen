#!/usr/bin/env bash
# Measure net-new LOC for a slice from git, filtered to the slice's author +
# adjacent_scope paths. Emits a single line of JSON:
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
#   - Path filter = union of the slice's author globs (resolved from author_agent via
#     the table embedded below) + the slice's optional adjacent_scope_allowed globs.
#   - Outputs JSON on stdout. Silent on stderr unless we genuinely cannot compute.
#
# The 120%-of-target hard gate lives in the orchestrator (execute-next.md); this
# script only reports the numbers.

set -u

slice_id="${1:-}"
if [ -z "$slice_id" ]; then
  echo '{"error":"missing slice_id"}'
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo '{"error":"jq not installed"}'
  exit 1
fi

if ! command -v git >/dev/null 2>&1; then
  echo '{"error":"git not installed"}'
  exit 1
fi

queue=".mutagen_queue_placeholder"
if [ -r slices/queue.json ]; then
  queue="slices/queue.json"
else
  echo '{"error":"slices/queue.json not found"}'
  exit 1
fi

slice_json=$(jq -c --arg id "$slice_id" '.slices[] | select(.id == $id)' "$queue" 2>/dev/null)
if [ -z "$slice_json" ] || [ "$slice_json" = "null" ]; then
  echo "{\"error\":\"slice $slice_id not found in $queue\"}"
  exit 1
fi

author_agent=$(printf '%s' "$slice_json" | jq -r '.author_agent // ""')
target_loc=$(printf '%s' "$slice_json" | jq -r '.target_loc // 0')
adjacent_globs=$(printf '%s' "$slice_json" | jq -r '(.adjacent_scope_allowed // []) | .[]' 2>/dev/null || true)

author_globs=""
case "$author_agent" in
  Bebop)
    author_globs=$'src/**\napp/**\napi/**\ncomponents/**\npages/**\ntests/**\nstyles/**\npublic/**'
    ;;
  Baxter)
    # Baxter's paths are cited-module-specific; fall back to the whole repo and
    # trust the caller to read this result as an upper bound.
    author_globs=$':(top)*'
    ;;
  Chaplin)
    author_globs=$'migrations/**\nschema/**\ndb/**\nprisma/**\nsrc/models/**\nsrc/queries/**\nsrc/repositories/**\nseeds/**\ntests/db/**\ntests/migrations/**'
    ;;
  Metalhead)
    author_globs=$'observability/**\ndashboards/**\nalerts/**\nslo/**\nrunbooks/alerts/**\nsrc/instrumentation/**\nsrc/tracing/**\nsrc/logging/**\nsrc/metrics/**\nsrc/telemetry/**\ntests/observability/**'
    ;;
  Splinter)
    author_globs=$'docs/api/**\ndocs/onboarding/**\ndocs/guides/**\ndocs/how-to/**\ndocs/architecture/**\ndocs/migration/**\ndocs/glossary.md\nrunbooks/ops/**\nREADME.md\nCONTRIBUTING.md\nCHANGELOG.md'
    ;;
  Tatsu)
    author_globs=$'src/security/**\nsrc/auth/**\nmiddleware/**\npolicies/**\ntests/security/**'
    ;;
  Krang)
    author_globs=$'.github/workflows/**\nfly.toml\nwrangler.toml\nDockerfile\ndocker-compose.*\ninfrastructure/**\nterraform/**\nmigrations/**\n.env.example'
    ;;
  *)
    echo "{\"error\":\"unknown author_agent '$author_agent'\"}"
    exit 1
    ;;
esac

ref_file=".mutagen/state/slice-start-ref/$slice_id"
base_ref=""
if [ -r "$ref_file" ]; then
  base_ref=$(tr -d '[:space:]' < "$ref_file")
fi
if [ -z "$base_ref" ]; then
  base_ref="HEAD^"
fi

# Build pathspec list for `git diff`. Include both author globs and adjacent globs.
pathspecs=()
while IFS= read -r g; do
  [ -n "$g" ] && pathspecs+=(":(glob)$g")
done <<< "$author_globs"
while IFS= read -r g; do
  [ -n "$g" ] && pathspecs+=(":(glob)$g")
done <<< "$adjacent_globs"

# `git diff --numstat` is easier to parse than `--stat`: tab-separated added\tdeleted\tpath.
diff_out=$(git diff --numstat "$base_ref" -- "${pathspecs[@]}" 2>/dev/null || true)

added=0
deleted=0
while IFS=$'\t' read -r a d _; do
  [ -z "$a" ] && continue
  # Binary files show '-' for both; skip them.
  [ "$a" = "-" ] && continue
  added=$((added + a))
  deleted=$((deleted + d))
done <<< "$diff_out"

net=$((added - deleted))

over_pct=0
if [ "$target_loc" -gt 0 ]; then
  over_pct=$(( (net * 100) / target_loc ))
fi

printf '{"slice":"%s","base_ref":"%s","added":%d,"deleted":%d,"net":%d,"target":%d,"over_target_pct":%d}\n' \
  "$slice_id" "$base_ref" "$added" "$deleted" "$net" "$target_loc" "$over_pct"
