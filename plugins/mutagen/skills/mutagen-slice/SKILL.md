---
name: mutagen-slice
description: Explicit-only skill. Run Shredder against the approved upstream design bundle (PRD / ADR / DDD / ISC / DSD) to produce a dependency-ordered slice queue. Invoke only when the user explicitly says $mutagen-slice. Do not trigger on ambient mentions of "slice" or "slicing".
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
3. Check for an existing queue at `slices/queue.json` (preferred) or
   `slices/queue.md` (legacy). If non-empty, warn the user that re-slicing
   replaces the queue. Ask for explicit confirmation before proceeding.
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
   - `slices/queue.md` — human-readable rendering of the same data
3. Every slice must cite upstream IDs: PRD [FR-*]/[NFR-*], ADR, DDD, ISC
   [ISC-NNN], DSD [DSD-###].
4. In lightweight mode, tag each slice `review_required: true|false` per
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
   write both `slices/queue.json` and `slices/queue.md`, stop and flag it.
3. Surface any deviations, conflicts, or escalation items Shredder flagged.
4. Clear the active-slice state file: `rm -f .mutagen/state/active-slice.json`.
5. Tell the user the next step is `$mutagen-execute-next`.

## Reminders

- Shredder does not execute slices. He **authors the plan**. Karai executes.
- If Shredder's Readiness Check fails, stop and escalate — do not author a
  partial queue. Still persist the Readiness Report to
  `.mutagen/state/validation-report.{md,json}` so `$mutagen-status` can see
  why slicing halted.
- Numbered IDs in slice citations MUST match upstream exactly.
- `slices/queue.json` is canonical. `slices/queue.md` is a human rendering.
  Regenerate both whenever the queue changes.
