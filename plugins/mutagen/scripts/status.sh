#!/usr/bin/env bash

set -uo pipefail

FORMAT="markdown"
ROOT="."
WINDOW_SECONDS=300

usage() {
  cat <<'EOF' >&2
Usage:
  status.sh [--format markdown|json] [--root PATH] [--window SECONDS]
EOF
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --format)
      [[ $# -ge 2 ]] || usage
      FORMAT="$2"
      shift 2
      ;;
    --root)
      [[ $# -ge 2 ]] || usage
      ROOT="$2"
      shift 2
      ;;
    --window)
      [[ $# -ge 2 ]] || usage
      WINDOW_SECONDS="$2"
      shift 2
      ;;
    *)
      usage
      ;;
  esac
done

case "$FORMAT" in
  markdown|json) ;;
  *) usage ;;
esac

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
  echo "status.sh: jq not found on PATH" >&2
  exit 1
}

ROOT="$(cd "$ROOT" 2>/dev/null && pwd)" || {
  echo "status.sh: root path not found: $ROOT" >&2
  exit 1
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

trim() {
  local value="${1:-}"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

strip_inline_markup() {
  local value
  value="$(trim "${1:-}")"
  value="${value//\`/}"
  value="${value//\*\*/}"
  value="${value//__/_}"
  value="${value//_/}"
  printf '%s' "$(trim "$value")"
}

rel_path() {
  local path="${1:-}"
  if [[ "$path" == "$ROOT/"* ]]; then
    printf '%s' "${path#"$ROOT/"}"
    return
  fi

  printf '%s' "$path"
}

mtime_or_zero() {
  local path="${1:-}"
  if [[ -e "$path" ]]; then
    stat -c %Y "$path" 2>/dev/null || echo 0
    return
  fi

  echo 0
}

json_string_or_null() {
  local value="${1:-}"
  if [[ -z "$value" ]]; then
    printf 'null'
    return
  fi

  "$JQ_BIN" -Rn --arg value "$value" '$value'
}

count_tbd() {
  local file="${1:-}"
  local count

  count="$(grep -o '<TBD>' "$file" 2>/dev/null | wc -l | tr -d '[:space:]')"
  if [[ -z "$count" ]]; then
    echo 0
    return
  fi

  echo "$count"
}

extract_table_value() {
  local label="${1:-}"
  local file="${2:-}"

  awk -F'|' -v label="$label" '
    BEGIN { IGNORECASE = 1 }
    $0 ~ "^\\|[[:space:]]*" label "[[:space:]]*\\|" {
      value = $3
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
      print value
      exit
    }
  ' "$file"
}

extract_inline_value() {
  local label="${1:-}"
  local file="${2:-}"

  awk -v label="$label" '
    BEGIN { IGNORECASE = 1 }
    $0 ~ "^\\*\\*" label ":\\*\\*" {
      line = $0
      sub("^\\*\\*" label ":\\*\\*[[:space:]]*", "", line)
      print line
      exit
    }
    $0 ~ "^" label ":" {
      line = $0
      sub("^" label ":[[:space:]]*", "", line)
      print line
      exit
    }
  ' "$file"
}

extract_status() {
  local file="${1:-}"
  local value=""

  value="$(extract_table_value 'Status' "$file")"
  if [[ -z "$value" ]]; then
    value="$(extract_inline_value 'Status' "$file")"
  fi

  value="$(strip_inline_markup "$value")"
  if [[ -z "$value" ]]; then
    value="Unknown"
  fi

  printf '%s' "$value"
}

extract_last_reviewed() {
  local file="${1:-}"
  local value=""

  for label in 'Last reviewed' 'Last updated' 'Date'; do
    value="$(extract_table_value "$label" "$file")"
    if [[ -n "$value" ]]; then
      break
    fi

    value="$(extract_inline_value "$label" "$file")"
    if [[ -n "$value" ]]; then
      break
    fi
  done

  strip_inline_markup "$value"
}

build_missing_doc_json() {
  local label="${1:-}"

  "$JQ_BIN" -nc \
    --arg label "$label" \
    '{
      label: $label,
      path: null,
      status: "Missing",
      last_reviewed: null,
      tbd_count: 0,
      exists: false
    }'
}

build_doc_json() {
  local label="${1:-}"
  local file="${2:-}"
  local path_rel
  local status
  local last_reviewed
  local tbd_count

  path_rel="$(rel_path "$file")"
  status="$(extract_status "$file")"
  last_reviewed="$(extract_last_reviewed "$file")"
  tbd_count="$(count_tbd "$file")"

  "$JQ_BIN" -nc \
    --arg label "$label" \
    --arg path "$path_rel" \
    --arg status "$status" \
    --argjson last_reviewed "$(json_string_or_null "$last_reviewed")" \
    --argjson tbd_count "${tbd_count:-0}" \
    '{
      label: $label,
      path: $path,
      status: $status,
      last_reviewed: $last_reviewed,
      tbd_count: $tbd_count,
      exists: true
    }'
}

find_first_existing() {
  local rel

  for rel in "$@"; do
    if [[ -f "$ROOT/$rel" ]]; then
      printf '%s\n' "$ROOT/$rel"
      return 0
    fi
  done

  return 1
}

collect_adr_files() {
  declare -A seen=()
  local file

  if [[ -d "$ROOT/docs/ADR" ]]; then
    while IFS= read -r file; do
      [[ -z "$file" ]] && continue
      if [[ -z "${seen[$file]+x}" ]]; then
        seen["$file"]=1
        printf '%s\n' "$file"
      fi
    done < <(find "$ROOT/docs/ADR" -type f -name '*.md' | sort)
  fi

  for file in "$ROOT"/docs/ADR-*.md "$ROOT"/ADR-*.md; do
    if [[ -f "$file" && -z "${seen[$file]+x}" ]]; then
      seen["$file"]=1
      printf '%s\n' "$file"
    fi
  done
}

collect_single_doc_files() {
  local file=""

  if file="$(find_first_existing "$@")"; then
    printf '%s\n' "$file"
  fi
}

LATEST_UPSTREAM_MTIME=0

track_upstream_mtime() {
  local file="${1:-}"
  local file_mtime

  file_mtime="$(mtime_or_zero "$file")"
  if [[ "$file_mtime" -gt "$LATEST_UPSTREAM_MTIME" ]]; then
    LATEST_UPSTREAM_MTIME="$file_mtime"
  fi
}

prd_file="$(collect_single_doc_files 'docs/PRD.md' 'PRD.md')"
ddd_file="$(collect_single_doc_files 'docs/DDD.md' 'DDD.md')"
isc_file="$(collect_single_doc_files 'docs/ISC.md' 'ISC.md')"
dsd_file="$(collect_single_doc_files 'docs/DSD.md' 'DSD.md')"

if [[ -n "$prd_file" ]]; then track_upstream_mtime "$prd_file"; fi
if [[ -n "$ddd_file" ]]; then track_upstream_mtime "$ddd_file"; fi
if [[ -n "$isc_file" ]]; then track_upstream_mtime "$isc_file"; fi
if [[ -n "$dsd_file" ]]; then track_upstream_mtime "$dsd_file"; fi

if [[ -n "$prd_file" ]]; then
  prd_json="$(build_doc_json 'PRD' "$prd_file")"
else
  prd_json="$(build_missing_doc_json 'PRD')"
fi

if [[ -n "$ddd_file" ]]; then
  ddd_json="$(build_doc_json 'DDD' "$ddd_file")"
else
  ddd_json="$(build_missing_doc_json 'DDD')"
fi

if [[ -n "$isc_file" ]]; then
  isc_json="$(build_doc_json 'ISC' "$isc_file")"
else
  isc_json="$(build_missing_doc_json 'ISC')"
fi

if [[ -n "$dsd_file" ]]; then
  dsd_json="$(build_doc_json 'DSD' "$dsd_file")"
else
  dsd_json="$(build_missing_doc_json 'DSD')"
fi

adr_entries_json="$("$JQ_BIN" -nc '[]')"
while IFS= read -r adr_file; do
  [[ -z "$adr_file" ]] && continue
  track_upstream_mtime "$adr_file"
  adr_entry="$(build_doc_json 'ADR' "$adr_file")"
  adr_entries_json="$(printf '%s' "$adr_entries_json" | "$JQ_BIN" -c --argjson entry "$adr_entry" '. + [$entry]')"
done < <(collect_adr_files)

adr_json="$(printf '%s' "$adr_entries_json" | "$JQ_BIN" -c '
  if length == 0 then
    {
      label: "ADR",
      status: "Missing",
      exists: false,
      accepted_count: 0,
      draft_count: 0,
      entries: []
    }
  else
    (map(select((.status // "") | ascii_downcase == "accepted")) | length) as $accepted
    | (length - $accepted) as $draft
    | {
        label: "ADR",
        status:
          (if $draft == 0 then "Accepted"
           elif $accepted == 0 then "Draft"
           else "Mixed"
           end),
        exists: true,
        accepted_count: $accepted,
        draft_count: $draft,
        entries: .
      }
  end
')"

upstream_json="$("$JQ_BIN" -nc \
  --argjson prd "$prd_json" \
  --argjson adr "$adr_json" \
  --argjson ddd "$ddd_json" \
  --argjson isc "$isc_json" \
  --argjson dsd "$dsd_json" \
  '{prd:$prd, adr:$adr, ddd:$ddd, isc:$isc, dsd:$dsd}')"

readiness_path="$ROOT/.mutagen/state/readiness-brief.json"
if [[ -f "$readiness_path" ]]; then
  readiness_json="$("$JQ_BIN" -c --arg path ".mutagen/state/readiness-brief.json" '. + {path:$path}' "$readiness_path" 2>/dev/null)"
else
  readiness_json='null'
fi

validation_path="$ROOT/.mutagen/state/validation-report.json"
if [[ -f "$validation_path" ]]; then
  validation_stale=false
  if [[ "$(mtime_or_zero "$validation_path")" -lt "$LATEST_UPSTREAM_MTIME" ]]; then
    validation_stale=true
  fi

  validation_json="$("$JQ_BIN" -c \
    --arg path ".mutagen/state/validation-report.json" \
    --argjson stale "$validation_stale" \
    '. + {path:$path, stale:$stale}' \
    "$validation_path" 2>/dev/null)"
else
  validation_json='null'
fi

workflow_path="$ROOT/.claude/workflow.json"
if [[ -f "$workflow_path" ]]; then
  workflow_json="$("$JQ_BIN" -c --arg path ".claude/workflow.json" '
    {
      present: true,
      path: $path,
      mode: (.pipeline_mode // "full"),
      review: {
        max_retries: (.review.max_retries // 2),
        max_micro_corrections: (.review.max_micro_corrections // 1)
      },
      heartbeat: {
        inspection_interval_min: (.heartbeat.inspection_interval_min // 5),
        low_cpm_threshold: (.heartbeat.low_cpm_threshold // 1),
        high_bytes_threshold: (.heartbeat.high_bytes_threshold // 500000),
        loop_threshold: (.heartbeat.loop_threshold // 5)
      }
    }
  ' "$workflow_path" 2>/dev/null)"
else
  workflow_json="$("$JQ_BIN" -nc '
    {
      present: false,
      path: ".claude/workflow.json",
      mode: "full",
      review: {
        max_retries: 2,
        max_micro_corrections: 1
      },
      heartbeat: {
        inspection_interval_min: 5,
        low_cpm_threshold: 1,
        high_bytes_threshold: 500000,
        loop_threshold: 5
      }
    }
  ')"
fi

queue_json_path="$ROOT/slices/queue.json"
slicemap_path="$ROOT/slices/slicemap.md"
legacy_queue_path="$ROOT/slices/queue.md"

parse_queue_markdown_summary() {
  local file="${1:-}"
  local total=0
  local pending=0
  local in_progress=0
  local completed=0
  local blocked_retry=0
  local refused=0
  local escalated=0
  local l1=0
  local l2=0
  local l3=0
  local l4=0
  local l5=0
  local l6=0
  local total_line=""
  local status_line=""
  local layer_line=""

  total_line="$(grep -m1 '^\- \*\*Total:\*\*' "$file" 2>/dev/null || true)"
  status_line="$(grep -m1 '^\- \*\*By status:\*\*' "$file" 2>/dev/null || true)"
  layer_line="$(grep -m1 '^\- \*\*By layer:\*\*' "$file" 2>/dev/null || true)"

  if [[ "$total_line" =~ ([0-9]+) ]]; then
    total="${BASH_REMATCH[1]}"
  fi

  if [[ "$status_line" =~ pending:\ ([0-9]+) ]]; then pending="${BASH_REMATCH[1]}"; fi
  if [[ "$status_line" =~ in_progress:\ ([0-9]+) ]]; then in_progress="${BASH_REMATCH[1]}"; fi
  if [[ "$status_line" =~ completed:\ ([0-9]+) ]]; then completed="${BASH_REMATCH[1]}"; fi
  if [[ "$status_line" =~ blocked_retry:\ ([0-9]+) ]]; then blocked_retry="${BASH_REMATCH[1]}"; fi
  if [[ "$status_line" =~ refused:\ ([0-9]+) ]]; then refused="${BASH_REMATCH[1]}"; fi
  if [[ "$status_line" =~ escalated:\ ([0-9]+) ]]; then escalated="${BASH_REMATCH[1]}"; fi

  if [[ "$layer_line" =~ L1:\ ([0-9]+) ]]; then l1="${BASH_REMATCH[1]}"; fi
  if [[ "$layer_line" =~ L2:\ ([0-9]+) ]]; then l2="${BASH_REMATCH[1]}"; fi
  if [[ "$layer_line" =~ L3:\ ([0-9]+) ]]; then l3="${BASH_REMATCH[1]}"; fi
  if [[ "$layer_line" =~ L4:\ ([0-9]+) ]]; then l4="${BASH_REMATCH[1]}"; fi
  if [[ "$layer_line" =~ L5:\ ([0-9]+) ]]; then l5="${BASH_REMATCH[1]}"; fi
  if [[ "$layer_line" =~ L6:\ ([0-9]+) ]]; then l6="${BASH_REMATCH[1]}"; fi

  "$JQ_BIN" -nc \
    --arg source "rendered_markdown" \
    --arg path "$(rel_path "$file")" \
    --argjson total "${total:-0}" \
    --argjson pending "${pending:-0}" \
    --argjson in_progress "${in_progress:-0}" \
    --argjson completed "${completed:-0}" \
    --argjson blocked_retry "${blocked_retry:-0}" \
    --argjson refused "${refused:-0}" \
    --argjson escalated "${escalated:-0}" \
    --argjson l1 "${l1:-0}" \
    --argjson l2 "${l2:-0}" \
    --argjson l3 "${l3:-0}" \
    --argjson l4 "${l4:-0}" \
    --argjson l5 "${l5:-0}" \
    --argjson l6 "${l6:-0}" \
    '{
      source: $source,
      path: $path,
      total: $total,
      by_status: {
        pending: $pending,
        in_progress: $in_progress,
        completed: $completed,
        blocked_retry: $blocked_retry,
        refused: $refused,
        escalated: $escalated
      },
      by_layer: {
        L1: $l1,
        L2: $l2,
        L3: $l3,
        L4: $l4,
        L5: $l5,
        L6: $l6
      },
      next_pending: null,
      open_escalations: [],
      gate_telemetry: {
        sample_size: 0,
        bishop: {clean: 0, advisory: 0, block: 0, skipped: 0},
        tiger_claw: {clean: 0, gap: 0, defect: 0, skipped: 0}
      }
    }'
}

if [[ -f "$queue_json_path" ]]; then
  queue_json="$("$JQ_BIN" -c '
    . as $root
    | ($root.slices // []) as $slices
    | ($slices | map(select(.status == "pending")) | .[0]) as $next
    | ($slices | map(select(.status == "completed"))) as $completed
    | ($completed | if length > 10 then .[-10:] else . end) as $recent
    | {
        source: "queue_json",
        path: "slices/queue.json",
        total: ($slices | length),
        by_status: {
          pending: ([ $slices[] | select(.status == "pending") ] | length),
          in_progress: ([ $slices[] | select(.status == "in_progress") ] | length),
          completed: ([ $slices[] | select(.status == "completed") ] | length),
          blocked_retry: ([ $slices[] | select(.status == "blocked_retry") ] | length),
          refused: ([ $slices[] | select(.status == "refused") ] | length),
          escalated: ([ $slices[] | select(.status == "escalated") ] | length)
        },
        by_layer: {
          L1: ([ $slices[] | select(.layer == 1) ] | length),
          L2: ([ $slices[] | select(.layer == 2) ] | length),
          L3: ([ $slices[] | select(.layer == 3) ] | length),
          L4: ([ $slices[] | select(.layer == 4) ] | length),
          L5: ([ $slices[] | select(.layer == 5) ] | length),
          L6: ([ $slices[] | select(.layer == 6) ] | length)
        },
        next_pending:
          (if $next == null then
            null
          else
            {
              id: $next.id,
              author_agent: ($next.author_agent // ""),
              layer: ($next.layer // 0),
              objective: ($next.objective // ""),
              attempts: ($next.attempts // 0),
              review_required: ($next.review_required // false)
            }
          end),
        open_escalations: [
          $slices[]
          | select(
              .status == "escalated"
              or .status == "refused"
              or .status == "blocked_retry"
            )
          | {
              id: .id,
              status: .status,
              escalation_reason: (.escalation_reason // "")
            }
        ],
        gate_telemetry: {
          sample_size: ($recent | length),
          bishop: {
            clean: ([ $recent[] | select(.verdicts.bishop == "clean") ] | length),
            advisory: ([ $recent[] | select(.verdicts.bishop == "advisory") ] | length),
            block: ([ $recent[] | select(.verdicts.bishop == "block") ] | length),
            skipped: ([ $recent[] | select(.verdicts.bishop == "skip") ] | length)
          },
          tiger_claw: {
            clean: ([ $recent[] | select(.verdicts.tiger_claw == "clean") ] | length),
            gap: ([ $recent[] | select(.verdicts.tiger_claw == "gap") ] | length),
            defect: ([ $recent[] | select(.verdicts.tiger_claw == "defect") ] | length),
            skipped: ([ $recent[] | select(.verdicts.tiger_claw == "skip") ] | length)
          }
        }
      }
  ' "$queue_json_path" 2>/dev/null)"
elif [[ -f "$slicemap_path" ]]; then
  queue_json="$(parse_queue_markdown_summary "$slicemap_path")"
elif [[ -f "$legacy_queue_path" ]]; then
  queue_json="$(parse_queue_markdown_summary "$legacy_queue_path")"
else
  queue_json="$("$JQ_BIN" -nc '
    {
      source: "missing",
      path: null,
      total: 0,
      by_status: {
        pending: 0,
        in_progress: 0,
        completed: 0,
        blocked_retry: 0,
        refused: 0,
        escalated: 0
      },
      by_layer: {L1: 0, L2: 0, L3: 0, L4: 0, L5: 0, L6: 0},
      next_pending: null,
      open_escalations: [],
      gate_telemetry: {
        sample_size: 0,
        bishop: {clean: 0, advisory: 0, block: 0, skipped: 0},
        tiger_claw: {clean: 0, gap: 0, defect: 0, skipped: 0}
      }
    }
  ')"
fi

queue_validation_path="$ROOT/.mutagen/state/queue-validation.json"
if [[ -f "$queue_validation_path" ]]; then
  queue_validation_stale=false
  queue_validation_orphaned=false
  queue_validation_freshness_basis="mtime"

  if [[ ! -f "$queue_json_path" ]]; then
    queue_validation_orphaned=true
  else
    report_contract_hash="$("$JQ_BIN" -r '.queue_contract_hash // empty' "$queue_validation_path" 2>/dev/null || true)"
    report_contract_basis="$("$JQ_BIN" -r '.queue_contract_hash_basis // empty' "$queue_validation_path" 2>/dev/null || true)"
    current_contract_hash=""
    current_contract_basis=""

    if [[ -n "$report_contract_hash" && -n "$report_contract_basis" ]]; then
      set +e
      current_contract_json="$(bash "$SCRIPT_DIR/queue_contract_hash.sh" "$queue_json_path" 2>/dev/null)"
      current_contract_status=$?
      set -e

      if [[ $current_contract_status -eq 0 ]] && printf '%s' "$current_contract_json" | "$JQ_BIN" empty >/dev/null 2>&1; then
        current_contract_hash="$(printf '%s' "$current_contract_json" | "$JQ_BIN" -r '.hash // empty')"
        current_contract_basis="$(printf '%s' "$current_contract_json" | "$JQ_BIN" -r '.basis // empty')"
      fi
    fi

    if [[ -n "$report_contract_hash" && -n "$report_contract_basis" && -n "$current_contract_hash" && -n "$current_contract_basis" ]]; then
      queue_validation_freshness_basis="$current_contract_basis"
      if [[ "$report_contract_basis" != "$current_contract_basis" || "$report_contract_hash" != "$current_contract_hash" ]]; then
        queue_validation_stale=true
      fi
    elif [[ "$(mtime_or_zero "$queue_json_path")" -gt "$(mtime_or_zero "$queue_validation_path")" ]]; then
      queue_validation_stale=true
    fi
  fi

  queue_validation_json="$("$JQ_BIN" -c \
    --arg path ".mutagen/state/queue-validation.json" \
    --arg freshness_basis "$queue_validation_freshness_basis" \
    --argjson stale "$queue_validation_stale" \
    --argjson orphaned "$queue_validation_orphaned" \
    '. + {path:$path, stale:$stale, orphaned:$orphaned, freshness_basis:$freshness_basis}' \
    "$queue_validation_path" 2>/dev/null)"
else
  queue_validation_json='null'
fi

active_slice_path="$ROOT/.mutagen/state/active-slice.json"
if [[ -f "$active_slice_path" ]]; then
  active_slice_json="$("$JQ_BIN" -c --arg path ".mutagen/state/active-slice.json" '. + {path:$path}' "$active_slice_path" 2>/dev/null)"
else
  active_slice_json='null'
fi

scope_violation_path="$ROOT/.mutagen/state/scope-violation.json"
if [[ -f "$scope_violation_path" ]]; then
  scope_violation_json="$("$JQ_BIN" -c --arg artifact_path ".mutagen/state/scope-violation.json" '. + {artifact_path:$artifact_path}' "$scope_violation_path" 2>/dev/null)"
else
  scope_violation_json='null'
fi

heartbeat_json='null'
if [[ "$active_slice_json" != "null" ]]; then
  heartbeat_raw="$(cd "$ROOT" && bash "$SCRIPT_DIR/heartbeat.sh" "$WINDOW_SECONDS" 2>/dev/null || true)"
  if [[ -n "$heartbeat_raw" ]]; then
    low_cpm_threshold="$(printf '%s' "$workflow_json" | "$JQ_BIN" -r '.heartbeat.low_cpm_threshold // 1')"
    high_bytes_threshold="$(printf '%s' "$workflow_json" | "$JQ_BIN" -r '.heartbeat.high_bytes_threshold // 500000')"
    loop_threshold="$(printf '%s' "$workflow_json" | "$JQ_BIN" -r '.heartbeat.loop_threshold // 5')"

    heartbeat_json="$(printf '%s' "$heartbeat_raw" | "$JQ_BIN" -c \
      --argjson low_cpm "$low_cpm_threshold" \
      --argjson high_bytes "$high_bytes_threshold" \
      --argjson loop_threshold "$loop_threshold" '
        . + {
          calls_per_minute:
            (if (.ok // false) != true then
              null
            else
              (.window_calls / ((.window_seconds / 60) | if . < 1 then 1 else . end))
            end),
          anomaly:
            (if (.ok // false) != true then
              "unavailable"
            else
              (.window_calls / ((.window_seconds / 60) | if . < 1 then 1 else . end)) as $cpm
              | if (.last_run_length // 0) >= $loop_threshold then
                  "tool_call_loop"
                elif $cpm < $low_cpm then
                  "stalled"
                elif (.bytes_last_window // 0) > $high_bytes then
                  "high_traffic"
                else
                  "nominal"
                end
            end)
        }
      ' 2>/dev/null)"
  fi
fi

review_count=0
review_recent_json="$("$JQ_BIN" -nc '[]')"

if [[ -d "$ROOT/reviews" ]]; then
  while IFS= read -r review_path; do
    [[ -z "$review_path" ]] && continue
    review_count=$((review_count + 1))
  done < <(find "$ROOT/reviews" -type f | sort)

  while IFS= read -r review_path; do
    [[ -z "$review_path" ]] && continue
    slice_id="$(basename "$(dirname "$review_path")")"
    review_rel="$(rel_path "$review_path")"
    verdict="unknown"

    if [[ -f "$queue_json_path" ]]; then
      queue_verdict="$("$JQ_BIN" -r --arg id "$slice_id" '
        .slices[]
        | select(.id == $id)
        | if (.verdicts.tiger_claw // empty) != "" then
            .verdicts.tiger_claw
          elif (.verdicts.bishop // empty) != "" then
            .verdicts.bishop
          else
            empty
          end
      ' "$queue_json_path" 2>/dev/null | head -n 1)"
      if [[ -n "$queue_verdict" ]]; then
        verdict="$queue_verdict"
      fi
    fi

    review_entry="$("$JQ_BIN" -nc \
      --arg slice_id "$slice_id" \
      --arg path "$review_rel" \
      --arg verdict "$verdict" \
      '{slice_id:$slice_id, path:$path, verdict:$verdict}')"
    review_recent_json="$(printf '%s' "$review_recent_json" | "$JQ_BIN" -c --argjson entry "$review_entry" '. + [$entry]')"
  done < <(find "$ROOT/reviews" -type f -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n 3 | cut -d' ' -f2-)
fi

generated_at="$(date '+%Y-%m-%d %H:%M')"

status_json="$("$JQ_BIN" -nc \
  --arg generated_at "$generated_at" \
  --argjson upstream_documents "$upstream_json" \
  --argjson readiness_brief "$readiness_json" \
  --argjson validation_report "$validation_json" \
  --argjson workflow "$workflow_json" \
  --argjson queue "$queue_json" \
  --argjson queue_validation "$queue_validation_json" \
  --argjson active_slice "$active_slice_json" \
  --argjson scope_violation "$scope_violation_json" \
  --argjson heartbeat "$heartbeat_json" \
  --argjson reviews_recent "$review_recent_json" \
  --argjson reviews_count "$review_count" \
  '{
    generated_at: $generated_at,
    upstream_documents: $upstream_documents,
    readiness_brief: $readiness_brief,
    validation_report: $validation_report,
    workflow: $workflow,
    queue: $queue,
    queue_validation: $queue_validation,
    active_slice: $active_slice,
    scope_violation: $scope_violation,
    heartbeat: $heartbeat,
    gate_telemetry: ($queue.gate_telemetry // {
      sample_size: 0,
      bishop: {clean: 0, advisory: 0, block: 0, skipped: 0},
      tiger_claw: {clean: 0, gap: 0, defect: 0, skipped: 0}
    }),
    open_escalations: ($queue.open_escalations // []),
    reviews: {
      count: $reviews_count,
      recent: $reviews_recent
    }
  }'
)"

status_json="$(printf '%s' "$status_json" | "$JQ_BIN" -c '
  def single_doc_ready(doc):
    ((doc.status // "") | ascii_downcase) == "approved";
  def accepted_doc_ready(doc):
    ((doc.status // "") | ascii_downcase) == "accepted";
  def upstream_ready:
    (
      single_doc_ready(.upstream_documents.prd)
      and accepted_doc_ready(.upstream_documents.adr)
      and single_doc_ready(.upstream_documents.ddd)
      and accepted_doc_ready(.upstream_documents.isc)
      and single_doc_ready(.upstream_documents.dsd)
    );
  def upstream_missing:
    (
      .upstream_documents.prd.status == "Missing"
      or .upstream_documents.adr.status == "Missing"
      or .upstream_documents.ddd.status == "Missing"
      or .upstream_documents.isc.status == "Missing"
      or .upstream_documents.dsd.status == "Missing"
    );
  . + {
    next_actions:
      ([
        if upstream_missing or (upstream_ready | not) then
          "Upstream design bundle is incomplete — run /mutagen:elicit."
        else empty end,
        if (.validation_report != null and (.validation_report.bundle_ready // true) == false) then
          "Shredder marked the bundle not ready for slicing — close the validation findings before re-slicing."
        else empty end,
        if upstream_ready and (.queue.total // 0) == 0 then
          "All upstream docs look ready, but no queue exists — run /mutagen:slice."
        else empty end,
        if (.queue.total // 0) > 0 and .queue_validation == null then
          "No queue validation report is on file — re-run /mutagen:slice before dispatch."
        else empty end,
        if (.queue_validation != null and (.queue_validation.stale // false)) then
          "Queue validation is stale — re-run /mutagen:slice before dispatch."
        else empty end,
        if (.queue_validation != null and (.queue_validation.orphaned // false)) then
          "Queue validation is orphaned — re-run /mutagen:slice before dispatch."
        else empty end,
        if (.queue_validation != null and (.queue_validation.ok // true) == false) then
          "Queue is not executable — fix Shredder output before /mutagen:execute-next."
        else empty end,
        if (.open_escalations | length) > 0 then
          "Resolve open escalations before proceeding."
        else empty end,
        if .scope_violation != null then
          "Scope violation recorded on \(.scope_violation.slice_id // "unknown slice") — resolve it before proceeding."
        else empty end,
        if (.active_slice != null and .heartbeat != null and (.heartbeat.anomaly // "") == "tool_call_loop") then
          "Active slice shows a tool-call loop — investigate before re-dispatching."
        else empty end,
        if (.active_slice != null and ((.active_slice.degraded_capabilities // []) | length) > 0) then
          "Current slice is running in degraded host mode — check active-slice degraded capabilities before assuming hard enforcement or parallel support."
        else empty end,
        if (
          (.queue.by_status.pending // 0) > 0
          and .queue_validation != null
          and (.queue_validation.ok // false) == true
          and (.queue_validation.stale // false) == false
          and (.queue_validation.orphaned // false) == false
        ) then
          "Queue is ready — run /mutagen:execute-next."
        else empty end,
        if (
          (.queue.total // 0) > 0
          and (.queue.by_status.pending // 0) == 0
          and (.open_escalations | length) == 0
        ) then
          "Queue clear — no pending slices remain."
        else empty end
      ] | unique)
  }
')"

if [[ "$FORMAT" == "json" ]]; then
  printf '%s\n' "$status_json"
  exit 0
fi

printf '%s' "$status_json" | "$JQ_BIN" -r '
  def yesno(v): if v then "true" else "false" end;
  def single_doc_line(doc):
    "  " + doc.label + "  " + (doc.status // "Missing")
    + (if doc.path then " (" + doc.path + ")" else "" end)
    + "  · TBDs: " + ((doc.tbd_count // 0) | tostring)
    + (if doc.last_reviewed then " · last reviewed: " + doc.last_reviewed else "" end);
  def readiness_color(v):
    if v == "green" then "🟢"
    elif v == "yellow" then "🟡"
    elif v == "red" then "🔴"
    else "?"
    end;
  def verdict_icon(v):
    if v == "clean" then "🟢"
    elif v == "gap" or v == "advisory" then "🟡"
    elif v == "defect" or v == "block" then "🔴"
    elif v == "skip" then "⏭"
    else "•"
    end;
  def heartbeat_status(h):
    if h == null then null
    elif (h.anomaly // "") == "tool_call_loop" then "tool-call loop detected"
    elif (h.anomaly // "") == "stalled" then "stalled"
    elif (h.anomaly // "") == "high_traffic" then "high traffic"
    elif (h.anomaly // "") == "nominal" then "nominal"
    else (h.reason // h.anomaly // "unavailable")
    end;

  "mutagen workflow status — \(.generated_at)",
  "",
  "Upstream documents:",
  single_doc_line(.upstream_documents.prd),
  "  ADR  \(.upstream_documents.adr.status // "Missing") · \((.upstream_documents.adr.accepted_count // 0) | tostring) accepted / \((.upstream_documents.adr.draft_count // 0) | tostring) draft",
  single_doc_line(.upstream_documents.ddd),
  single_doc_line(.upstream_documents.isc),
  single_doc_line(.upstream_documents.dsd),
  "",
  (
    if .readiness_brief == null then
      "April Readiness Brief: no readiness brief on file — run /mutagen:elicit"
    else
      "April Readiness Brief (\(.readiness_brief.date // "unknown") · \(.readiness_brief.mode // "unknown")):\n"
      + "  Recommendation: \(.readiness_brief.recommendation // "—")\n"
      + "  Shredder readiness: PRD \(readiness_color(.readiness_brief.shredder_readiness.prd // "")) · ADR \(readiness_color(.readiness_brief.shredder_readiness.adr // "")) · DDD \(readiness_color(.readiness_brief.shredder_readiness.ddd // "")) · ISC \(readiness_color(.readiness_brief.shredder_readiness.isc // "")) · DSD \(readiness_color(.readiness_brief.shredder_readiness.dsd // ""))\n"
      + "  Cross-doc issues: \((.readiness_brief.cross_consistency // []) | length | tostring)"
    end
  ),
  "",
  (
    if .validation_report == null then
      "Shredder Validation Report: no validation report on file — run /mutagen:slice"
    else
      "Shredder Validation Report (\(.validation_report.date // "unknown")\((if (.validation_report.stale // false) then " · stale" else "" end)):\n"
      + "  Bundle ready: \(yesno(.validation_report.bundle_ready // false))\n"
      + "  Readiness issues: \((.validation_report.readiness_issues // []) | length | tostring)\n"
      + "  Validation findings: \((.validation_report.validation_findings // []) | length | tostring)"
    end
  ),
  "",
  "Pipeline mode: \(.workflow.mode // "full")  ·  max retries: \((.workflow.review.max_retries // 2) | tostring)",
  "Heartbeat thresholds: inspect every \((.workflow.heartbeat.inspection_interval_min // 5) | tostring)m · low CPM < \((.workflow.heartbeat.low_cpm_threshold // 1) | tostring) · high bytes > \((.workflow.heartbeat.high_bytes_threshold // 500000) | tostring) · loop >= \((.workflow.heartbeat.loop_threshold // 5) | tostring)",
  "",
  "Slice queue (\(.queue.path // "missing")):",
  "  Source: \(.queue.source // "missing")",
  "  Total: \((.queue.total // 0) | tostring)  ·  pending: \((.queue.by_status.pending // 0) | tostring) · in_progress: \((.queue.by_status.in_progress // 0) | tostring) · completed: \((.queue.by_status.completed // 0) | tostring) · blocked_retry: \((.queue.by_status.blocked_retry // 0) | tostring) · refused: \((.queue.by_status.refused // 0) | tostring) · escalated: \((.queue.by_status.escalated // 0) | tostring)",
  "  By layer: L1: \((.queue.by_layer.L1 // 0) | tostring) · L2: \((.queue.by_layer.L2 // 0) | tostring) · L3: \((.queue.by_layer.L3 // 0) | tostring) · L4: \((.queue.by_layer.L4 // 0) | tostring) · L5: \((.queue.by_layer.L5 // 0) | tostring) · L6: \((.queue.by_layer.L6 // 0) | tostring)",
  (
    if .queue.next_pending == null then
      "  Next up: none"
    else
      "  Next up: \(.queue.next_pending.id)  ·  \(.queue.next_pending.author_agent // "unassigned")  ·  L\((.queue.next_pending.layer // 0) | tostring)  ·  attempts \((.queue.next_pending.attempts // 0) | tostring)"
      + (if (.queue.next_pending.review_required // false) then " · review required" else "" end)
      + " · \"\(.queue.next_pending.objective // "—")\""
    end
  ),
  "",
  (
    if .queue_validation == null then
      "Queue validation: no queue validation report on file — run /mutagen:slice"
    else
      "Queue validation (\(.queue_validation.path // ".mutagen/state/queue-validation.json")\((if (.queue_validation.stale // false) then " · stale" elif (.queue_validation.orphaned // false) then " · orphaned" else "" end)):\n"
      + "  Executable: \(yesno(.queue_validation.ok // false))\n"
      + "  Errors: \((.queue_validation.error_count // 0) | tostring)  ·  warnings: \((.queue_validation.warning_count // 0) | tostring)"
      + (
        if (.queue_validation.issues // []) | length > 0 then
          "\n" + ((.queue_validation.issues // [])
            | map("  - [" + (.level // "error") + "] " + (.code // "issue") + (if .slice_id then " · " + .slice_id else "" end) + ": " + (.message // "")) | join("\n"))
        elif (.queue_validation.message // "") != "" then
          "\n  - " + (.queue_validation.error // "validator_runtime_failure") + ": " + (.queue_validation.message // "")
        else
          "\n  Issues: none noted"
        end
      )
    end
  ),
  "",
  (
    if .active_slice == null then
      "Active slice: no active slice"
    else
      "Active slice:\n"
      + "  \(.active_slice.slice_id // "unknown")  ·  stage: \(.active_slice.stage // "unknown")  ·  agent: \(.active_slice.active_agent // .active_slice.author_agent // "unknown")  ·  host: \(.active_slice.host // "unknown")  ·  attempts \((.active_slice.attempts // 0) | tostring)"
      + (
        if ((.active_slice.degraded_capabilities // []) | length) == 0 then
          ""
        else
          "\n  Degraded host features: " + ((.active_slice.degraded_capabilities // []) | join(", "))
        end
      )
      + (
        if .heartbeat == null then
          "\n  Heartbeat: unavailable"
        else
          "\n  Heartbeat (last \((.heartbeat.window_seconds // 0) | tostring)s): calls \((.heartbeat.window_calls // 0) | tostring) · bytes \((.heartbeat.bytes_last_window // 0) | tostring) · last-run \((.heartbeat.last_run_tool // "—"))×\((.heartbeat.last_run_length // 0) | tostring)\n"
          + "    (\(heartbeat_status(.heartbeat)))"
        end
      )
    end
  ),
  "",
  (
    if .scope_violation == null then
      "Latest scope violation: none"
    else
      "Latest scope violation:\n"
      + "  \(.scope_violation.slice_id // "unknown")  ·  stage: \(.scope_violation.stage // "unknown")  ·  agent: \(.scope_violation.active_agent // .scope_violation.author_agent // "unknown")  ·  class: \(.scope_violation.class // "unknown")\n"
      + "  path: \(.scope_violation.path // "unknown")"
      + (if .scope_violation.ts then "\n  recorded: " + .scope_violation.ts else "" end)
      + (if .scope_violation.artifact_path then "\n  artifact: " + .scope_violation.artifact_path else "" end)
    end
  ),
  "",
  "Recent gate telemetry (last \((.gate_telemetry.sample_size // 0) | tostring) completed slices):",
  "  Bishop:     clean \((.gate_telemetry.bishop.clean // 0) | tostring) · advisory \((.gate_telemetry.bishop.advisory // 0) | tostring) · block \((.gate_telemetry.bishop.block // 0) | tostring) · skipped \((.gate_telemetry.bishop.skipped // 0) | tostring)",
  "  Tiger Claw: clean \((.gate_telemetry.tiger_claw.clean // 0) | tostring) · gap \((.gate_telemetry.tiger_claw.gap // 0) | tostring) · defect \((.gate_telemetry.tiger_claw.defect // 0) | tostring) · skipped \((.gate_telemetry.tiger_claw.skipped // 0) | tostring)",
  "",
  "Open escalations: \((.open_escalations | length) | tostring)",
  (
    if (.open_escalations | length) == 0 then
      "  - none"
    else
      (.open_escalations[] | "  - \(.id) [\(.status)]: \(.escalation_reason // "reason not recorded")")
    end
  ),
  "",
  "Recent reviews (last 3 of \((.reviews.count // 0) | tostring)):",
  (
    if (.reviews.recent | length) == 0 then
      "  - none"
    else
      (.reviews.recent[] | "  - \(.slice_id): \(verdict_icon(.verdict)) \(.verdict // "unknown") — \(.path)")
    end
  ),
  "",
  "Next actions:",
  (
    if (.next_actions | length) == 0 then
      "  - none"
    else
      (.next_actions[] | "  - " + .)
    end
  )
'
