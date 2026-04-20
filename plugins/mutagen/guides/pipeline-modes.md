# Pipeline Modes — Full vs. Lightweight

Companion to [`guides/README.md`](README.md). Governs how deeply every slice is inspected before it lands.

## Why this document exists

The full pipeline — **author → Karai (structural) → Bishop (review) → Tiger Claw (adversarial) → Karai (state)** — is high-ceremony. On projects with real design weight (multi-context DDD, security surface, data modelling, observability needs), that ceremony pays off. On a small CLI, a one-off script, a throwaway prototype, or a single-developer hobby project, it can feel like overhead that smothers momentum.

This guide defines two modes and how to opt in.

## Modes

| Mode | Bishop | Tiger Claw | When it fits |
|------|--------|------------|--------------|
| **Full** (default) | Every slice | Every slice | Production systems. Multi-developer teams. Anything with real security, data, or reliability surface. Projects whose failure cost is larger than the review cost. |
| **Lightweight** | Only slices tagged `review_required: true` | Only slices tagged `review_required: true` | Prototypes. Early-phase MVPs. Single-developer projects. Internal tools with a clear blast radius. Projects where review cost currently exceeds failure cost. |

**Karai's structural conformance check always runs.** Traag's scope enforcement always runs. April, Shredder, and every executor's showpiece (Threat Model, Algorithmic Proof, Data Model Analysis, Observability Plan, Documentation Brief) always run. Lightweight only affects Bishop and Tiger Claw.

## Which slices get `review_required: true` in lightweight mode?

Shredder tags the slice when **any** of the following is true:

1. **Security-critical surface.** Traces-to cites a Security / External Integration / Data Integrity ISC, a security / privacy / compliance NFR, a DDD element carrying PII / credentials / secrets / audit data, or a trust-boundary crossing. Routes to Tatsu — and must be reviewed.
2. **Data-layer non-trivially.** Chaplin-owned slices with composite indexes, multi-tenant predicates, partitioning, online migrations, or explicit consistency trade-offs.
3. **External contract changes.** A published API, webhook, shared schema, or library surface changes in a way that could break callers.
4. **Production-reachable infrastructure.** Krang slices that touch prod deployment, DNS, secrets, IAM, or cost-impacting resources.
5. **Irreversibility.** Anything whose rollback is non-trivial (data migration past the backfill point, schema drop, public API removal).
6. **Observability contracts.** SLO authored, alert authored, runbook authored — reviewed so on-call isn't surprised.
7. **Size threshold.** Slice is larger than 250 LOC net-new, or touches more than 10 files.
8. **Explicit author opt-in.** The slice author prefers review. Always honor.

Slices that miss all eight criteria default to `review_required: false` in lightweight mode — Bishop and Tiger Claw skip.

## Configuration

Projects opt in by creating `.claude/workflow.json`:

```json
{
  "pipeline_mode": "lightweight",
  "review_threshold_loc": 250,
  "review_threshold_files": 10
}
```

Omitted or absent = `"full"`. Downgrade or upgrade at any time; Shredder re-evaluates tags on the next `/mutagen:slice`.

## Adopting lightweight — the opinionated first ADR

Projects that choose lightweight SHOULD capture the decision as their first ADR so reviewers understand the posture. Recommended template:

```markdown
---
adr: ADR-0001
title: Use lightweight pipeline mode for this project
status: Accepted
date: YYYY-MM-DD
deciders: [<name>]
consulted: [<names>]
informed: [<names>]
---

## Context

<Project description. Why is full-ceremony review cost greater than
 failure cost at this stage? Who is affected by defects that slip?
 What will trigger a revisit?>

## Decision

We will run the Shredder workflow in `"lightweight"` mode.
Bishop and Tiger Claw run only on slices Shredder tags
`review_required: true`. All other gates run on every slice.

## Alternatives

1. **Full pipeline.** Rejected at this stage — review cost currently
   exceeds failure cost for a <size / audience / sensitivity>
   project.
2. **No pipeline (ad-hoc development).** Rejected — we lose
   structural conformance, scope enforcement, and showpieces, which
   are cheap and high-value even in prototype work.

## Consequences

- Positive: faster iteration; lower review overhead on trivial
  slices.
- Negative: defects in low-tagged slices may reach runtime; their
  cost is on us.
- Neutral: tag criteria are encoded in
  `guides/pipeline-modes.md`; Shredder applies them mechanically.

## Compliance / validation

- Every slice in the queue carries an explicit
  `review_required: true | false` tag; Shredder's output is the
  record.
- A quarterly review reopens this ADR when the project outgrows
  prototype posture (team size, user count, production dependency).

## Follow-ups

- Add a cadence to re-evaluate this ADR at <milestone or date>.
```

Upgrading back to full mode is the same operation in reverse — supersede the ADR.

## Gotchas

- **Lightweight is not "skip review."** Every criterion above is a real risk class. A slice that trips any criterion gets the full treatment, even in lightweight mode — that is not optional.
- **The tag is Shredder's call, not the author's.** An executor agent who would *prefer* review should say so explicitly, and Shredder will tag; but the author cannot demand skipping review on a slice that trips a criterion.
- **Do not silently downgrade mid-project.** If failure cost has risen (more users, production dependency, regulatory attention), the ADR must be superseded before the mode switches — otherwise the project has a stale decision record.
- **Structural, scope, and showpiece enforcement never change.** Karai's conformance check runs; Traag's scope guard runs; the author's showpiece (Threat Model / Algorithmic Proof / Data Model Analysis / Observability Plan / Documentation Brief) runs. Lightweight does not remove any of these.
- **Security and data work always reviewed.** No amount of project smallness justifies skipping review of auth / session / token / schema-migration / PII-handling slices. The criteria encode this.

## Revision triggers

Re-open `.claude/workflow.json` (and the ADR) when:

- Team size crosses a threshold (solo → 2+).
- A defect reaches production that a review would have caught.
- The project transitions from prototype to shipped product.
- External dependencies grow (public API, first paying customer, regulatory regime).
- The tag criteria above need adjustment for the project's actual risk profile.
