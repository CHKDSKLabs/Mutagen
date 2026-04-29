---
name: mutagen-pause
description: Explicit-only skill. Stage-boundary pause / resume / status for the mutagen execute-next loop. Drops a sentinel file at .mutagen/state/pause.json that makes the next iteration of $mutagen-execute-next exit cleanly before claiming a slice. Does NOT pre-empt work already in flight inside a dispatch — that is by design. Invoke only when the user explicitly says $mutagen-pause.
---

# $mutagen-pause — stage-boundary pause for the execute-next loop

When the pause sentinel exists, the next iteration of `$mutagen-execute-next`
stops at the next stage boundary and returns `status: "paused"` with the
operator-supplied reason. It does **not** kill an in-flight Rust dispatch —
if you need to stop work that is already running, kill the dispatch process
directly (use OS signals; that's intentionally not a harness contract).

## Subcommands

```bash
# Pause at the next stage boundary
bash "$MUTAGEN_ROOT/scripts/pause.sh" on --reason "investigating L4-World-004"

# Resume normal operation
bash "$MUTAGEN_ROOT/scripts/pause.sh" off

# Inspect current pause state
bash "$MUTAGEN_ROOT/scripts/pause.sh" status
```

The sentinel lives at `.mutagen/state/pause.json` (workspace-relative) and
records a UTC timestamp plus the optional reason. `pause.sh status` returns
`state: "paused"` or `state: "running"` as JSON.

## Behaviour

- **`pause on`** writes the sentinel. The next `$mutagen-execute-next`
  iteration checks the sentinel before claiming a slice, exits with
  `status: "paused"`, and surfaces the reason in the terminal payload.
- **`pause off`** removes the sentinel. The next `$mutagen-execute-next`
  invocation proceeds normally.
- **`pause status`** reports current state without changing anything.

## What this does not do

- It does **not** pre-empt an active Rust dispatch. If a slice is mid-execution
  inside the harness binary, the harness finishes that work and only checks
  the sentinel on the next loop iteration. To stop in-flight work, kill the
  process.
- It does **not** modify the slice queue, active state, or any verdicts. It
  only short-circuits the loop runner's "claim next slice" decision.
- It does **not** loop or dispatch on its own. After flipping pause state,
  the operator decides whether to invoke `$mutagen-execute-next` again.

## Output protocol

After running the requested subcommand, surface the JSON payload verbatim,
then briefly summarise the new state in one sentence (e.g. "Paused — reason
recorded. The next $mutagen-execute-next iteration will exit at the stage
boundary."). Do not re-interpret the JSON; the caller wants the raw payload.
