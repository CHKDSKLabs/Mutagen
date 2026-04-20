# DDD — Authoring & Review Guide

Companion to [`templates/DDD-template.md`](../templates/DDD-template.md). Read [`guides/README.md`](README.md) first for the shared principles.

## What the DDD is for

The **domain model** that binds the team and the code. It answers: what are the bounded contexts, what is the ubiquitous language, what are the aggregates and their invariants, what events and commands flow between them, and how do contexts relate to each other. Every execution agent's code lifts identifiers from here verbatim; every ISC cites elements from here; every Splinter glossary entry points back here.

A DDD that drifts from how real users and operators speak about the system is worse than no DDD at all — it hides model drift behind official vocabulary.

## When to author

- **In parallel with the ADR**, once the PRD is stable. The PRD provides the user goals and business rules; the DDD models them.
- **Before the ISC.** The ISC names invariants against specific aggregates; those aggregates must exist on paper first.
- **Before engineering estimates for anything domain-heavy.** Estimates against a missing domain model are fiction.

## Who authors

- Primary owner: a tech lead in collaboration with a **domain expert** (a real person who knows the business).
- Co-authors: product (to anchor language to user JTBD), senior engineers (to anchor aggregates to transaction boundaries).
- [April](../agents/April.md) can run the interview, but domain modeling rewards at least one real-time conversation between a tech lead and a domain expert with a whiteboard.

## Authoring process

Work outward from language, inward to structure.

1. **Ubiquitous language first.** Collect real vocabulary from PRD personas, support tickets, internal docs, sales calls. If two departments use the same word differently, surface the ambiguity and resolve it — one term, one meaning per bounded context.
2. **Sub-domain classification.** For each sub-domain, tag it **core** (competitive edge, bespoke), **supporting** (needed but undifferentiated), or **generic** (buy / use off-the-shelf). This drives where investment goes.
3. **Bounded contexts.** A bounded context is a boundary within which one consistent model applies. Pick names from the ubiquitous language. Start with **fewer, larger** contexts and split only when you see a linguistic or model conflict — over-eager splitting creates shared-kernel-everywhere.
4. **Per context — aggregates.** The **transaction boundary**. Every aggregate has a single root, a clear set of invariants, and a rule: "nothing outside the aggregate may reference anything inside it except the root." Small aggregates are almost always better than large ones.
5. **Per context — entities, value objects, events, commands, queries, invariants, policies.** Value objects are preferable to entities when identity doesn't matter; prefer them.
6. **Context map.** How contexts relate. Use the standard DDD patterns — Partnership, Shared Kernel, Customer / Supplier, Conformist, Anti-corruption Layer, Open-host Service, Published Language, Separate Ways. Every relationship has a direction (upstream / downstream) and a contract. *"Shared kernel everywhere"* is a code smell on this map.
7. **Cross-cutting concerns.** Identity, tenancy, audit, time, money, localization, PII — name the single source of truth for each. These cross context boundaries; they are not owned by a single context.
8. **Modeling decisions that don't merit an ADR.** Record why a term is a value object rather than an entity, why an aggregate boundary was chosen, why two contexts merged or split. Keep them brief.
9. **Open modeling questions.** Each with an owner and due date.

## What "good" looks like

- **Ubiquitous language matches how real users speak.** If your DDD says *"Transactions"* and users say *"Orders"*, the DDD is wrong.
- **Every aggregate names its transaction boundary and its invariants.** An aggregate without invariants is just a table.
- **Aggregates are small.** A root with thirty descendant entities is almost always an indicator that you haven't found the real aggregate boundaries yet.
- **Every bounded context has a purpose sentence** and a named owning team.
- **The context map uses standard patterns.** *"Shared Kernel"* is explicit about what's shared; *"Anti-corruption Layer"* is explicit about what it protects.
- **Sub-domains are classified** (core / supporting / generic). Investment maps to classification.
- **Cross-cutting concerns have one source of truth each.** Tenant ID is set in context A and respected everywhere else, not redefined per context.
- **Invariants are numbered** (`[INV-*]`) so ISCs and execution agents can cite them.
- **Events carry business-meaningful names** (*"OrderPlaced"*, not *"CreateEvent"*). Commands carry actor-meaningful names (*"PlaceOrder"*, not *"UpdateState"*).

## Review checklist

- [ ] Does the ubiquitous language match the language in the PRD, support tickets, and user interviews?
- [ ] Is every ambiguous term resolved (one meaning per bounded context, with context-specific meanings explicitly enumerated)?
- [ ] Are sub-domains classified core / supporting / generic?
- [ ] Does every bounded context have a purpose, an owner, and a link to the PRD?
- [ ] Are aggregate boundaries justified by transaction scope and invariants?
- [ ] Are aggregates small?
- [ ] Do value objects and entities appear in the right roles? (Preference: value object unless identity truly matters.)
- [ ] Are events business-named? Are commands actor-named?
- [ ] Are invariants numbered (`[INV-*]`) so ISCs can cite them?
- [ ] Does the context map use recognized DDD patterns with direction stated?
- [ ] Is there **at most one** shared kernel, with a named owner?
- [ ] Are cross-cutting concerns (identity, tenancy, time, money) assigned to a single source of truth?
- [ ] Are open modeling questions actually open (owner + due date), or have they been quietly decided without record?

## Common pitfalls

- **CRUD-thinking disguised as DDD.** Symptom: aggregates are tables, entities are rows, commands are `POST/PUT/DELETE`. Remedy: restart from the ubiquitous language — what do users *do*? Those verbs are commands; the state changes are events.
- **Shared kernel everywhere.** Symptom: every context map relationship is *Shared Kernel*. Remedy: name what's actually shared; everything else is a conformist, customer/supplier, or ACL relationship.
- **Invented terminology.** Symptom: the DDD introduces words nobody outside the modeling team uses. Remedy: language flows **from** users **into** the DDD, not the other way around.
- **Over-eager context splitting.** Symptom: fifteen contexts for a product with four distinct user flows. Remedy: collapse until you hit a linguistic conflict; split there.
- **Monster aggregates.** Symptom: the root aggregate contains most of the application. Remedy: look for invariants that don't need to be enforced in the same transaction — those are candidate seams.
- **Entities everywhere.** Symptom: every concept has an identity and a database row. Remedy: ask *"if two of these have the same fields, are they the same thing?"* If yes, it's a value object.
- **Context map without direction.** Symptom: lines between contexts with no upstream/downstream. Remedy: every relationship has a power direction — name it.
- **Classification missing.** Symptom: no sub-domain is tagged core/supporting/generic. Remedy: classify; this governs where you invest.

## Revision triggers

Reopen the DDD when:

- A new bounded context emerges (new product area, new team, new external integration with its own model).
- An aggregate boundary reveals an invariant it cannot enforce — that's a split signal.
- A context map relationship changes (a former Partnership becomes Customer/Supplier; a Shared Kernel is broken into an Open-host Service + ACL).
- A term in the ubiquitous language shifts meaning (users start calling *"Orders"* what you called *"Transactions"*). The DDD changes; the PRD, ISC, DSD, and glossary follow.
- A new ADR adopts a pattern that forces a model change (event-driven refactor, CQRS, read-model separation).
- A recurring defect class traces back to a model weakness — the bug is a symptom; the model is the cause.

Revisions must keep invariant IDs stable where possible. New invariants get new numbers; superseded invariants are marked `Deprecated` with a pointer to the replacement rather than deleted.
