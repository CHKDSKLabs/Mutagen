---
name: mutagen-execute-next
description: Explicit-only skill. Run the mutagen pipeline on the next ready slice — dispatch the assigned executor, run a structural-check script, run Bishop and Tiger Claw in parallel, retry on Block / Defect up to MAX_RETRIES with a separate micro-correction budget, auto-advance to the next slice on success until the queue is empty or a stage escalates. Invoke only when the user explicitly says $mutagen-execute-next.
---

# $mutagen-execute-next — run the pipeline until the queue is empty

You orchestrate the full execution pipeline across slices: for each ready
slice you run **author → structural-check script → Bishop ∥ Tiger Claw
(parallel review) → Karai (state record + advisory backlog)**, with a
re-review retry loop on Bishop 🔴 Block or Tiger Claw 🔴 Defect. On success
this skill **auto-advances to the next ready slice** without a fresh prompt;
it stops when the queue is empty or a stage forces escalation.

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

## Codex scope-enforcement note

The Claude Code plugin rotates `.mutagen/state/active-slice.json` between
stages so a `PreToolUse` hook can block out-of-scope writes. Codex's
`codex_hooks` feature is still under development and currently disabled on
Windows, so this skill does not ship manifest-level hooks. You still write
the manifest at each stage transition — for audit, `$mutagen-status`
visibility, and Traag's mediated amendments via `$mutagen-amend-scope` —
but enforcement is advisory. Every dispatch prompt must inline the stage's
`allowed_write_globs` and instruct the agent to self-honour them.

## Parallel mode on Codex

Bounded parallel dispatch (see § Readiness cohort in the Claude variant)
depends on the Agent-tool `isolation: "worktree"` parameter, which has no
direct Codex analogue in v1. This skill runs **serial only** — cohort size
is always 1, regardless of `.claude/workflow.json` `max_parallel_slices`.
The `depends_on` readiness check still applies (you skip slices whose
dependencies are unmet), but you never fan out to multiple slices
concurrently. When Codex ships worktree support, revisit.

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
2. Read `slices/queue.json` (canonical). If the JSON is missing but
   `slices/queue.md` exists, refuse and tell the user to re-run
   `$mutagen-slice`. Find the first **ready** slice whose status is `pending`
   or `blocked_retry`. A slice is ready iff every ID in its `depends_on`
   array (if any) has `status == "completed"` in the queue. Slices with
   unmet dependencies are skipped — never run them out of order just because
   an earlier one is blocked. If nothing is ready, report "queue clear —
   nothing left to dispatch" (or "queue stalled — dependencies unmet:
   <list>" when every remaining pending slice has an unmet dep) and stop.
3. Read `.claude/workflow.json` if present. Extract:
   - `pipeline_mode` — `"full"` (default) or `"lightweight"`.
   - `review.max_retries` — default **2** (up to 3 total author dispatches).
   - `review.max_micro_corrections` — default **1**. Bounds one-shot
     mechanical-fix dispatches, tracked independently of `attempts`.
   - `heartbeat.*` — optional thresholds.
4. Extract from the chosen slice: `slice_id`, `author_agent`, `layer`,
   `bounded_context`, `title`, `objective`, `traces_to`, `review_required`,
   `attempts`, `context_to_update`, `adjacent_scope_allowed` (optional glob
   array), `depends_on` (optional ID array).
5. **Build the Evidence Bundle for this slice and write it to disk.** From
   `traces_to`, resolve every citation to a verbatim excerpt from the cached
   bundle docs. Citation forms:
   - `[FR-NNN]` / `[NFR-NNN]` → PRD bracketed ID + parent bullet/section
   - `ADR-NNNN` → the entire ADR file
   - DDD element → named section verbatim + cited `[INV-N]` lines
   - `[ISC-NNN]` → ISC bracketed ID + invariant + detection context
   - `[DSD-NNN]` → DSD rule line + section heading

   Structure:

   ```
   ## Evidence Bundle for <slice_id>

   ### PRD citations
   <verbatim excerpts>

   ### ADR(s)
   <full ADR text>

   ### DDD citations
   <named element + cited [INV-*] lines>

   ### ISC citations
   <verbatim excerpts>

   ### DSD citations
   <verbatim excerpts>
   ```

   **Write it to `.mutagen/state/evidence/<slice_id>.md`.** Every subsequent
   spawn in this slice (author, Bishop, Tiger Claw, and any retry re-spawn)
   receives the file *path* plus the instruction *"Read this file once; do
   not re-read upstream docs."* — not the inlined text. This keeps prompts
   small, cache-friendly across retries, and guarantees every agent sees
   byte-identical evidence. If the file already exists from a prior attempt
   on the same slice, overwrite it. If a citation cannot be resolved, halt
   and escalate — Shredder bug.

6. Initialise `.mutagen/state/active-slice.json` with the **author** stage
   manifest:

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

   **Adjacent-scope merge (retry only).** On first dispatch (`attempts == 0`
   before the Stage 1 bump), `allowed_write_globs` is strictly the author
   paths + state paths. On any retry dispatch (`attempts >= 1` before Stage
   1 bumps it further), if the slice carries a non-empty
   `adjacent_scope_allowed`, append those globs to `allowed_write_globs`.
   Shredder anticipated these cross-cutting files; the retry loop uses them
   without the human having to hand-edit the manifest. Micro-correction
   dispatches count as retries for this rule.

7. Mark the slice `in_progress` in `slices/queue.json`. After any mutation
   of `queue.json`, re-render: `bash "$MUTAGEN_ROOT/scripts/render_queue.sh"`.

## Per-stage scope manifests

| Stage | `active_agent(s)` | `allowed_write_globs` |
|-------|-------------------|------------------------|
| `author` | author agent | author paths (below) + `project_state.md` + `infrastructure_state.md` + `.mutagen/state/**` (+ `adjacent_scope_allowed` globs on retry) |
| `karai_structural` | `Karai` (script-run, no agent spawn) | `.mutagen/state/**` |
| `review_parallel` | `Bishop`, `TigerClaw` | `reviews/**` + `tests/qa/**` (+ `tests/qa/security/**` when `author_agent == "Tatsu"`) + `.mutagen/state/**` |
| `karai_state` | `Karai` | `project_state.md` + `infrastructure_state.md` + `slices/**` + `.mutagen/state/**` |

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

## Dispatch sequence (per slice)

### Stage 1 — Author

1. Rotate `active-slice.json` to the `author` manifest. Bump `attempts` by 1.
   Mirror `attempts` into `slices/queue.json`.
2. **On the first dispatch for this slice** (pre-bump `attempts == 0`),
   record the start-of-slice git ref for LOC telemetry:
   `git rev-parse HEAD > .mutagen/state/slice-start-ref/<slice_id>`. Skip on
   retry — the base ref stays pinned to before the slice began so
   `scripts/slice_loc.sh` measures net-new across the whole attempt sequence.
3. Spawn the `author_agent`:

   ```bash
   bash "$MUTAGEN_ROOT/bin/agent.sh" <AuthorAgent> "$(cat <<'PROMPT'
   <Slice text — reconstructed from queue.json or copied from queue.md>

   Evidence Bundle path: .mutagen/state/evidence/<slice_id>.md
   Read that file once — every upstream citation required for this slice is
   already extracted there. Do NOT re-read docs/PRD*, docs/ADR*, docs/DDD*,
   docs/ISC*, or docs/DSD*; the evidence file is authoritative. Read source
   files, tests, and existing project state freely.

   Write the State Update block to: <context_to_update>

   Your write scope for this stage (advisory — Codex does not hard-enforce):
     <allowed_write_globs for this stage, verbatim>
   Self-honour these globs. If your work requires a path outside them, stop
   and surface the gap — do not widen scope silently.

   On retry only: the reviewer reports attached below contain Suggested Fix
   blocks. Address each fix in order; keep the change minimal — no refactor.
   Prior reports (by path, do not inline):
     - reviews/<slice_id>/bishop.md
     - reviews/<slice_id>/tiger-claw.md
   PROMPT
   )"
   ```

4. Capture the author's output verbatim and write it to
   `.mutagen/state/author-output/<slice_id>.md`. Clobber-on-write; the file
   reflects the latest attempt and is what the structural-check script
   reads in Stage 2.

### Stage 2 — Structural conformance (script)

Karai the agent is **not** dispatched here. Section-presence, trace-ID
matching, state-block landing, and LOC-vs-target are pattern-matching
checks; a script runs them without burning an agent spawn per slice per
attempt. Karai only wakes for Stage 4 (state verify + dispatch log +
advisory backlog) and reviewer escalations.

1. Rotate `active-slice.json` to the `karai_structural` manifest (Karai
   still owns this stage for scope purposes — the manifest is
   `.mutagen/state/**` writes only).
2. Run the check:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/karai_structural_check.sh" <slice_id>
   ```

   Capture stdout as a single JSON object: `{verdict, findings[], loc}`.
   The script reads the author's output from
   `.mutagen/state/author-output/<slice_id>.md`, the slice metadata from
   `slices/queue.json`, and the context file (`project_state.md` or
   `infrastructure_state.md`). It returns `verdict: "pass"` or
   `verdict: "fail"` deterministically; no prompting involved.
3. Branch on `.verdict`:
   - **`"pass"`** — record `verdicts.karai_structural: "pass"` in
     `slices/queue.json`. Continue to Stage 3.
   - **`"fail"`** — **halt** the pipeline. Mark `slices/queue.json` →
     `verdicts.karai_structural: "fail"`, `status: "escalated"`,
     `escalation_reason: "<concat of findings[].detail>"`. Present the full
     `findings` array to the user verbatim. Do not clear
     `active-slice.json`. Do not auto-advance. Fire a Pushover halt
     notification:

     ```bash
     bash "$MUTAGEN_ROOT/scripts/notify.sh" structural_fail \
       "mutagen — structural fail on {slice_id}" \
       "Structural check halted {slice_id} ({title}): {first finding.detail}. Needs human input."
     ```

   If the script itself fails to run (jq missing, queue unreadable) it
   returns `verdict: "fail"` with a tooling finding — treat as any other
   structural fail. A broken check script is not a pass.

### Stage 3 — Bishop ∥ Tiger Claw (parallel review)

**Skip entire stage** if `pipeline_mode == "lightweight"` AND
`review_required == false`. Record `verdicts.bishop: "skip"` and
`verdicts.tiger_claw: "skip"`. Continue to Stage 4.

Otherwise:

1. Rotate `active-slice.json` to the `review_parallel` manifest.
   Before dispatch, `mkdir -p reviews/<slice_id>` — Bishop and Tiger Claw
   each write one file into that directory.
2. **Dispatch Bishop and Tiger Claw concurrently** via the parallel wrapper.
   Build the shared prompt once (slice artifacts + Evidence Bundle *path*
   + author output *path* + advisory scope rules), then:

   ```bash
   bash "$MUTAGEN_ROOT/bin/agents-parallel.sh" \
     Bishop TigerClaw "$(cat review_prompt.md)"
   ```

   The prompt instructs Bishop to write `reviews/<slice_id>/bishop.md` and
   Tiger Claw to write `reviews/<slice_id>/tiger-claw.md` + a convenience
   copy at `.mutagen/state/tiger-claw-latest.md`. Each reviewer reads
   `.mutagen/state/evidence/<slice_id>.md` once — do NOT inline the bundle
   into the prompt.
3. Record verdicts in `slices/queue.json`:
   - Bishop: 🟢 Clean → `"clean"`; 🟡 Advisory → `"advisory"`; ⏭ Skip →
     `"skip"`; 🔴 Block → `"block"`.
   - Tiger Claw: 🟢 Clean → `"clean"`; 🟡 Gap → `"gap"`; ⏭ Skip → `"skip"`;
     🔴 Defect → `"defect"`.
4. **Confirm both reports are persisted**:
   - Bishop: `reviews/<slice_id>/bishop.md`.
   - Tiger Claw: `reviews/<slice_id>/tiger-claw.md` (per-slice audit trail)
     + `.mutagen/state/tiger-claw-latest.md` (convenience pointer the retry
     loop can read without knowing the slice ID; clobber-on-write).
   If either file is missing, the reviewer did not follow protocol — treat
   as a non-conformant return, escalate. Do not fabricate the file.
5. Evaluate:
   - Both non-🔴 → continue to Stage 4.
   - Either 🔴 → enter the re-review retry loop.

### Stage 4 — Karai state verification + dispatch log

1. Rotate `active-slice.json` to the `karai_state` manifest.
2. Re-spawn Karai for Verify-state. Append a Dispatch Log row with final
   verdicts. Karai also appends each of Bishop's 🟡 advisories from
   `reviews/<slice_id>/bishop.md` to `.mutagen/state/advisory-backlog.jsonl`
   (one JSON object per line — see `agents/Karai.md` § Dispatch Protocol
   step 6). The backlog is consumed by `$mutagen-consolidate-advisories`;
   Karai never dequeues, only appends.
3. Record completion: `status: "completed"`, `completed_at: <ISO-8601 UTC>`.

### Stage 5 — Record, offload, advance

1. Re-render: `bash "$MUTAGEN_ROOT/scripts/render_queue.sh"`.
2. **Write the slice summary.** `mkdir -p slices/<slice_id>` and emit
   `slices/<slice_id>/summary.md` with this shape:

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
   - Bishop: <clean|advisory|block|skip>
   - Tiger Claw: <clean|gap|defect|skip>

   ## Files touched
   <from scripts/slice_loc.sh + the author's Code Artifacts list — paths only>

   ## Advisories logged
   <count + one-line per advisory, or "none">

   ## Retry path
   <brief — "first-pass clean", "1 Bishop retry cleared", "micro-correction on attempt 2", etc.>

   ## Reports
   - Review: reviews/<slice_id>/bishop.md
   - QA:     reviews/<slice_id>/tiger-claw.md
   - Evidence: .mutagen/state/evidence/<slice_id>.md
   ```

   Once written, the orchestrator must **not** carry per-agent transcripts
   (author output, Bishop report body, Tiger Claw report body) forward in
   its own context. Reference the summary file; re-read on demand.

3. Clear `.mutagen/state/active-slice.json`.
4. **Milestone check.** Inspect `slices/queue.json`: if no `pending` or
   `blocked_retry` slice remains in the just-completed slice's `layer`,
   fire:

   ```bash
   bash "$MUTAGEN_ROOT/scripts/notify.sh" layer_complete \
     "mutagen — layer <N> complete" \
     "<M> slices completed in layer <N>. Next pending slice: <id or 'queue clear'>"
   ```

   The notifier self-gates via `.claude/workflow.json` `notify.milestones`.
5. **Emit the one-line completion marker AND immediately continue in the
   same turn.** The full summary is on disk at
   `slices/<slice_id>/summary.md`; do not restate its contents here. The
   marker is exactly one line in this shape and nothing more:

   `✔ <slice_id> — <bishop verdict>/<tiger_claw verdict>, attempts=<N>[, micro_correction][ — heartbeat: <anomaly>]`

   Do **not** append "Next slice:", "Proceeding to…", "Ready to
   continue?", file-touched lists, cross-slice findings, or any other
   prose. The marker is a log line, not a conversation turn. In the
   **same assistant turn** that emits this marker, issue the Preflight
   tool calls for the next slice. Ending your turn after the marker
   without having dispatched the next Preflight is the violation we're
   trying to avoid.
6. **Auto-advance** if the queue has a `pending` or `blocked_retry` slice.
   Jump back to Preflight — in the same turn as step 5's marker. No fresh
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
  Fire `notify.sh queue_clear "mutagen — queue clear" "N slices completed."`.
- **Queue stalled** — pending slices remain but every one has an unmet
  `depends_on`. Report the stall list and stop. (No separate notify event;
  this is a planning issue, surface via normal output.)
- **Structural escalation** — Stage 2 fail. Notification fired in Stage 2.
- **Retry budget exhausted** — retry loop escalation.
- **Scope violation** — an agent self-reported or a reviewer caught an
  out-of-scope write. Fire `notify.sh scope_violation "mutagen — scope
  violation on {slice_id}" "Out-of-scope write in stage {stage}. Agent:
  {active_agent}."`.
- **User interrupt** — complete in-flight stage cleanly, report, wait.

---

## Re-review retry loop

Triggered by Bishop 🔴 Block or Tiger Claw 🔴 Defect (or both) from Stage 3.

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

1. Read `attempts` and `micro_corrections_used` from
   `.mutagen/state/active-slice.json` (initialise `micro_corrections_used:
   0` on first entry).

2. **Escape-hatch evaluation** — run first, on every 🔴 entry.

   **Hatch availability:** `micro_corrections_used < max_micro_corrections`.
   If the budget is spent, skip to step 3.

   **Hatch conditions** — all four must hold:

   - **Convergence.** Either only one reviewer fired, or both blocked on
     the same defect — same file(s), same root cause when stated in prose.
   - **Mechanical scope.** ≲ 20 LOC, confined to tests, wiring, imports,
     renames, stale comments. No new behavior, no contract change.
   - **Named fix.** You can state the exact file(s), change, and point at
     a reviewer's `Suggested Fix` block.
   - **In-scope executor.** Fix path is in `author_agent`'s globs + any
     `adjacent_scope_allowed` globs, or in Bebop's globs (fallback for
     test / wiring misses).

   All four hold → dispatch a **micro-correction**:

   1. Rotate `active-slice.json` to the `author` manifest (retry rules
      apply — `adjacent_scope_allowed` merges in). Choose the executor:
      current `author_agent` if the fix sits in their globs, otherwise
      Bebop. Record chosen agent in `active_agent`.
   2. Increment `micro_corrections_used` by 1 in active-slice.json and
      mirror to `slices/queue.json` as `verdicts.micro_corrections_used`.
      **Do not** bump `attempts` — a micro-correction is not a full author
      dispatch and must not consume the retry budget.
   3. Prompt the executor with the Evidence Bundle path, the prior author
      output (by path), and a tight micro-correction instruction: the
      cited `Suggested Fix` verbatim, the exact file(s) and change, and
      the rule *"change only what is named here — no refactor, no
      tangential cleanup, no scope expansion."*
   4. On return, run Stage 2 (structural check script) normally, then
      Stage 3 (Bishop ∥ Tiger Claw) one more time.
   5. If both reviewers return non-🔴 → normal Stage 4 completion. Record
      `verdicts.micro_correction: true` for telemetry. Auto-advance.
   6. If anything blocks again → return to the top of this retry loop
      (step 1). The next entry will find `micro_corrections_used`
      exhausted and skip the hatch, falling through to the standard
      retry branch or escalation based on `attempts`.

3. **Standard retry branch** — reached when the hatch is unavailable or
   its conditions don't hold.

   - If `attempts >= max_retries + 1`, retry budget is exhausted →
     **escalate** (step 4).
   - Otherwise, retry is allowed:
     - Mark `status: "blocked_retry"`. Re-render.
     - Confirm triggering reports are persisted
       (`reviews/<slice_id>/bishop.md`, `reviews/<slice_id>/tiger-claw.md`,
       `.mutagen/state/tiger-claw-latest.md`).
     - Loop back to **Stage 1**. The author is re-dispatched with every
       Suggested Fix from every reviewer that blocked (only the ones that
       fired). After Stage 1 returns, proceed through Stage 2, then Stage
       3 re-runs Bishop and Tiger Claw fresh and in parallel.
     - `attempts` is bumped in Stage 1 (not here). `adjacent_scope_allowed`
       merges into the manifest because `attempts >= 1` on any retry.

4. **Escalation** — reached when the hatch is unavailable/declined AND the
   retry budget is exhausted, or when a micro-correction returned a fresh
   block AND both budgets are now spent:

   - Mark `status: "escalated"`, `escalation_reason: "Bishop Block / Tiger
     Claw Defect after N attempts (micro_corrections_used: M)"`. Leave
     verdicts recorded.
   - Re-render `slices/queue.md`.
   - Do not clear `active-slice.json`.
   - Fire:

     ```bash
     bash "$MUTAGEN_ROOT/scripts/notify.sh" escalation \
       "mutagen — halted at {slice_id}" \
       "{slice_id} ({title}) escalated after {N} attempts ({M} micro-corrections). Blocked by: {reviewer(s)}. Needs human input."
     ```

   - Stop auto-advance. Present blocking reports verbatim.
   - Wait for user instruction.

Bishop and Tiger Claw re-run fresh on each retry. A prior Block cleared on
retry becomes a new 🟢 Clean (or 🟡 Advisory, etc.) in `verdicts.bishop`.

---

## On any escalation

- Do **not** clear `.mutagen/state/active-slice.json`.
- Do **not** rotate stages further.
- Present failing gate's report(s) verbatim.
- `slices/queue.json` has `status: "escalated"` and populated
  `escalation_reason`; re-render.
- **Do not auto-advance.**

## Reminders

- In Codex the manifest is advisory. Reviewers are the backstop.
- Every gate's verdict is recorded in `slices/queue.json` under `verdicts.*`
  and in Karai's Dispatch Log.
- In lightweight mode, the slice's `review_required` tag is authoritative.
- The retry loop is author-only. Structural failures go straight to the
  human.
- `attempts` and `micro_corrections_used` persist across invocations on the
  same slice.
- Parallel mode (`max_parallel_slices > 1`) is unsupported on Codex v1 —
  this skill runs serial regardless of the config value.
