# PRD: <Title>

*Product Requirements Document. Answers **what** is being built, **for whom**, and **why**. Upstream source of truth for the ADR, DDD, ISC, and DSD.*

## Metadata

| Field | Value |
|-------|-------|
| PRD ID | PRD-NNNN |
| Status | Draft / In Review / Approved / Superseded |
| Owner | <name> |
| Authors | <names> |
| Stakeholders | <names / roles> |
| Created | YYYY-MM-DD |
| Last updated | YYYY-MM-DD |
| Target release | <version or date> |
| Related ADRs | ADR-NNNN, ... |
| Related DDD | <link> |
| Related ISC | <link> |
| Related DSD | <link> |

## 1. Summary

*One paragraph. What this is, who it is for, and what outcome it produces. A reader should be able to stop here and know whether to keep reading.*

## 2. Problem & Background

*What is broken, missing, or worth doing. Include evidence: data, user quotes, support tickets, competitive pressure. State the cost of doing nothing.*

## 3. Users & Personas

*Primary persona, secondary personas, and anti-personas (who this is explicitly **not** for). For each, list the job-to-be-done this PRD addresses.*

| Persona | Job to be done | Current workaround | Pain |
|---------|----------------|--------------------|------|

## 4. Goals

*Measurable outcomes this PRD delivers. Not features — outcomes.*

- G1. ...
- G2. ...

## 5. Non-goals

*Explicitly out of scope. Decisions deferred. Guards against scope creep. Each entry should be something a reasonable reader might assume is in scope.*

- NG1. ...
- NG2. ...

## 6. Success Metrics

*How we will know, post-launch, that the goals were achieved. Prefer leading indicators alongside lagging ones.*

| Metric | Baseline | Target | How measured | Goal ref |
|--------|----------|--------|--------------|----------|

## 7. Requirements

### 7.1 Functional

*Numbered so downstream docs (ADR, DDD, ISC) can cite specific requirements. Use MUST / SHOULD / MAY language.*

- [FR-1] The system MUST ...
- [FR-2] The system SHOULD ...

### 7.2 Non-functional

*Performance, reliability, security, privacy, accessibility, observability, compliance, localization. Make each testable.*

- [NFR-1] Performance: ...
- [NFR-2] Security & privacy: ...
- [NFR-3] Accessibility: ...
- [NFR-4] Reliability / availability: ...
- [NFR-5] Observability: ...
- [NFR-6] Compliance / regulatory: ...

## 8. User Experience

*High-level flows, not pixel-level design. Reference the DSD for visual and interaction conventions. Include wireframes or link to them.*

## 9. Constraints & Assumptions

*Budget, timeline, platform, legal, and organizational constraints. Assumptions that must hold for this PRD to remain valid — if an assumption breaks, the PRD must be revisited.*

- C1. ...
- A1. ...

## 10. Dependencies

*Upstream teams, external systems, prerequisite work, third-party services. Name the owner and the hand-off artifact.*

| Dependency | Owner | Needed by | Status |
|------------|-------|-----------|--------|

## 11. Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|

## 12. Open Questions

*Unresolved items that block the ADR or DDD. Each question must have an owner and a due date; questions without both are blockers, not open questions.*

| # | Question | Owner | Due | Blocks |
|---|----------|-------|-----|--------|

## 13. Release Criteria

*What must be true to ship. Bind each criterion to a requirement ID and, where possible, an automated test.*

- [ ] All [FR-*] MUST requirements implemented and tested
- [ ] All [NFR-*] targets measured in staging
- [ ] Success metrics instrumented
- [ ] Rollback plan documented

## 14. Rollout & Post-launch

*Phased rollout, feature flags, telemetry to watch, kill switches, support readiness, post-launch review date.*

## 15. Change Log

| Date | Author | Change |
|------|--------|--------|
