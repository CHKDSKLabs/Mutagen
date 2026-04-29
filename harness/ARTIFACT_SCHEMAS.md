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
