# Shredder Output Spec

`Shredder` should emit two artifacts from the same slicing pass:

- `slicemap.md`
- `queue.json`

## Why

The slicemap is for humans.
The queue is for the harness.

That split gives us:

- reviewable prose without teaching the runtime to scrape novels
- deterministic execution contracts
- smaller prompts during execution
- less drift between planning and runtime state

## Required outputs

### 1. `slicemap.md`

Human-readable planning and review artifact.

Defined by:

- [SLICEMAP_SPEC.md](/C:/Users/spork/dev/agentic_design_workflow/harness/SLICEMAP_SPEC.md)

### 2. `queue.json`

Canonical machine-readable execution artifact.

Defined by:

- [QUEUE_SCHEMA.md](/C:/Users/spork/dev/agentic_design_workflow/harness/QUEUE_SCHEMA.md)
- [schemas/queue.schema.json](/C:/Users/spork/dev/agentic_design_workflow/harness/schemas/queue.schema.json)

## Emission rules

- Both artifacts must describe the same slice set.
- `queue.json` is authoritative when a mismatch exists.
- Every execution-critical fact must appear in `queue.json`.
- The slicemap may contain extra explanation, rationale, and review notes.
- The harness should validate `queue.json` immediately after Shredder emits it.

## Queue validation

Use:

```bash
cargo run --manifest-path harness/Cargo.toml -- validate-queue --queue slices/queue.json
```

The validator should be treated as the first consumer of Shredder output. If it flags errors, the queue is not ready for execution.

## Shredder contract

Shredder is no longer done when it has written readable slices.
Shredder is done when it has emitted:

- a reviewable slicemap
- a valid queue JSON artifact that passes harness validation
