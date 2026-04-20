---
description: Run Shredder against the approved upstream design bundle to produce a dependency-ordered slice queue.
---

# Slice — run Shredder on the approved design bundle

The user has invoked `/mutagen:slice`. You are orchestrating the Shredder subagent to consume the five upstream documents and emit a slice queue.

## Preflight

1. Confirm the state directory: `mkdir -p .claude/state slices`.
2. **Readiness check.** Verify every upstream document exists and carries a status line indicating `Approved` (PRD / DDD / DSD) or `Accepted` (ADR / ISC). If any are missing or still `Draft`, **halt** and report to the user:
   - Which documents are missing.
   - Which are present but not Approved / Accepted.
   - Suggest running `/mutagen:elicit` to close the gaps.
3. Check for an existing slice queue at `slices/queue.json` (preferred) or `slices/queue.md` (legacy). If one exists and is non-empty, warn the user: re-slicing replaces the queue and may invalidate work in progress. Ask for explicit confirmation before proceeding.
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
       ".claude/state/**"
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
  - Asks Shredder to produce the Slice Queue following his Output Protocol, writing both `slices/queue.json` (canonical, per [`guides/queue-schema.md`](../guides/queue-schema.md)) **and** `slices/queue.md` (human-readable rendering of the same data).
  - Reminds him that every slice must cite upstream IDs (PRD `[FR-*]` / `[NFR-*]`, ADR, DDD, ISC `[ISC-NNN]`, DSD `[DSD-###]`).
  - In lightweight mode, asks him to tag each slice `review_required: true | false` based on the criteria in `${CLAUDE_PLUGIN_ROOT}/guides/pipeline-modes.md`.

## After Shredder returns

1. **Persist the validation report.** Extract Shredder's Validation Report (the conflict-check section or Readiness Report he produced). Write it to two files:
   - `.claude/state/validation-report.md` — the full markdown verbatim.
   - `.claude/state/validation-report.json` — structured summary:
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
2. Surface the full Slice Queue to the user for review. If Shredder did not write both `slices/queue.json` and `slices/queue.md`, stop and flag it — the JSON is required for Karai.
3. Surface any deviations, conflicts, or escalation items Shredder flagged.
4. Clear the active-slice state file: `rm -f .claude/state/active-slice.json`. The queue is authored; no slice is yet in flight.
5. Tell the user the next step is `/mutagen:execute-next` to begin dispatching slices through Karai.

## Reminders

- Shredder does not execute slices. He **authors the plan**. Karai executes.
- If Shredder's Readiness Check fails on any document, stop and escalate — do not author a partial queue. Still persist the resulting Readiness Report to `.claude/state/validation-report.{md,json}` so `/mutagen:status` can see why slicing halted.
- Numbered IDs in slice citations MUST match upstream exactly; any renumbering in upstream docs post-slice is a bug.
- `slices/queue.json` is the canonical queue. `slices/queue.md` is a human rendering. Regenerate both whenever the queue changes.

$ARGUMENTS
