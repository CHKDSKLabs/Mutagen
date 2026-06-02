---
name: mutagen-resume
description: Explicit invocation only. Resume a hand-repaired in-flight slice in one call (structural-check + update-queue + transition-active-slice + dispatch-stage), treating the on-disk repair as the canonical author output.
---

# $mutagen-resume — restart a paused slice

The operator counterpart to `$mutagen-execute-next`. It does the four-step
manual recovery sequence in one call:

1. Optionally flip the slice back to `in_progress` and clear the escalation
   reason (`--reset-status`).
2. Transition the active-slice state to the requested stage (default
   `structural-check`).
3. If structural-check, run Karai's structural pass against the repaired
   author output and record the verdict.
4. Transition to `review` and dispatch the review stage.

## Run

```bash
bash "$MUTAGEN_ROOT/scripts/resume_after_escalation.sh" \
  --slice-id <SLICE_ID> \
  --host codex
```

Add `--reset-status` if the slice is currently `escalated` or `blocked_retry`
and you want to flip it back to `in_progress` as part of the resume.

Add `--stage review` to skip structural-check and go straight to review (use
this only when structural-check already passed before the pause).

## When to use this

- An author dispatch produced a partial artifact and you fixed it on disk.
- A run was paused mid-flight via `$mutagen-pause`, you investigated and
  resolved the issue, and now want to pick up at the next stage.
- A slice is in `escalated` status because of a structural problem that you
  hand-corrected in the workspace.

## When not to use this

- The slice has not been claimed yet — use `$mutagen-execute-next` instead.
- The author output is fundamentally wrong (not just incomplete). Reset the
  slice to `pending` and let the author re-run.
- Multiple slices are stuck. Resume only handles one slice at a time; chain
  with `$mutagen-execute-next` to drain the queue afterward.

## Stop conditions

- The script returns `ok: true` with the dispatch payload — surface that
  payload and stop. Recommend `$mutagen-execute-next` to keep going.
- The script returns `ok: false` with `stage: "structural-check"` — surface
  the structural findings and stop. Do not auto-retry; the structural
  failure means the repaired artifact is still wrong.
- Any non-JSON output or non-zero exit — surface the wrapper error verbatim
  and stop.

This skill does not loop. After it succeeds, the next slice (if any) is
picked up by `$mutagen-execute-next` as usual.
