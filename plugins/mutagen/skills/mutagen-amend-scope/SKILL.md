---
name: mutagen-amend-scope
description: Explicit invocation only. Traag evaluates an in-flight scope-manifest amendment (path/glob + mutation kind + reason) against the active slice's stage, agent, and the global denylist; decides ALLOW or DENY.
---

# $mutagen-amend-scope — mediated scope amendment via Traag

The user's request describes a path (or glob) they want added to the
currently-active slice's `allowed_write_globs`, the mutation kind (`create`
/ `modify` / `delete`), and a reason.

In the Claude Code plugin this skill mediates hook enforcement. In the
Codex port there is no hook, but the harness now owns the deterministic
ALLOW / DENY decision. Use the audited runtime path for widening an
active manifest and keep Traag as policy flavor, not as the primary
execution engine.

## Preflight

1. Confirm `.mutagen/state/active-slice.json` exists. If not, refuse — no
   slice to amend. Tell the user `$mutagen-execute-next` must be in flight.
2. Confirm `slices/queue.json` exists. If not, refuse.
3. Read both files. Pull the active slice entry from the queue by matching
   `slice_id`.
4. Sanity-check the user's request prose:
   - If empty, refuse.
   - Extract one or more requested paths / globs, the mutation kind
     (`create` / `modify` / `delete`), and a reason.
   - If no reason evident, tell the user the amendment requires a
     justification and stop.
   - If the path list or mutation kind is too ambiguous to extract
     safely, stop and ask for the missing detail instead of guessing.

## Dispatch

```bash
bash "$MUTAGEN_ROOT/scripts/amend_scope.sh" \
  --requested-glob "<glob-1>" \
  [--requested-glob "<glob-2>" ...] \
  --mutation-kind create|modify|delete \
  --reason "<user reason>"
```

The wrapper delegates to the Rust harness `amend-scope` runtime. Treat its
JSON payload as authoritative for `decision`, `class`, `matched_rule`,
`rationale`, `suggested_next_step`, `justification_gap`, `added_globs`,
and `allowed_write_globs`.

## After the runtime returns

### If ALLOW

1. The runtime already rewrote `.mutagen/state/active-slice.json` and
   appended `.mutagen/state/amendments.jsonl`.
2. Report the allow decision concisely:
   - path(s) added
   - whether `justification_gap` was flagged
   - that the amendment is live for the current stage only
3. If `added_globs` is empty, say so plainly.

### If DENY

1. The runtime already left `.mutagen/state/active-slice.json` untouched
   and appended a denial record to `.mutagen/state/amendments.jsonl`.
2. Present the denial using the returned `class`, `matched_rule`,
   `rationale`, and `suggested_next_step`.

## Reminders

- The harness decision is final for the current request. Do not retry, do
  not reword, do not argue.
- Amendments are per-slice and per-stage. When `$mutagen-execute-next`
  rotates to the next stage, the manifest is rewritten from the per-stage
  template; amendments do not carry forward.
- Global denylist paths cannot be amended in via this skill. If a slice
  genuinely needs a globally-denied path, re-slice and reassign.
- Emergency hand-edits to `.mutagen/state/active-slice.json` bypass the
  audit trail. Prefer this skill.
