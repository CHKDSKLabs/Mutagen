---
name: mutagen-slice
description: Explicit invocation only. Run Shredder against the approved upstream design bundle (PRD / ADR / DDD / ISC / DSD) to produce a dependency-ordered slice queue.
---

# $mutagen-slice — run Shredder on the approved design bundle

Orchestrate the Shredder persona against the five upstream documents to emit
a slice queue. The persona text lives at `$MUTAGEN_ROOT/agents/Shredder.md`
and is injected into a subprocess `codex exec` call via
`$MUTAGEN_ROOT/bin/agent.sh`.

`$MUTAGEN_ROOT` must point to this plugin's install path
(`<repo>/plugins/mutagen/`). Set it in your shell rc or let the marketplace
installer add it.

## Preflight

1. Ensure state directories exist: `mkdir -p .mutagen/state slices`.
2. **Readiness check.** Verify every upstream document exists and carries a
   status line indicating `Approved` (PRD / DDD / DSD) or `Accepted` (ADR /
   ISC). If any are missing or still `Draft`, **halt** and report:
   - Which documents are missing.
   - Which are present but not Approved / Accepted.
   - Suggest `$mutagen-elicit` to close the gaps.
3. Check for an existing queue at `slices/queue.json` (canonical),
   `slices/slicemap.md` (human-readable), or `slices/queue.md` (legacy
   shadow). If non-empty, warn the user that re-slicing replaces the queue.
   Ask for explicit confirmation before proceeding.
4. Read pipeline mode from `.claude/workflow.json` if present. Default
   `"full"`. Modes:
   - `"full"` — every slice runs Bishop review + Tiger Claw adversarial QA.
   - `"lightweight"` — those gates run only on slices Shredder tags
     `review_required: true`. See `$MUTAGEN_ROOT/guides/pipeline-modes.md`.
5. Write the active-slice state file scoping Shredder's writes (advisory in
   Codex — no hook enforces it, but the file is read by `$mutagen-status`
   and by Traag during `$mutagen-amend-scope`):

   ```json
   {
     "slice_id": "shredder-{YYYY-MM-DD-HHMM}",
     "author_agent": "Shredder",
     "stage": "authoring_queue",
     "allowed_write_globs": [
       "slices/**",
       ".mutagen/state/**"
     ]
   }
   ```

## Dispatch

Spawn Shredder via the agent wrapper:

```bash
bash "$MUTAGEN_ROOT/bin/agent.sh" Shredder "$(cat <<'PROMPT'
Upstream documents:
  PRD: <resolved path>
  ADR: <resolved path(s)>
  DDD: <resolved path>
  ISC: <resolved path>
  DSD: <resolved path>

Pipeline mode: full | lightweight

Tasks:
1. Run your Readiness Check and Validation Phase. Emit a **Validation Report**
   as the first section of your response (even when clean — `bundle_ready: true`
   with no issues is a valid report).
2. Produce the Slice Queue per your Output Protocol. Write both:
   - `slices/queue.json` — canonical, per $MUTAGEN_ROOT/guides/queue-schema.md
   - `slices/slicemap.md` — human-readable, per $MUTAGEN_ROOT/guides/slicemap-spec.md
3. Every slice must cite upstream IDs: PRD [FR-*]/[NFR-*], ADR, DDD, ISC
   [ISC-NNN], DSD [DSD-###].
4. Every slice must include `depends_on`, `write_set`, structured
   `implementation_details`, and
   `human_check_needed.{required,reason,resolved_at}`.
5. In lightweight mode, tag each slice `review_required: true|false` per
   $MUTAGEN_ROOT/guides/pipeline-modes.md.

Your write scope for this spawn: slices/**, .mutagen/state/** (see
.mutagen/state/active-slice.json). Do not write anywhere else.
PROMPT
)"
```

Capture Shredder's stdout for the After step.

## After Shredder returns

1. **Persist the validation report.** Extract Shredder's Validation Report
   and write two files:
   - `.mutagen/state/validation-report.md` — full markdown verbatim.
   - `.mutagen/state/validation-report.json` — structured summary:
     ```json
     {
       "date": "YYYY-MM-DD",
       "generated_by": "Shredder",
       "bundle_ready": true,
       "readiness_issues": [],
       "validation_findings": [
         { "check": "ADR ↔ ISC", "severity": "flag", "summary": "..." }
       ],
       "pipeline_mode": "full | lightweight"
     }
     ```
     `bundle_ready` is `false` when Shredder returned a Readiness Report or
     surfaced a blocking cross-doc conflict.
2. Surface the full Slice Queue to the user for review. If Shredder did not
   write both `slices/queue.json` and `slices/slicemap.md`, stop and flag
   it.
3. Re-render from the JSON:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/render_queue.sh"
   ```

   This normalizes `slices/slicemap.md` and refreshes `slices/queue.md` as
   a compatibility shadow.
4. **Validate the canonical queue before execution.** Run:

   ```bash
   validator_json="$(bash "$MUTAGEN_ROOT/scripts/validate_queue.sh" slices/queue.json)"
   validator_status=$?
   printf '%s\n' "$validator_json" > .mutagen/state/queue-validation.json
   ```

   Handle the exit code strictly:
   - `0` → queue valid, continue.
   - `2` → queue parsed but failed harness validation. Surface the JSON
     report verbatim, clear `.mutagen/state/active-slice.json`, and stop.
     Do **not** recommend `$mutagen-execute-next`.
   - anything else → tooling failure. Surface the JSON payload verbatim,
     clear `.mutagen/state/active-slice.json`, and stop.
5. Surface any deviations, conflicts, or escalation items Shredder flagged.
6. Clear the active-slice state file: `rm -f .mutagen/state/active-slice.json`.
7. Tell the user the next step is `$mutagen-execute-next`.

## Reminders

- Shredder does not execute slices. He **authors the plan**. Karai executes.
- If Shredder's Readiness Check fails, stop and escalate — do not author a
  partial queue. Still persist the Readiness Report to
  `.mutagen/state/validation-report.{md,json}` so `$mutagen-status` can see
  why slicing halted.
- A queue that fails `$MUTAGEN_ROOT/scripts/validate_queue.sh` is not
  executable, even if the markdown rendering looks fine. The harness
  validator is the first consumer that gets to veto it.
- Numbered IDs in slice citations MUST match upstream exactly.
- `slices/queue.json` is canonical. `slices/slicemap.md` is the primary
  human rendering. `slices/queue.md` is a legacy shadow regenerated for
  compatibility.
