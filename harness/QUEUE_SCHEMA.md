# Queue Schema

This document defines the canonical machine-readable queue artifact.

Related runtime artifact contracts live in
[`ARTIFACT_SCHEMAS.md`](ARTIFACT_SCHEMAS.md). JSON schemas live under
[`schemas/`](schemas/).

## Purpose

`queue.json` is the source of truth for execution.

The harness uses `queue.json` for:

- deterministic readiness
- scope-aware scheduling
- evidence construction
- retry and escalation accounting
- summary and telemetry persistence

## Dual-artifact model

`Shredder` should emit both artifacts directly:

- `slicemap.md` for humans
- `queue.json` for the harness

The two artifacts describe the same plan from different angles. The queue is authoritative whenever there is a mismatch.

## Top-level shape

```json
{
  "version": 1,
  "generated_at": "2026-04-22T12:00:00Z",
  "generated_by": "Shredder",
  "pipeline_mode": "full",
  "planning_advisories": [],
  "slices": []
}
```

## Top-level fields

- `version`: schema version
- `generated_at`: UTC timestamp
- `generated_by`: generator identity
- `pipeline_mode`: `full` or `lightweight`
- `planning_advisories`: machine-readable planning notes that affect slicing or execution
- `slices`: canonical execution units

## Planning advisory object

Each advisory should contain:

- `id`
- `severity`
- `summary`
- `decision`
- `user_response_required`
- `references`
- `affects_slices`

Example:

```json
{
  "id": "ISC-012",
  "severity": "high",
  "summary": "Firestore offline cache writes plaintext LevelDB on Android.",
  "decision": "Slice assuming documented exception unless user overrides.",
  "user_response_required": false,
  "references": ["ISC-012", "PRD-8.3", "ADR-006"],
  "affects_slices": ["L1-04", "L2-06"]
}
```

## Slice object

Each slice must contain at least:

- `id`
- `title`
- `status`
- `author_agent`
- `layer`
- `bounded_context`
- `target_loc`
- `objective`
- `context_to_update`
- `depends_on`
- `write_set`
- `implementation_details`
- `verification_steps`
- `traces_to`
- `human_check_needed`

Optional but recommended:

- `phase`
- `review_required`
- `adjacent_scope_allowed`
- `attempts`
- `micro_corrections_used`

Runtime-managed during execution:

- `verdicts`
- `completed_at`
- `escalation_reason`

## Human-check object

```json
{
  "required": true,
  "reason": "Requires Firebase Console project creation credentials.",
  "resolved_at": null
}
```

`resolved_at` is null until the human prerequisite is cleared. Once resolved, the harness may proceed without rediscovering the same obstacle every turn like it has never seen a computer before.

## Runtime Artifact Schemas

The queue is only one runtime artifact. The current schema set is:

- [`schemas/queue.schema.json`](schemas/queue.schema.json)
- [`schemas/active-slice.schema.json`](schemas/active-slice.schema.json)
- [`schemas/gate-verdict.schema.json`](schemas/gate-verdict.schema.json)
- [`schemas/dispatch-log-entry.schema.json`](schemas/dispatch-log-entry.schema.json)
- [`schemas/finalize-result.schema.json`](schemas/finalize-result.schema.json)

The evidence bundle and summary are Markdown artifacts with stable section
contracts documented in [`ARTIFACT_SCHEMAS.md`](ARTIFACT_SCHEMAS.md).

## Verdict object

```json
{
  "karai_structural": "pass",
  "bishop": "skip",
  "tiger_claw": "clean",
  "micro_correction": true,
  "micro_corrections_used": 1
}
```

All fields are optional because the queue is authored before execution begins.
The harness adds them as stages complete.

## Slice contract rules

- `target_loc` is the intended slice size and must be used in structural telemetry.
- `depends_on` is authoritative for readiness.
- `write_set` is authoritative for scheduling and scope policy.
- `implementation_details` must be a structured list, not a prose blob.
- `traces_to` must point to concrete evidence references.
- `human_check_needed.required` is the execution gate, not prose in the slicemap.
- Runtime bookkeeping lives in the same queue artifact. If the harness records a
  verdict or escalation, later mutations must preserve it instead of politely
  dropping it on the floor.

## Scheduling rule

The harness schedules from `queue.json` only.

Parallel cohorts are selected from slices whose:

- `status` is ready
- `depends_on` are all completed
- `write_set` does not conflict
- `human_check_needed.required` is false or already resolved

## Drift rule

If `slicemap.md` and `queue.json` disagree, the harness trusts `queue.json` and reports the mismatch.
