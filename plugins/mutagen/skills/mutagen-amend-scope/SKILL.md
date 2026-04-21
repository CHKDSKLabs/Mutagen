---
name: mutagen-amend-scope
description: Explicit-only skill. Invoke Traag to evaluate an in-flight scope-manifest amendment request. Traag decides ALLOW or DENY against the active slice's stage, agent, and the global denylist. Invoke only when the user explicitly says $mutagen-amend-scope and provides the path (or glob) to add, mutation kind, and a reason.
---

# $mutagen-amend-scope — mediated scope amendment via Traag

The user's request describes a path (or glob) they want added to the
currently-active slice's `allowed_write_globs`, the mutation kind (`create`
/ `modify` / `delete`), and a reason.

In the Claude Code plugin this skill mediates hook enforcement. In the
Codex port there is no hook, but Traag's evaluation is still useful: it
keeps an audit trail, catches global-denylist violations, and prevents
quiet scope creep. Run it as the audited channel for widening an active
manifest.

## Preflight

1. Confirm `.mutagen/state/active-slice.json` exists. If not, refuse — no
   slice to amend. Tell the user `$mutagen-execute-next` must be in flight.
2. Confirm `slices/queue.json` exists. If not, refuse.
3. Read both files. Pull the active slice entry from the queue by matching
   `slice_id`.
4. Sanity-check the user's request prose:
   - If empty, refuse.
   - If no reason evident, tell the user Traag requires a justification
     and stop.

## Dispatch

```bash
bash "$MUTAGEN_ROOT/bin/agent.sh" Traag "$(cat <<'PROMPT'
This is a **Mediated Amendment** request, not a hook invocation.

Active slice state (.mutagen/state/active-slice.json):
<paste verbatim>

Matching slice entry from slices/queue.json:
<paste verbatim>

Amendment request (from user):
<user arguments verbatim>

Apply your Decision Process (stage fidelity, agent-domain fidelity, global
denylist, slice-citation justification gap). Return either:
  - an **ALLOW — amended manifest** block, or
  - a **DENY — Violation Report** block
per your Output Format.
PROMPT
)"
```

## After Traag returns

### If ALLOW

1. Parse Traag's amended manifest JSON from his response.
2. Overwrite `.mutagen/state/active-slice.json` with the amended JSON.
   Preserve any fields Traag did not touch.
3. Append to `.mutagen/state/amendments.jsonl`:
   ```json
   {"ts":"YYYY-MM-DDTHH:MM:SSZ","slice":"<slice_id>","stage":"<stage>","agent":"<active_agent>","added":["<glob>"],"reason":"<user's reason>","justification_gap":false}
   ```
4. Surface Traag's decision block to the user verbatim.
5. **Reminder:** Codex does not hard-enforce the manifest. The active agent
   is expected to self-honour the expanded scope; Traag's record is the
   audit trail.

### If DENY

1. Present Traag's Violation Report to the user verbatim.
2. **Do not** touch `.mutagen/state/active-slice.json`.
3. Append to `.mutagen/state/amendments.jsonl`:
   ```json
   {"ts":"...","slice":"...","stage":"...","agent":"...","requested":["<glob>"],"reason":"...","decision":"deny","class":"<class>"}
   ```
4. Relay Traag's suggested next step.

## Reminders

- Traag's decision is final. Do not retry, do not reword, do not argue.
- Amendments are per-slice and per-stage. When `$mutagen-execute-next`
  rotates to the next stage, the manifest is rewritten from the per-stage
  template; amendments do not carry forward.
- Global denylist paths cannot be amended in via this skill. If a slice
  genuinely needs a globally-denied path, re-slice and reassign.
- Emergency hand-edits to `.mutagen/state/active-slice.json` bypass the
  audit trail. Prefer this skill.
