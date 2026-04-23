# Slicemap Spec

This document defines the human-readable slicemap artifact.

## Purpose

The slicemap is for humans.

It exists to support:

- design review
- slice review
- planning discussion
- unresolved-advisory visibility
- phase and milestone communication

It is not the canonical execution artifact.

## Canonical rule

The harness executes from machine-readable queue data, not from slicemap prose.

The preferred output of `Shredder` is:

- `slicemap.md` for humans
- `queue.json` for the harness

If a future compatibility layer compiles a slicemap into queue JSON, that compiler is a temporary bridge, not the long-term control plane.

## Required slicemap structure

### 1. Advisory preamble

Optional in length, required in shape when present.

The slicemap may begin with planning advisories that describe assumptions, unresolved design tensions, or boundary conditions that affect slicing.

Each advisory should include:

- stable identifier if available, such as `ISC-012`
- severity
- short description of the issue
- slicing assumption or default decision
- explicit note that the user may override the assumption

### 2. Layer sections

Slices must be grouped under named layer headings such as:

- `Layer 1 — Foundation`
- `Layer 2 — Data`
- `Layer 3 — Security`

### 3. Slice blocks

Each slice block must include:

- slice ID
- title
- objective
- target LOC
- context to update
- implementation details
- verification step
- human check needed

Optional fields in the slicemap:

- phase labels
- notes about rationale
- manual setup notes
- supersession notes

## Example shape

```md
[Slice ID: L1-01] [Phase 1] - Android Project Scaffold

- Objective: Stand up the Gradle project.
- Target LOC: ~300
- Context to Update: project_state.md → "Foundation / Project Setup"
- Implementation Details:
  - ...
- Verification Step: ./gradlew assembleDebug
- Human Check Needed?: No.
```

## Allowed looseness

Because the slicemap is human-facing, it may contain:

- explanatory prose
- wrapped lines
- clarifying notes
- review commentary

It may not be the only place where execution-critical facts exist.

## Forbidden dependency

The harness must not depend on slicemap prose to discover:

- `author_agent`
- `depends_on`
- `write_set`
- `traces_to`
- retry policy
- scope policy
- machine-resolved human-check status

Those belong in the canonical queue artifact.
