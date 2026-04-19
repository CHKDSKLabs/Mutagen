# Implied Systems Contract (ISC) — Template

> The quiet parts, said out loud.

An ISC documents the behaviors, invariants, and assumptions a system relies on but that you won't find in a feature spec or PRD. If you break an ISC, the failure is usually silent, confusing, and hard to reproduce — things go wrong in the seams between components, across restarts, across processes, or at 3am when no human is watching.

This template is the distillation of what worked (and what was missing) in Nexus's original `ISC.md`. Copy this file as `ISC.md` in a new project and fill it in.

---

## How to use this template

1. **Write ISCs after you know something that bit you, or something you fear will.** ISCs should feel discovered, not invented. If you can't describe a concrete failure mode, you don't have one yet.
2. **One contract per entry.** If an invariant has two independent failure modes, split it.
3. **Name it like a slogan.** The title is how teammates will refer to it in PR reviews ("that violates ISC-003 — phone numbers are always E.164"). Short, memorable, declarative.
4. **Keep it short.** A well-written ISC fits on one screen. If it's longer, it's probably an ADR or a design doc.
5. **An ISC is not an ADR.** ADRs capture a decision between alternatives. ISCs capture a constraint that must hold regardless of the decision. Link between them when relevant.
6. **An ISC is not a PRD requirement.** PRD items describe what the system does for the user. ISCs describe what the system assumes about itself to keep working at all.

### When to add a new ISC

Ask these prompts during design review, post-incident, or onboarding:

- What happens at 3am when no human is online?
- What happens if this external webhook/callback never arrives? Arrives twice? Arrives out of order?
- Which identifier is the join key between components, and what format must it be in?
- What state would be lost if every process restarted right now?
- Which boundary verifies auth, and what's trusted after that boundary?
- What implicit format/normalization rule must hold across the whole system (units, encoding, timezone, case)?
- What's the blast radius if this runs twice instead of once?
- Which two processes/services look like they share memory but actually don't?
- What invariants does a new contributor need to know before their first PR won't silently break production?

If any answer surprises someone on the team, it's an ISC candidate.

---

## Document header (copy into your ISC.md)

```markdown
# <Project> — Implied Systems Contract

The quiet parts, said out loud. These are the behaviors, invariants, and
assumptions that the system relies on but that you won't find in a feature
spec. Break these and things fail in ways that are confusing to debug.

**Last reviewed:** YYYY-MM-DD
**Owners:** <team or individuals responsible for keeping this current>
**Related docs:** [PRD](./PRD.md) · [ADR](./ADR.md) · [DDD](./DDD.md)
```

---

## Entry template

Copy this block per contract. Delete sections that don't apply (but keep **Invariant** and **What breaks if violated** — those are mandatory).

```markdown
## ISC-NNN: <Slogan-style title>

**Status:** Accepted | Proposed | Deprecated (superseded by ISC-XXX)
**Category:** Operational | Security | Data Integrity | Process Boundary | External Integration | Idempotency | Storage Schema | State Durability
**Context(s):** <bounded context(s) from DDD, e.g. Voicemail, Telephony>
**Related:** ADR-NNN, PRD §F2, ISC-NNN

<One short paragraph of narrative. What is the quiet assumption? Why does
it exist? Give enough background that a new contributor understands the
shape of the problem without needing to read the whole codebase.>

**Invariant:** <A single, declarative, testable statement. Must be phrased
so that a reviewer can check a PR against it. Avoid hedging words like
"should" or "usually" — use "must", "is", "always", "never".>

**What breaks if violated:** <Concrete, specific failure mode. Not "bad
things happen" — describe what a user sees, what a log line looks like,
what data gets corrupted. Prefer two or three short sentences over a
bulleted list unless the failures are genuinely independent.>

**How we detect violations:** <Optional but recommended. A test, a lint
rule, a monitoring alert, a code review checklist item, a DB constraint,
a type, a schema. If the answer is "we hope someone notices", say so —
that's a signal the invariant needs hardening.>

**Implication:** <Optional. Downstream consequences — what other design
choices this forces. Use this when the invariant has non-obvious knock-on
effects (e.g. "therefore this state must be persisted, not in memory").>
```

---

## Category taxonomy (use these to tag new ISCs)

Drawn from the categories that showed up in Nexus. Consistent tags make the doc scannable as it grows past ~20 entries.

| Category | What it covers | Example from Nexus |
|---|---|---|
| **Operational** | Availability, uptime, scheduling, always-on constraints | ISC-001: worker never sleeps |
| **External Integration** | Trust boundary with third-party systems (webhooks, APIs) | ISC-002: webhooks are source of truth |
| **Data Integrity** | Join keys, identifier formats, referential integrity | ISC-003, ISC-010: phone numbers / E.164 |
| **Security** | Auth boundaries, signature verification, secret scoping | ISC-004, ISC-006, ISC-011 |
| **State Durability** | What survives restart, what's ephemeral | ISC-005, ISC-012 |
| **Process Boundary** | What's shared vs. isolated between runtimes | ISC-009: web and worker share nothing |
| **Storage Schema** | Object key patterns, partition keys, path conventions | ISC-007: Tigris key patterns |
| **Idempotency** | Safe-to-retry guarantees, deduplication rules | ISC-002 (implied), ISC-008 |

---

## Starter checklist — the eight questions every project should answer

A minimum-viable ISC document should have at least one entry in each category below. If a category is genuinely N/A, say so explicitly with one line of justification — silence is ambiguity.

- [ ] **Availability:** What must be running 24/7? What's the `min_machines_running` equivalent?
- [ ] **Async event trust:** Who is the source of truth for events the system doesn't originate? What happens when a delivery fails?
- [ ] **Identifier format:** What's the canonical format for every identifier that crosses a boundary (user ID, phone number, email, currency amount, timestamp)?
- [ ] **Join keys:** Which field links which contexts? What's the uniqueness guarantee?
- [ ] **Auth boundary:** Where is auth checked? What is trusted after that point?
- [ ] **Signature/integrity verification:** Which inbound payloads are signed, and where is the signature checked?
- [ ] **Durable state:** What is the system of record? What's cache? What's ephemeral?
- [ ] **Process isolation:** Which processes share memory vs. DB vs. nothing?

---

## Improvements learned from the Nexus ISC

What the original doc did well (keep these):

1. **Slogan-style titles** that work as shared vocabulary in PR reviews.
2. **Three-part body** — narrative, invariant, failure mode — lets a reader skim for the invariant alone.
3. **Concrete failure modes**, not abstract risks. "Callers hear infinite ringing" beats "reliability may degrade".
4. **Occasional `Implication:` section** to surface cascading constraints (e.g. "therefore state must be persisted").
5. **Covered breadth**: operational, security, data format, external integration, state, process boundaries — not just one flavor.

What was missing (this template adds):

1. **`Status` field** — ISCs evolve. Without a status, deprecated invariants linger and confuse readers. Borrowed from ADR practice.
2. **`Category` tag** — once a project has 20+ ISCs, unstructured prose is hard to scan. Categories make the document indexable.
3. **`Context(s)` link to DDD** — ties invariants to bounded contexts so ownership is explicit. Nexus has a clean DDD doc but the ISC didn't reference it.
4. **`Related` cross-references** — an ISC often exists *because* of an ADR decision (e.g. "we chose Telnyx, therefore webhooks are trust boundary"). Making that link explicit helps future readers understand the *why* behind the invariant.
5. **`How we detect violations` field** — the biggest gap in the original. An invariant without a detection mechanism is a wish. Surfacing this field forces the author to answer: is this enforced by a type, a test, an alert, a migration, or just good manners?
6. **`Last reviewed` on the document** — ISCs rot. A date prompts periodic review.
7. **Category taxonomy and starter checklist** — lowers the barrier to starting an ISC doc on a new project. You don't have to stare at a blank file and invent categories; start from the eight questions.
8. **Explicit distinction from ADR and PRD** — the original didn't say what an ISC *isn't*, which made it tempting to dump design decisions or feature requirements into it.

---

## Worked example (adapted from Nexus ISC-005, rewritten in the new format)

```markdown
## ISC-005: Voicemail Is a State Machine, Not a Single Action

**Status:** Accepted
**Category:** State Durability
**Context(s):** Voicemail
**Related:** ADR-001 (Telnyx), ADR-004 (separate worker process), PRD §F2

Recording a voicemail is a multi-step Call Control sequence: answer →
playback greeting → wait for playback-finished webhook → start recording →
wait for hangup or timeout → stop recording → receive recording URL via
webhook → download → upload to Tigris → save metadata → transcribe.
Each step depends on a Telnyx webhook confirming the previous step. It
is not a synchronous flow and the worker handles many calls concurrently.

**Invariant:** Voicemail call state keyed by `call_control_id` must be
persisted to durable storage (Postgres) between webhook invocations. No
in-memory-only state for a call that has not yet ended.

**What breaks if violated:** Callers hear their greeting and then silence
after a worker restart. The "recording saved" webhook arrives and the
worker has no context for which call it belongs to, so the recording is
dropped on the floor. Symptom in logs: `unknown call_control_id` at
`call.recording.saved`.

**How we detect violations:** (1) Integration test that restarts the
worker mid-call and asserts the voicemail still lands in the DB.
(2) Prometheus alert on `voicemail_orphan_recording_total > 0`.
(3) Type-level: the `CallSession` handler takes a `Persisted<CallState>`,
not a plain object, so in-memory-only state won't typecheck.

**Implication:** Worker cannot hold per-call state in module-level
variables or closures. Any call-scoped caching must be keyed by
`call_control_id` and backed by the DB.
```

---

## Anti-patterns to avoid

- **"Everything should be fast."** Not an invariant — no failure mode, no detection.
- **"Don't write bad code."** Reviewer checklist, not a system contract.
- **"The database is Postgres."** Architecture decision — belongs in ADR.
- **"Users can reset their password."** Feature requirement — belongs in PRD.
- **Listing ten invariants in one entry.** Split them. Each should be independently violable.
- **Invariants with no detection.** Either add one, or mark the ISC `Proposed` until you can.
