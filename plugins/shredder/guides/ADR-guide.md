# ADR — Authoring & Review Guide

Companion to [`templates/ADR-template.md`](../templates/ADR-template.md). Read [`guides/README.md`](README.md) first for the shared principles.

## What the ADR is for

The record of a **single significant architectural decision** — the context that forced it, the alternatives genuinely considered, the choice made, and the consequences (positive AND negative). One ADR per decision. Do not bundle. A decision you cannot point at later because it's buried inside a larger document is a decision that will be re-litigated.

## When to author

- **As soon as a decision is actually being made.** Not after; the alternatives evaporate in retrospect.
- **Before the code that implements it lands.** An ADR authored after merge is a post-hoc justification, not a decision record.
- **Any decision whose reversal would cost the team more than a day of work**, or whose reversal would require coordinated changes across teams.

## Who authors

- The engineer proposing the decision drafts.
- The **deciders** (named in the metadata) are the people with authority to accept or reject.
- A separate set of **consulted** and **informed** is recorded for traceability.

## Authoring process

1. **Name the decision in the title.** Verbs and nouns, short. *"ADR-0042: Use PostgreSQL for the orders service"*, not *"Database choice."* The title is how the team will refer to it.
2. **Context.** The forces at play — technical, organizational, regulatory, economic. Cite the PRD `[FR-*]`/`[NFR-*]` the decision must satisfy. Frame the problem as a **question** the decision answers.
3. **Decision.** Active voice. *"We will …"*. One or two sentences of decision, then enough detail to make it unambiguous.
4. **Alternatives.** At least two beyond the chosen option. *"Do nothing"* is often a valid alternative and must appear when relevant. Each alternative gets Pros, Cons, and a **Why rejected** that a proponent of that alternative would recognise as fair.
5. **Consequences.** Positive, negative, and neutral. A consequences section without any negatives is almost always dishonest — what did we give up?
6. **Compliance & validation.** How will we know the decision is being upheld over time? Tests, lint rules, architectural fitness functions, review checklists, dashboards. If the answer is *"we hope people remember"*, the decision is not yet landable.
7. **Follow-ups.** Work items unblocked or created. Link to tickets / PRs.
8. **Status.** `Proposed` → `Accepted` / `Rejected` → (later) `Deprecated` / `Superseded by ADR-MMMM`. Status drives trust.

## What "good" looks like

- **One decision per ADR.** If you need the word *"and"* to describe the decision, split it.
- **Alternatives that a proponent would recognise as fairly represented.** No straw men.
- **Consequences that include real negatives.** The decision is a trade-off; name the trade.
- **A compliance mechanism that isn't *"we hope."*** A test, a lint rule, a code-owner file, a scheduled drift review.
- **Cross-links to the PRD** — specific `[FR-*]`/`[NFR-*]` that drove the decision.
- **Cross-links between ADRs** — `Supersedes` / `Superseded by` always current.
- **Short.** A well-written ADR fits on one screen; the honesty is in the alternatives, not the word count.

## Review checklist

- [ ] Is the decision **significant**? (If reversing it costs < a day, it may not need an ADR.)
- [ ] Is the decision **singular**? (One change per ADR.)
- [ ] Does the context cite the PRD items that force the decision?
- [ ] Are at least two real alternatives present?
- [ ] Are the alternatives' cons fairly described (not straw men)?
- [ ] Does the consequences section include genuine negatives?
- [ ] Is there a concrete compliance / validation mechanism?
- [ ] Is the status current?
- [ ] If this ADR supersedes another, is the prior ADR marked `Superseded by`?
- [ ] Would a new engineer, a year from now, understand why this decision was made?

## Common pitfalls

- **Rationalization, not decision record.** Symptom: ADR is written after the code ships; alternatives are all clearly inferior; consequences are all positive. Remedy: author ADRs while the decision is live.
- **Missing alternatives.** *"We chose X because it was obvious."* Rarely true; surface what you didn't choose.
- **No compliance mechanism.** Decision drifts silently over the next year. Remedy: pair every ADR with a test, a lint rule, a fitness function, or a recurring review.
- **Status rot.** Superseded ADRs still read `Accepted`. Remedy: when you accept the new ADR, update the old one's status in the same commit.
- **Bundled decisions.** *"We will use PostgreSQL, Prisma, and Fly.io."* That is three decisions. Split them — each has its own alternatives.
- **Decision too small for an ADR.** If the choice between two libraries has no meaningful consequences and reverses in an hour, it does not need an ADR.
- **Decision too big for an ADR.** If a decision requires multiple distinct compliance mechanisms and its consequences span quarters, it is probably two or three decisions in a trenchcoat.

## Revision triggers

ADRs are not revised in place; they are **superseded**. Open a new ADR when:

- New information invalidates the original decision (a library is deprecated, a regulation changes, a constraint vanishes).
- The trade-off is revisited because the conditions under which the original was made have changed.
- The compliance mechanism reveals sustained drift that can't be corrected in-place.
- A larger ADR (e.g. architecture style change) forces re-evaluation.

On supersession: open the new ADR, reference the old one in its metadata, and update the old ADR's status to `Superseded by ADR-MMMM` in the same commit. Do not delete the old ADR — the history is the record.
