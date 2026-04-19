# ISC — Authoring & Review Guide

Companion to [`templates/ISC-template.md`](../templates/ISC-template.md). Read [`guides/README.md`](README.md) first for the shared principles.

## What the ISC is for

The **quiet parts said out loud** — the invariants the system depends on that are not in the PRD, not in the ADR, not obvious from the DDD. Break one and the failure is usually silent, confusing, and hard to reproduce. Phone numbers must be E.164; webhooks are the source of truth; the voicemail worker never sleeps; the log pipeline redacts PII at the edge. Those are ISC territory.

The ISC is the distilled memory of what bit you — or what you expect will bite you. It is how agents produce code that does not surprise production.

## When to author

- **Once the ADR and DDD are stable enough to name contracts.** ISCs live at the seams that ADR and DDD produce; you cannot write them before those seams exist.
- **After every incident.** A production incident that wasn't predicted by an existing ISC is a signal that an ISC is missing.
- **During design review.** The questions in the template's *"When to add a new ISC"* section belong in every review.

Unlike PRD / ADR / DDD, the ISC is a **living registry**. New entries are added continuously as the team discovers invariants. Mark `Last reviewed` at the top and rotate through the entries on a cadence.

## Who authors

- Primary owner: a senior engineer who has been on-call.
- Co-authors: anyone who has debugged production at 3 a.m. The ISC is distilled experience — it needs people who have the scars.
- [April](../agents/April.md) can elicit candidates by asking the template's prompts, but the final wording needs someone who can attest to the failure mode.

## Authoring process

1. **Start from pain.** Post-incident reviews, known flaky behaviors, confusing debugging sessions, cross-team misunderstandings. Each is a candidate.
2. **Ask the template's eight starter questions.** Availability, async event trust, identifier format, join keys, auth boundary, signature/integrity verification, durable state, process isolation. If a category genuinely has no ISC, write that down explicitly.
3. **For each candidate:**
   - **Slogan-style title.** Short, declarative, memorable. *"Phone numbers are always E.164."* *"Webhooks are the source of truth."* *"Voicemail is a state machine, not a single action."* The title must be usable in a PR review — *"that violates ISC-003"*.
   - **One short narrative paragraph.** What is the quiet assumption? Why does it exist? Give a new contributor enough context.
   - **Invariant.** A **single, declarative, testable** statement. No *"should"* or *"usually"* — use *"must"*, *"is"*, *"always"*, *"never"*.
   - **What breaks if violated.** Concrete, specific failure mode. What does a user see? What does a log line look like? What data gets corrupted?
   - **How we detect violations.** A test, a lint rule, a monitoring alert, a code-review checklist, a DB constraint, a type, a schema. If the answer is *"we hope someone notices"*, mark the ISC `Proposed` until the answer is real.
   - **Implication** (optional). Downstream consequences. *"Therefore state must be persisted, not in memory."*
4. **Tag category.** Operational, Security, Data Integrity, Process Boundary, External Integration, Idempotency, Storage Schema, State Durability. Consistency matters for scannability.
5. **Link to DDD context(s)** and **related ADR / PRD / other ISCs.** An ISC exists because of upstream decisions; name them.
6. **Status.** `Proposed` → `Accepted` → (later) `Deprecated (superseded by ISC-XXX)`. No ISC lives forever unchallenged.

## What "good" looks like

- **Slogan titles.** The team can refer to ISCs by name in PRs without looking them up.
- **One invariant per entry.** If an invariant has two independent failure modes, split it.
- **Declarative language.** *"Must"*, *"is"*, *"always"*, *"never"*. No hedge words.
- **Concrete failure modes.** *"Callers hear infinite ringing"* beats *"reliability degrades."*
- **Every entry has a detection mechanism.** The biggest single quality signal for an ISC doc is whether every invariant can be caught automatically or by a repeatable review.
- **Fits on a screen.** If an entry runs long, it's probably trying to be an ADR or a design doc.
- **Categories are balanced.** A doc that is all Security and nothing else is missing most of its surface.
- **Cross-references are present.** *"Related: ADR-001 (Telnyx), PRD §F2."*
- **`Last reviewed` date is current.** ISCs rot; re-verify on a cadence.

## Review checklist

For each individual entry:

- [ ] Is the title a usable slogan?
- [ ] Does the invariant read as a **single** testable statement?
- [ ] Is the failure mode concrete enough to picture in the logs?
- [ ] Is there a real detection mechanism (not *"we hope"*)?
- [ ] Is the category tag correct and useful?
- [ ] Are the Context(s) and Related links filled in?
- [ ] Is the status current?

For the document as a whole:

- [ ] Does every starter-checklist category have at least one entry (or an explicit *"N/A — justification"*)?
- [ ] Are there any entries that are really ADRs in disguise? Decisions between alternatives belong in an ADR.
- [ ] Are there any entries that are really PRD requirements in disguise? User-facing behaviors belong in the PRD.
- [ ] Are there any entries that are really style rules in disguise? Conventions belong in the DSD.
- [ ] Is the `Last reviewed` date within the project's review cadence?

## Common pitfalls

- **"Should" language.** *"The system should be fast."* Not an invariant — no failure mode, no detection. Remedy: either upgrade to *"must"* with a bound (PRD NFR territory) or delete.
- **No detection mechanism.** *"We rely on developers remembering."* Remedy: add one (test, lint, alert, type), or mark the ISC `Proposed` until you can.
- **ADRs in disguise.** *"The database is Postgres."* That's an architecture decision — it belongs in an ADR. ISCs capture constraints that must hold **regardless** of the decision.
- **PRD requirements in disguise.** *"Users can reset their password."* Feature requirement — PRD.
- **Style rules in disguise.** *"Timestamps are ISO-8601 UTC with Z."* Convention — DSD (where it becomes a numbered `[DSD-###]` rule). An ISC would be *"Timestamps stored in UTC must never be returned in the caller's local timezone without explicit conversion."*
- **Ten invariants in one entry.** Symptom: a single ISC lists a half-dozen bullets under "Invariant." Remedy: split — each should be independently violable.
- **Entries that rot silently.** The system changed; the ISC did not. Remedy: `Last reviewed` at the top, scheduled review, deprecate entries whose invariants no longer apply.
- **Over-broad entries.** *"The system must be secure."* Remedy: specific, surgical statements that a reviewer can check.

## Revision triggers

Add or update an ISC when:

- A production incident reveals an invariant that was implicit and is now explicit.
- A new ADR introduces a new trust boundary, process boundary, or external integration.
- A new DDD bounded context or context-map relationship introduces new join keys, identifiers, or events.
- A new DSD rule implies an invariant (*"every audit log is append-only"* implies the ISC *"audit records must not be modified after write"*).
- An old ISC's detection mechanism is replaced with a better one (update in place).
- An ISC is superseded because the underlying assumption changed — mark deprecated, link to successor.

Rotate through the full registry on a cadence (the template suggests quarterly) — update `Last reviewed` and demote entries whose detection has lapsed.
