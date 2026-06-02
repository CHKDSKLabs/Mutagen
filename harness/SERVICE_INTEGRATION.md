# Harness Service Integration Notes

These are the core contracts `harness/service` must preserve when it moves
from active development to completion.

## State Targets

Core no longer treats `context_to_update` as an arbitrary path. It parses the
field into a typed target:

- `project_state.md`
- `infrastructure_state.md`
- `project_state.md § <section>`
- `infrastructure_state.md § <section>`

Parenthetical pseudo-paths such as `project_state.md (RBAC section)` are
invalid. The service must reject them before dispatch and must never create
files from section labels.

Use `mutagen_core::state_target::StateTarget` for all target handling. Do not
duplicate parsing rules in service code.

## State Recording

The state-record stage is mechanical. The service should call the core path
that parses the author's `## State Update` block and applies it to the
resolved `StateTarget`.

If the target has a section anchor, the update belongs under that markdown
section in the canonical file. Missing sections may be created as headings;
new files with section text in their names are a bug.

## Human Checks

`human_check_needed.required = true` with no `resolved_at` is an execution
gate. Core will skip or block those slices during selection and refuses resume
for the same condition.

The service should surface this as a blocked slice, not as a warning buried in
logs. Resolving the check must go through the queue mutation path that records
`resolved_at`.

## Queue Readiness

The queue readiness hash includes execution-critical fields, including human
gate resolution. If `resolved_at` changes after activation, the active-slice
readiness snapshot is stale and the slice must be re-prepared.

Service orchestration must keep passing the queue-validation path into core
entry points. Do not bypass readiness checks because a queue file happens to
exist.

## Dirty Workspace Summary

Core activation and finalization results include `workspace_dirty`, a scoped
summary from `git status --porcelain`. It is advisory telemetry, not a hard
block.

The service should show this summary when present, especially for paths under
the current slice write set and `.mutagen/state/**`. If `checked` is false,
show `skipped_reason` rather than inventing certainty.

## Runtime Artifacts

Consumer repos should ignore runtime harness state:

```gitignore
.mutagen/state/
.mutagen/worktrees/
```

Durable planning artifacts such as `slices/queue.json`, `slices/queue.md`,
`slices/slicemap.md`, `reviews/**`, and slice summaries are policy decisions
for the consuming repo. Runtime transcripts, evidence bundles, tool-call logs,
and active state are not durable source artifacts.
