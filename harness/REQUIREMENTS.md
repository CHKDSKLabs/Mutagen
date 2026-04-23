# Harness Requirements

This file turns the workflow failures we observed into hard requirements for the new harness.

## Problem statement

The old workflow had momentum but weak governance. The newer workflow added governance, but mostly as prompt ceremony instead of runtime behavior. That trade made the system slower, chattier, more interrupt-driven, and more expensive.

The harness exists to restore automatic execution while keeping the few controls that actually matter.

## Desired product flow

1. The user gives `Architect` a natural-language project brief.
2. `Architect` produces `PRD`, `ADR`, `DDD`, `ISC`, and `DSD`.
3. The user reviews and approves the design bundle.
4. `Shredder` slices the approved bundle into small executable units.
5. The harness schedules as many ready slices in parallel as safely possible.
6. Execution agents build the project with minimal chatter and minimal human interruption.

## Primary goals

- Restore execution speed and momentum.
- Reduce human interrupts to the cases that actually need judgment.
- Cut token usage back down by removing repeated context loading and prompt ceremony.
- Keep agents focused on producing artifacts instead of narrating their feelings about producing artifacts.
- Preserve enough governance to stop genuinely dangerous or semantically risky behavior.

## Non-goals

- Recreating the entire current workflow inside another folder with different stationery.
- Turning routine execution into a conversational supervision ritual.
- Using reviewers as a substitute for deterministic runtime checks.

## Core principles

### Design is a compile step

`Architect` and `Shredder` produce planning artifacts. They do not become the control plane for execution.

### The harness owns execution truth

Queue state, slice selection, retries, escalation, summaries, and degraded-mode reporting belong to the harness runtime.

### Chat is not the control plane

If a policy matters, the harness must enforce it or persist a violation. Prompt instructions alone do not count as enforcement.

### Quiet by default

Execution mode should emit artifacts and machine-readable state, not running commentary and social lubrication.

## Functional requirements

### R1. Design bundle generation

- The harness must support a single natural-language brief as input to `Architect`.
- `Architect` may ask follow-up questions only when the missing information changes architecture, stack choice, or a binding product constraint.
- `Architect` must produce `PRD`, `ADR`, `DDD`, `ISC`, and `DSD` in reviewable form.

### R2. Explicit design checkpoint

- Execution must not begin until the design bundle is approved by the user.
- The approval boundary must be explicit in persisted state.

### R3. Canonical slice contract

Every slice must include at least:

- `id`
- `title`
- `author_agent`
- `depends_on`
- `bounded_context`
- `target_loc`
- `write_set`
- `objective`
- `implementation_details`
- `verification_steps`
- `traces_to`

Slices missing required fields must be rejected before execution.

### R3a. Dual artifacts

- `Shredder` should emit a human-readable slicemap and a machine-readable queue.
- The harness must execute from machine-readable queue data, not from prose.
- The slicemap exists for review and communication, not as the control plane.

### R4. Small slice target

- The default target size for a slice is about `300 LOC`.
- The harness must preserve the target as metadata and use it in telemetry and structural checks.
- Oversized slices should be treated as a slicing quality issue, not an execution-agent personality quirk.

### R5. Parallel-first execution

- The harness must schedule all ready slices whose dependencies are satisfied and whose write sets do not conflict.
- Parallelism is the default posture, not a special event.
- Serial execution should happen only when dependencies or scope overlap require it.

### R6. Deterministic readiness

- Slice readiness must be computed from persisted queue state and `depends_on`.
- The harness must never execute a blocked slice out of order because the queue feels lonely.

### R7. Mechanical issue autopilot

- Known mechanical failures with bounded fixes must not interrupt the user by default.
- The harness must support micro-corrections for issues such as imports, wiring, stale references, small test fixes, and similar low-risk repair work.
- Micro-corrections must be bounded by policy and tracked separately from full retries.

### R8. Human interrupt policy

The harness may interrupt the user only for:

- missing or conflicting product/design intent
- unresolved evidence or trace references
- exhausted retry budgets
- non-mechanical review failures
- scope violations that cannot be resolved automatically
- host limitations that materially reduce safety or correctness

Everything else should stay on the rails.

### R9. Evidence locality

- Upstream design evidence must be resolved once per slice and written to a slice-scoped evidence bundle.
- Execution and review agents must consume the evidence bundle rather than reloading the full design corpus on every dispatch.

### R10. Quiet execution mode

- Slice execution prompts must be terse and artifact-oriented.
- The harness must discourage conversational summaries, permission-seeking, and between-slice recaps during active execution.
- The normal output of execution mode should be state updates, reports, and completion markers.

### R11. Host capability isolation

- The harness must separate host-specific behavior from canonical runtime behavior.
- Every host adapter must declare its capabilities, including scope enforcement, worktree isolation, tool restriction, and event streaming support.
- If a host lacks a capability, the harness must record a degraded mode explicitly.

### R12. Runtime-owned retries and escalation

- Retry limits, micro-correction limits, and escalation conditions must be runtime policy, not reviewer improvisation.
- The harness must make retry decisions from persisted state.

### R13. Machine-checkable gates first

- Deterministic checks should run before expensive reviewer stages whenever possible.
- Review agents should be reserved for semantic risk, adversarial QA, or non-trivial judgment.

### R14. Resumability

- Every active run must be resumable from disk after interruption or host restart.
- Queue state, active slice state, evidence bundle, reports, and summaries must be enough to continue without rebuilding the whole conversation.

## Behavioral requirements

### B1. No permission theater

The harness must not ask the user whether to continue between successful slices.

### B2. No recap spam

The harness must not generate narrative recaps between slices during execution mode.

### B3. No prompt-shaped laziness

Execution agents should receive bounded tasks with explicit artifacts to produce. The harness should assume that vague prompts create vague behavior because that is, regrettably, how entropy works.

### B4. No silent degradation

If the host cannot enforce write scope, isolate parallel work, or restrict tools, that fact must be surfaced in runtime state and status reporting.

## Token-efficiency requirements

### T1. Reuse stable context

Stable design context must be cached into reusable artifacts instead of re-inlined into every stage prompt.

### T2. Minimize prompt scaffolding

Persona framing and stage instructions must be as short as possible while preserving execution quality.

### T3. Prefer structured artifacts over prose

The harness should persist structured state and short reports rather than long conversational summaries.

### T4. Use deterministic code where possible

Anything that can be done by a script, schema validator, or runtime policy engine should not burn an agent turn.

## Governance requirements

### G1. Scope policy

- Write scope must be represented as runtime data, not buried in prompt text.
- When the host supports enforcement, the harness must enforce it.
- When the host does not support enforcement, the harness must still use scope for scheduling, telemetry, and violation reporting.

### G2. Reviewer role reduction

- Reviewers must not become the primary mechanism for catching routine mechanical misses.
- Reviewer stages should focus on semantic defects, contract gaps, and adversarial checks.

### G3. Documentation truthfulness

- Documentation must describe the runtime that actually ships.
- If a feature is disabled, degraded, or host-specific, the docs must say so plainly.

## Success criteria for the first harness version

The first version is successful if it can:

1. Accept an approved design bundle.
2. Load a canonical queue.
3. Select the next ready non-conflicting slice or cohort.
4. Build a slice-scoped evidence bundle.
5. Execute bounded author stages through a host adapter.
6. Apply deterministic structural checks.
7. Auto-handle bounded mechanical failures without human interruption.
8. Persist queue state, active state, verdicts, and summaries.
9. Resume after interruption.
10. Report any degraded host capabilities explicitly.

## Things we are deliberately rejecting

- endless conversational check-ins during build mode
- reviewers for every papercut
- repeated full-document cold loads
- soft rules presented as hard guarantees
- orchestration logic that only exists inside long Markdown prompts

## Short version

The new harness should feel like this:

- design once
- review once
- slice small
- run wide
- interrupt rarely
- talk less
- build more

If we drift from that, the harness is back on its nonsense.
