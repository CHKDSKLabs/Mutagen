---
description: Run Karai on the next pending slice — dispatches the assigned executor, routes through Bishop review and Tiger Claw adversarial QA, records state.
---

# Execute-next — run Karai on the next pending slice

The user has invoked `/shredder:execute-next`. You are orchestrating the full execution pipeline for a single slice: **author → Karai (structural) → Bishop (review) → Tiger Claw (QA) → Karai (state record)**.

## Preflight

1. `mkdir -p .claude/state reviews`.
2. Read `slices/queue.md`. Find the first slice whose status is not `completed`, `refused`, or `escalated`. If none, report "queue clear" and stop.
3. Read `.claude/workflow.json` (if present). Note the pipeline mode — `"full"` (default) runs every slice through Bishop + Tiger Claw; `"lightweight"` runs those gates only on slices tagged `review_required: true`.
4. Extract from the chosen slice:
   - `slice_id`
   - `author_agent` — one of Bebop / Baxter / Chaplin / Metalhead / Splinter / Tatsu / Krang
   - `layer`
   - Traces-to (PRD / ADR / DDD / ISC / DSD citations)
   - `review_required` tag (lightweight mode only)
5. Build the **union allowlist** — the scope the PreToolUse guard will enforce across the entire pipeline for this slice. Start from the author agent's default scope (see `agents/Traag.md` for the per-agent table) and add the supervisory / gate paths needed by Karai, Bishop, and Tiger Claw. A reasonable default:

   ```json
   {
     "slice_id": "<from queue>",
     "author_agent": "<from queue>",
     "pipeline_mode": "full | lightweight",
     "review_required": true,
     "allowed_write_globs": [
       "<author-specific paths from Traag's table>",
       "reviews/**",
       "tests/qa/**",
       "tests/qa/security/**",
       "project_state.md",
       "infrastructure_state.md",
       ".claude/state/**"
     ]
   }
   ```

   Example author-scope mappings:

   | author_agent | author paths to include |
   |--------------|-------------------------|
   | Bebop | `src/**`, `app/**`, `api/**`, `components/**`, `pages/**`, `tests/**` (excluding `tests/qa/**`, `tests/security/**`, `tests/db/**`), `styles/**`, `public/**` |
   | Baxter | cited algorithmic modules + their tests |
   | Chaplin | `migrations/**`, `schema/**`, `db/**`, `prisma/**`, `src/models/**`, `src/queries/**`, `src/repositories/**`, `seeds/**`, `tests/db/**`, `tests/migrations/**` |
   | Metalhead | `observability/**`, `dashboards/**`, `alerts/**`, `slo/**`, `runbooks/alerts/**`, `src/instrumentation/**`, `src/tracing/**`, `src/logging/**`, `src/metrics/**`, `src/telemetry/**`, `tests/observability/**` |
   | Splinter | `docs/api/**`, `docs/onboarding/**`, `docs/guides/**`, `docs/how-to/**`, `docs/architecture/**`, `docs/migration/**`, `docs/glossary.md`, `runbooks/ops/**`, `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md` |
   | Tatsu | `src/security/**`, `src/auth/**`, `middleware/**`, `policies/**`, cited security-relevant migrations, `tests/security/**` |
   | Krang | `.github/workflows/**`, `fly.toml`, `wrangler.toml`, `Dockerfile`, `docker-compose.*`, `infrastructure/**`, `terraform/**`, `migrations/**`, `.env.example` |

6. Write `.claude/state/active-slice.json` with the union allowlist.

## Dispatch sequence

### 1. Author

Spawn the `author_agent` subagent via the Agent tool. Prompt includes:
- The full slice text from `slices/queue.md`.
- A reminder of their Output Format (see the agent's `.md` in `${CLAUDE_PLUGIN_ROOT}/agents/`).
- Instruction to write their State Update block to `project_state.md` (or `infrastructure_state.md` for Krang / Splinter runbook-ops).

Wait for the author to return. Capture their output.

### 2. Karai — structural conformance

Spawn Karai with the author's output. She validates against the Conformance Validation checklist for the author agent. On failure, **halt** and escalate to the user with Karai's escalation report verbatim.

### 3. Bishop — code review

**Skip if** pipeline mode is `lightweight` AND the slice's `review_required` is false. Otherwise, spawn Bishop with the slice's artifacts. Verdict is 🟢 Clean, 🟡 Advisory, 🔴 Block, or ⏭ Skip.
- 🔴 Block → halt; escalate with Bishop's Review Report; leave the active-slice state file in place so the author can iterate.
- 🟡 Advisory → log the advisory; continue.
- 🟢 Clean or ⏭ Skip → continue.

### 4. Tiger Claw — adversarial QA

**Skip if** pipeline mode is `lightweight` AND `review_required` is false. Otherwise, spawn Tiger Claw with the slice's artifacts. Verdict is 🟢 Clean, 🟡 Gap, 🔴 Defect, or ⏭ Skip.
- 🔴 Defect → halt; escalate with Tiger Claw's QA Report.
- 🟡 Gap → log; continue.
- 🟢 Clean or ⏭ Skip → continue.

### 5. Karai — state verification + dispatch log

Re-spawn Karai for her Verify-state step: confirm the author's State Update block landed in the correct file. Append a Dispatch Log row with the slice's final status.

### 6. Record & advance

1. Mark the slice `completed` in `slices/queue.md`.
2. Clear `.claude/state/active-slice.json`.
3. Report the slice summary + telemetry (Bishop verdict, Tiger Claw verdict) to the user.
4. Offer: run `/shredder:execute-next` again for the next slice, or `/shredder:status` for an overview.

## On any escalation

- Do **not** clear the active-slice state file.
- Present the escalation report to the user verbatim.
- Wait for user instruction (fix in place, re-slice via Shredder, abandon).

## Reminders

- The PreToolUse guard enforces the union allowlist. If an agent needs to write outside that scope, the guard blocks. If the block is legitimate (scope needs to extend), edit `.claude/state/active-slice.json`; if illegitimate (agent overreach), escalate.
- Every gate's verdict is logged to `project_state.md` / `infrastructure_state.md` via Karai's Dispatch Log row.
- Never run Bishop AFTER Tiger Claw. Bishop always first.
- In lightweight mode, the slice's `review_required` tag is authoritative; do not skip gates based on your own judgment of complexity.

$ARGUMENTS
