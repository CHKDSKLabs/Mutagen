---
description: Report workflow state — upstream document status, slice queue progress, active slice, gate telemetry, open escalations.
---

# Status — report on the workflow

The user has invoked `/mutagen:status`. Gather the current state and report concisely. This command is strictly read-only.

## Collect

1. **Upstream documents.** For each of PRD / ADR / DDD / ISC / DSD, find the instantiated file (check `docs/` conventions and repo root) and read the status line. Report for each: path, status (Draft / In Review / Approved / Accepted / Missing), `Last reviewed` date if present, count of `<TBD>` markers.
2. **April's Readiness Brief.** Read `.mutagen/state/readiness-brief.json` if present. Report its `date`, per-document status, Shredder readiness projection (green / yellow / red), and `recommendation`. If the JSON is present, trust it over re-computing from the raw documents (the brief encodes April's cross-document consistency judgment, which a status command cannot re-derive cheaply). If the JSON is absent, fall back to the raw-document scan from step 1.
3. **Shredder's Validation Report.** Read `.mutagen/state/validation-report.json` if present. Report `date`, `bundle_ready`, and summarise any `readiness_issues` or `validation_findings`. A stale report (bundle has been edited since) is still worth surfacing — flag it as stale rather than suppressing.
4. **Pipeline mode.** Read `.claude/workflow.json` if present. Report the mode (`full` / `lightweight`) and any relevant settings (including `review.max_retries` and the heartbeat thresholds if configured). Absent = default `full`, default retries = 2.
5. **Slice queue.** Prefer `slices/queue.json` (canonical); fall back to parsing `slices/queue.md` only if the JSON is missing. Report:
   - Total slices, grouped by layer.
   - Per-status counts: `pending`, `in_progress`, `completed`, `refused`, `escalated`, `blocked_retry`.
   - The next pending slice: ID, assigned agent, layer, one-line objective, `attempts` count, `review_required` (in lightweight mode).
6. **Active slice.** Read `.mutagen/state/active-slice.json` if present. Report the slice ID, current `stage`, `active_agent`, and `attempts`. If the file exists outside an `/mutagen:execute-next` run, flag it — it means a prior run did not clean up.
7. **Heartbeat telemetry (only if an active slice exists).** Run `${CLAUDE_PLUGIN_ROOT}/scripts/heartbeat.sh 300` and report its JSON summary (`total`, `window_calls`, `bytes_last_window`, `last_run_length`). If `last_run_length` is ≥ the configured `LOOP_THRESHOLD` (default 5), flag it as a likely tool-call loop.
8. **Gate telemetry.** From `slices/queue.json`, report Bishop and Tiger Claw verdict counts across the last 10 completed slices (use `verdicts.bishop` and `verdicts.tiger_claw`).
9. **Open escalations.** From `slices/queue.json`, list slices whose `status` is `escalated`, `refused`, or `blocked_retry` along with their `escalation_reason`.
10. **Reviews.** Count entries under `reviews/` and list the last three with their verdict and slice ID.

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

April Readiness Brief ({date} · {mode}):
  Recommendation: {Ready for Shredder | Ready after N items | Not yet — ...}
  Shredder readiness: PRD 🟢 · ADR 🟢 · DDD 🟡 · ISC 🔴 · DSD 🟢
  Cross-doc issues: N                                (or "none noted")
  (or: no readiness brief on file — run /mutagen:elicit)

Shredder Validation Report ({date}):
  Bundle ready: true | false
  Readiness issues: N        (enumerate if small)
  Validation findings: N     (enumerate if small)
  (or: no validation report on file — run /mutagen:slice)

Pipeline mode: full | lightweight   ·  max retries: N

Slice queue (slices/queue.json):
  Total: N  ·  pending: N · in_progress: N · completed: N · blocked_retry: N · refused: N · escalated: N
  By layer: L1: N · L2: N · L3: N · L4: N · L5: N · L6: N
  Next up: {slice-id}  ·  {author_agent}  ·  L{n}  ·  attempts {k} · "{objective}"

Active slice:
  {slice-id}  ·  stage: {stage}  ·  agent: {active_agent}  ·  attempts {k}
  Heartbeat (last 5 min): calls {N} · bytes {N} · last-run {tool}×{len}
    (⚠ tool-call loop detected) | (⚠ stalled) | (nominal)
  (or: no active slice)

Recent gate telemetry (last N completed slices):
  Bishop:     clean M · advisory N · block 0 · skipped K
  Tiger Claw: clean M · gap N · defect 0 · skipped K

Open escalations: {count}
  - {slice-id} [{status}]: {escalation_reason}

Recent reviews (last 3):
  - {slice-id}: 🟢/🟡/🔴 — reviews/{slice-id}.md
  - ...

Next actions:
  - {e.g. "PRD still Draft — run /mutagen:elicit"}
  - {e.g. "Queue clear — design bundle may need new elicitation round"}
  - {e.g. "Escalation pending on L3-Auth-0001 — awaiting user decision"}
  - {e.g. "Active slice stale — .mutagen/state/active-slice.json present but no /mutagen:execute-next in progress"}
```

## Reminders

- **Read-only.** `/mutagen:status` never writes. If `.mutagen/state/active-slice.json` exists from a previous command, do not modify it; flag it.
- If any upstream document is missing, recommend `/mutagen:elicit`.
- If all are Approved but there is no queue, recommend `/mutagen:slice`.
- If there is a queue with pending slices, recommend `/mutagen:execute-next`.
- If there is an open escalation, recommend resolving it before proceeding.
- If the active-slice JSON shows a high `last_run_length` on heartbeat, recommend investigating before re-dispatching.

$ARGUMENTS
