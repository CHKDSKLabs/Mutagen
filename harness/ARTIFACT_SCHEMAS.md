# Harness Artifact Schemas

This document is the index for the harness runtime artifacts. JSON artifacts
have JSON Schema files under `harness/schemas/`. Markdown artifacts have a
stable section contract because validating prose with JSON Schema is how one
summons paperwork and gets no safety in return.

## JSON Artifacts

| Artifact | Schema | Producer |
| --- | --- | --- |
| `.mutagen/project.json` | `schemas/project-capsule.schema.json` | `project init` |
| `slices/queue.json` | `schemas/queue.schema.json` | Shredder, then harness runtime mutations |
| `.mutagen/state/active-slice.json` | `schemas/active-slice.schema.json` | `prepare-next`, `prepare-selected-slice`, `transition-active-slice`, `amend-scope` |
| Structural gate report | `schemas/gate-verdict.schema.json` | `structural-check` |
| `.mutagen/state/dispatch-log.jsonl` line | `schemas/dispatch-log-entry.schema.json` | `finalize-slice`, `apply-cohort-dispatch` |
| `finalize-slice` JSON result | `schemas/finalize-result.schema.json` | `finalize-slice` |

## Evidence Bundle Contract

Path: `.mutagen/state/evidence/<slice_id>.md`

Producer: `prepare-next` or `prepare-selected-slice`.

Required shape:

```markdown
## Evidence Bundle for <slice_id>

### PRD citations
...

### ADR(s)
...

### DDD citations
...

### ISC citations
...

### DSD citations
...
```

Each non-empty section contains one or more citation blocks:

```markdown
#### <citation-id>

<source excerpt>
```

The harness treats the evidence bundle as immutable per slice attempt. Agents
read it; they do not rewrite it.

## Summary Contract

Path: `slices/<slice_id>/summary.md`

Producer: `finalize-slice`.

Required shape:

```markdown
# Slice summary — <slice_id>
**Title:** <title>
**Author:** <agent>
**Layer / Context:** L<n> / <bounded_context>
**Completed at:** <timestamp>
**Duration:** <duration>
**Attempts:** <n> (micro_correction: <bool>)

## Verdicts
...

## Files touched
...

## Advisories logged
...

## Retry path
...

## Reports
...
```

This file is the durable human-readable closeout. The orchestrator should
reference it instead of carrying author or reviewer transcripts forward.

## State Target Contract

`context_to_update` is parsed as a state target, not as a filesystem path.
Valid values are:

- `project_state.md`
- `infrastructure_state.md`
- `project_state.md § <section>`
- `infrastructure_state.md § <section>`

The `§` suffix names a markdown section inside the canonical state file.
Parenthetical pseudo-paths such as `project_state.md (RBAC section)` are
invalid. The harness rejects them because pretending a label is a path is how
one gets a repo full of accidental paperwork.

When a section target is present, `finalize-slice` applies the parsed
`State Update` block under that section in the canonical file.

## Shredder Dual-Emission Contract

Shredder emits two artifacts from the same slicing pass:

- `slices/slicemap.md` — human-readable planning + review artifact. See
  [`plugins/mutagen/guides/slicemap-spec.md`](../plugins/mutagen/guides/slicemap-spec.md).
- `slices/queue.json` — canonical machine-readable execution artifact. See
  [`plugins/mutagen/guides/queue-schema.md`](../plugins/mutagen/guides/queue-schema.md)
  and [`schemas/queue.schema.json`](schemas/queue.schema.json).

That split gives reviewable prose without teaching the runtime to scrape
novels, deterministic execution contracts, smaller prompts during execution,
and less drift between planning and runtime state.

### Emission rules

- Both artifacts must describe the same slice set.
- `queue.json` is authoritative when a mismatch exists.
- Every execution-critical fact must appear in `queue.json`.
- The slicemap may contain extra explanation, rationale, and review notes.
- The harness validates `queue.json` immediately after Shredder emits it.

### Validation

```bash
cargo run --manifest-path harness/Cargo.toml -- validate-queue --queue slices/queue.json
```

The validator is the first consumer of Shredder output. If it flags errors,
the queue is not ready for execution. Shredder is not done when it has
written readable slices — Shredder is done when it has emitted a reviewable
slicemap and a valid queue JSON artifact that passes harness validation.
