---
description: Invoke Traag to evaluate an in-flight scope-manifest amendment request. Traag decides ALLOW or DENY against the active slice's stage, agent, and the global denylist.
---

# Amend-scope — mediated scope amendment via Traag

The user has invoked `/mutagen:amend-scope`. Their `$ARGUMENTS` describes a path (or glob) they want added to the currently-active slice's `allowed_write_globs`, the mutation kind (`create` / `modify` / `delete`), and a reason.

This command is the only audited channel for widening an active manifest. It spawns Traag as a subagent, passes him the slice context and the amendment request, and either applies his ALLOW verdict or surfaces his DENY verbatim.

## Preflight

1. Confirm `.claude/state/active-slice.json` exists. If not, refuse: there is no slice to amend. Tell the user `/mutagen:execute-next` must be in flight.
2. Confirm `slices/queue.json` exists. If not, refuse: no queue, no slice context for Traag to evaluate against.
3. Read both files. Pull the active slice entry from the queue by matching `slice_id`.
4. Sanity-check `$ARGUMENTS`:
   - If empty, refuse: Traag will DENY an empty request anyway; save the round trip.
   - If no reason is evident in the prose, tell the user Traag requires a justification and stop.

## Dispatch

Spawn the Traag subagent via the Agent tool with:

- `subagent_type`: `Traag`.
- A prompt that:
  - States this is a **Mediated Amendment** request (not a hook invocation).
  - Includes the full `.claude/state/active-slice.json` content verbatim.
  - Includes the matching slice entry from `slices/queue.json` verbatim.
  - Includes the user's `$ARGUMENTS` as the amendment request.
  - Asks Traag to apply his Decision Process (including stage fidelity, agent-domain fidelity, global denylist, and slice-citation justification gap) and return either an **ALLOW — amended manifest** block or a **DENY — Violation Report** block per his Output Format.

## After Traag returns

### If ALLOW

1. Parse Traag's amended manifest JSON from his response.
2. Overwrite `.claude/state/active-slice.json` with the amended JSON. Preserve any fields Traag did not touch (e.g. `pipeline_mode`, `review_required`, `max_retries`).
3. Append a record to `.claude/state/amendments.jsonl` (one line per amendment ever granted):
   ```json
   {"ts":"YYYY-MM-DDTHH:MM:SSZ","slice":"<slice_id>","stage":"<stage>","agent":"<active_agent>","added":["<glob>"],"reason":"<user's reason>","justification_gap":false}
   ```
4. Surface Traag's decision block to the user verbatim.
5. Tell the user the amendment is live — the next Write / Edit by the active agent inside the expanded scope will be permitted by the guard.

### If DENY

1. Present Traag's Violation Report to the user verbatim.
2. **Do not** touch `.claude/state/active-slice.json`.
3. Append a record to `.claude/state/amendments.jsonl` marking the denial:
   ```json
   {"ts":"...","slice":"...","stage":"...","agent":"...","requested":["<glob>"],"reason":"...","decision":"deny","class":"<class>"}
   ```
4. Relay Traag's suggested next step (re-slice, wait for a later stage, escalate to the human).

## Reminders

- Traag's decision is final. Do not retry, do not reword, do not argue — if the user wants different paths, they change the request and re-invoke.
- Amendments are **per-slice and per-stage**. When `/mutagen:execute-next` rotates to the next stage, the manifest is rewritten from the per-stage template; amendments do not carry forward.
- Global denylist paths cannot be amended in via this command. If a slice genuinely needs a globally-denied path (infra config from a Bebop slice, etc.), the correct answer is to re-slice and reassign the owning agent.
- Emergency hand-edits to `.claude/state/active-slice.json` still work, but bypass the audit trail and the Decision Process. Prefer this command.

$ARGUMENTS
