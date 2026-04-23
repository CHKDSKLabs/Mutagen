---
name: mutagen-status
description: Explicit-only skill. Read-only report on the mutagen workflow — upstream document status, April's Readiness Brief, Shredder's Validation Report, pipeline mode, slice queue progress, harness queue validation, active slice, heartbeat telemetry, gate telemetry, open escalations, recent reviews. Invoke only when the user explicitly says $mutagen-status.
---

# $mutagen-status — report on the workflow

Strictly read-only. Collect state, report concisely, recommend the next
action.

## Fast path

1. Run `bash "$MUTAGEN_ROOT/scripts/status.sh" --format markdown`.
2. If it succeeds, use that as the primary status report.
3. Fall back to the manual collection steps below only when the helper fails,
   when a required file is malformed, or when you need to augment the helper
   with extra context it could not derive.

## Collect

1. **Upstream documents.** For each of PRD / ADR / DDD / ISC / DSD, find the
   instantiated file (check `docs/` conventions and repo root) and read its
   status line. Report path, status (Draft / In Review / Approved / Accepted
   / Missing), `Last reviewed` date if present, count of `<TBD>` markers.
2. **April's Readiness Brief.** Read `.mutagen/state/readiness-brief.json`
   if present. Report `date`, per-document status, Shredder readiness
   projection (green / yellow / red), `recommendation`. If present, trust it
   over re-computing from raw docs. If absent, fall back to step 1.
3. **Shredder's Validation Report.** Read `.mutagen/state/validation-report.json`
   if present. Report `date`, `bundle_ready`, summarise `readiness_issues`
   and `validation_findings`. Flag as stale if the bundle has been edited
   since.
4. **Pipeline mode.** Read `.claude/workflow.json`. Report mode
   (`full` / `lightweight`), `review.max_retries`, heartbeat thresholds.
   Absent = default `full`, default retries = 2.
5. **Slice queue.** Prefer `slices/queue.json`; fall back to
   `slices/slicemap.md` when JSON is missing, and only then to legacy
   `slices/queue.md` if needed. Report:
   - Total slices, grouped by layer.
   - Per-status counts: `pending`, `in_progress`, `completed`, `refused`,
     `escalated`, `blocked_retry`.
   - Next pending slice: ID, assigned agent, layer, one-line objective,
     `attempts`, `review_required` (in lightweight mode).
6. **Queue validation.** Read `.mutagen/state/queue-validation.json` if
   present. Treat it as the harness verdict on whether `slices/queue.json`
   is executable. Report:
   - `ok`, `error_count`, `warning_count`.
   - Any issues, summarised with `level`, `code`, `slice_id` when present,
     and `message`.
   - Whether the report is stale. If `slices/queue.json` has a newer
     modified time than `.mutagen/state/queue-validation.json`, flag the
     validator report as stale rather than suppressing it.
   - If `slices/queue.json` is missing but the validator report exists,
     flag it as orphaned.
7. **Active slice.** Read `.mutagen/state/active-slice.json` if present.
   Report slice ID, `stage`, `active_agent`, `host`, `attempts`, and any
   `degraded_capabilities`. If the file exists outside a
   `$mutagen-execute-next` run, flag it.
8. **Heartbeat telemetry (only if an active slice exists).** Run
   `bash "$MUTAGEN_ROOT/scripts/heartbeat.sh" 300`. Report the JSON
   (`total`, `window_calls`, `bytes_last_window`, `last_run_length`). If
   `last_run_length >= LOOP_THRESHOLD` (default 5), flag a likely tool-call
   loop.
9. **Gate telemetry.** From `slices/queue.json`, report Bishop and Tiger
   Claw verdict counts across the last 10 completed slices.
10. **Open escalations.** From `slices/queue.json`, list slices whose
   `status` is `escalated`, `refused`, or `blocked_retry` with their
   `escalation_reason`.
11. **Reviews.** Count entries under `reviews/` and list the last three.

## Report

```
mutagen workflow status — {YYYY-MM-DD HH:MM}

Upstream documents:
  PRD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  ADR  {aggregate status} · {count} accepted / {count} draft
  DDD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  ISC  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD
  DSD  {status} ({path})  · TBDs: N · last reviewed: YYYY-MM-DD

April Readiness Brief ({date} · {mode}):
  Recommendation: {...}
  Shredder readiness: PRD 🟢 · ADR 🟢 · DDD 🟡 · ISC 🔴 · DSD 🟢
  Cross-doc issues: N                                (or "none noted")
  (or: no readiness brief on file — run $mutagen-elicit)

Shredder Validation Report ({date}):
  Bundle ready: true | false
  Readiness issues: N       · Validation findings: N
  (or: no validation report on file — run $mutagen-slice)

Pipeline mode: full | lightweight   ·  max retries: N

Slice queue (slices/queue.json):
  Total: N  ·  pending: N · in_progress: N · completed: N · blocked_retry: N · refused: N · escalated: N
  By layer: L1: N · L2: N · L3: N · L4: N · L5: N · L6: N
  Next up: {slice-id}  ·  {author_agent}  ·  L{n}  ·  attempts {k} · "{objective}"

Queue validation ({path}):
  Executable: true | false                         (flag stale / orphaned when applicable)
  Errors: N  ·  warnings: N
  Issues: none noted
  - [error|warning] {code} · {slice-id?}: {message}
  (or: no queue validation report on file — run $mutagen-slice)

Active slice:
  {slice-id}  ·  stage: {stage}  ·  agent: {active_agent}  ·  host: {host}  ·  attempts {k}
  Degraded host features: {comma-separated list}   (omit when empty)
  Heartbeat (last 5 min): calls {N} · bytes {N} · last-run {tool}×{len}
    (⚠ tool-call loop detected) | (⚠ stalled) | (nominal)
  (or: no active slice)

Latest scope violation:
  {slice-id}  ·  stage: {stage}  ·  agent: {active_agent}  ·  class: {class}
  path: {path}
  recorded: {ts}
  artifact: .mutagen/state/scope-violation.json
  (or: none)

Recent gate telemetry (last N completed slices):
  Bishop:     clean M · advisory N · block 0 · skipped K
  Tiger Claw: clean M · gap N · defect 0 · skipped K

Open escalations: {count}
  - {slice-id} [{status}]: {escalation_reason}

Recent reviews (last 3):
  - {slice-id}: 🟢/🟡/🔴 — reviews/{slice-id}.md

Next actions:
  - {e.g. "PRD still Draft — run $mutagen-elicit"}
  - {e.g. "Queue validation stale — re-run $mutagen-slice before dispatch"}
  - {e.g. "Queue invalid — fix Shredder output before $mutagen-execute-next"}
  - {e.g. "Queue clear — design bundle may need a new elicitation round"}
  - {e.g. "Escalation pending on L3-Auth-0001 — awaiting user decision"}
```

## Reminders

- **Read-only.** Never writes. Flag a stale `.mutagen/state/active-slice.json`
  rather than modifying it.
- If any upstream document is missing, recommend `$mutagen-elicit`.
- If all Approved but no queue, recommend `$mutagen-slice`.
- If `.mutagen/state/queue-validation.json` is missing, stale, orphaned, or
  reports `ok: false`, recommend `$mutagen-slice` instead of
  `$mutagen-execute-next`.
- If a queue has pending slices and the queue validation report is current
  with `ok: true`, recommend `$mutagen-execute-next`.
- If an escalation is open, recommend resolving before proceeding.
- If `.mutagen/state/scope-violation.json` exists, surface it even when the
  queue already shows the slice escalated — it is the canonical detail
  payload for the latest Traag DENY.
- If the active slice reports `degraded_capabilities`, surface them plainly.
  "Serial only" and "advisory scope" are runtime facts, not folklore.
- If heartbeat shows a high `last_run_length`, recommend investigating.
- Shredder's Validation Report tells you whether the design bundle was
  sliceable. The queue validation report tells you whether the emitted queue
  is actually executable.
