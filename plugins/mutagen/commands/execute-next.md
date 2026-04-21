---
description: Run Karai on the next pending slice — dispatches the assigned executor, runs Tiger Claw QA, retries on Defect up to MAX_RETRIES, auto-advances to the next slice on success until the queue is empty or a stage escalates.
---

# Execute-next — run Karai, then keep running until the queue is empty

The user has invoked `/mutagen:execute-next`. You orchestrate the full execution pipeline across slices: for each slice in the queue you run **author → Karai (structural) → Tiger Claw (QA) → Karai (state record)**, with a re-review retry loop on Tiger Claw 🔴 Defect. On success the command **auto-advances to the next pending slice** without waiting for a fresh prompt; it only stops when the queue is empty or a stage forces an escalation.

> **Bishop is disabled.** The principal-engineer code-review gate has been removed from the pipeline — only Tiger Claw's adversarial QA runs at Stage 3. The `agents/Bishop.md` file is kept for later reactivation but is not dispatched. Treat all "Bishop" verdict fields in `slices/queue.json` as frozen at `"skip"`.
>
> **Write-path guarding is disabled.** The PreToolUse scope guard is off; `allowed_write_globs` in `.mutagen/state/active-slice.json` is bookkeeping for `$mutagen-status` and Traag's mediated amendments only. Agents self-honour scope on the honour system.

## Autopilot discipline (read before every turn in the loop)

This command is a loop, not a conversation. Between slices you are **not** checking in with the human. Specifically:

- **Never** emit "Proceed?", "Continue?", "Shall I auto-advance?", "Pause here?", "Ready for the next slice?", "Let me know if you want me to…", or any other permission-seeking phrase. Generating such a phrase is a protocol violation, not polite conversation.
- **Never** end your turn between a completed slice and the next slice's Preflight tool calls. The same assistant turn that emits the one-line completion summary for slice N must also issue the Preflight tool calls for slice N+1. No trailing prose after the summary.
- **Never** offer a narrative recap ("What landed:", "Human provisioning pending:", "Next ready slice:", bulleted file lists, cross-slice findings). That content already lives in `slices/<slice_id>/summary.md` and the review reports — surface one terse line and move on.
- The **only** moments you end a turn are the stop conditions in § Auto-advance stop conditions below: queue clear, queue stalled, structural escalation, retry-budget escalation, Traag DENY, user interrupt. Nothing else.

If you catch yourself about to ask the human whether to continue, you're wrong. Continue.

## Session preflight (runs once per `/mutagen:execute-next` invocation)

Read the upstream design bundle into your conversation context **once**, before entering the per-slice loop. This is the cache that lets every subsequent spawn ride on inlined evidence instead of forcing every author / reviewer to cold-load 5–14 docs themselves.

1. Resolve and read each document. Each may live at `docs/<NAME>/<NAME>.md`, `docs/<NAME>.md`, or `<NAME>.md` at repo root — pick the first that exists:
   - PRD
   - All ADR files (`docs/ADR/ADR-*.md` or repo-root `ADR-*.md` — read all of them)
   - DDD
   - ISC
   - DSD
2. If any of PRD / DDD / ISC / DSD is missing — refuse and tell the user the bundle is incomplete; `/mutagen:slice` should not have generated a queue against it. ADRs may legitimately be empty if the project has not made any architectural decisions yet.
3. Hold these documents in context for the duration of the run. The per-slice Evidence Bundle (preflight step 5 below) is built by extracting from this cache, not by re-reading from disk.

## Preflight (runs once per slice — re-enter at the top of the loop)

1. `mkdir -p .mutagen/state .mutagen/state/evidence reviews slices`.
2. Read `slices/queue.json` (canonical — see [`guides/queue-schema.md`](../guides/queue-schema.md)). If the JSON is missing but `slices/queue.md` exists, refuse and tell the user to re-run `/mutagen:slice` — Karai drives from the JSON. Find the first **ready** slice whose status is `pending` or `blocked_retry`. A slice is ready iff every ID in its `depends_on` array (if any) has `status == "completed"` in the queue. Slices with unmet dependencies are skipped — never run them out of order just because an earlier one is blocked. If nothing is ready (queue has no pending / blocked_retry, *or* every remaining pending slice has an unmet dep), report "queue clear — nothing left to dispatch" (or "queue stalled — dependencies unmet: <list>" if the latter) and stop.
3. Read `.claude/workflow.json` (if present). Extract:
   - `pipeline_mode` — `"full"` (default) runs every slice through Tiger Claw; `"lightweight"` runs QA only on slices with `review_required: true`.
   - `review.max_retries` — how many re-dispatch attempts after a 🔴 Block or 🔴 Defect before escalating. Default: **2** (i.e. up to 3 total author dispatches per slice).
   - `review.max_micro_corrections` — how many one-shot mechanical-fix dispatches may fire against a single slice, evaluated independently of `attempts`. Default: **1**. See the Re-review retry loop for semantics.
   - `max_parallel_slices` — how many ready sibling slices may run concurrently. Default: **1** (fully serial; preserves every behavior of prior mutagen builds). Values > 1 activate bounded parallel dispatch via Agent-tool worktree isolation — see § Bounded parallel dispatch.
   - `heartbeat.*` — optional inspection thresholds (see `agents/Karai.md`).
4. Extract from the chosen slice (straight from the JSON):
   - `slice_id`, `author_agent`, `layer`, `bounded_context`, `title`, `objective`
   - `traces_to` (PRD / ADR / DDD / ISC / DSD citations)
   - `review_required` (lightweight mode only)
   - `attempts` (starts at 0 on a fresh slice; carries the count if the slice was previously `blocked_retry`)
   - `context_to_update` (`project_state.md` or `infrastructure_state.md`)
   - `adjacent_scope_allowed` (optional array of globs; may be absent / empty on slices where Shredder did not anticipate cross-cutting work)
   - `depends_on` (optional array of slice IDs; used by the DAG readiness check in preflight step 2)
5. **Build the Evidence Bundle for this slice and write it to disk.** From the slice's `traces_to` block, resolve every citation to a verbatim excerpt out of the bundle docs you cached in Session preflight. Assemble the bundle once, then **write it to `.mutagen/state/evidence/<slice_id>.md`**. Every subsequent spawn in this slice (author, Tiger Claw, and any retry re-spawn) receives the file *path* plus the instruction *"Read this file once; do not re-read upstream docs."* — not the inlined text. This keeps the prompt small, cache-friendly across retries, and guarantees every agent sees byte-identical evidence. If the file already exists from a prior attempt on the same slice, overwrite it — the citation set is a function of the slice, not the attempt.

   Citation forms to handle:
   - `[FR-NNN]` / `[NFR-NNN]` → grep PRD for the bracketed ID; include the line plus the parent bullet/section header (typically 2–10 lines)
   - `ADR-NNNN` → include the entire ADR file (they're short and the Decision section without Context is often misleading)
   - DDD element (aggregate / command / query / event named in `traces_to.ddd`) → include the named section verbatim, plus any `[INV-N]` lines under it
   - `[ISC-NNN]` → grep ISC for the bracketed ID; include the line plus surrounding context that defines the invariant and its detection
   - `[DSD-NNN]` → grep DSD for the bracketed ID; include the rule line and the section heading it sits under

   Structure the bundle as:

   ```
   ## Evidence Bundle for <slice_id>

   ### PRD citations
   <verbatim excerpts, one block per [FR-*]/[NFR-*]>

   ### ADR(s)
   <full ADR text per cited ADR-NNNN>

   ### DDD citations
   <named element + cited [INV-*] lines>

   ### ISC citations
   <verbatim excerpts, one block per [ISC-NNN]>

   ### DSD citations
   <verbatim excerpts, one block per [DSD-NNN]>
   ```

   If a citation cannot be resolved (ID not found in the cited doc), halt and escalate — the slice queue is referencing evidence that doesn't exist, which is a Shredder bug, not something to paper over.

6. Initialise the active-slice state file with the **author** stage's scope. Per-stage rotation rewrites this file at each stage transition so the PreToolUse guard only grants the exact paths a given agent needs:

   ```json
   {
     "slice_id": "<from queue>",
     "author_agent": "<from queue>",
     "active_agent": "<same as author_agent for stage 1>",
     "stage": "author",
     "pipeline_mode": "full | lightweight",
     "review_required": true,
     "attempts": <current from queue>,
     "max_retries": <from workflow.json or 2>,
     "micro_corrections_used": <current from queue or 0>,
     "max_micro_corrections": <from workflow.json or 1>,
     "allowed_write_globs": [ "<author paths — see table below>" ]
   }
   ```

   **Adjacent-scope merge (retry only).** On first dispatch (`attempts == 0` before the Stage 1 bump), `allowed_write_globs` is strictly the author paths + state paths. On any retry dispatch (`attempts >= 1` before Stage 1 bumps it further), if the slice carries a non-empty `adjacent_scope_allowed`, append those globs to `allowed_write_globs`. Shredder anticipated these cross-cutting files; the retry loop gets to use them without the human having to hand-edit the manifest. Micro-correction dispatches count as retries for this rule.

   For the review stage the `active_agent` field is `"TigerClaw"`.
7. Mark the slice `in_progress` in `slices/queue.json` (overwrite status). After every mutation of `queue.json` in any stage below, re-render the markdown: `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh`.

## Per-stage scope manifests

The guard (`scripts/guard.sh`) reads `allowed_write_globs` on every Write / Edit. Rewriting that list between stages is the mechanism that enforces per-subagent scope without per-subagent hooks. For each stage below, **overwrite `.mutagen/state/active-slice.json`** with the manifest shown before spawning.

| Stage | `active_agent(s)` | `allowed_write_globs` |
|-------|-------------------|------------------------|
| `author` | author agent | author paths (table below) + `project_state.md` + `infrastructure_state.md` + `.mutagen/state/**` |
| `karai_structural` | `Karai` | `.mutagen/state/**` (Karai emits a report; she does not write to project files at this stage) |
| `review_qa` | `TigerClaw` | `reviews/**` + `tests/qa/**` (+ `tests/qa/security/**` when `author_agent == "Tatsu"`) + `.mutagen/state/**` |
| `karai_state` | `Karai` | `project_state.md` + `infrastructure_state.md` + `slices/**` + `.mutagen/state/**` |

The `review_qa` manifest is bookkeeping only (the guard is disabled). Tiger Claw writes under `tests/qa/**` and drops her report under `reviews/<slice_id>/`.

### Author paths per agent

| author_agent | author paths |
|--------------|--------------|
| Bebop | `src/**`, `app/**`, `api/**`, `components/**`, `pages/**`, `tests/**` (excluding `tests/qa/**`, `tests/security/**`, `tests/db/**`), `styles/**`, `public/**` |
| Baxter | cited algorithmic modules + their tests |
| Chaplin | `migrations/**`, `schema/**`, `db/**`, `prisma/**`, `src/models/**`, `src/queries/**`, `src/repositories/**`, `seeds/**`, `tests/db/**`, `tests/migrations/**` |
| Metalhead | `observability/**`, `dashboards/**`, `alerts/**`, `slo/**`, `runbooks/alerts/**`, `src/instrumentation/**`, `src/tracing/**`, `src/logging/**`, `src/metrics/**`, `src/telemetry/**`, `tests/observability/**` |
| Splinter | `docs/api/**`, `docs/onboarding/**`, `docs/guides/**`, `docs/how-to/**`, `docs/architecture/**`, `docs/migration/**`, `docs/glossary.md`, `runbooks/ops/**`, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md` |
| Tatsu | `src/security/**`, `src/auth/**`, `middleware/**`, `policies/**`, cited security-relevant migrations, `tests/security/**` |
| Krang | `.github/workflows/**`, `fly.toml`, `wrangler.toml`, `Dockerfile`, `docker-compose.*`, `infrastructure/**`, `terraform/**`, `migrations/**`, `.env.example` |

---

## Bounded parallel dispatch

When `max_parallel_slices > 1` in `.claude/workflow.json`, the orchestrator may fan out to multiple ready sibling slices at once. When `max_parallel_slices == 1` (default), skip this section — the pipeline runs strictly serial and nothing in the Dispatch sequence below changes.

### Readiness cohort

In preflight step 2, after identifying the first ready slice, keep walking the queue: collect up to `max_parallel_slices` ready slices that satisfy **all** of the following:

- Every slice in the cohort has `status` in {`pending`, `blocked_retry`} and all `depends_on` IDs are `completed`.
- No two slices in the cohort share an author-path glob after union. Parallel authors writing the same paths is a collision guarantee — the guard hook would deny it anyway, but catching it here avoids wasted dispatches. Disjoint contexts (e.g. four L2 data-model slices for four different bounded contexts) pass trivially; overlapping contexts do not.
- No slice in the cohort depends on another slice in the cohort. They must be true siblings in the DAG.
- Every slice in the cohort is in the same `layer`. Cross-layer parallelism is disallowed — a lower layer is always a potential upstream for a higher layer, and Shredder's ordering is authoritative.

If fewer than `max_parallel_slices` slices satisfy the constraints, run whatever subset is ready — do not wait for more slices to free up. Cohorts of 1 are fine; they behave identically to serial mode.

### Per-slice worktree isolation

Each slice in the cohort runs Stages 1 → 3 (author → structural check → parallel review) in an **isolated worktree** spawned via the Agent tool's `isolation: "worktree"` parameter. The orchestrator holds references to each worktree and treats each as an independent pipeline for the purposes of state, reports, and retry.

- `.mutagen/state/active-slice.json` is **per-worktree** when running parallel — i.e. each isolated Agent call initialises its own copy inside its own worktree. Don't try to multiplex the file in the main tree. The PreToolUse guard runs inside the worktree and reads that worktree's copy.
- `.mutagen/state/evidence/<slice_id>.md` is written by the orchestrator in the main tree *before* fanning out; each parallel Agent call reads its own slice's evidence file from the path the orchestrator passes in. Evidence files are per-slice, so there is no collision.
- `reviews/<slice_id>/` is written by the parallel reviewers inside their slice's worktree. The orchestrator merges these into the main tree during Stage 4 (below).
- `slices/queue.json` is **read-only** for parallel author / reviewer agents. Only the orchestrator (main tree, Stage 4) mutates the queue.

### Serial merge (Stage 4)

Stage 4 is serialized across the cohort, in deterministic order (the order slices appeared in `slices/queue.json`). For each returned worktree:

1. Inspect the slice's cohort status. If its Stages 1 – 3 returned with any verdict other than passing (structural fail, retry budget exhausted, escalation), halt the orchestrator — a failed slice inside a parallel cohort stops the whole run so the human sees one coherent state. Parallel siblings already in flight are allowed to complete their current stage, but nothing advances to Stage 4 for them; they mark as `status: "in_progress"` and wait for the human.
2. Otherwise, merge the worktree's changes back into the main tree. Concrete steps: `git merge --ff-only <worktree_branch>` from the main worktree; if the fast-forward fails (another cohort member merged first and their edits overlapped non-trivially), halt — overlapping edits were supposed to be caught by the path-disjointness check, so a non-FF merge is a bug in the cohort-selection logic and must be surfaced, not force-resolved.
3. Run Stage 4 (Karai state verify + dispatch log + advisory backlog append) for that slice normally, against the merged main-tree state.
4. Run Stage 5 (summary + auto-advance evaluation) for that slice.
5. Once every cohort member has been merged and recorded, return to Preflight to pick the next cohort.

If the cohort is size 1, all of the above collapses to the existing serial flow — no worktree needed, no merge step.

**Guardrail.** `max_parallel_slices` > 1 is an advanced mode. Start with 2 and only raise it after observing real behavior. Shredder can still author a queue with zero explicit `depends_on` fields and the serial default will handle it correctly; the DAG only matters when the user opts in to parallel.

---

## Dispatch sequence (per slice)

Run stages 1 → 4 in order. The retry loop below wraps stages 1 + 3 (author + parallel review). When a cohort is in flight (parallel mode), each slice runs its own instance of this sequence; Stage 4 serializes per § Bounded parallel dispatch.

### Stage 1 — Author

1. Rotate active-slice.json to the `author` manifest. Bump `attempts` by 1 (first dispatch → 1; first retry → 2; and so on). Mirror `attempts` into `slices/queue.json` for the slice.
2. **On the first dispatch for this slice** (pre-bump `attempts == 0`), record the start-of-slice git ref for LOC telemetry: `mkdir -p .mutagen/state/slice-start-ref && git rev-parse HEAD > .mutagen/state/slice-start-ref/<slice_id>`. Skip on retry — the base ref stays pinned to before the slice began so `scripts/slice_loc.sh` measures net-new across the whole attempt sequence, not just the latest retry.
3. Spawn the `author_agent` subagent via the Agent tool. Prompt includes:
   - The slice text (reconstructed from the JSON or copied verbatim from the rendered markdown).
   - **Path to the Evidence Bundle:** `.mutagen/state/evidence/<slice_id>.md` — written in preflight step 5.
   - **Explicit instruction:** *"Read `.mutagen/state/evidence/<slice_id>.md` once — every upstream citation required for this slice is already extracted there. Do NOT re-read `docs/PRD*`, `docs/ADR*`, `docs/DDD*`, `docs/ISC*`, or `docs/DSD*`; the evidence file is authoritative. Read source files, tests, and existing project state freely; just don't cold-load the design bundle."*
   - A reminder of the agent's Output Format (see `${CLAUDE_PLUGIN_ROOT}/agents/<Agent>.md`).
   - Instruction to write the State Update block to `context_to_update`.
   - **On retry only:** attach every `Suggested Fix` from Tiger Claw's prior QA report. Include the full QA Report (`reviews/<slice_id>/tiger-claw.md`) as a read path at the end of the prompt — by path, not inlined — and instruct the author to address each fix in order and keep the change minimal.
4. Wait for the author to return. Capture their output verbatim and write it to `.mutagen/state/author-output/<slice_id>.md` (`mkdir -p .mutagen/state/author-output` first). Clobber-on-write; the file reflects the latest attempt and is what `karai_structural_check.sh` reads in Stage 2.

### Stage 2 — Structural conformance (script)

Karai the agent is **not** dispatched here. Section-presence, trace-ID matching, state-block landing, and LOC-vs-target are pattern-matching checks; a script runs them without burning an agent spawn per slice per attempt. Karai only wakes for Stage 4 (state verify + dispatch log + advisory backlog) and reviewer escalations.

1. Rotate active-slice.json to the `karai_structural` manifest (Karai still *owns* this stage for scope purposes even though the check runs as a script — the manifest is `.mutagen/state/**` writes only).
2. Run `bash ${CLAUDE_PLUGIN_ROOT}/scripts/karai_structural_check.sh <slice_id>` and capture stdout as a single JSON object: `{verdict, findings[], loc}`. The script reads the author's output from `.mutagen/state/author-output/<slice_id>.md`, the slice metadata from `slices/queue.json`, and the context file (`project_state.md` or `infrastructure_state.md`). It returns `verdict: "pass"` or `verdict: "fail"` deterministically; no prompting involved.
3. Branch on `.verdict`:
   - **`"pass"`** — record `verdicts.karai_structural: "pass"` in `slices/queue.json`. Continue to Stage 3.
   - **`"fail"`** — **halt** the pipeline. This is not retryable by the author within this command; it is a structural break that needs the human. Mark `slices/queue.json` → `verdicts.karai_structural: "fail"`, `status: "escalated"`, `escalation_reason: "<concat of findings[].detail>"`. Escalate with the full `findings` array presented to the user verbatim. **Do not** clear active-slice.json. **Do not auto-advance** — stop here and wait for the user. Before stopping, fire a Pushover halt notification: `bash ${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh structural_fail "mutagen — structural fail on {slice_id}" "Structural check halted {slice_id} ({title}): {first finding.detail}. Needs human input."` — the script silently no-ops when Pushover is not configured.

If the script itself fails to run (jq missing, queue unreadable) it returns `verdict: "fail"` with a tooling finding. Treat that the same as any other structural fail — surface it and halt. A broken check script is not a pass.

### Stage 3 — Tiger Claw (QA)

**Skip the entire stage** if `pipeline_mode == "lightweight"` AND `review_required == false`. Record `verdicts.bishop: "skip"` and `verdicts.tiger_claw: "skip"`. Continue to Stage 4.

Otherwise:

1. Rotate active-slice.json to the `review_qa` manifest. Before dispatch, `mkdir -p reviews/<slice_id>`.
2. Dispatch Tiger Claw via the Agent tool (`subagent_type: "TigerClaw"`). Prompt includes the slice artifacts, the **path to the Evidence Bundle** (`.mutagen/state/evidence/<slice_id>.md` — same file the author read), the author's output, and the *"Read the evidence file once; do not re-read upstream docs"* instruction. Tell her to write her report to `reviews/<slice_id>/tiger-claw.md`.
3. Record her verdict in `slices/queue.json`:
   - Tiger Claw: 🟢 Clean → `"clean"`; 🟡 Gap → `"gap"`; ⏭ Skip → `"skip"`; 🔴 Defect → `"defect"`.
   - Always record `verdicts.bishop: "skip"` — Bishop is disabled.
4. Confirm the QA report is persisted: `reviews/<slice_id>/tiger-claw.md` + `.mutagen/state/tiger-claw-latest.md` (clobber-on-write). If missing, treat as non-conformant and escalate; do not fabricate.
5. Evaluate:
   - Non-🔴 (clean / gap / skip) → continue to Stage 4.
   - 🔴 Defect → enter the **re-review retry loop** below with Tiger Claw's Suggested Fix blocks.

### Stage 4 — Karai state verification + dispatch log

1. Rotate active-slice.json to the `karai_state` manifest.
2. Re-spawn Karai for her Verify-state step: confirm the author's State Update block landed in `context_to_update`. Append a Dispatch Log row with the slice's final Tiger Claw verdict. The advisory-backlog append (Bishop's old job) is skipped while Bishop is disabled.
3. Record completion in `slices/queue.json`: `status: "completed"`, `completed_at: <ISO-8601 UTC>`.

### Stage 5 — Record, offload, advance

1. Re-render `slices/queue.md` from the updated JSON: `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh`.
2. **Write the slice summary.** `mkdir -p slices/<slice_id>` and emit `slices/<slice_id>/summary.md` with the shape below. This file is the orchestrator's memory of the slice — once written, the orchestrator must **not** carry per-agent transcripts (author output, Tiger Claw report body) forward in its own context. When a later slice needs "what happened on L2-Orders-003", read the summary file; don't re-summarise from transcript.

   ```markdown
   # Slice summary — <slice_id>
   **Title:** <title>
   **Author:** <author_agent>
   **Layer / Context:** L<layer> / <bounded_context>
   **Completed at:** <ISO-8601 UTC>
   **Duration:** <wall time from Stage 1 start to here>
   **Attempts:** <final attempts count>  (micro_correction: <true|false>)

   ## Verdicts
   - Karai structural: pass
   - Bishop: skip (disabled)
   - Tiger Claw: <clean|gap|defect|skip>

   ## Files touched
   <from `scripts/slice_loc.sh` + the author's Code Artifacts list — paths only>

   ## Advisories logged
   <count + one-line per advisory, or "none">

   ## Retry path
   <brief — "first-pass clean", "1 Tiger Claw retry cleared", "micro-correction on attempt 2", etc.>

   ## Reports
   - QA: `reviews/<slice_id>/tiger-claw.md`
   - Evidence: `.mutagen/state/evidence/<slice_id>.md`
   ```

3. Clear `.mutagen/state/active-slice.json`.
4. **Milestone check.** After recording completion, inspect `slices/queue.json`: if no `pending` or `blocked_retry` slice remains in the just-completed slice's `layer`, fire `bash ${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh layer_complete "mutagen — layer <N> complete" "<M> slices completed in layer <N>. Next pending slice: <id or 'queue clear'>"`. The notifier self-gates via `.claude/workflow.json` `notify.milestones` — no need to check here.
5. **Emit the one-line completion marker AND immediately continue in the same turn.** The full summary is on disk at `slices/<slice_id>/summary.md`; do not restate its contents here. The marker is exactly one line in this shape and nothing more:

   `✔ <slice_id> — <tiger_claw verdict>, attempts=<N>[, micro_correction][ — heartbeat: <anomaly>]`

   Do **not** append "Next slice:", "Proceeding to…", "Ready to continue?", file-touched lists, cross-slice findings, or any other prose. The marker is a log line, not a conversation turn. In the **same assistant turn** that emits this marker, issue the Preflight tool calls for the next slice (the first Read/Bash calls of Preflight steps 1–2). Ending your turn after the marker without having dispatched the next Preflight is the violation we're trying to avoid.
6. **Auto-advance.** Jump straight back to **Preflight** for the next ready slice — in the same turn as step 5's marker. No fresh prompt, no permission question, no "let me know if you'd like to continue." Keep looping until one of the stop conditions below fires. The only thing that ends a turn mid-queue is a stop condition.

**Orchestrator context-offload rule.** For any closed slice, reference `slices/<slice_id>/summary.md` rather than carrying the agent's transcripts, Evidence Bundle, or report bodies forward. The summary plus the paths it points to is the full record; re-read on demand. This keeps orchestrator context bounded as the queue progresses.

### Auto-advance stop conditions

Halt the loop (and do not clear active-slice.json) when any of the following happens. Every halt also fires a Pushover notification via `${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh <event> "<title>" "<message>"` so the human knows the pipeline is waiting — the script silently no-ops when Pushover is not configured, so the call is safe to make unconditionally.

- **Queue clear** — no `pending` or `blocked_retry` slices remain. Report "queue clear — all slices completed" and stop. Fire `notify.sh queue_clear "mutagen — queue clear" "N slices completed."` (normal priority, typically the only good-news notification users keep enabled).
- **Structural escalation** — Karai's Stage 2 fires a fail. Present her report verbatim and stop. Notification already fired in Stage 2.
- **Retry budget exhausted** — the retry loop's escalation branch (see below) fires on the current slice. Present the blocking report verbatim and stop. Notification fired from inside the retry loop.
- **Traag DENY** — the guard hook blocks a write during any stage. Karai treats that as a Red inspection outcome and halts; auto-advance stops. Fire `notify.sh scope_violation "mutagen — scope violation on {slice_id}" "Traag DENY on {path} (class: {class}) during stage {stage}. Agent: {active_agent}."`.
- **User interrupt** — the user sends a message while the loop is running. Complete the in-flight stage cleanly, report where you stopped, and wait. Do not fire a notification — the user is already here.

---

## Re-review retry loop

Triggered by Tiger Claw 🔴 Defect from Stage 3.

Two independent budgets govern iteration on a blocked slice:

- **`attempts`** — full author re-dispatches. Default cap: `max_retries + 1` (workflow.json `review.max_retries`, default 2, so 3 total author dispatches).
- **`micro_corrections_used`** — one-shot mechanical fix dispatches. Default cap: `max_micro_corrections` (workflow.json, default **1**). Tracked separately in `.mutagen/state/active-slice.json` and mirrored to `slices/queue.json`.

Evaluate the escape hatch **on every 🔴 verdict**, not just after the retry budget is exhausted. A 3-line mechanical fix shouldn't cost a full author + structural-check + parallel-review cycle when a micro-correction closes it on attempt 1.

1. Read `attempts` and `micro_corrections_used` from `.mutagen/state/active-slice.json` (initialise `micro_corrections_used: 0` on first entry).

2. **Escape-hatch evaluation** — run first, on every 🔴 entry. If the hatch is available and its conditions hold, take it; otherwise fall through to the standard retry branch.

   **Hatch availability:** `micro_corrections_used < max_micro_corrections`. If the budget is spent, skip to step 3.

   **Hatch conditions** — all must hold. Bias toward plowing ahead.

   - **Mechanical scope.** The fix is ≲ 20 LOC and confined to test updates, wiring (DI, constructor params), imports, a missed rename, a stale comment, or similar plumbing. No new behavior, no contract change, no ADR / DDD / ISC / DSD implication, no algorithmic doubt.
   - **Named fix.** You can state the exact file(s), the exact change, and point at a reviewer's `Suggested Fix` block that matches. Paraphrasing, guessing, or "try X and see" → no hatch.
   - **In-scope executor.** The fix path sits inside the slice's `author_agent` globs + any `adjacent_scope_allowed` globs the slice carries, or inside Bebop's globs (Bebop is the fallback fixer for test / wiring misses when the original author's globs don't cover the file).

   If all four hold, dispatch a **micro-correction** and continue the pipeline:

   1. Rotate `active-slice.json` to the `author` manifest (retry rules apply — `adjacent_scope_allowed` merges in). Choose the executor: the current `author_agent` if the fix sits in their globs, otherwise Bebop. Record the chosen agent in `active_agent`.
   2. Increment `micro_corrections_used` by 1 in active-slice.json and mirror to `slices/queue.json` as `verdicts.micro_corrections_used`. **Do not** bump `attempts` — a micro-correction is not a full author dispatch and must not consume the retry budget.
   3. Prompt the executor with the Evidence Bundle path (unchanged), the prior author output (by path), and a tight micro-correction instruction: the cited `Suggested Fix` block verbatim, the exact file(s) and change, and the explicit rule *"change only what is named here — no refactor, no tangential cleanup, no scope expansion."*
   4. On return, run Stage 2 (structural check script) normally, then Stage 3 (Tiger Claw) one more time.
   5. If Tiger Claw returns non-🔴 → normal Stage 4 completion. Record `verdicts.micro_correction: true` in `slices/queue.json` for telemetry. Auto-advance as usual.
   6. If she blocks again → return to the top of this retry loop (step 1). The next entry will: (a) find `micro_corrections_used` exhausted and skip the hatch, and (b) evaluate the standard retry branch or escalate based on `attempts`.

3. **Standard retry branch** — reached when the hatch is unavailable (budget spent) or its conditions don't hold.

   - If `attempts >= max_retries + 1`, the retry budget is exhausted → **escalate** (step 4).
   - Otherwise, retry is allowed:
     - Mark `slices/queue.json` → `status: "blocked_retry"`. Re-render.
     - Confirm Tiger Claw's QA report is persisted (`reviews/<slice_id>/tiger-claw.md` + `.mutagen/state/tiger-claw-latest.md`).
     - Loop back to **Stage 1 (Author)**. The author is re-dispatched with every Suggested Fix from Tiger Claw's report. After Stage 1 returns, proceed through Stage 2, then Stage 3 re-runs Tiger Claw fresh.
     - `attempts` is bumped in Stage 1 (not here); only the status flip + report capture happen here. `adjacent_scope_allowed` merges into the manifest because `attempts >= 1` on any retry.

4. **Escalation** — reached when the hatch is unavailable/declined AND the retry budget is exhausted, or when a micro-correction returned a fresh block AND both budgets are now spent:

   - Mark `slices/queue.json` → `status: "escalated"`, `escalation_reason: "Tiger Claw Defect after N attempts (micro_corrections_used: M)"`. Leave the verdicts recorded.
   - Re-render `slices/queue.md`.
   - **Do not** clear active-slice.json.
   - **Fire a Pushover notification:** `bash ${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh escalation "mutagen — halted at {slice_id}" "{slice_id} ({title}) escalated after {N} attempts ({M} micro-corrections). Blocked by: Tiger Claw. Needs human input."` — silent no-op when Pushover is not configured.
   - **Stop auto-advance.** Present Tiger Claw's QA report verbatim.
   - Wait for user instruction (amend scope via `/mutagen:amend-scope`, re-slice via `/mutagen:slice`, fix in place manually, abandon).

Tiger Claw re-runs fresh on each retry; her prior report is not treated as a carry-over verdict.

---

## On any escalation

- Do **not** clear `.mutagen/state/active-slice.json`.
- Do **not** rotate stages further — leave the manifest at the stage that halted.
- Present the failing gate's report(s) to the user verbatim.
- Ensure `slices/queue.json` has `status: "escalated"` (or `"refused"` for intake refusal) and a populated `escalation_reason`; re-render `slices/queue.md`.
- **Do not auto-advance.** Wait for user instruction.

## Reminders

- Stage 3 dispatches Tiger Claw alone. Bishop is disabled; record `verdicts.bishop: "skip"` on every slice.
- The PreToolUse guard hook is disabled. `allowed_write_globs` in `.mutagen/state/active-slice.json` is kept for `$mutagen-status` visibility and Traag's mediated amendments, but no longer blocks writes. Agents are expected to self-honour scope.
- Every gate's verdict is recorded in `slices/queue.json` under `verdicts.*` and in Karai's Dispatch Log. The markdown rendering shows verdicts at a glance.
- In lightweight mode, the slice's `review_required` tag is authoritative; do not skip Tiger Claw based on your own judgment of complexity.
- The retry loop is author-only. Karai's structural failures and Traag's scope denies go straight to the human — the author cannot retry around discipline violations.
- `attempts` in `slices/queue.json` persists across multiple `/mutagen:execute-next` invocations on the same slice, so resuming a `blocked_retry` after a session restart picks up where you left off.
- Auto-advance keeps the loop running until a stop condition fires. Between slices, the only state that matters is `slices/queue.json`, `project_state.md`, `infrastructure_state.md`, and the persisted reviews — the per-slice `active-slice.json` is cleared at the end of a successful slice and rewritten at the start of the next.

$ARGUMENTS
