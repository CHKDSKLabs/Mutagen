---
name: mutagen-execute-next
description: Explicit-only skill. Run the harness-owned mutagen execution loop until the queue clears, stalls, or escalates. Invoke only when the user explicitly says $mutagen-execute-next.
---

# $mutagen-execute-next

Run the mutagen execution loop through the harness. The Rust runtime owns
selection, evidence, stage state, dispatch prompt preparation, structural
checks, review verdicts, retry branching, finalization, cohort execution,
deterministic imports, notifications, and stop conditions.

This skill is a host adapter instruction, not the pipeline implementation.

## Run

```bash
bash "$MUTAGEN_ROOT/scripts/run_execute_next.sh" --host codex
```

Treat the JSON payload as authoritative.

## Stop Conditions

- `status: "queue_clear"`: report queue clear and stop.
- `status: "stalled"`: surface the blocked dependency payload and stop.
- `status: "escalated"`: surface the terminal payload and stop.
- `status: "queue_validation_failed"` with exit code `2`: surface the payload, recommend `$mutagen-slice`, and stop.
- Non-JSON output or wrapper failure: surface the wrapper error and stop.

If `completion_markers` are present, emit them exactly. Do not replace them
with a file list, recap, or "what landed" section.

## Autopilot Discipline

Do not ask whether to continue between successful slices. The runner owns
auto-advance until queue clear, queue stall, escalation, validation failure,
tooling failure, or user interrupt.

## Debugging

Use these only to inspect a boundary after a failure:

```bash
bash "$MUTAGEN_ROOT/scripts/run_slice_once.sh" --host codex
bash "$MUTAGEN_ROOT/scripts/prepare_cohort.sh" --host codex
bash "$MUTAGEN_ROOT/scripts/dispatch_cohort_members.sh" --help
bash "$MUTAGEN_ROOT/scripts/apply_cohort_dispatch.sh" --help
bash "$MUTAGEN_ROOT/scripts/finalize_slice.sh" --help
```

The harness result wins over any prose. If prose and JSON disagree, believe
the JSON; it at least had the decency to be produced by code.
