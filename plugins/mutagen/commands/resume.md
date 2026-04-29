---
description: Resume a slice after manual repair without manually re-running structural-check, update-queue, transition-active-slice, and dispatch-stage in sequence.
---

# Resume — restart a paused slice

The user has invoked `/mutagen:resume`. Use this when an in-flight slice was
hand-repaired (for example: an author dispatch wrote a partial artifact, the
operator fixed the file on disk, and the queue should now resume as if the
repair was the canonical author output).

This is the operator counterpart to `/mutagen:execute-next`. It does the
four-step manual sequence in one call:

1. Optionally flip the slice back to `in_progress` and clear the escalation
   reason (`--reset-status`).
2. Transition the active-slice state to the requested stage (default
   `structural-check`).
3. If structural-check, run Karai's structural pass against the repaired
   author output and record the verdict.
4. Transition to `review` and dispatch the review stage.

## Run

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/resume_after_escalation.sh" \
  --slice-id <SLICE_ID> \
  --host claude
```

Add `--reset-status` if the slice is currently `escalated` or `blocked_retry`
and you want to flip it back to `in_progress` as part of the resume.

Add `--stage review` to skip structural-check and go straight to review (use
this only when structural-check already passed before the pause).

## Stop conditions

- The script returns `ok: true` with the dispatch payload — surface that
  payload and stop. Recommend `/mutagen:execute-next` to keep going.
- The script returns `ok: false` with `stage: "structural-check"` — surface
  the structural findings and stop. Do not auto-retry.
- Any non-JSON output or non-zero exit — surface verbatim and stop.

This command does not loop. After it succeeds, the next slice (if any) is
picked up by `/mutagen:execute-next` as usual.
