---
description: Invoke Traag to evaluate an in-flight scope-manifest amendment request. Traag decides ALLOW or DENY against the active slice's stage, agent, and the global denylist.
---

# Amend-scope — mediated scope amendment via Traag

The user has invoked `/mutagen:amend-scope`. Their `$ARGUMENTS` describes a path (or glob) they want added to the currently-active slice's `allowed_write_globs`, the mutation kind (`create` / `modify` / `delete`), and a reason.

This command is the only audited channel for widening an active manifest. The harness now owns the actual ALLOW / DENY decision. Traag remains the policy voice in the docs, but the canonical path is a deterministic runtime call that evaluates stage fidelity, agent domain, global deny rules, and justification-gap telemetry without burning a subagent turn.

## Preflight

1. Confirm `.mutagen/state/active-slice.json` exists. If not, refuse: there is no slice to amend. Tell the user `/mutagen:execute-next` must be in flight.
2. Confirm `slices/queue.json` exists. If not, refuse: no queue, no slice context for Traag to evaluate against.
3. Read both files. Pull the active slice entry from the queue by matching `slice_id`.
4. Sanity-check `$ARGUMENTS`:
   - If empty, refuse.
   - Extract one or more requested paths / globs, the mutation kind (`create` / `modify` / `delete`), and a reason.
   - If no reason is evident in the prose, tell the user the amendment requires a justification and stop.
   - If the path list or mutation kind is too ambiguous to extract safely, stop and ask the user for the missing detail instead of free-styling.

## Dispatch

Run the harness wrapper:

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/amend_scope.sh" \
  --requested-glob "<glob-1>" \
  [--requested-glob "<glob-2>" ...] \
  --mutation-kind create|modify|delete \
  --reason "<user reason>"
```

The wrapper delegates to the Rust harness `amend-scope` runtime. Treat its JSON payload as authoritative for:

- `decision` → `allow` or `deny`
- `class` / `matched_rule` when denied
- `rationale`
- `suggested_next_step`
- `justification_gap`
- `added_globs`
- `allowed_write_globs`

Fallback only when the request is too ambiguous to structure: in that case ask the user for the missing path / mutation kind / reason instead of spawning Traag and hoping he parses the prose better than you do.

## After the runtime returns

### If ALLOW

1. The runtime already rewrote `.mutagen/state/active-slice.json` and appended `.mutagen/state/amendments.jsonl`.
2. Report the allow decision concisely:
   - path(s) added
   - whether `justification_gap` was flagged
   - that the amendment is live for the **current stage only**
3. If `added_globs` is empty, say so plainly: the request was allowed but the manifest already contained those globs.

### If DENY

1. The runtime already left `.mutagen/state/active-slice.json` untouched and appended a denial record to `.mutagen/state/amendments.jsonl`.
2. Present the denial using the returned fields:
   - `class`
   - `matched_rule` when present
   - `rationale`
   - `suggested_next_step`

## Reminders

- The harness decision is final for the current request. Do not retry, do not reword, do not argue — if the user wants different paths, they change the request and re-invoke.
- Amendments are **per-slice and per-stage**. When `/mutagen:execute-next` rotates to the next stage, the manifest is rewritten from the per-stage template; amendments do not carry forward.
- Global denylist paths cannot be amended in via this command. If a slice genuinely needs a globally-denied path (infra config from a Bebop slice, etc.), the correct answer is to re-slice and reassign the owning agent.
- Emergency hand-edits to `.mutagen/state/active-slice.json` still work, but bypass the audit trail and the Decision Process. Prefer this command.

$ARGUMENTS
