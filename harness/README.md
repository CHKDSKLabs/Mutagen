# harness

This directory is the start of turning `mutagen` from a host-shaped workflow bundle into a real harness.

## Why this exists

The current workflow has a few structural problems:

- Most orchestration lives in skill and command prose instead of executable runtime code.
- Core behavior changes by host. Claude can enforce some rules that Codex on Windows can only describe politely and hope for the best.
- Safety, scope, and stage sequencing depend too much on agents behaving themselves.
- Runtime behavior and documentation have already drifted. That situation tends to age like milk.

## Goal

Build a canonical runtime that owns:

- queue selection and stage transitions
- evidence bundle construction
- scope policy and enforcement decisions
- structural checks, retries, and escalation rules
- persisted state, summaries, and telemetry
- host adapters for Claude, Codex, and anything else later

## Non-goals for now

- rewriting the existing plugin in place
- cloning the current prompt stack under a new folder and calling that architecture
- baking host-specific quirks into the core runtime

## First milestone

Ship a minimal harness that can:

1. Read `slices/queue.json`.
2. Pick the next ready slice deterministically.
3. Materialize `.mutagen/state/active-slice.json`.
4. Build the evidence bundle for the selected slice.
5. Run deterministic structural checks.
6. Hand stage execution to a host adapter.
7. Persist verdicts, summaries, and escalation state.

## Rust runtime

The harness now has a Rust crate under this directory.

Plugin scripts prefer a packaged `plugins/mutagen/bin/mutagen-harness`
executable, then this source crate through `cargo run`, then
`mutagen-harness` on `PATH`. Build the plugin-local binary with:

```bash
bash plugins/mutagen/scripts/build_harness_binary.sh --release
```

For a local development deployment, use the dev launcher and doctor wrappers:

```bash
bash plugins/mutagen/scripts/dev_console.sh --workspace-root /path/to/workspace
bash plugins/mutagen/scripts/dashboard_dev.sh --workspace-root /path/to/workspace
bash plugins/mutagen/scripts/doctor_dev.sh --workspace-root /path/to/workspace
```

The deployment runbook lives in [DEPLOY_DEV.md](/mnt/c/Users/spork/dev/agentic_design_workflow/harness/DEPLOY_DEV.md).

Useful commands:

```bash
cargo run --manifest-path harness/Cargo.toml -- project init --name crew-scheduler --stack nextjs-postgres --design-system shadcn --deploy-target cloudflare
cargo run --manifest-path harness/Cargo.toml -- project create --name crew-scheduler --stack vite-express-sqlite --design-system plain-css
cargo run --manifest-path harness/Cargo.toml -- project inspect
cargo run --manifest-path harness/Cargo.toml -- project doctor
cargo run --manifest-path harness/Cargo.toml -- project status
cargo run --manifest-path harness/Cargo.toml -- project intake --prompt "Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime."
cargo run --manifest-path harness/Cargo.toml -- project intake --prompt "Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime." --queue-feature
cargo run --manifest-path harness/Cargo.toml -- project add-feature --title "Add due dates" --description "Tasks should include optional due dates."
cargo run --manifest-path harness/Cargo.toml -- project features
cargo run --manifest-path harness/Cargo.toml -- project plan-feature --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project feature-status --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project slice-feature --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project enqueue-feature --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project feature-flow --title "Add due dates" --description "Tasks should include optional due dates."
cargo run --manifest-path harness/Cargo.toml -- project execute-feature --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project feature-progress --feature-id feature-...
cargo run --manifest-path harness/Cargo.toml -- project dashboard
cargo run --manifest-path harness/Cargo.toml -- project dashboard-serve --port 7788
# dashboard-serve can now open on an empty workspace, create the project capsule/scaffold from the UI, then expose a persistent builder conversation, design bundle workbench, guided build readiness repairs, Claude/Codex host selector, preview/build controls, execution console, slice/debug artifacts, operator actions, queue management, a recent activity feed, and bootstrap health actions
cargo run --manifest-path harness/Cargo.toml -- project blueprints
cargo run --manifest-path harness/Cargo.toml -- project apply-blueprint
cargo run --manifest-path harness/Cargo.toml -- project scaffold
cargo run --manifest-path harness/Cargo.toml -- project repair --scaffold
cargo run --manifest-path harness/Cargo.toml -- project run-command --kind test --dry-run
cargo run --manifest-path harness/Cargo.toml -- project verify-generated
cargo run --manifest-path harness/Cargo.toml -- project preview-plan
cargo run --manifest-path harness/Cargo.toml -- project preview-start
cargo run --manifest-path harness/Cargo.toml -- project preview-status
cargo run --manifest-path harness/Cargo.toml -- project preview-check
cargo run --manifest-path harness/Cargo.toml -- project preview-stop
cargo run --manifest-path harness/Cargo.toml -- host-capabilities --host codex
cargo run --manifest-path harness/Cargo.toml -- host-profile --host codex --workflow-config .claude/workflow.json
cargo run --manifest-path harness/Cargo.toml -- validate-queue --queue slices/queue.json
cargo run --manifest-path harness/Cargo.toml -- prepare-next --queue slices/queue.json --dry-run
cargo run --manifest-path harness/Cargo.toml -- prepare-selected-slice --queue slices/queue.json --slice-id L1-orders-001 --dry-run
cargo run --manifest-path harness/Cargo.toml -- prepare-cohort --queue slices/queue.json --host claude --dry-run
cargo run --manifest-path harness/Cargo.toml -- run-execute-next --workspace-root . --host claude
cargo run --manifest-path harness/Cargo.toml -- run-cohort-once --workspace-root . --host claude
cargo run --manifest-path harness/Cargo.toml -- run-slice-once --workspace-root . --host claude --slice-id L1-orders-001
cargo run --manifest-path harness/Cargo.toml -- dispatch-stage --workspace-root . --host claude --slice-id L1-orders-001
cargo run --manifest-path harness/Cargo.toml -- dispatch-cohort-members --runner-script plugins/mutagen/scripts/run_slice_once.sh --host claude --member-json '{"slice_id":"L1-orders-001","worktree_path":".mutagen/worktrees/run/L1-orders-001","result_path":".mutagen/worktrees/run/L1-orders-001.result","status_path":".mutagen/worktrees/run/L1-orders-001.exit"}'
cargo run --manifest-path harness/Cargo.toml -- apply-cohort-dispatch --member-json '{"slice_id":"L1-orders-001","worktree_path":".mutagen/worktrees/run/L1-orders-001","result_path":".mutagen/worktrees/run/L1-orders-001.result","status_path":".mutagen/worktrees/run/L1-orders-001.exit","outcome":{"status":"ready","slice_id":"L1-orders-001","worktree_path":".mutagen/worktrees/run/L1-orders-001","member_status":"completed","run_output":{"status":"completed"}}}'
cargo run --manifest-path harness/Cargo.toml -- prepare-dispatch --slice-id L1-orders-001
cargo run --manifest-path harness/Cargo.toml -- record-review-verdict --slice-id L1-orders-001
cargo run --manifest-path harness/Cargo.toml -- update-slice --slice-id L1-orders-001 --status in_progress --attempts 1
cargo run --manifest-path harness/Cargo.toml -- transition-active-slice --slice-id L1-orders-001 --stage author --bump-attempts
cargo run --manifest-path harness/Cargo.toml -- review-decision --slice-id L1-orders-001
cargo run --manifest-path harness/Cargo.toml -- scope-violation --violation-report .mutagen/state/scope-violation.json
cargo run --manifest-path harness/Cargo.toml -- amend-scope --requested-glob src/orders/support/** --mutation-kind modify --reason "Need a helper beside the aggregate."
cargo run --manifest-path harness/Cargo.toml -- finalize-slice --slice-id L1-orders-001 --completed-at 2026-04-22T18:00:00Z
```

Runtime artifact contracts are documented in `ARTIFACT_SCHEMAS.md`; JSON
schemas live under `schemas/`.

Operational usage is documented in `RUNBOOK.md`.

`prepare-next` now resolves and validates a slice-scoped evidence bundle before
claiming the next slice. On non-dry runs it writes the bundle to
`.mutagen/state/evidence/<slice_id>.md`.

`prepare-selected-slice` materializes a specific slice by ID instead of picking
the next queue head. It writes `active-slice.json` plus the evidence bundle for
that exact slice, returns a machine-readable `blocked` result when dependencies
or status make the request invalid, and is the runtime seam the future
worktree-backed cohort runner will stand on.

`prepare-cohort` is the canonical bounded-parallel preflight. On hosts whose
execution profile resolves to `bounded_cohort`, it selects the first safe
same-layer sibling set in queue order, reports deferred ready slices with
machine-readable reasons such as `layer_mismatch`, `write_set_conflict`, or
`cohort_limit_reached`, and writes evidence
bundles for the selected cohort on non-dry runs.

The Rust harness now owns the execution runners directly:
`run-execute-next`, `run-cohort-once`, `run-slice-once`, and
`dispatch-stage`. The plugin scripts with matching names are compatibility
shims that resolve the harness binary and pass through to those commands. The
native cohort runner fans selected siblings out into isolated git worktrees,
runs the one-slice pipeline inside each workspace, then imports accepted
outputs back into the main tree in queue order. State updates are emitted as
author-output artifacts and applied back into the main workspace in queue
order, so same-context siblings no longer get serialized just for sharing
`project_state.md` or `infrastructure_state.md`.

`prepare-dispatch` is the canonical stage-prompt builder for `author` and
`review`. It reads the active slice plus queue metadata, writes the prompt
artifact under `.mutagen/state/dispatch/<slice_id>/`, and returns the target
agent, capture path, required artifacts, and scope metadata so the runner can
dispatch without re-assembling the contract in markdown.

`record-review-verdict` is the canonical Stage 3 verdict normalizer. It reads
Tiger Claw's persisted QA report, parses the verdict section, verifies the
latest-report convenience copy agrees, and records `verdicts.bishop: "skip"`
plus the canonical `verdicts.tiger_claw` value in `slices/queue.json`.

`host-profile` is the canonical host abstraction edge. It resolves the
requested workflow config against the selected host and returns the effective
execution profile: serial vs. bounded parallel, hard vs. advisory scope
enforcement, worktree isolation, telemetry collection mode, interrupt support,
and any explicit downgrades.

`update-slice` is the canonical queue mutation path for runtime bookkeeping:
status flips, gate verdicts, retry counters, completion timestamps, and
escalation reasons.

`transition-active-slice` is the canonical active-state rotation path. It
rewrites `active-slice.json` for the current stage and syncs retry counters
back into the queue when the transition changes them.

`review-decision` is the canonical Stage 3 control path after verdict
recording. It reads Tiger Claw's persisted QA report, parses the
machine-readable retry contract, decides continue vs. micro-correction vs.
blocked retry vs. escalation, and persists queue state when the decision
changes it.

`finalize-slice` is the canonical successful-closeout path. It verifies the
state update artifact, applies it to the target context file, records
completion in the queue, writes the slice summary,
appends the dispatch log, and clears `active-slice.json`.

The harness now also emits canonical notification intents for queue-clear,
structural failure, scope violation, retry-budget exhaustion, and
layer-complete milestones. The plugin shell wrappers relay those intents
through the existing `notify.sh` transport.

`scope-violation` is the canonical Traag halt normalizer. It reads the
machine-written violation artifact from `.mutagen/state/scope-violation.json`,
marks the active slice escalated in the queue when possible, and emits the
canonical halt metadata plus notification payload.

`amend-scope` is the canonical policy evaluator for widening the current
stage manifest. It decides ALLOW / DENY from persisted slice state, global
deny rules, stage fidelity, and active-agent domain, then writes the live
manifest plus `.mutagen/state/amendments.jsonl` audit records itself.

## Working rule

If a behavior matters, the harness should enforce it or record it. If the only control is "the prompt said pretty please," that is not a control plane.

See `RUNBOOK.md`, `REQUIREMENTS.md`, `SLICEMAP_SPEC.md`, `QUEUE_SCHEMA.md`, `ARTIFACT_SCHEMAS.md`, `RULE_INVENTORY.md`, `SHREDDER_OUTPUT_SPEC.md`, `ARCHITECTURE.md`, `STATE_MACHINE.md`, and `WORKLIST.md`.
