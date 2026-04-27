---
description: Run the harness-owned mutagen execution loop until the queue clears, stalls, or escalates.
---

# Execute-next

The user has invoked `/mutagen:execute-next`.

The Rust harness owns queue selection, host profile resolution, evidence
bundle materialization, active-slice state, stage transitions, structural
checks, Tiger Claw verdict recording, retry decisions, finalization, cohort
selection, cohort dispatch, deterministic cohort import, notification planning,
and stop-condition reporting.

This command is a host entrypoint. Do not reimplement the pipeline in markdown.

## Run

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/run_execute_next.sh" --host claude
```

Treat the script's JSON payload as authoritative.

## Stop Conditions

- `status: "queue_clear"`: report queue clear and stop.
- `status: "stalled"`: surface the returned blocked dependency payload and stop.
- `status: "escalated"`: surface the returned terminal payload and stop.
- `status: "paused"`: an operator dropped `.mutagen/state/pause.json`. Surface the pause reason (if any) and stop. Resume by clearing the sentinel via `bash "${CLAUDE_PLUGIN_ROOT}/scripts/pause.sh" off` (or `/mutagen:pause off`) and re-running `/mutagen:execute-next`.
- `status: "queue_validation_failed"` with exit code `2`: surface the payload, recommend `/mutagen:slice`, and stop.
- Any non-JSON output or non-zero tooling failure: surface the wrapper error and stop.

If the payload contains `completion_markers`, emit those markers exactly. Do
not replace them with a narrative recap.

## Autopilot Discipline

Successful slice completion is not a permission checkpoint. The runner owns
auto-advance. Do not ask whether to continue between successful slices.

The only valid end states are the stop conditions above or a user interrupt.

## Debugging

Use the one-slice runner only when investigating a failed run:

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/run_slice_once.sh" --host claude
```

Use focused wrappers when inspecting a specific harness boundary:

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/prepare_cohort.sh" --host claude
bash "${CLAUDE_PLUGIN_ROOT}/scripts/dispatch_cohort_members.sh" --help
bash "${CLAUDE_PLUGIN_ROOT}/scripts/apply_cohort_dispatch.sh" --help
bash "${CLAUDE_PLUGIN_ROOT}/scripts/finalize_slice.sh" --help
```

Those wrappers delegate to `cargo run --manifest-path harness/Cargo.toml`.
The runtime result wins over any older prose, including this file if it ever
drifts. Documentation can be wrong; state is less imaginative.
