# PRD — Authoring & Review Guide

Companion to [`templates/PRD-template.md`](../templates/PRD-template.md). Read [`guides/README.md`](README.md) first for the shared principles.

## What the PRD is for

The source of truth for **what** the system does, for **whom**, and **why**. Every downstream document (ADR, DDD, ISC, DSD) and every slice Shredder produces must trace back to a PRD entry. When the PRD is unclear, every document downstream inherits that ambiguity.

## When to author

- **First, always.** No ADR, DDD, ISC, or DSD work begins until the PRD is stable enough to cite.
- **Before engineering estimates.** Estimates against a fuzzy PRD are fiction.
- **Before any agent is dispatched.** Shredder's Readiness Check will refuse a bundle without an Approved PRD.

## Who authors

- Primary owner: the product owner or product lead.
- Co-authors: tech lead (constraints), design lead (UX), domain expert (language).
- [April](../agents/April.md) can run the interview end-to-end if no one on the team has written a PRD before.

## Authoring process

Work roughly in this order; iterate as later sections reveal gaps in earlier ones.

1. **Problem & Background.** What is broken, missing, or worth doing? What does it cost to leave it alone? Include evidence — data, user quotes, support tickets. A PRD that cannot name the cost of doing nothing is solving a feature request, not a problem.
2. **Users & personas.** Primary, secondary, and **anti-personas**. Job-to-be-done per persona. If you cannot name the anti-persona, the scope will drift.
3. **Goals.** Measurable outcomes, not features. A goal of *"reduce onboarding time to under 5 minutes"* is reviewable; a goal of *"improve onboarding"* is not.
4. **Non-goals.** Things a reasonable reader might assume are in scope but are not. Every non-goal is a scope-creep guard.
5. **Success metrics.** Baseline + target + measurement mechanism + which goal it traces to. Prefer leading indicators alongside lagging ones.
6. **Requirements.** Split functional (`[FR-*]`) and non-functional (`[NFR-*]`). Use MUST / SHOULD / MAY language. Each MUST the system MUST do, written so a test can falsify it.
7. **Constraints & assumptions.** Budget, timeline, compliance, platform. Assumptions are explicit — if the assumption breaks, the PRD is revisited.
8. **Dependencies.** Upstream teams, external systems, prerequisite work. Owner + hand-off artifact + status.
9. **Risks.** Likelihood × impact × mitigation. Known-unknowns only; don't invent.
10. **Open questions.** Every question has an owner and a due date. Questions without both are blockers, not open questions.
11. **Release criteria.** Checkable. Each bound to a requirement ID and, where feasible, an automated test.

## What "good" looks like

- **Every FR and NFR has a stable numbered ID** (`[FR-1]`, `[FR-2]`, …). Downstream agents cite these.
- **Every goal has at least one success metric.** Unmeasured goals are wishes.
- **Every non-goal is something a reader might reasonably have assumed was in scope.** Obvious non-goals ("we won't build a rocket") add no value.
- **Every MUST requirement is falsifiable.** A reviewer can read the requirement and sketch the test in under a minute.
- **Every assumption is labelled.** If it breaks, the PRD is revisited.
- **The problem statement would survive a five-whys interrogation.** If the fifth "why" lands on *"a stakeholder wanted this,"* the problem is not yet understood.
- **Users are named with their job-to-be-done**, not with their job title alone.
- **Non-functional requirements are quantified.** *"Fast"* is not an NFR; *"p99 < 250ms at 500 RPS"* is.

## Review checklist

A reviewer should ask:

- [ ] Does the PRD answer *what*, *for whom*, *why now*, and *how we'll know it worked*?
- [ ] Is every goal measurable and bound to a metric?
- [ ] Are there non-goals? Are they the right non-goals (things a reader might assume in-scope)?
- [ ] Are FR/NFR IDs numbered and stable?
- [ ] Does every MUST have an implied test?
- [ ] Are NFRs quantified with specific numbers and conditions?
- [ ] Are assumptions explicit?
- [ ] Do open questions have owners and due dates?
- [ ] Are risks the real risks (not just generic ones)?
- [ ] Does the document survive a five-whys against the problem statement?
- [ ] Is the ubiquitous language seeded here consistent with how real users talk?

## Common pitfalls

- **Feature list disguised as a PRD.** Symptom: section 7 (Requirements) is two pages; section 2 (Problem) is two sentences. Remedy: rewrite the problem statement until it would justify the features, or cut features that cannot justify themselves against the problem.
- **Non-falsifiable goals.** *"Delight users"* / *"improve quality"*. Remedy: pair each goal with a metric whose target would be uncomfortable to commit to.
- **Missing non-goals.** Symptom: reviewers keep proposing features the author doesn't want; there is no document to point at. Remedy: add non-goals as you realize what people assume.
- **Untestable requirements.** *"The system should be fast."* Remedy: MUST / SHOULD + a numeric bound + a condition.
- **Hidden assumptions.** The PRD assumes a team, a library, a regulatory regime — but doesn't say so. Remedy: surface assumptions explicitly; note what invalidates them.
- **Personas as job titles.** *"Admin users."* Remedy: named persona + JTBD + current workaround + pain.
- **Success metrics with no measurement path.** *"Increase retention."* Remedy: name the instrumentation, owner, baseline, and target.

## Revision triggers

Reopen the PRD when:

- User research reveals a new persona, pain, or workaround.
- A success metric is missed for a defined period and the goal must be reframed.
- A constraint or assumption has broken (team change, new regulation, budget shift, platform change).
- A downstream agent (Shredder, Karai, or any execution agent) raises an ambiguity that cannot be resolved without re-scoping.
- A release-criteria item is renegotiated during implementation — the PRD, not the plan, is where that lives.
- An ADR rejects a path the PRD assumed.

Reopening does not delete history. Add to the change log; supersede sections with explicit `Superseded by <date>` markers where appropriate.
