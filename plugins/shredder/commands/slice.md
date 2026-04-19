---
description: Run Shredder against the approved upstream design bundle to produce a dependency-ordered slice queue.
---

# Slice — run Shredder on the approved design bundle

The user has invoked `/shredder:slice`. You are orchestrating the Shredder subagent to consume the five upstream documents and emit a slice queue.

## Preflight

1. Confirm the state directory: `mkdir -p .claude/state`.
2. **Readiness check.** Verify every upstream document exists and carries a status line indicating `Approved` (PRD / DDD / DSD) or `Accepted` (ADR / ISC). If any are missing or still `Draft`, **halt** and report to the user:
   - Which documents are missing.
   - Which are present but not Approved / Accepted.
   - Suggest running `/shredder:elicit` to close the gaps.
3. Check for an existing slice queue at `slices/queue.md` (or wherever the project has established). If one exists and is non-empty, warn the user: re-slicing replaces the queue and may invalidate work in progress. Ask for explicit confirmation before proceeding.
4. Read the project's pipeline-mode setting from `.claude/workflow.json` if present. Default to `"full"` when absent. Modes:
   - `"full"` — every slice runs Bishop review + Tiger Claw adversarial QA.
   - `"lightweight"` — Bishop + Tiger Claw run only on slices Shredder tags `review_required: true`. See `${CLAUDE_PLUGIN_ROOT}/guides/pipeline-modes.md`.
5. Write the active-slice state file scoping Shredder's writes:

   ```json
   {
     "slice_id": "shredder-{YYYY-MM-DD-HHMM}",
     "author_agent": "Shredder",
     "allowed_write_globs": [
       "slices/**",
       "project_state.md",
       "infrastructure_state.md",
       ".claude/state/**"
     ]
   }
   ```

## Dispatch

Spawn the Shredder subagent via the Agent tool with:

- `subagent_type`: `Shredder`.
- A prompt that (a) lists the paths of the five upstream documents, (b) states the pipeline mode, (c) asks Shredder to produce the Slice Queue following his Output Protocol, (d) reminds him that every slice must cite upstream IDs (PRD `[FR-*]` / `[NFR-*]`, ADR, DDD, ISC `[ISC-NNN]`, DSD `[DSD-###]`), (e) tells him to write the queue to `slices/queue.md`, and (f) in lightweight mode, asks him to tag each slice `review_required: true | false` based on the criteria in `${CLAUDE_PLUGIN_ROOT}/guides/pipeline-modes.md`.

## After Shredder returns

1. Surface the full Slice Queue to the user for review.
2. Surface any deviations, conflicts, or escalation items Shredder flagged.
3. Clear the active-slice state file: `rm -f .claude/state/active-slice.json`. The queue is authored; no slice is yet in flight.
4. Tell the user the next step is `/shredder:execute-next` to begin dispatching slices through Karai.

## Reminders

- Shredder does not execute slices. He **authors the plan**. Karai executes.
- If Shredder's Readiness Check fails on any document, stop and escalate — do not author a partial queue.
- Numbered IDs in slice citations MUST match upstream exactly; any renumbering in upstream docs post-slice is a bug.

$ARGUMENTS
