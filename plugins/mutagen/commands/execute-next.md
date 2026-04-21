---
description: Run Karai on the next pending slice — dispatches the assigned executor, runs Bishop and Tiger Claw in parallel, retries on Block / Defect up to MAX_RETRIES, auto-advances to the next slice on success until the queue is empty or a stage escalates.
---

# Execute-next — run Karai, then keep running until the queue is empty

The user has invoked `/mutagen:execute-next`. You orchestrate the full execution pipeline across slices: for each slice in the queue you run **author → Karai (structural) → Bishop ∥ Tiger Claw (parallel review) → Karai (state record)**, with a re-review retry loop on Bishop 🔴 Block or Tiger Claw 🔴 Defect, and per-stage scope manifest rotation so each stage only has the write paths it actually needs. On success the command **auto-advances to the next pending slice** without waiting for a fresh prompt; it only stops when the queue is empty or a stage forces an escalation.

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

1. `mkdir -p .mutagen/state reviews slices`.
2. Read `slices/queue.json` (canonical — see [`guides/queue-schema.md`](../guides/queue-schema.md)). If the JSON is missing but `slices/queue.md` exists, refuse and tell the user to re-run `/mutagen:slice` — Karai drives from the JSON. Find the first slice whose status is `pending` or `blocked_retry`. If none, report "queue clear — nothing left to dispatch" and stop.
3. Read `.claude/workflow.json` (if present). Extract:
   - `pipeline_mode` — `"full"` (default) runs every slice through Bishop + Tiger Claw; `"lightweight"` runs those gates only on slices with `review_required: true`.
   - `review.max_retries` — how many re-dispatch attempts after a 🔴 Block or 🔴 Defect before escalating. Default: **2** (i.e. up to 3 total author dispatches per slice).
   - `heartbeat.*` — optional inspection thresholds (see `agents/Karai.md`).
4. Extract from the chosen slice (straight from the JSON):
   - `slice_id`, `author_agent`, `layer`, `bounded_context`, `title`, `objective`
   - `traces_to` (PRD / ADR / DDD / ISC / DSD citations)
   - `review_required` (lightweight mode only)
   - `attempts` (starts at 0 on a fresh slice; carries the count if the slice was previously `blocked_retry`)
   - `context_to_update` (`project_state.md` or `infrastructure_state.md`)
5. **Build the Evidence Bundle for this slice.** From the slice's `traces_to` block, resolve every citation to a verbatim excerpt out of the bundle docs you cached in Session preflight. The bundle is reused for every spawn in this slice (author, Bishop, Tiger Claw, and any retry author re-spawn) — assemble it once.

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
     "allowed_write_globs": [ "<author paths — see table below>" ]
   }
   ```

   For the parallel review stage the `active_agent` field is replaced with `active_agents: ["Bishop", "TigerClaw"]`; the guard doesn't care about the name field (it only evaluates `allowed_write_globs`), but the status command and telemetry do.
7. Mark the slice `in_progress` in `slices/queue.json` (overwrite status). After every mutation of `queue.json` in any stage below, re-render the markdown: `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh`.

## Per-stage scope manifests

The guard (`scripts/guard.sh`) reads `allowed_write_globs` on every Write / Edit. Rewriting that list between stages is the mechanism that enforces per-subagent scope without per-subagent hooks. For each stage below, **overwrite `.mutagen/state/active-slice.json`** with the manifest shown before spawning.

| Stage | `active_agent(s)` | `allowed_write_globs` |
|-------|-------------------|------------------------|
| `author` | author agent | author paths (table below) + `project_state.md` + `infrastructure_state.md` + `.mutagen/state/**` |
| `karai_structural` | `Karai` | `.mutagen/state/**` (Karai emits a report; she does not write to project files at this stage) |
| `review_parallel` | `Bishop`, `TigerClaw` | `reviews/**` + `tests/qa/**` (+ `tests/qa/security/**` when `author_agent == "Tatsu"`) + `.mutagen/state/**` |
| `karai_state` | `Karai` | `project_state.md` + `infrastructure_state.md` + `slices/**` + `.mutagen/state/**` |

The `review_parallel` manifest is the union of what Bishop and Tiger Claw need, and nothing more. Bishop only writes `reviews/{slice_id}.md`; Tiger Claw only writes under `tests/qa/**`. The paths are disjoint, so the narrow union is still tight enough to catch any stray write into production source, author tests, infra, or the design bundle.

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

Run stages 1 → 4 in order. The retry loop below wraps stages 1 + 3 (author + parallel review).

### Stage 1 — Author

1. Rotate active-slice.json to the `author` manifest. Bump `attempts` by 1 (first dispatch → 1; first retry → 2; and so on). Mirror `attempts` into `slices/queue.json` for the slice.
2. Spawn the `author_agent` subagent via the Agent tool. Prompt includes:
   - The slice text (reconstructed from the JSON or copied verbatim from the rendered markdown).
   - **The Evidence Bundle** assembled in preflight step 5, inlined verbatim.
   - **Explicit instruction:** *"All upstream evidence required to execute this slice is inlined above as the Evidence Bundle. Do NOT re-read `docs/PRD*`, `docs/ADR*`, `docs/DDD*`, `docs/ISC*`, or `docs/DSD*` — every cited fragment is already in your context. Read source files, tests, and existing project state freely; just don't cold-load the design bundle."*
   - A reminder of the agent's Output Format (see `${CLAUDE_PLUGIN_ROOT}/agents/<Agent>.md`).
   - Instruction to write the State Update block to `context_to_update`.
   - **On retry only:** attach every `Suggested Fix` from the prior review stage's reports (Bishop's per-Block `Suggested Fix`, Tiger Claw's per-Defect `Suggested Fix`, or both if both blocked). Include the full Review Report and QA Report at the end of the prompt so the author has surrounding evidence, but highlight the Suggested Fix blocks up front. Instruct the author to address each fix in order and to keep the change minimal — do not refactor around the fix.
3. Wait for the author to return. Capture their output.

### Stage 2 — Karai structural conformance

1. Rotate active-slice.json to the `karai_structural` manifest.
2. Spawn Karai with the author's output **and the slice text** (no Evidence Bundle needed — her checks are structural: required-section presence, identifier shape, traces-to drift, LOC vs target). She validates against the Conformance Validation checklist for the author agent.
3. On failure (missing / empty required section, mis-filed state block, identifier mismatch, traces-to drift, LOC > 120 % of target): **halt** the pipeline — this is not retryable by the author within this command; it is a structural break that needs the human. Mark `slices/queue.json` → `verdicts.karai_structural: "fail"`, `status: "escalated"`, `escalation_reason: "<from Karai>"`. Escalate with Karai's report verbatim. **Do not** clear active-slice.json. **Do not auto-advance** — stop here and wait for the user. Before stopping, fire a Pushover halt notification: `bash ${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh structural_fail "mutagen — structural fail on {slice_id}" "Karai halted {slice_id} ({title}) at stage karai_structural: {short reason from report}. Needs human input."` — the script silently no-ops when Pushover is not configured.
4. On pass: record `verdicts.karai_structural: "pass"`. Continue.

### Stage 3 — Bishop ∥ Tiger Claw (parallel review)

**Skip the entire stage** if `pipeline_mode == "lightweight"` AND `review_required == false`. Record `verdicts.bishop: "skip"` and `verdicts.tiger_claw: "skip"`. Continue to Stage 4.

Otherwise:

1. Rotate active-slice.json to the `review_parallel` manifest.
2. **Dispatch Bishop and Tiger Claw in a single assistant turn.** Issue both Agent tool calls in the *same* message — one `subagent_type: "Bishop"`, one `subagent_type: "TigerClaw"` — so Claude Code runs them concurrently. Each gets the slice artifacts, the **Evidence Bundle from preflight step 5** (same text the author received), the author's output, and the same *"do not re-read upstream docs"* instruction. They evaluate against the inlined evidence; neither sees the other's findings.
3. Wait for both to return, then record verdicts in `slices/queue.json`:
   - Bishop: 🟢 Clean → `"clean"`; 🟡 Advisory → `"advisory"`; ⏭ Skip → `"skip"`; 🔴 Block → `"block"`.
   - Tiger Claw: 🟢 Clean → `"clean"`; 🟡 Gap → `"gap"`; ⏭ Skip → `"skip"`; 🔴 Defect → `"defect"`.
4. Before any rotation out of this stage, **persist both reports** so the retry path has them:
   - Bishop writes his Review Report to `reviews/{slice_id}.md` directly — already persistent.
   - Capture Tiger Claw's QA Report to `.mutagen/state/tiger-claw-report.md` (clobber-on-write is fine; only one slice is active at a time).
5. Evaluate the joint verdict:
   - Both non-🔴 (clean / advisory / gap / skip in any combination) → continue to Stage 4.
   - Either 🔴 Block or 🔴 Defect (or both) → enter the **re-review retry loop** below. On retry, the author gets *every* Suggested Fix from whichever reports blocked; if Bishop was 🟢/🟡 and only Tiger Claw fired, attach only Tiger Claw's, and vice versa.

### Stage 4 — Karai state verification + dispatch log

1. Rotate active-slice.json to the `karai_state` manifest.
2. Re-spawn Karai for her Verify-state step: confirm the author's State Update block landed in `context_to_update`. Append a Dispatch Log row with the slice's final verdicts (both Bishop and Tiger Claw).
3. Record completion in `slices/queue.json`: `status: "completed"`, `completed_at: <ISO-8601 UTC>`.

### Stage 5 — Record & advance

1. Re-render `slices/queue.md` from the updated JSON: `${CLAUDE_PLUGIN_ROOT}/scripts/render_queue.sh`.
2. Clear `.mutagen/state/active-slice.json`.
3. Report the slice summary + telemetry (attempts, Bishop verdict, Tiger Claw verdict, any heartbeat anomalies from `scripts/heartbeat.sh`) to the user as a short update — enough that progress is visible, terse enough that it doesn't bury the next slice's report.
4. **Auto-advance.** If the queue still has a `pending` or `blocked_retry` slice, jump straight back to **Preflight** and run the next one. Do not wait for a fresh prompt, do not ask for permission. Keep looping until one of the stop conditions below fires.

### Auto-advance stop conditions

Halt the loop (and do not clear active-slice.json) when any of the following happens. Every halt also fires a Pushover notification via `${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh <event> "<title>" "<message>"` so the human knows the pipeline is waiting — the script silently no-ops when Pushover is not configured, so the call is safe to make unconditionally.

- **Queue clear** — no `pending` or `blocked_retry` slices remain. Report "queue clear — all slices completed" and stop. Fire `notify.sh queue_clear "mutagen — queue clear" "N slices completed."` (normal priority, typically the only good-news notification users keep enabled).
- **Structural escalation** — Karai's Stage 2 fires a fail. Present her report verbatim and stop. Notification already fired in Stage 2.
- **Retry budget exhausted** — the retry loop's escalation branch (see below) fires on the current slice. Present the blocking report verbatim and stop. Notification fired from inside the retry loop.
- **Traag DENY** — the guard hook blocks a write during any stage. Karai treats that as a Red inspection outcome and halts; auto-advance stops. Fire `notify.sh scope_violation "mutagen — scope violation on {slice_id}" "Traag DENY on {path} (class: {class}) during stage {stage}. Agent: {active_agent}."`.
- **User interrupt** — the user sends a message while the loop is running. Complete the in-flight stage cleanly, report where you stopped, and wait. Do not fire a notification — the user is already here.

---

## Re-review retry loop

Triggered by Bishop 🔴 Block or Tiger Claw 🔴 Defect (or both) from Stage 3.

1. Read the current `attempts` from `.mutagen/state/active-slice.json`.
2. If `attempts >= max_retries + 1`, the retry budget is exhausted:
   - Mark `slices/queue.json` → `status: "escalated"`, `escalation_reason: "Bishop Block / Tiger Claw Defect after N attempts"` (name whichever reviewers blocked). Leave the verdicts recorded.
   - Re-render `slices/queue.md`.
   - **Do not** clear active-slice.json.
   - **Fire a Pushover notification:** `bash ${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh escalation "mutagen — halted at {slice_id}" "{slice_id} ({title}) escalated after {N} attempts. Blocked by: {Bishop and/or Tiger Claw}. Needs human input."` — silent no-op when Pushover is not configured.
   - **Stop auto-advance.** Present the blocking reports verbatim (Bishop's and/or Tiger Claw's).
   - Wait for user instruction (amend scope via `/mutagen:amend-scope`, re-slice via `/mutagen:slice`, fix in place manually, abandon).
3. Otherwise, retry is allowed:
   - Mark `slices/queue.json` → `status: "blocked_retry"`. Re-render.
   - Confirm the triggering reports are persisted (Bishop: `reviews/{slice_id}.md`; Tiger Claw: `.mutagen/state/tiger-claw-report.md`).
   - Loop back to **Stage 1 (Author)**. The author is re-dispatched with every Suggested Fix from every reviewer that blocked (only the ones that fired — don't attach reports from a reviewer that returned 🟢 / 🟡 / ⏭). After Stage 1 returns, proceed through Stage 2, then Stage 3 re-runs Bishop and Tiger Claw **both fresh and in parallel** again, regardless of which one had blocked previously.
   - `attempts` is bumped in Stage 1 (not here); only the status flip + report capture happen here.

Bishop and Tiger Claw re-run fresh on each retry; their prior reports are not treated as carry-over verdicts. A prior Block cleared on retry becomes a new 🟢 Clean (or 🟡 Advisory, etc.) in `verdicts.bishop`.

---

## On any escalation

- Do **not** clear `.mutagen/state/active-slice.json`.
- Do **not** rotate stages further — leave the manifest at the stage that halted.
- Present the failing gate's report(s) to the user verbatim.
- Ensure `slices/queue.json` has `status: "escalated"` (or `"refused"` for intake refusal) and a populated `escalation_reason`; re-render `slices/queue.md`.
- **Do not auto-advance.** Wait for user instruction.

## Reminders

- Stage 3 dispatches Bishop and Tiger Claw as two tool calls in a **single** assistant turn. Do not serialise them — the point of the redesign is to collapse what used to be two stages into one parallel window. They share the manifest for this stage and write to disjoint paths.
- The guard hook enforces `allowed_write_globs` literally. Rotating the manifest between stages is what gives each subagent the minimum writable surface; the previous union-allowlist design is gone. A write that belongs in a later stage is a bug — fix the call order, don't widen the manifest.
- Every gate's verdict is recorded in `slices/queue.json` under `verdicts.*` and in Karai's Dispatch Log. The markdown rendering shows verdicts at a glance.
- In lightweight mode, the slice's `review_required` tag is authoritative; do not skip gates based on your own judgment of complexity. Either both reviewers run or both skip — there's no half-parallel mode.
- The retry loop is author-only. Karai's structural failures and Traag's scope denies go straight to the human — the author cannot retry around discipline violations.
- `attempts` in `slices/queue.json` persists across multiple `/mutagen:execute-next` invocations on the same slice, so resuming a `blocked_retry` after a session restart picks up where you left off.
- Auto-advance keeps the loop running until a stop condition fires. Between slices, the only state that matters is `slices/queue.json`, `project_state.md`, `infrastructure_state.md`, and the persisted reviews — the per-slice `active-slice.json` is cleared at the end of a successful slice and rewritten at the start of the next.

$ARGUMENTS
