---
description: Turn the Bishop advisory backlog into a cleanup slice (or series) by dispatching Shredder against .mutagen/state/advisory-backlog.jsonl.
---

# Consolidate-advisories — promote non-blocking findings into plannable work

The user has invoked `/mutagen:consolidate-advisories`. Bishop's 🟡 Advisory findings are legitimate but non-blocking; `/mutagen:execute-next` records them to `.mutagen/state/advisory-backlog.jsonl` (one JSON object per line, appended as slices complete). Left alone, they rot in `reviews/` and never get addressed. This command walks Shredder through authoring a cleanup slice — or a short series — that closes them.

## Preflight

1. Read `.mutagen/state/advisory-backlog.jsonl`. If the file is missing or empty, report "advisory backlog clear — nothing to consolidate" and stop.
2. Parse each line as a JSON object with `{slice_id, severity, category, location, assertion, remedy, recorded_at}`. Silently skip malformed lines but surface the count to the user at the end so they know to clean up.
3. Read `slices/queue.json`. If the queue still has `pending` or `blocked_retry` slices, tell the user — a cleanup slice authored into a mid-run queue can clash with in-flight work. Offer two paths: (a) let the current queue finish and re-run this command, or (b) author the cleanup slice as a standalone queue in a separate `slices/cleanup-queue-{YYYY-MM-DD}.json`. Default to (a) unless the user directs otherwise.
4. Read the design bundle (PRD / ADRs / DDD / ISC / DSD) the same way `/mutagen:execute-next` does — Shredder needs the bundle in context to keep the cleanup slice traced.

## Dispatch

1. Spawn Shredder via the Agent tool. Prompt includes:
   - The full contents of `.mutagen/state/advisory-backlog.jsonl` inlined.
   - The design-bundle documents inlined (same treatment `/mutagen:slice` gives Shredder).
   - The current `slices/queue.json` for reference on ID conventions and ordering.
   - Explicit instruction: *"Cluster these advisories by file path and category. Author a cleanup slice — or a short series, one per coherent cluster — that addresses each cluster with a single minimal change. Every cleanup slice must carry its own Traces-to block — trace each finding back to the originating slice's citations (PRD / ADR / DDD / ISC / DSD). Route each slice to the author agent whose globs cover the touched files; Bebop is the default fallback for cross-cutting test / wiring cleanup. Target LOC ≤ 200 per cleanup slice; refuse to pack a cluster that wouldn't fit and split it instead."*
   - Instruction on where to write: append to `slices/queue.json` (option (a) above) or author a fresh `slices/cleanup-queue-{YYYY-MM-DD}.json` (option (b)). Re-render `slices/slicemap.md` and refresh legacy `slices/queue.md` via `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh` after mutation.

2. Wait for Shredder to return. Capture the authored slice(s).

## Post-dispatch

1. **Archive the consumed backlog.** Move the current `.mutagen/state/advisory-backlog.jsonl` to `.mutagen/state/advisory-backlog-consumed-{YYYY-MM-DD-HHMM}.jsonl`. Start a fresh empty backlog at the original path so `/mutagen:execute-next` can keep appending new advisories without re-consuming the old ones. If Shredder refused to author a cleanup slice (e.g. all advisories were stale / already addressed), record the reason in a sibling `.notes.md` alongside the archive and **do not** create a fresh empty file — leave the existing backlog alone so the user can inspect.
2. Report to the user: how many advisories were consumed, how many cleanup slices were authored, and whether the cleanup was appended to the main queue or written as a standalone file.
3. **Do not auto-dispatch the cleanup slices.** Authoring is enough; the human chooses when to run `/mutagen:execute-next` against them.

## On refusal

If Shredder returns a refusal (backlog is hollow / contradictory / cannot be traced back to a valid citation), surface the refusal verbatim and leave the backlog intact. The human decides whether to hand-edit the backlog, re-slice the originating slices, or dismiss the advisories outright.

$ARGUMENTS
