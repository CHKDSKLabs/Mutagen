# Orders Demo

A minimal, populated workspace that shows what a mutagen project looks like
mid-flight. Useful for:

- New users who want to see what an "approved" upstream design bundle, a
  generated slice queue, and a Tiger Claw review report all look like
  *together*, in their canonical filesystem layout.
- Contributors who need a fixture they can copy into a scratch directory
  and run `/mutagen:execute-next` against to exercise the pipeline.

This is not a runnable project — there's no application code under it. It's
the design-side artifacts a real workspace would have on disk after April
finished elicitation, Shredder produced the queue, and Karai dispatched at
least one slice through Tiger Claw.

## Layout

```
examples/orders-demo/
├── docs/                              # the five upstream design documents
│   ├── PRD.md                         # what we are building (FR-* / NFR-*)
│   ├── ADR/ADR-0001.md                # how, at the system level
│   ├── DDD.md                         # the domain model (aggregates, ubiquitous language)
│   ├── ISC.md                         # implied systems contract (ISC-NNN invariants)
│   └── DSD.md                         # design style guide (DSD-### rules)
├── slices/
│   └── queue.json                     # Shredder's dependency-ordered slice queue
└── reviews/
    └── L1-orders-001/
        └── tiger-claw.md              # Tiger Claw's adversarial QA on the first slice
```

## What the artifacts mean

- **`docs/PRD.md`** — Two requirements (`[FR-001]`, `[NFR-001]`) for an
  Orders capability. This is what April produces in `/mutagen:elicit`.
- **`docs/ADR/ADR-0001.md`** — One accepted architecture record. Slices
  cite this in their `traces_to.adr` array.
- **`docs/DDD.md`** — Names the `OrderAggregate` and the operations on it.
  Slice identifiers and code identifiers must match this ubiquitous
  language verbatim.
- **`docs/ISC.md`** — One systems contract entry (`Orders to Billing`).
  Each `[ISC-NNN]` reference in a slice must map to a code site that
  upholds the invariant.
- **`docs/DSD.md`** — The style rules every slice must conform to.
- **`slices/queue.json`** — Two pending slices: `L1-orders-001` (create
  the order aggregate) and `L2-orders-002` (expose the creation API).
  Each slice carries `traces_to`, `write_set`, `verification_steps`, and
  `human_check_needed`.
- **`reviews/L1-orders-001/tiger-claw.md`** — Tiger Claw's adversarial QA
  artifact for the first slice, in the canonical 🐅 QA format.

## Running the pipeline against this example

The fastest way to see mutagen drive a slice end-to-end is:

```bash
# Copy the example into a scratch workspace
mkdir -p /tmp/mutagen-orders-demo
cp -r examples/orders-demo/* /tmp/mutagen-orders-demo/
cp -r examples/orders-demo/docs/ADR /tmp/mutagen-orders-demo/docs/

# Initialise the project capsule and run the workflow
cd /tmp/mutagen-orders-demo
bash /path/to/agentic_design_workflow/plugins/mutagen/scripts/project.sh init \
    --workspace-root . \
    --name orders-demo \
    --stack vite-express-sqlite \
    --design-system plain-css

# Then drive the queue from your Claude Code or Codex session:
#   /mutagen:execute-next         (Claude)
#   $mutagen-execute-next         (Codex)
```

Don't run mutagen against this directory in-place — the harness writes
state into `.mutagen/`, regenerates `slices/slicemap.md`, and appends to
review artifacts. Treat the example as read-only and copy it elsewhere
before exercising it.
