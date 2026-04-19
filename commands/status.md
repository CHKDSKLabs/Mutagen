---
description: Report workflow state — upstream document status, slice queue progress, active slice, gate telemetry, open escalations.
---

# Status — report on the workflow

The user has invoked `/shredder:status`. Gather the current state and report concisely.

## Collect

1. **Upstream documents.** For each of PRD / ADR / DDD / ISC / DSD, find the instantiated file (check `docs/` conventions and repo root) and read the status line. Report for each: path, status (Draft / In Review / Approved / Accepted / Missing), `Last reviewed` date if present, count of `<TBD>` markers.
2. **Pipeline mode.** Read `.claude/workflow.json` if present. Report the mode (`full` / `lightweight`) and any relevant settings. Absent = default `full`.
3. **Slice queue.** Read `slices/queue.md` if present. Report:
   - Total slices, grouped by layer.
   - Per-status counts: `pending`, `in-flight`, `completed`, `refused`, `escalated`.
   - The next pending slice: ID, assigned agent, layer, one-line objective.
4. **Active slice.** Read `.claude/state/active-slice.json` if present. Report the slice ID and author agent. If the file exists outside an `/shredder:execute-next` run, flag it — it means a prior run did not clean up.
5. **Gate telemetry.** Scan `project_state.md` and `infrastructure_state.md` for Karai's recent Completion Rollup entries (if any). Report Bishop's advisory / block / skipped counts and Tiger Claw's gap / defect / skipped counts over the last 10 completed slices (or however many exist).
6. **Open escalations.** Search state files for unresolved escalation markers (`escalated`, `Block`, `Defect` without resolution). List them with slice IDs.
7. **Reviews.** Count entries under `reviews/` and list the last three with their verdict and slice ID.

## Report

Produce a concise, scannable status block. Template:

```
Shredder workflow status — {YYYY-MM-DD HH:MM}

Upstream documents:
  PRD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  ADR  {aggregate status} · {count} accepted / {count} draft
  DDD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  ISC  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  DSD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD

Pipeline mode: full | lightweight

Slice queue ({path}):
  Total: N  ·  pending: N · in-flight: N · completed: N · escalated: N
  By layer: L1: N · L2: N · L3: N · L4: N · L5: N · L6: N
  Next up: {slice-id}  ·  {author_agent}  ·  L{n}  ·  "{objective}"

Active slice:
  {slice-id}  ·  author: {agent}  ·  state-file present
  (or: no active slice)

Recent gate telemetry (last N slices):
  Bishop: clean M · advisory N · block 0 · skipped K
  Tiger Claw: clean M · gap N · defect 0 · skipped K

Open escalations: {count}
  - {slice-id}: {reason}

Recent reviews (last 3):
  - {slice-id}: 🟢/🟡/🔴 — reviews/{slice-id}.md
  - ...

Next actions:
  - {e.g. "PRD still Draft — run /shredder:elicit"}
  - {e.g. "Queue clear — design bundle may need new elicitation round"}
  - {e.g. "Escalation pending on L3-Auth-0001 — awaiting user decision"}
```

## Reminders

- **Read-only.** `/shredder:status` never writes. If `.claude/state/active-slice.json` exists from a previous command, do not modify it; flag it.
- If any upstream document is missing, recommend `/shredder:elicit`.
- If all are Approved but there is no queue, recommend `/shredder:slice`.
- If there is a queue with pending slices, recommend `/shredder:execute-next`.
- If there is an open escalation, recommend resolving it before proceeding.

$ARGUMENTS
