# Harness Runbook

This runbook explains how to operate the mutagen harness from a clean design
bundle through queue execution, parallel cohorts, halts, recovery, and artifact
inspection.

The harness is the source of execution truth. Plugin commands and Codex skills
are host entrypoints; they should call the runtime and trust its JSON output.

## Quick Start

From the repository root:

```bash
bash plugins/mutagen/scripts/validate_queue.sh slices/queue.json > .mutagen/state/queue-validation.json
bash plugins/mutagen/scripts/run_execute_next.sh --host codex
```

For Claude Code:

```bash
bash plugins/mutagen/scripts/validate_queue.sh slices/queue.json > .mutagen/state/queue-validation.json
bash plugins/mutagen/scripts/run_execute_next.sh --host claude
```

The runner continues until one of these terminal statuses appears:

- `queue_clear`
- `stalled`
- `escalated`
- `queue_validation_failed`

## Prerequisites

Required tools:

- `bash`
- `jq`
- `git`

Required for source checkouts only:

- `cargo` and `rustc`

Installed plugins can run without Rust when `plugins/mutagen/bin/` contains a
packaged `mutagen-harness` binary.

Host-specific agent launchers:

- Codex host dispatches stages through `plugins/mutagen/bin/agent.*` and runs `codex exec --profile <persona>`. Override the binary with `CODEX_BIN`.
- Claude host dispatches stages through the same launcher boundary and runs `claude --print`. Override the binary with `CLAUDE_BIN`.
- Custom launch behavior can be supplied with `MUTAGEN_AGENT_LAUNCHER`; it receives `<host> <persona> <profile> <framing>`.

Useful checks:

```bash
jq --version
git --version
bash plugins/mutagen/scripts/harness_runtime.sh host-capabilities --host codex
```

For source checkouts, also check:

```bash
cargo --version
cargo test --manifest-path harness/Cargo.toml schema_files
```

## Harness Installation

Plugin scripts resolve the harness in this order:

1. `MUTAGEN_HARNESS_BIN`, when set to an executable.
2. `plugins/mutagen/bin/mutagen-harness` or `mutagen-harness.exe`, when packaged.
3. `cargo run --manifest-path harness/Cargo.toml` as a source-checkout fallback.
4. `mutagen-harness` on `PATH`.

Build a plugin-local binary from a repo checkout:

```bash
bash plugins/mutagen/scripts/build_harness_binary.sh --release
```

The command copies the compiled executable into `plugins/mutagen/bin/`.

Override the binary for testing:

```bash
MUTAGEN_HARNESS_BIN=/absolute/path/to/mutagen-harness \
  bash plugins/mutagen/scripts/harness_runtime.sh host-capabilities --host codex
```

## Important Paths

| Path | Purpose |
| --- | --- |
| `.mutagen/project.json` | Portable project capsule for app-builder metadata and control-plane paths. |
| `slices/queue.json` | Canonical execution queue. |
| `slices/slicemap.md` | Human-readable queue rendering. Not execution truth. |
| `.mutagen/state/queue-validation.json` | Latest queue validation report. |
| `.mutagen/state/active-slice.json` | Current slice state and stage manifest. |
| `.mutagen/state/evidence/<slice_id>.md` | Slice-scoped evidence bundle. |
| `.mutagen/state/dispatch/<slice_id>/` | Harness-built stage prompts. |
| `.mutagen/state/author-output/<slice_id>.md` | Captured author output. |
| `reviews/<slice_id>/tiger-claw.md` | Tiger Claw QA report. |
| `.mutagen/state/tiger-claw-latest.md` | Convenience copy of the latest QA report. |
| `.mutagen/state/dispatch-log.jsonl` | One JSON record per completed slice. |
| `slices/<slice_id>/summary.md` | Durable slice closeout. |
| `.mutagen/worktrees/` | Managed bounded-cohort worktrees. |

## Project Capsule

Greenfield app-builder work starts with a project capsule:

```bash
bash plugins/mutagen/scripts/project.sh init \
  --name crew-scheduler \
  --stack nextjs-postgres \
  --design-system shadcn \
  --deploy-target cloudflare
```

The command creates `.mutagen/project.json`, starter upstream documents,
`slices/queue.json`, `.claude/workflow.json`, `.mutagen/design/`, and durable
state logs. Inspect the capsule before planning or execution:

```bash
bash plugins/mutagen/scripts/project.sh inspect
```

Status `ready` means the capsule and required control-plane artifacts exist.
Status `incomplete` lists the missing paths.

Apply a stack blueprint after initialization:

```bash
bash plugins/mutagen/scripts/project.sh blueprints
bash plugins/mutagen/scripts/project.sh apply-blueprint
```

Blueprints populate the capsule's `commands` block. The preview and test
runners use those commands instead of guessing how a project starts.

Current stack IDs:

- `nextjs-postgres`
- `vite-express-sqlite`
- `fastapi-react`
- `aspnet-blazor`
- `cloudflare-worker`
- `rust-bevy`

Resolve or run a project command:

Create a project in one pass:

```bash
bash plugins/mutagen/scripts/project.sh create --name crew-scheduler --stack vite-express-sqlite --design-system plain-css
```

`create` runs init, blueprint application, and scaffold materialization. It
preflights scaffold collisions and requires `--force` before replacing files.

Check local stack prerequisites:

```bash
bash plugins/mutagen/scripts/project.sh doctor
```

`doctor` reports required executables for the selected stack and includes
version output when a tool is found.

Summarize the project dashboard:

```bash
bash plugins/mutagen/scripts/project.sh status
```

`status` reports capsule readiness, scaffold presence, doctor status, preview
state, and the latest build-log entry.

Repair missing scaffold files:

```bash
bash plugins/mutagen/scripts/project.sh repair --scaffold
```

`repair --scaffold` restores missing generated scaffold files. Existing files
are skipped unless `--force` is provided.

Queue a feature intent without changing app code:

```bash
bash plugins/mutagen/scripts/project.sh add-feature --title "Add due dates" --description "Tasks should include optional due dates."
```

`add-feature` writes `.mutagen/features/<feature-id>/brief.md` and appends the
machine-readable intent to `.mutagen/state/features.jsonl`.

List queued feature intents:

```bash
bash plugins/mutagen/scripts/project.sh features
```

`features` reads `.mutagen/state/features.jsonl` and returns the current
feature backlog.

Plan a queued feature without changing app code:

```bash
bash plugins/mutagen/scripts/project.sh plan-feature --feature-id feature-...
```

`plan-feature` writes `.mutagen/features/<feature-id>/plan.json` with target
paths, verification commands, and implementation steps. Existing plans require
`--force` before they are replaced.

Inspect feature readiness:

```bash
bash plugins/mutagen/scripts/project.sh feature-status --feature-id feature-...
```

`feature-status` reports the intent, brief presence, plan presence, and whether
the feature is ready for slicing or execution.

Slice a planned feature without changing app code:

```bash
bash plugins/mutagen/scripts/project.sh slice-feature --feature-id feature-...
```

`slice-feature` writes `.mutagen/features/<feature-id>/slices.json` from the
feature plan. Existing slice manifests require `--force` before replacement.

Enqueue a sliced feature for harness execution:

```bash
bash plugins/mutagen/scripts/project.sh enqueue-feature --feature-id feature-...
```

`enqueue-feature` imports the feature-local slices into `slices/queue.json` and
adds a small PRD evidence section so `prepare-next` can resolve the feature
citation. Existing imported slices require `--force` before replacement.

Run the full feature intake flow:

```bash
bash plugins/mutagen/scripts/project.sh feature-flow \
  --title "Add due dates" \
  --description "Tasks should include optional due dates."
```

`feature-flow` runs `add-feature`, `plan-feature`, `slice-feature`, and
`enqueue-feature` in one pass. The individual commands remain available for
inspection-first workflows.

Prepare the next queued slice for a feature:

```bash
bash plugins/mutagen/scripts/project.sh execute-feature --feature-id feature-...
```

`execute-feature` selects the next non-completed queue slice for that feature
and prepares it through the normal selected-slice activation path. Pass
`--dry-run` to preview the selected slice without claiming it.

Inspect feature execution progress:

```bash
bash plugins/mutagen/scripts/project.sh feature-progress --feature-id feature-...
```

`feature-progress` summarizes the feature's queue slices by status and reports
the active slice when the feature is currently being executed.

Load a project dashboard snapshot (read-only JSON):

```bash
bash plugins/mutagen/scripts/project.sh dashboard
```

`dashboard` combines project health, preview state, build history, feature
backlog, and active feature progress into one JSON response. The embedded
HTTP dashboard UI (`project dashboard-serve` and the
`scripts/dashboard_dev.sh` / `scripts/dev_console.sh` wrappers) has been
retired. Operator control runs through the CLI surface
(`/mutagen:execute-next`, `/mutagen:status`, `/mutagen:pause`,
`/mutagen:resume`, `/mutagen:amend-scope`).

For workspace-doctor checks during development, use:

```bash
bash plugins/mutagen/scripts/doctor_dev.sh --workspace-root /path/to/workspace
```

The deployment runbook lives in [DEPLOY_DEV.md](DEPLOY_DEV.md).

Resolve or run a project command:

```bash
bash plugins/mutagen/scripts/project.sh run-command --kind test --dry-run
bash plugins/mutagen/scripts/project.sh run-command --kind test
```

Supported command kinds are `setup`, `dev`, `test`, and `build`. Non-dry runs
append a JSONL record to `.mutagen/state/build-log.jsonl`.

Materialize the selected stack:

```bash
bash plugins/mutagen/scripts/project.sh scaffold
```

`scaffold` currently writes runnable starters for `vite-express-sqlite` and
`rust-bevy`. It refuses to overwrite existing files unless `--force` is
provided.

Verify the generated project:

```bash
bash plugins/mutagen/scripts/project.sh verify-generated
```

`verify-generated` runs inspect, doctor, setup, test, build, preview-start,
preview-check, and preview-stop. It returns a single JSON result with each step
and stops at the first failed prerequisite.

Inspect the preview target:

```bash
bash plugins/mutagen/scripts/project.sh preview-plan
```

The preview plan returns the dev command, target URL, and readiness timeout.
Web stacks use localhost URLs. Native stacks such as `rust-bevy` use a
non-HTTP target like `native://bevy` until a richer preview adapter exists.

Manage the preview process:

```bash
bash plugins/mutagen/scripts/project.sh preview-start
bash plugins/mutagen/scripts/project.sh preview-status
bash plugins/mutagen/scripts/project.sh preview-check
bash plugins/mutagen/scripts/project.sh preview-stop
```

Preview state is written to `.mutagen/state/preview.json`. Process output is
captured in `.mutagen/state/preview-output.log`.

`preview-check` reports mode, process state, and target readiness. Web previews
use a socket reachability check. Native previews require the managed process to
be running.

## Runtime Artifacts

Schema and artifact contracts live here:

- `harness/ARTIFACT_SCHEMAS.md`
- `harness/RULE_INVENTORY.md`
- `harness/schemas/*.schema.json`

Validate schema files parse:

```bash
cargo test --manifest-path harness/Cargo.toml schema_files
```

## Host Profiles

Inspect raw host capabilities:

```bash
cargo run --manifest-path harness/Cargo.toml -- host-capabilities --host codex
cargo run --manifest-path harness/Cargo.toml -- host-capabilities --host claude
```

Resolve the effective execution profile:

```bash
cargo run --manifest-path harness/Cargo.toml -- host-profile --host codex --workflow-config .claude/workflow.json
cargo run --manifest-path harness/Cargo.toml -- host-profile --host claude --workflow-config .claude/workflow.json
```

Interpretation:

- `parallel_dispatch: "serial_only"` means the runner executes one slice at a time.
- `parallel_dispatch: "bounded_cohort"` means same-layer siblings may run in isolated worktrees.
- `scope_enforcement: "hard"` means the host can block writes before they land.
- `scope_enforcement: "advisory"` means the harness records scope and prompts agents to honor it, but the host cannot block writes.
- `downgrades` are not warnings to ignore. They are the harness saying the floor is lower than requested.

## Queue Validation

Validate the queue before execution:

```bash
bash plugins/mutagen/scripts/validate_queue.sh slices/queue.json > .mutagen/state/queue-validation.json
```

Exit codes:

- `0`: queue valid. Warnings may exist.
- `1`: validator unavailable or runtime failure.
- `2`: queue parsed but failed validation.

Inspect the report:

```bash
jq . .mutagen/state/queue-validation.json
```

Common validation failures:

- unknown dependency
- duplicate slice ID
- missing required slice fields
- unresolved human check
- trace references missing from source documents

If validation fails, fix the queue source or rerun slicing. Do not hand-edit
runtime fields unless you are deliberately repairing a broken run.

## Full Queue Execution

Default Codex run:

```bash
bash plugins/mutagen/scripts/run_execute_next.sh --host codex
```

Claude run:

```bash
bash plugins/mutagen/scripts/run_execute_next.sh --host claude
```

Explicit paths:

```bash
bash plugins/mutagen/scripts/run_execute_next.sh \
  --workspace-root "$PWD" \
  --queue "$PWD/slices/queue.json" \
  --queue-validation "$PWD/.mutagen/state/queue-validation.json" \
  --workflow-config "$PWD/.claude/workflow.json" \
  --active-state "$PWD/.mutagen/state/active-slice.json" \
  --author-output-dir "$PWD/.mutagen/state/author-output" \
  --dispatch-root "$PWD/.mutagen/state/dispatch" \
  --dispatch-log "$PWD/.mutagen/state/dispatch-log.jsonl" \
  --summary-root "$PWD/slices" \
  --slicemap "$PWD/slices/slicemap.md" \
  --legacy "$PWD/slices/queue.md" \
  --host codex
```

The full runner repeatedly calls `run_cohort_once.sh`, accumulates completed
slices, and stops only on a terminal condition.

## One-Cohort Execution

Run exactly one cohort or one serial slice:

```bash
bash plugins/mutagen/scripts/run_cohort_once.sh --host codex
```

Use this when debugging selection, worktree creation, cohort import, or a
single execution pass. The output shape includes:

- `mode`: `serial_only` or `bounded_cohort`
- `status`: `completed`, `queue_clear`, `stalled`, `escalated`, or `queue_validation_failed`
- `completed_slices`
- `completion_markers`
- `terminal` for halt details

## One-Slice Execution

Run one slice through the stage loop:

```bash
bash plugins/mutagen/scripts/run_slice_once.sh --host codex
```

Run a specific slice:

```bash
bash plugins/mutagen/scripts/run_slice_once.sh --host codex --slice-id L1-orders-001
```

Use this for focused debugging after `run_execute_next.sh` has halted.

## Boundary Commands

These commands are useful when you need to inspect or reproduce one runtime
boundary without running the whole loop.

### Select the Next Slice

```bash
cargo run --manifest-path harness/Cargo.toml -- prepare-next --queue slices/queue.json --host codex --dry-run
bash plugins/mutagen/scripts/prepare_next.sh --host codex
```

Non-dry runs claim the slice, write `active-slice.json`, and create the
evidence bundle.

### Select a Specific Slice

```bash
cargo run --manifest-path harness/Cargo.toml -- prepare-selected-slice --queue slices/queue.json --slice-id L1-orders-001 --host codex --dry-run
bash plugins/mutagen/scripts/prepare_selected_slice.sh --slice-id L1-orders-001 --host codex
```

Use this when reproducing a failed slice or debugging dependency blocking.

### Select a Cohort

```bash
cargo run --manifest-path harness/Cargo.toml -- prepare-cohort --queue slices/queue.json --host claude --dry-run
bash plugins/mutagen/scripts/prepare_cohort.sh --host claude
```

The cohort selector considers readiness, layer, write-set conflicts, and host
parallel capability.

### Materialize Worktrees

```bash
bash plugins/mutagen/scripts/materialize_cohort_worktrees.sh \
  --workspace-root "$PWD" \
  --slice-id L1-orders-001 \
  --slice-id L2-orders-002
```

Worktrees are created under `.mutagen/worktrees/`.

### Dispatch Cohort Members

Usually `run_cohort_once.sh` handles this. For debugging:

```bash
bash plugins/mutagen/scripts/dispatch_cohort_members.sh \
  --workspace-root "$PWD" \
  --runner-script "$PWD/plugins/mutagen/scripts/run_slice_once.sh" \
  --host claude \
  --member-json '{"slice_id":"L1-orders-001","worktree_path":".mutagen/worktrees/run/L1-orders-001","result_path":".mutagen/worktrees/run/L1-orders-001.result","status_path":".mutagen/worktrees/run/L1-orders-001.exit"}'
```

### Apply Cohort Dispatch

Usually `run_cohort_once.sh` handles this. For debugging:

```bash
bash plugins/mutagen/scripts/apply_cohort_dispatch.sh \
  --workspace-root "$PWD" \
  --queue "$PWD/slices/queue.json" \
  --dispatch-log "$PWD/.mutagen/state/dispatch-log.jsonl" \
  --member-json '<member-json-from-dispatch-cohort-members>'
```

This reconciles worktree output, imports allowed changes, applies state
updates, syncs queue state, and stops on merge conflict.

### Prepare Stage Dispatch

```bash
cargo run --manifest-path harness/Cargo.toml -- prepare-dispatch --slice-id L1-orders-001
bash plugins/mutagen/scripts/dispatch_stage.sh --slice-id L1-orders-001
```

The runtime writes prompt artifacts under `.mutagen/state/dispatch/<slice_id>/`
and returns the target agent plus capture paths.

### Structural Check

```bash
cargo run --manifest-path harness/Cargo.toml -- structural-check L1-orders-001
bash plugins/mutagen/scripts/karai_structural_check.sh L1-orders-001
```

Failures halt the slice and produce `structural_failure` metadata.

### Record Review Verdict

```bash
cargo run --manifest-path harness/Cargo.toml -- record-review-verdict --slice-id L1-orders-001
bash plugins/mutagen/scripts/record_review_verdict.sh --slice-id L1-orders-001
```

This parses Tiger Claw output, verifies the latest copy, and records queue
verdict fields.

### Decide Review Branch

```bash
cargo run --manifest-path harness/Cargo.toml -- review-decision --slice-id L1-orders-001
bash plugins/mutagen/scripts/review_decision.sh --slice-id L1-orders-001
```

Possible actions:

- `continue`
- `micro_correction`
- `retry`
- `escalated`

### Finalize a Slice

```bash
completed_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
bash plugins/mutagen/scripts/finalize_slice.sh --slice-id L1-orders-001 --completed-at "$completed_at"
```

Finalization:

- verifies and applies the State Update block
- marks the queue slice completed
- writes `slices/<slice_id>/summary.md`
- appends `.mutagen/state/dispatch-log.jsonl`
- clears `.mutagen/state/active-slice.json`
- emits layer-complete notification intents when applicable

## Scope Amendments

Request a live scope amendment:

```bash
bash plugins/mutagen/scripts/amend_scope.sh \
  --requested-glob 'src/orders/support/**' \
  --mutation-kind modify \
  --reason 'Need a helper beside the aggregate.'
```

The harness decides `allow` or `deny` from:

- active stage
- active agent
- requested glob
- mutation kind
- global deny rules

Audit records are appended to `.mutagen/state/amendments.jsonl`.

## Scope Violation Halt

Normalize a Traag scope violation:

```bash
bash plugins/mutagen/scripts/scope_violation.sh \
  --violation-report .mutagen/state/scope-violation.json
```

The harness enriches the violation, marks the current slice escalated when
possible, and emits notification intent metadata.

## Status and Telemetry

High-level workflow status:

```bash
bash plugins/mutagen/scripts/status.sh
```

Heartbeat for an active slice:

```bash
bash plugins/mutagen/scripts/heartbeat.sh 300
```

LOC telemetry for a slice:

```bash
bash plugins/mutagen/scripts/slice_loc.sh L1-orders-001
```

## Reading Terminal Results

### Queue Clear

`status: "queue_clear"` means there are no ready or retryable slices left.

Check:

```bash
jq '.completed_count, .completion_markers' run-output.json
```

### Stalled

`status: "stalled"` means there are pending slices, but their dependencies or
human checks block readiness.

Check:

```bash
jq '.terminal.blocked // .terminal' run-output.json
```

### Escalated

`status: "escalated"` means execution halted on a real gate.

Common escalation sources:

- structural failure
- retry budget exhausted
- scope violation
- cohort merge conflict
- member tooling failure

Check:

```bash
jq '.terminal // .' run-output.json
```

### Queue Validation Failed

`status: "queue_validation_failed"` means the runner refused to execute because
the queue validation report is missing, stale, orphaned, or failed.

Re-run:

```bash
bash plugins/mutagen/scripts/validate_queue.sh slices/queue.json > .mutagen/state/queue-validation.json
```

If it still fails, fix the queue source or rerun slicing.

## Recovery Procedures

### Active Slice Is Left Behind

Inspect it first:

```bash
jq . .mutagen/state/active-slice.json
```

If the slice is truly completed in `slices/queue.json` and the active state is
stale, remove it:

```bash
rm .mutagen/state/active-slice.json
```

Do not remove active state for an in-progress or escalated slice unless you are
deliberately abandoning that run.

### Queue and Markdown Rendering Diverge

The queue wins. Regenerate markdown renderings:

```bash
bash plugins/mutagen/scripts/render_queue.sh slices/queue.json slices/slicemap.md slices/queue.md
```

### Managed Worktrees Remain

Clean up only harness-managed worktrees:

```bash
bash plugins/mutagen/scripts/cleanup_cohort_worktrees.sh \
  --workspace-root "$PWD" \
  --worktree-root "$PWD/.mutagen/worktrees/<run-id>"
```

The cleanup command rejects roots outside the managed prefix. It has trust
issues. Good.

### Retry Budget Exhausted

Inspect:

```bash
jq '.slices[] | select(.status == "escalated")' slices/queue.json
```

Then read:

- `reviews/<slice_id>/tiger-claw.md`
- `.mutagen/state/author-output/<slice_id>.md`
- `slices/<slice_id>/summary.md` if it exists

Decide whether to amend scope, reslice, or manually patch and update queue
state through `update_queue_slice.sh`.

### Cohort Merge Conflict

The apply phase imports completed earlier siblings and halts at the conflict.

Inspect terminal payload:

```bash
jq '.conflicting_slice_id, .conflicting_path, .completed_slices' run-output.json
```

Resolve by either:

- reslicing conflicting siblings so their write sets do not overlap
- manually applying one side, then updating the queue with the harness updater
- rerunning after clearing managed worktrees and stale active state

## Manual Queue Mutation

Prefer runtime wrappers over hand-editing JSON:

```bash
bash plugins/mutagen/scripts/update_queue_slice.sh \
  --slice-id L1-orders-001 \
  --status blocked_retry \
  --attempts 1
```

Direct Rust entrypoint:

```bash
cargo run --manifest-path harness/Cargo.toml -- update-slice \
  --slice-id L1-orders-001 \
  --status blocked_retry \
  --attempts 1
```

After mutation, render the queue:

```bash
bash plugins/mutagen/scripts/render_queue.sh slices/queue.json slices/slicemap.md slices/queue.md
```

## Notifications

The harness emits notification intents; shell wrappers relay them through
`notify.sh`.

Supported events include:

- queue clear
- structural failure
- scope violation
- retry budget exhaustion
- layer complete

Transport setup is host/user-specific. The runtime payload is still emitted
even when the transport is not configured.

## Testing the Harness

Fast-ish checks:

```bash
cargo fmt --manifest-path harness/Cargo.toml -- --check
cargo test --manifest-path harness/Cargo.toml schema_files
cargo test --manifest-path harness/Cargo.toml --test prepare_next
cargo test --manifest-path harness/Cargo.toml --test prepare_cohort
```

Full suite:

```bash
cargo test --manifest-path harness/Cargo.toml
```

The full suite includes bounded parallel smoke tests and can take a couple of
minutes.

## Operational Rules

- `slices/queue.json` is execution truth.
- `active-slice.json` is the current live stage contract.
- Agents write outputs; the harness applies durable state.
- Do not ask the user whether to continue between successful slices.
- Do not dispatch blocked slices because the queue looks lonely.
- Do not widen scope silently.
- Do not carry transcripts forward when `slices/<slice_id>/summary.md` exists.
- If shell prose and harness JSON disagree, believe the JSON.

## Common Failure Matrix

| Symptom | Likely Cause | First Command |
| --- | --- | --- |
| `queue_validation_failed` | Missing/stale/failed validation report | `bash plugins/mutagen/scripts/validate_queue.sh slices/queue.json` |
| `active_slice_present` | Stale or live `.mutagen/state/active-slice.json` | `jq . .mutagen/state/active-slice.json` |
| `structural_failure` | Missing author sections, trace drift, State Update issue, LOC overrun | `jq '.terminal.structural // .terminal' run-output.json` |
| `retry_budget_exhausted` | Tiger Claw defect survived retries | `cat reviews/<slice_id>/tiger-claw.md` |
| `merge_conflict` | Cohort siblings imported the same path | `jq '.conflicting_path' run-output.json` |
| `worktree_unavailable` | Host/profile requested cohort mode outside a git repo | `git rev-parse --is-inside-work-tree` |
| `validator_unavailable` | Missing `cargo`, `jq`, or harness manifest | `cargo --version && jq --version` |

## Release Checklist

Before treating harness changes as ready:

```bash
cargo fmt --manifest-path harness/Cargo.toml -- --check
cargo test --manifest-path harness/Cargo.toml
git diff --check -- harness plugins/mutagen/scripts plugins/mutagen/commands plugins/mutagen/skills
```

If full `git diff --check` reports unrelated line-ending churn, scope the check
to the files touched by the harness slice and record the exception in the
handoff. Pretending the noise is signal is how hours go missing.
