---
name: mutagen-execute-next
description: Explicit-only skill. Run the mutagen pipeline on the next ready slice — dispatch the assigned executor, run a structural-check script, run Tiger Claw QA, retry on Defect up to MAX_RETRIES with a separate micro-correction budget, auto-advance to the next slice on success until the queue is empty or a stage escalates. Invoke only when the user explicitly says $mutagen-execute-next.
---

# $mutagen-execute-next — run the pipeline until the queue is empty

You orchestrate the full execution pipeline across slices: for each ready
slice you run **author → structural-check script → Tiger Claw (QA) → Karai
(state record)**, with a re-review retry loop on Tiger Claw 🔴 Defect. On
success this skill **auto-advances to the next ready slice** without a fresh
prompt; it stops when the queue is empty or a stage forces escalation.

> **Bishop is disabled.** The principal-engineer code-review gate has been
> removed — Tiger Claw is the sole Stage 3 reviewer. Always record
> `verdicts.bishop: "skip"` in `slices/queue.json`; never dispatch Bishop.
>
> **Write-path guarding is disabled.** `allowed_write_globs` in
> `.mutagen/state/active-slice.json` is bookkeeping only; the PreToolUse
> guard hook no longer blocks writes. Agents self-honour scope on the
> honour system.

## Autopilot discipline (read before every turn in the loop)

This skill is a loop, not a conversation. Between slices you are **not**
checking in with the human. Specifically:

- **Never** emit "Proceed?", "Continue?", "Shall I auto-advance?",
  "Pause here?", "Ready for the next slice?", "Let me know if you want
  me to…", or any other permission-seeking phrase. Generating such a
  phrase is a protocol violation, not polite conversation.
- **Never** end your turn between a completed slice and the next
  slice's Preflight tool calls. The same assistant turn that emits the
  one-line completion marker for slice N must also issue the Preflight
  tool calls for slice N+1. No trailing prose after the marker.
- **Never** offer a narrative recap ("What landed:", "Human
  provisioning pending:", "Next ready slice:", bulleted file lists,
  cross-slice findings). That content already lives in
  `slices/<slice_id>/summary.md` and the review reports — surface one
  terse line and move on.
- The **only** moments you end a turn are the stop conditions in
  § Auto-advance stop conditions below: queue clear, queue stalled,
  structural escalation, retry-budget escalation, scope violation,
  user interrupt. Nothing else.

If you catch yourself about to ask the human whether to continue,
you're wrong. Continue.

## Serial fast path

On the default serial path, prefer one shell entrypoint:

```bash
bash "$MUTAGEN_ROOT/scripts/run_execute_next.sh" --host codex
```

That runner owns the full serial queue path: it loops the one-slice runner
until the queue clears, stalls, or a slice escalates. Treat its JSON payload
as authoritative for:

- `status: "queue_clear"` — stop cleanly. Any slices closed during this
  invocation are listed in `completion_markers` and `completed_slices`.
- `status: "stalled"` — stop cleanly and surface the returned terminal
  dependency payload. Any slices already closed in this invocation are still
  listed in `completion_markers`.
- `status: "escalated"` — stop auto-advance and surface the stage payload it
  returned. Any earlier successful slices are still listed in
  `completion_markers`.
- Exit `2` with `status: "queue_validation_failed"` — surface the payload
  verbatim, recommend `$mutagen-slice`, and stop.

`run_slice_once.sh` remains the authoritative one-slice contract and the
debugging fallback when you need to inspect the inner loop directly. The
detailed stage sections below are the contract that inner runner implements.

## Host execution profile

Resolve the host execution profile through
`bash "$MUTAGEN_ROOT/scripts/host_profile.sh" --host codex` before each slice
preflight. Treat the JSON payload as authoritative for:

- `scope_enforcement` — `hard` vs. `advisory`
- `parallel_dispatch` — `serial_only` vs. `bounded_cohort`
- `requested_max_parallel_slices` / `effective_max_parallel_slices`
- `worktree_isolation`
- `degraded_features` and `downgrades`

If `scope_enforcement == "advisory"`, inline the current stage's
`allowed_write_globs` into every spawned prompt and instruct the agent to
self-honour them. If `parallel_dispatch == "serial_only"`, do not improvise
cohort mode just because `.claude/workflow.json` asked for it.

## Session preflight (runs once per invocation)

Read the upstream design bundle into your conversation context **once**,
before entering the per-slice loop.

1. Resolve and read each document. Each may live at `docs/<NAME>/<NAME>.md`,
   `docs/<NAME>.md`, or `<NAME>.md` at repo root — pick the first that exists:
   - PRD
   - All ADR files (`docs/ADR/ADR-*.md` or repo-root `ADR-*.md` — read all)
   - DDD
   - ISC
   - DSD
2. If any of PRD / DDD / ISC / DSD is missing — refuse and tell the user the
   bundle is incomplete; `$mutagen-slice` should not have generated a queue
   against it. ADRs may legitimately be empty.
3. Hold these in context for the duration of the run. The per-slice Evidence
   Bundle is built by extracting from this cache, not by re-reading from
   disk.

## Preflight (runs once per slice — re-enter at the top of the loop)

1. `mkdir -p .mutagen/state .mutagen/state/evidence .mutagen/state/author-output .mutagen/state/slice-start-ref reviews slices`.
2. **Resolve host behavior through the harness.** Run:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/host_profile.sh" --host codex
   ```

   - Treat the returned `scope_enforcement`, `parallel_dispatch`,
     `effective_max_parallel_slices`, and `degraded_features` as
     authoritative.
   - If `parallel_dispatch != "serial_only"`, stop and surface the payload.
     This skill is the serial Codex path; bounded parallel support belongs in
     a host adapter upgrade, not in wishful improvisation.
3. **Claim the next ready slice through the harness.** Run:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/prepare_next.sh" --host codex
   ```

   - Exit `0` with `status: "ready"` → continue. Treat the JSON payload as
     authoritative for `slice_id`, `title`, `author_agent`, `layer`,
     `bounded_context`, `objective`, `review_required`, `attempts`,
     `context_to_update`, `write_set`, `adjacent_scope_allowed`,
     `depends_on`, `queue_path`, `active_state_path`, and
     `evidence_bundle_path`.
   - Also treat the returned `host_profile` object as authoritative for the
     claimed slice. Do not re-derive host behavior from host name or old
     command lore.
   - Exit `0` with `status: "queue_clear"` → report queue clear and stop.
   - Exit `0` with `status: "stalled"` → report the `blocked` dependency
     list verbatim and stop.
   - Exit `2` → refuse execution. Surface the JSON payload verbatim,
     recommend re-running `$mutagen-slice`, and stop. This covers missing,
     stale, orphaned, or failed queue-validator state.
   - Any other exit → tooling failure. Surface the payload verbatim and stop.
4. Read `.mutagen/state/active-slice.json`. The harness already claimed the
   slice, wrote the author-stage manifest, and materialized the Evidence
   Bundle. Do **not** re-select the slice, re-claim it in `slices/queue.json`,
   or rebuild `.mutagen/state/evidence/<slice_id>.md` by hand.
5. Read `.claude/workflow.json` if present. Extract `heartbeat.*` for
   inspection thresholds. Treat `pipeline_mode`, `max_retries`, and
   `max_micro_corrections` from `.mutagen/state/active-slice.json` as
   authoritative for the claimed slice, since the harness already resolved
   the workflow config when it wrote the active state.
6. Every queue mutation after this point goes through
   `bash "$MUTAGEN_ROOT/scripts/update_queue_slice.sh" ...`. Do not hand-edit
   `slices/queue.json`.
7. Every active-slice stage mutation after this point goes through
   `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" ...`. Do not
   hand-edit `.mutagen/state/active-slice.json`.

## Per-stage scope manifests

| Stage | `active_agent(s)` | `allowed_write_globs` |
|-------|-------------------|------------------------|
| `author` | author agent | `write_set` from the queue + `.mutagen/state/**` (+ `adjacent_scope_allowed` globs on retry) |
| `karai_structural` | `Karai` (script-run, no agent spawn) | `.mutagen/state/**` |
| `review_qa` | `TigerClaw` | `reviews/**` + `tests/qa/**` (+ `tests/qa/security/**` when `author_agent == "Tatsu"`) + `.mutagen/state/**` |
| `karai_state` | `Karai` | `project_state.md` + `infrastructure_state.md` + `slices/**` + `.mutagen/state/**` |

### Legacy fallback author paths per agent

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

## Dispatch sequence (per slice)

### Stage 1 — Author

1. Rotate `active-slice.json` and sync the author-dispatch counter through
   `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" --slice-id <slice_id> --stage author --bump-attempts`.
2. **On the first dispatch for this slice** (pre-bump `attempts == 0`),
   record the start-of-slice git ref for LOC telemetry:
   `git rev-parse HEAD > .mutagen/state/slice-start-ref/<slice_id>`. Skip on
   retry — the base ref stays pinned to before the slice began so
   `scripts/slice_loc.sh` measures net-new across the whole attempt sequence.
3. Run:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/dispatch_stage.sh" --slice-id <slice_id>
   ```

   The wrapper delegates prompt assembly to the Rust harness
   `prepare-dispatch` runtime, dispatches the current `active_agent`
   through `bin/agent.sh`, and captures stdout verbatim to
   `.mutagen/state/author-output/<slice_id>.md`. Treat the JSON payload as
   authoritative for `agent`, `dispatch_kind`, `prompt_path`,
   `stdout_capture_path`, `scope_enforcement`, `allowed_write_globs`, and
   any attached `qa_report_path`. On failure, surface the JSON and stop.

### Stage 2 — Structural conformance (script)

Karai the agent is **not** dispatched here. Section-presence, trace-ID
matching, state-block landing, and LOC-vs-target are pattern-matching
checks; a script runs them without burning an agent spawn per slice per
attempt. Karai only wakes for Stage 4 (state verify + dispatch log +
advisory backlog) and reviewer escalations.

1. Rotate `active-slice.json` through
   `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" --slice-id <slice_id> --stage structural-check`.
   Karai still owns this stage for scope purposes — the manifest is
   `.mutagen/state/**` writes only).
2. Run the check:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/karai_structural_check.sh" <slice_id>
   ```

   Capture stdout as a single JSON object: `{verdict, findings[], loc}`.
   The shell entrypoint is a compatibility wrapper; it delegates the real
   Stage 2 logic to the Rust harness `structural-check` runtime, which reads
   the author's output from `.mutagen/state/author-output/<slice_id>.md`,
   validates the emitted `State Update` block directly out of that artifact,
   reads the slice metadata from `slices/queue.json`, and runs the LOC telemetry
   script. It returns `verdict: "pass"` or `verdict: "fail"`
   deterministically; no prompting involved.
3. Branch on `.verdict`:
   - **`"pass"`** — record it through
     `bash "$MUTAGEN_ROOT/scripts/update_queue_slice.sh" --slice-id <slice_id> --karai-structural pass`.
     Continue to Stage 3.
   - **`"fail"`** — **halt** the pipeline. Mark `slices/queue.json` →
     record the halt through
     `bash "$MUTAGEN_ROOT/scripts/update_queue_slice.sh" --slice-id <slice_id> --status escalated --karai-structural fail --escalation-reason "<concat of findings[].detail>"`.
     Present the full `findings` array to the user verbatim. Do not clear
     `active-slice.json`. Do not auto-advance. `karai_structural_check.sh`
     now emits and dispatches the canonical `structural_fail`
     notification from the harness result before returning.

   If the wrapper or harness runtime fails it returns `verdict: "fail"`
   with a tooling finding — treat as any other structural fail. A broken
   check runtime is not a pass.

### Stage 3 — Tiger Claw (QA)

**Skip entire stage** if `pipeline_mode == "lightweight"` AND
`review_required == false`. Record the skip through
`bash "$MUTAGEN_ROOT/scripts/update_queue_slice.sh" --slice-id <slice_id> --bishop skip --tiger-claw skip`.
Continue to Stage 4.

Otherwise:

1. Rotate `active-slice.json` through
   `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" --slice-id <slice_id> --stage review`.
   Before dispatch, `mkdir -p reviews/<slice_id>`.
  2. Run:

     ```bash
     bash "$MUTAGEN_ROOT/scripts/dispatch_stage.sh" --slice-id <slice_id>
     ```

     The wrapper delegates prompt assembly to the Rust harness
     `prepare-dispatch` runtime, dispatches Tiger Claw through
     `bin/agent.sh`, captures stdout to `.mutagen/state/review-output/<slice_id>.md`,
     and verifies the review artifacts exist:
     `reviews/<slice_id>/tiger-claw.md` +
     `.mutagen/state/tiger-claw-latest.md`. It also records
     `verdicts.bishop: "skip"` plus the canonical `verdicts.tiger_claw`
     value through `record_review_verdict.sh`. Treat the JSON payload as
     authoritative for `agent`, `prompt_path`, `stdout_capture_path`,
     `qa_report_path`, `latest_qa_report_path`, and
     `required_written_artifacts`.
  3. Run:

     ```bash
     bash "$MUTAGEN_ROOT/scripts/review_decision.sh" --slice-id <slice_id>
     ```

   The wrapper delegates to the Rust harness `review-decision` runtime. It
   treats `slices/queue.json` as canonical for the recorded Tiger Claw
   verdict, parses Tiger Claw's machine-readable `Retry Contract` JSON from
   the QA report, validates retry budgets from `.mutagen/state/active-slice.json`,
   and returns one of four actions:
   - `continue` → proceed to Stage 4.
   - `micro_correction` → use the returned `active_agent`,
     `suggested_fix_files`, and `suggested_fix_summary` in the
     micro-correction branch below.
   - `retry` → the harness already marked `status: "blocked_retry"`; loop
     back to Stage 1 with the QA report path.
   - `escalated` → the harness already marked `status: "escalated"` and
     populated `escalation_reason`; stop, notify, and present the QA report
     verbatim.

### Stage 4 — Canonical closure (state verification + dispatch log)

1. Rotate `active-slice.json` through
   `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" --slice-id <slice_id> --stage state-record`.
2. Run:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/finalize_slice.sh" --slice-id <slice_id>
   ```

   The wrapper delegates to the Rust harness `finalize-slice` runtime. It:
   parses the State Update block from author output, applies it to
   `context_to_update`, and verifies the marker landed,
   records `status: "completed"` plus `completed_at`, writes
   `slices/<slice_id>/summary.md`, appends `.mutagen/state/dispatch-log.jsonl`,
   clears `.mutagen/state/active-slice.json`, and re-renders the queue
   markdown. Treat the JSON payload as authoritative for `summary_path`,
   `dispatch_log_path`, `retry_path`, `files_touched`, `layer_complete`,
   `completed_in_layer`, `next_pending_slice_id`, `completion_marker`, and
   any emitted `notifications`.
3. The summary written by the harness has this shape:

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
   <from scripts/slice_loc.sh + the author's Code Artifacts list — paths only>

   ## Advisories logged
   <count + one-line per advisory, or "none">

   ## Retry path
   <brief — "first-pass clean", "1 Tiger Claw retry cleared", "micro-correction on attempt 2", etc.>

   ## Reports
   - QA: reviews/<slice_id>/tiger-claw.md
   - Evidence: .mutagen/state/evidence/<slice_id>.md
   ```

   Once written, the orchestrator must **not** carry per-agent transcripts
   (author output, Tiger Claw report body) forward in
   its own context. Reference the summary file; re-read on demand.

### Stage 5 — Notify, offload, advance

1. **Milestone notification.** `finalize_slice.sh` now emits and dispatches
   any milestone notifications listed in its `notifications` array. Today
   that means `layer_complete` when the finalized slice closes the last
   pending / blocked-retry slice in its layer. The shell wrapper relays
   those events through `notify.sh`, which still self-gates via
   `.claude/workflow.json` `notify.milestones`.
2. **Emit the one-line completion marker AND immediately continue in the
   same turn.** The full summary is on disk at
   `slices/<slice_id>/summary.md`; do not restate its contents here. Use the
   exact `completion_marker` returned by `finalize_slice.sh`, which follows
   this shape and nothing more:

   `✔ <slice_id> — <tiger_claw verdict>, attempts=<N>[, micro_correction][ — heartbeat: <anomaly>]`

   Do **not** append "Next slice:", "Proceeding to…", "Ready to
   continue?", file-touched lists, cross-slice findings, or any other
   prose. The marker is a log line, not a conversation turn. In the
   **same assistant turn** that emits this marker, issue the Preflight
   tool calls for the next slice. Ending your turn after the marker
   without having dispatched the next Preflight is the violation we're
   trying to avoid.
3. **Auto-advance** if the queue has a `pending` or `blocked_retry` slice.
   Jump back to Preflight — in the same turn as step 2's marker. No fresh
   prompt, no permission question, no "let me know if you'd like to
   continue." Keep looping until a stop condition fires.

**Orchestrator context-offload rule.** For any closed slice, reference
`slices/<slice_id>/summary.md` rather than carrying the agent's transcripts,
Evidence Bundle, or report bodies forward. The summary plus the paths it
points to is the full record; re-read on demand. This keeps orchestrator
context bounded as the queue progresses.

### Auto-advance stop conditions

Halt the loop (do not clear active-slice.json) and fire Pushover
notifications on:

- **Queue clear** — report "queue clear — all slices completed" and stop.
  `prepare_next.sh` now emits and dispatches the canonical `queue_clear`
  notification from the harness result.
- **Queue stalled** — pending slices remain but every one has an unmet
  `depends_on`. Report the stall list and stop. (No separate notify event;
  this is a planning issue, surface via normal output.)
- **Structural escalation** — Stage 2 fail. `karai_structural_check.sh`
  already emitted and dispatched the canonical `structural_fail`
  notification from the harness result.
- **Retry budget exhausted** — retry loop escalation. `review_decision.sh`
  emits and dispatches the escalation notification from the harness result.
- **Scope violation** — the guard hook blocked a write and persisted
  `.mutagen/state/scope-violation.json`. Run
  `bash "$MUTAGEN_ROOT/scripts/scope_violation.sh"`. It normalizes the
  violation through the harness, marks the slice `escalated` when
  possible, emits the canonical `scope_violation` notification, and
  returns the violation payload to surface verbatim.
- **User interrupt** — complete in-flight stage cleanly, report, wait.

---

## Re-review retry loop

Triggered by Tiger Claw 🔴 Defect from Stage 3.

Two independent budgets govern iteration on a blocked slice:

- **`attempts`** — full author re-dispatches. Default cap: `max_retries + 1`
  (workflow.json `review.max_retries`, default 2, so 3 total author
  dispatches).
- **`micro_corrections_used`** — one-shot mechanical fix dispatches. Default
  cap: `max_micro_corrections` (workflow.json, default **1**). Tracked
  separately in `.mutagen/state/active-slice.json` and mirrored to
  `slices/queue.json`.

Evaluate the escape hatch **on every 🔴 verdict**, not just after the retry
budget is exhausted. A 3-line mechanical fix shouldn't cost a full author +
structural-check + parallel-review cycle when a micro-correction closes it
on attempt 1.

1. Run `bash "$MUTAGEN_ROOT/scripts/review_decision.sh" --slice-id <slice_id>`
   and treat its JSON payload as authoritative for the retry branch. The
   harness owns the budget math, queue mutation, and retry/escalation
   decision.

2. **Escape-hatch evaluation** — the runtime decides this first, on every
   🔴 entry.

   **Hatch availability:** `micro_corrections_used < max_micro_corrections`.
   If the budget is spent, skip to step 3.

   **Hatch conditions** — all must hold:

   - **Mechanical scope.** ≲ 20 LOC, confined to tests, wiring, imports,
     renames, stale comments. No new behavior, no contract change.
   - **Named fix.** You can state the exact file(s), change, and point at
     a reviewer's `Suggested Fix` block.
   - **In-scope executor.** Fix path is in the slice `write_set` + any
     `adjacent_scope_allowed` globs, or in the legacy author-path fallback
     when `write_set` is absent. Bebop stays the fallback for test / wiring
     misses.

   All four hold → dispatch a **micro-correction**:

   1. Rotate `active-slice.json` and sync micro-correction bookkeeping
      through
      `bash "$MUTAGEN_ROOT/scripts/transition_active_slice.sh" --slice-id <slice_id> --stage author --active-agent <chosen agent> --bump-micro-corrections`,
      where `<chosen agent>` is the `active_agent` returned by
      `review_decision.sh`. Retry rules still apply —
      `adjacent_scope_allowed` merges in when the queue is already in retry
      state.
   2. **Do not** bump `attempts` — a micro-correction is not a full author
      dispatch and must not consume the retry budget.
   3. Run:

      ```bash
      bash "$MUTAGEN_ROOT/scripts/dispatch_stage.sh" --slice-id <slice_id> --dispatch-kind micro_correction --qa-report <qa_report_path>
      ```

      The harness-prepared prompt carries the evidence path, QA report path,
      current stage scope, and bounded-fix instruction. Treat its JSON
      payload as authoritative.
   4. On return, run Stage 2 (structural check script) normally, then
      Stage 3 (Tiger Claw) one more time.
   5. If Tiger Claw returns non-🔴 → normal Stage 4 completion. Record
      `verdicts.micro_correction: true` for telemetry. Auto-advance.
   6. If she blocks again → return to the top of this retry loop
      (step 1). The next entry will find `micro_corrections_used`
      exhausted and skip the hatch, falling through to the standard
      retry branch or escalation based on `attempts`.

3. **Standard retry branch** — reached when the hatch is unavailable or
   its conditions don't hold.

   - If `attempts >= max_retries + 1`, retry budget is exhausted →
     **escalate** (step 4).
   - Otherwise, retry is allowed:
     - `review_decision.sh` already marked `status: "blocked_retry"` through
       the harness and left `.mutagen/state/active-slice.json` intact at
       `stage: review`.
     - Loop back to **Stage 1** and run:

       ```bash
       bash "$MUTAGEN_ROOT/scripts/dispatch_stage.sh" --slice-id <slice_id> --dispatch-kind retry --qa-report <qa_report_path>
       ```

       The harness-prepared prompt carries the QA report path and retry
       instructions; do not hand-inline Suggested Fixes.
     - `attempts` is bumped in Stage 1 (not here). `adjacent_scope_allowed`
       merges into the manifest because `attempts >= 1` on any retry.

4. **Escalation** — reached when the hatch is unavailable/declined AND the
   retry budget is exhausted, or when a micro-correction returned a fresh
   block AND both budgets are now spent:

   - Mark the halt through
     `review_decision.sh` already marked the slice `escalated` and populated
     the canonical `escalation_reason`. Leave verdicts recorded.
   - Do not clear `active-slice.json`.
   - The wrapper already emits and dispatches the canonical escalation
     notification returned by `review_decision.sh`.

   - Stop auto-advance. Present Tiger Claw's QA report verbatim.
   - Wait for user instruction.

Tiger Claw re-runs fresh on each retry.

---

## On any escalation

- Do **not** clear `.mutagen/state/active-slice.json`.
- Do **not** rotate stages further.
- Present failing gate's report(s) verbatim.
- `slices/queue.json` has `status: "escalated"` and populated
  `escalation_reason`; use the update helper rather than hand-editing it.
- **Do not auto-advance.**

## Reminders

- In Codex the manifest is advisory. Reviewers are the backstop.
- Every gate's verdict is recorded in `slices/queue.json` under `verdicts.*`
  and in Karai's Dispatch Log.
- In lightweight mode, the slice's `review_required` tag is authoritative — do not skip Tiger Claw based on your own judgment.
- The retry loop is author-only. Structural failures go straight to the
  human.
- `attempts` and `micro_corrections_used` persist across invocations on the
  same slice.
- Host behavior comes from `host_profile.sh`, not the host name in your
  head. If the profile says `serial_only`, believe it and stay serial.
- `$mutagen-execute-next` does **not** refresh or regenerate
  `.mutagen/state/queue-validation.json`. Missing, stale, orphaned, or
  failed validator state hands the workflow back to `$mutagen-slice`.
