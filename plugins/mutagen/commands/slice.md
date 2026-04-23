---
description: Run Shredder against the approved upstream design bundle to produce a dependency-ordered slice queue.
---

# Slice — run Shredder on the approved design bundle

The user has invoked `/mutagen:slice`. You are orchestrating the Shredder subagent to consume the five upstream documents and emit a slice queue.

## Preflight

1. Confirm the state directory: `mkdir -p .mutagen/state slices`.
2. **Readiness check.** Verify every upstream document exists and carries a status line indicating `Approved` (PRD / DDD / DSD) or `Accepted` (ADR / ISC). If any are missing or still `Draft`, **halt** and report to the user:
   - Which documents are missing.
   - Which are present but not Approved / Accepted.
   - Suggest running `/mutagen:elicit` to close the gaps.
3. Check for an existing slice queue at `slices/queue.json` (canonical), `slices/slicemap.md` (human-readable), or `slices/queue.md` (legacy shadow). If one exists and is non-empty, warn the user: re-slicing replaces the queue and may invalidate work in progress. Ask for explicit confirmation before proceeding.
4. Read the project's pipeline-mode setting from `.claude/workflow.json` if present. Default to `"full"` when absent. Modes:
   - `"full"` — every slice runs Bishop review + Tiger Claw adversarial QA.
   - `"lightweight"` — Bishop + Tiger Claw run only on slices Shredder tags `review_required: true`. See `${CLAUDE_PLUGIN_ROOT}/guides/pipeline-modes.md`.
5. Write the active-slice state file scoping Shredder's writes:

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

Spawn the Shredder subagent via the Agent tool with:

- `subagent_type`: `Shredder`.
- A prompt that:
  - Lists the paths of the five upstream documents.
  - States the pipeline mode.
  - Asks Shredder to run his Readiness Check and Validation Phase and emit a **Validation Report** as the first section of his response (even when the bundle is clean — `"bundle_ready": true` with no issues is a valid report).
  - Asks Shredder to produce the Slice Queue following his Output Protocol, writing both `slices/queue.json` (canonical, per [`guides/queue-schema.md`](../guides/queue-schema.md)) **and** `slices/slicemap.md` (human-readable, per [`guides/slicemap-spec.md`](../guides/slicemap-spec.md)).
  - Reminds him that every slice must cite upstream IDs (PRD `[FR-*]` / `[NFR-*]`, ADR, DDD, ISC `[ISC-NNN]`, DSD `[DSD-###]`).
  - Reminds him that every slice must carry `depends_on`, `write_set`, structured `implementation_details`, and `human_check_needed.{required,reason,resolved_at}`.
  - In lightweight mode, asks him to tag each slice `review_required: true | false` based on the criteria in `${CLAUDE_PLUGIN_ROOT}/guides/pipeline-modes.md`.

## After Shredder returns

1. **Persist the validation report.** Extract Shredder's Validation Report (the conflict-check section or Readiness Report he produced). Write it to two files:
   - `.mutagen/state/validation-report.md` — the full markdown verbatim.
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
     `bundle_ready` is `false` when Shredder returned a Readiness Report (halted on missing / draft docs) or surfaced a blocking cross-doc conflict. `readiness_issues` enumerates missing / draft documents; `validation_findings` enumerates cross-doc conflicts Shredder raised for the human (these may be informational and non-blocking).
2. Surface the full Slice Queue to the user for review. If Shredder did not write both `slices/queue.json` and `slices/slicemap.md`, stop and flag it — the JSON is required for Karai and the slicemap is required for humans.
3. Normalize the human renderings from the JSON: run `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh` so `slices/slicemap.md` is regenerated from the canonical queue and `slices/queue.md` is refreshed as a legacy shadow.
4. **Validate the canonical queue before execution.** Run `${CLAUDE_PLUGIN_ROOT}/scripts/validate_queue.sh slices/queue.json` and persist stdout to `.mutagen/state/queue-validation.json`.
   - If the script exits `0`, the queue is valid and execution may proceed.
   - If the script exits `2`, the queue parsed but failed harness validation. Surface the issues verbatim, clear `.mutagen/state/active-slice.json`, and stop. Do **not** recommend `/mutagen:execute-next`.
   - If the script exits anything else, treat it as a tooling failure. Surface the JSON error payload verbatim, clear `.mutagen/state/active-slice.json`, and stop.
5. Surface any deviations, conflicts, or escalation items Shredder flagged.
6. Clear the active-slice state file: `rm -f .mutagen/state/active-slice.json`. The queue is authored; no slice is yet in flight.
7. Tell the user the next step is `/mutagen:execute-next` to begin dispatching slices through Karai.

## Reminders

- Shredder does not execute slices. He **authors the plan**. Karai executes.
- If Shredder's Readiness Check fails on any document, stop and escalate — do not author a partial queue. Still persist the resulting Readiness Report to `.mutagen/state/validation-report.{md,json}` so `/mutagen:status` can see why slicing halted.
- A queue that fails `${CLAUDE_PLUGIN_ROOT}/scripts/validate_queue.sh` is not executable, even if the slicemap looks fine to a human. The harness validator is the first consumer that gets to be rude.
- Numbered IDs in slice citations MUST match upstream exactly; any renumbering in upstream docs post-slice is a bug.
- `slices/queue.json` is the canonical queue. `slices/slicemap.md` is the primary human rendering. `slices/queue.md` is a legacy shadow regenerated from the JSON for compatibility.

$ARGUMENTS
