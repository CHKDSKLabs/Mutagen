# Harness State Machine

This document defines the canonical runtime states for the harness. Host adapters may present different UX, but they do not get to invent different state semantics.

## Top-level run states

### `idle`

No active harness-managed operation is in flight.

### `elicit_active`

April is authoring or revising the upstream bundle.

### `bundle_ready`

The upstream bundle is complete enough to hand to the slicer.

### `slice_active`

Shredder is generating or revising the queue.

### `queue_ready`

The queue exists and no slice is currently in progress.

### `execute_active`

One slice is currently in flight under harness control.

### `halted`

The harness stopped on an escalation, structural failure, scope denial, or user interrupt. State must be resumable from disk.

### `completed`

The queue is empty and no active slice remains.

## Slice states

These values belong in `slices/queue.json` and are the canonical slice lifecycle states:

- `pending`
- `in_progress`
- `blocked_retry`
- `completed`
- `escalated`
- `refused`

## Stage states

These values belong in `active-slice.json` while a slice is in flight:

- `author`
- `structural_check`
- `review`
- `state_record`

## Top-level transitions

| From | Event | To | Notes |
|------|-------|----|-------|
| `idle` | `elicit.start` | `elicit_active` | Begins upstream document authoring or revision. |
| `elicit_active` | `bundle.ready` | `bundle_ready` | Upstream bundle is ready for slicing. |
| `elicit_active` | `elicit.stop` | `idle` | User stops before handoff. |
| `bundle_ready` | `slice.start` | `slice_active` | Shredder generates or revises the queue. |
| `slice_active` | `queue.generated` | `queue_ready` | Canonical queue persisted. |
| `queue_ready` | `execute.start` | `execute_active` | Harness selects the next ready slice. |
| `execute_active` | `slice.completed` | `queue_ready` | A slice completed and more work may remain. |
| `execute_active` | `queue.clear` | `completed` | No pending or blocked-retry slices remain. |
| `execute_active` | `run.halted` | `halted` | Escalation, structural failure, scope denial, or interrupt. |
| `halted` | `resume.execute` | `execute_active` | Resume from persisted state. |
| `halted` | `resume.elicit` | `elicit_active` | Resume upstream authoring. |
| `completed` | `elicit.start` | `elicit_active` | New revision cycle begins. |

## Slice transitions

| From | Event | To | Notes |
|------|-------|----|-------|
| `pending` | `slice.selected` | `in_progress` | Harness creates `active-slice.json`. |
| `blocked_retry` | `slice.selected` | `in_progress` | Retry path resumes the same slice. |
| `in_progress` | `review.blocked_retry` | `blocked_retry` | Review returned a retryable defect. |
| `in_progress` | `slice.completed` | `completed` | All required stages passed and state was recorded. |
| `in_progress` | `slice.escalated` | `escalated` | Human input required before any further progress. |
| `in_progress` | `slice.refused` | `refused` | Slice cannot be executed as written. |

## Stage transitions inside `execute_active`

1. Harness selects the next ready slice and writes `active-slice.json`.
2. Stage enters `author`.
3. Author returns artifacts or execution fails.
4. Stage enters `structural_check`.
5. Structural check either:
   - passes and advances to `review`
   - fails and halts the run
6. Review either:
   - passes and advances to `state_record`
   - requests retry and returns the slice to `blocked_retry`
   - escalates and halts the run
7. State record persists summary, verdicts, and telemetry.
8. Harness clears `active-slice.json`.
9. Harness either selects the next ready slice or marks the run `completed`.

## Stop conditions

The harness must stop and persist `halted` when any of these events occur:

- structural check failure
- retry budget exhaustion
- non-recoverable scope denial
- missing required artifact
- unresolved evidence citation
- user interrupt

## Invariants

- At most one active slice exists in a serial run.
- `active-slice.json` and `slices/queue.json` must agree on the selected slice ID while a slice is in progress.
- A slice cannot enter `in_progress` unless all `depends_on` slices are `completed`.
- `completed`, `escalated`, `refused`, and `pending` slices may not have an active stage.
- Every transition that changes queue state must be persisted before the next stage dispatch.
- Every degraded host capability must be recorded in runtime state.

## Immediate implication for the harness code

The first executable runtime should treat this file as the contract and make illegal transitions impossible without an explicit error. If the current workflow can do something this state machine cannot represent, that is a design bug we should surface instead of hiding under prompt confetti.
