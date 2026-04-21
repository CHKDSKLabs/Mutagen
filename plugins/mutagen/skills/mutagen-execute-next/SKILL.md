---
name: mutagen-execute-next
description: Explicit-only skill. Run Karai on the next pending slice — dispatch the assigned executor, run Bishop and Tiger Claw in parallel, retry on Block / Defect up to MAX_RETRIES, auto-advance to the next slice on success until the queue is empty or a stage escalates. Invoke only when the user explicitly says $mutagen-execute-next.
---

# $mutagen-execute-next — run Karai, then keep running until the queue is empty

You orchestrate the full execution pipeline across slices: for each pending
slice you run **author → Karai (structural) → Bishop ∥ Tiger Claw (parallel
review) → Karai (state record)**, with a re-review retry loop on Bishop 🔴
Block or Tiger Claw 🔴 Defect. On success this skill **auto-advances to the
next pending slice** without a fresh prompt; it only stops when the queue is
empty or a stage forces escalation.

## Codex scope-enforcement note

The Claude Code plugin rotates `.mutagen/state/active-slice.json` between
stages so a `PreToolUse` hook can block out-of-scope writes. Codex's
`codex_hooks` feature is still under development and currently disabled on
Windows, so this skill does not ship manifest-level hooks. You still write
the manifest at each stage transition — for audit, `$mutagen-status`
visibility, and Traag's mediated amendments via `$mutagen-amend-scope` —
but enforcement is advisory. Every dispatch prompt must inline the stage's
`allowed_write_globs` and instruct the agent to self-honour them.

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
3. Hold these in context for the duration of the run.

## Preflight (runs once per slice — re-enter at the top of the loop)

1. `mkdir -p .mutagen/state reviews slices`.
2. Read `slices/queue.json` (canonical). If the JSON is missing but
   `slices/queue.md` exists, refuse and tell the user to re-run
   `$mutagen-slice`. Find the first slice whose status is `pending` or
   `blocked_retry`. If none, report "queue clear — nothing left to dispatch"
   and stop.
3. Read `.claude/workflow.json` if present. Extract:
   - `pipeline_mode` — `"full"` (default) or `"lightweight"`.
   - `review.max_retries` — default **2**.
   - `heartbeat.*` — optional thresholds.
4. Extract from the chosen slice: `slice_id`, `author_agent`, `layer`,
   `bounded_context`, `title`, `objective`, `traces_to`, `review_required`,
   `attempts`, `context_to_update`.
5. **Build the Evidence Bundle for this slice.** From `traces_to`, resolve
   every citation to a verbatim excerpt from the cached bundle docs.
   Citation forms:
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

   If a citation cannot be resolved, halt and escalate — Shredder bug.

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
     "allowed_write_globs": [ "<author paths — see table below>" ]
   }
   ```

7. Mark the slice `in_progress` in `slices/queue.json`. After any mutation
   of `queue.json`, re-render: `bash "$MUTAGEN_ROOT/scripts/render_queue.sh"`.

## Per-stage scope manifests

| Stage | `active_agent(s)` | `allowed_write_globs` |
|-------|-------------------|------------------------|
| `author` | author agent | author paths (below) + `project_state.md` + `infrastructure_state.md` + `.mutagen/state/**` |
| `karai_structural` | `Karai` | `.mutagen/state/**` |
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
2. Spawn the `author_agent`:

   ```bash
   bash "$MUTAGEN_ROOT/bin/agent.sh" <AuthorAgent> "$(cat <<'PROMPT'
   <Slice text — reconstructed from queue.json or copied from queue.md>

   <Evidence Bundle from preflight step 5, inlined verbatim>

   All upstream evidence required to execute this slice is inlined above.
   Do NOT re-read docs/PRD*, docs/ADR*, docs/DDD*, docs/ISC*, or docs/DSD*.
   Read source files, tests, and existing project state freely.

   Write the State Update block to: <context_to_update>

   Your write scope for this stage (advisory — Codex does not hard-enforce):
     <allowed_write_globs for this stage, verbatim>
   Self-honour these globs. If your work requires a path outside them, stop
   and surface the gap — do not widen scope silently.

   On retry only: the reviewer reports attached below contain Suggested Fix
   blocks. Address each fix in order; keep the change minimal — no refactor.
   PROMPT
   )"
   ```

3. Capture the author's output.

### Stage 2 — Karai structural conformance

1. Rotate `active-slice.json` to the `karai_structural` manifest.
2. Spawn Karai with the author's output + the slice text. Structural checks
   only — no Evidence Bundle.
3. On failure: **halt**. Mark `slices/queue.json` →
   `verdicts.karai_structural: "fail"`, `status: "escalated"`,
   `escalation_reason: "<from Karai>"`. Do not clear `active-slice.json`.
   Do not auto-advance. Fire a Pushover halt notification:
   ```bash
   bash "$MUTAGEN_ROOT/scripts/notify.sh" structural_fail \
     "mutagen — structural fail on {slice_id}" \
     "Karai halted {slice_id} ({title}) at stage karai_structural: {short reason}. Needs human input."
   ```
4. On pass: record `verdicts.karai_structural: "pass"`. Continue.

### Stage 3 — Bishop ∥ Tiger Claw (parallel review)

**Skip entire stage** if `pipeline_mode == "lightweight"` AND
`review_required == false`. Record `verdicts.bishop: "skip"` and
`verdicts.tiger_claw: "skip"`. Continue to Stage 4.

Otherwise:

1. Rotate `active-slice.json` to the `review_parallel` manifest.
2. **Dispatch Bishop and Tiger Claw concurrently** via the parallel wrapper.
   Build the shared prompt once (slice artifacts + Evidence Bundle + author
   output + advisory scope rules), then:

   ```bash
   bash "$MUTAGEN_ROOT/bin/agents-parallel.sh" \
     Bishop TigerClaw "$(cat review_prompt.md)"
   ```

   The wrapper captures each agent's stdout to
   `.mutagen/state/bishop.stdout` and `.mutagen/state/tigerclaw.stdout`.
3. Record verdicts in `slices/queue.json`:
   - Bishop: 🟢 Clean → `"clean"`; 🟡 Advisory → `"advisory"`; ⏭ Skip →
     `"skip"`; 🔴 Block → `"block"`.
   - Tiger Claw: 🟢 Clean → `"clean"`; 🟡 Gap → `"gap"`; ⏭ Skip → `"skip"`;
     🔴 Defect → `"defect"`.
4. Persist both reports for retry:
   - Bishop writes to `reviews/{slice_id}.md` directly.
   - Capture Tiger Claw's report to `.mutagen/state/tiger-claw-report.md`.
5. Evaluate:
   - Both non-🔴 → continue to Stage 4.
   - Either 🔴 → enter the re-review retry loop.

### Stage 4 — Karai state verification + dispatch log

1. Rotate `active-slice.json` to the `karai_state` manifest.
2. Re-spawn Karai for Verify-state. Append a Dispatch Log row with final
   verdicts.
3. Record completion: `status: "completed"`, `completed_at: <ISO-8601 UTC>`.

### Stage 5 — Record & advance

1. Re-render: `bash "$MUTAGEN_ROOT/scripts/render_queue.sh"`.
2. Clear `.mutagen/state/active-slice.json`.
3. Report slice summary + telemetry.
4. **Auto-advance** if the queue has a `pending` or `blocked_retry` slice.

### Auto-advance stop conditions

Halt the loop (do not clear active-slice.json) and fire Pushover
notifications on:

- **Queue clear** — report "queue clear — all slices completed" and stop.
  Fire `notify.sh queue_clear "mutagen — queue clear" "N slices completed."`.
- **Structural escalation** — Stage 2 fail. Notification fired in Stage 2.
- **Retry budget exhausted** — retry loop escalation.
- **Scope violation** — an agent self-reported or a reviewer caught an
  out-of-scope write. Fire `notify.sh scope_violation "mutagen — scope
  violation on {slice_id}" "Out-of-scope write in stage {stage}. Agent:
  {active_agent}."`.
- **User interrupt** — complete in-flight stage cleanly, report, wait.

---

## Re-review retry loop

Triggered by Bishop 🔴 Block or Tiger Claw 🔴 Defect (or both).

1. Read `attempts` from `.mutagen/state/active-slice.json`.
2. If `attempts >= max_retries + 1`, evaluate the **micro-correction escape
   hatch** — all four conditions must hold:
   - **Convergence.** Either only one reviewer fired, or both blocked on
     the same defect.
   - **Mechanical scope.** ≲ 20 LOC, confined to tests, wiring, imports,
     renames, stale comments.
   - **Named fix.** You can state the exact file(s), change, and point at
     a reviewer's `Suggested Fix`.
   - **In-scope executor.** Fix path is in `author_agent`'s or Bebop's globs.

   All four hold → dispatch a one-shot micro-correction. Bump `attempts` to
   `max_retries + 2`. Run Stage 2 + Stage 3 once more. Clean → Stage 4 +
   `verdicts.micro_correction: true`. Blocks again → hard escalate.

   **Escalation:** mark `status: "escalated"`, re-render, fire:
   ```bash
   bash "$MUTAGEN_ROOT/scripts/notify.sh" escalation \
     "mutagen — halted at {slice_id}" \
     "{slice_id} ({title}) escalated after {N} attempts. Blocked by: {reviewer(s)}. Needs human input."
   ```
   Stop auto-advance. Present blocking reports verbatim.

3. Otherwise retry is allowed:
   - Mark `status: "blocked_retry"`. Re-render.
   - Confirm reports are persisted.
   - Loop back to **Stage 1**.

Bishop and Tiger Claw re-run fresh on each retry.

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
- `attempts` persists across invocations on the same slice.
