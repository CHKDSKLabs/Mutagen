# Harness Worklist

## Phase 0: draw the boundary

- [x] Inventory every rule that currently exists only in skill or command prose.
- [x] Separate host-specific behavior from canonical runtime behavior.
- [x] Define the runtime state machine for `elicit`, `slice`, `execute-next`, and `amend-scope`.
- [x] Define the dual-artifact boundary between human slicemap and canonical queue JSON.

## Phase 1: make the engine real

- [x] Define schemas for queue, active slice, evidence bundle, gate verdict, dispatch log, and summary.
- [x] Implement ready-slice selection with `depends_on`.
- [x] Implement stage transitions and retry accounting.
- [x] Implement active-slice stage rotation and counter sync.
- [x] Implement evidence bundle generation as code.
- [x] Implement structural checks as code.
- [x] Implement canonical queue mutation as code.
- [x] Implement canonical successful slice closure as code.
- [x] Implement canonical review verdict recording as code.
- [x] Implement canonical review-decision and retry branching as code.
- [x] Implement canonical bounded-cohort selection with write-set conflict detection.
- [x] Implement canonical targeted slice materialization for isolated workspace execution.
- [x] Implement worktree-backed bounded cohort execution with deterministic serial import.
- [x] Implement canonical stop-condition / notification planning for queue clear, retry exhaustion, and layer milestones.
- [x] Implement canonical structural-fail stop-condition / notification planning.
- [x] Implement canonical scope-violation artifact normalization and notification planning.
- [x] Add queue validation as the first consumer of Shredder output.

## Phase 2: stop trusting vibes

 - [x] Define policy evaluation for write scope, deny rules, and scope amendments.
- [x] Introduce a host adapter contract with capability flags.
- [x] Support hard enforcement when the host allows it.
- [x] Surface explicit degraded mode when the host does not.

## Phase 3: make it portable

- [x] Move Claude and Codex integration behind adapters.
- [x] Remove core orchestration logic from skill and command markdown.
- [x] Make the docs describe the runtime we actually ship.

## Immediate next slices

- [x] Write the harness requirements from observed workflow failures.
- [x] Write the canonical state machine spec.
- [x] Define the adapter interface in executable code.
- [x] Pick the implementation language for the harness runtime.
- [x] Create the first runtime module under `harness/`.
- [x] Define slicemap and queue artifact specs.
- [x] Define the Shredder dual-emission contract.
