# `slices/slicemap.md` — human-readable slice map

The human-facing planning artifact is **`slices/slicemap.md`**.

The harness does not execute from this file.
It exists for review, discussion, and progress scanning without making humans read JSON for sport.

`slices/queue.md` is a legacy compatibility shadow for older Mutagen commands.
If `slicemap.md` and `queue.md` ever differ, `slicemap.md` is the intended rendering and `queue.json` is the canonical source of truth.

---

## Purpose

The slicemap is where Shredder shows his work in a way humans can review.

It should surface:

- planning advisories that affect execution
- layer ordering
- slice objectives and implementation intent
- human checkpoints
- review-friendly summaries of dependencies and scope

It must not be the only place where execution-critical facts exist.

---

## Required structure

### 1. Planning advisories

If the slicing pass produced assumptions, unresolved tensions, or environment caveats, the slicemap begins with a planning-advisories section.

Each advisory should include:

- stable identifier when one exists, such as `ISC-012`
- severity
- concise summary
- default slicing decision or assumption
- whether user input is still required
- referenced upstream IDs
- affected slice IDs, if known

### 2. Layer sections

Slices are grouped beneath explicit layer headings, for example:

- `## Layer 1 — Foundation`
- `## Layer 2 — Data`
- `## Layer 3 — Security`

### 3. Slice blocks

Each slice block should include:

- slice ID
- optional phase label
- title
- assigned agent
- objective
- bounded context
- target LOC
- dependencies
- write set summary
- context to update
- traces-to summary
- implementation details
- verification steps
- human-check status

---

## Allowed looseness

Because the slicemap is for humans, it may include:

- explanatory prose
- rationale
- review notes
- migration or setup reminders
- supersession notes

It may not invent execution-critical facts that are absent from `queue.json`.

---

## Forbidden dependency

The runtime must not depend on slicemap prose to discover:

- `author_agent`
- `depends_on`
- `write_set`
- `traces_to`
- `human_check_needed.resolved_at`
- retry state
- scheduler state

Those live in `slices/queue.json`.
