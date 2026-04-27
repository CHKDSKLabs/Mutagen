---
description: Pause / resume / status the harness execute-next loop at the next stage boundary.
---

# Pause — stage-boundary pause for the execute-next loop

The user has invoked `/mutagen:pause`. This is a stage-boundary contract: when
the pause sentinel exists, the next iteration of `/mutagen:execute-next` stops
before claiming a new slice and returns `status: "paused"` with the reason.

It does **not** pre-empt work already in flight inside a Rust dispatch. If you
need to kill an active dispatch immediately, use OS signals on the dispatch
process — that is intentionally not a harness-level contract.

## Run

```bash
# Pause at the next stage boundary
bash "${CLAUDE_PLUGIN_ROOT}/scripts/pause.sh" on --reason "investigating L4-World-004"

# Resume
bash "${CLAUDE_PLUGIN_ROOT}/scripts/pause.sh" off

# Inspect current pause state
bash "${CLAUDE_PLUGIN_ROOT}/scripts/pause.sh" status
```

The sentinel lives at `.mutagen/state/pause.json` and is workspace-relative.
`pause.sh status` returns `state: "paused"` or `state: "running"`.

## Behaviour

- `pause on` writes the sentinel with the optional reason and a UTC timestamp.
  The next `/mutagen:execute-next` iteration sees the sentinel before claiming
  a slice and exits with `status: "paused"`.
- `pause off` removes the sentinel. The next `/mutagen:execute-next` run
  proceeds normally.
- `pause status` reports current state without changing anything.

This command does not loop or dispatch. After flipping pause state, the
operator decides whether to invoke `/mutagen:execute-next` again.
