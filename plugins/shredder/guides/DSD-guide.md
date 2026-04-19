# DSD — Authoring & Review Guide

Companion to [`templates/DSD-template.md`](../templates/DSD-template.md). Read [`guides/README.md`](README.md) first for the shared principles.

## What the DSD is for

The **cross-cutting conventions** every slice must conform to — brand, voice, content style, visual language, interaction patterns, accessibility, code style, API conventions, logging and privacy guardrails. Numbered `[DSD-###]` rules so that execution agents, reviewers, and QA can cite them verbatim. A living document that runs in parallel with everything else and binds every surface the product exposes.

A DSD that describes without enforcing is a style guide; a DSD that enforces is a contract. The goal is the contract.

## When to author

- **Initiated alongside the PRD.** DSD does not wait for ADR / DDD / ISC to stabilize — brand voice, accessibility target, and code-style choices can begin day one.
- **Continuous.** The DSD is the most obviously living of the five documents. Every release-boundary review should check for drift.
- **Before the first slice ships.** Shredder's Readiness Check will not accept a missing DSD. Downstream agents cite `[DSD-###]` on every slice; they need numbered rules to cite.

## Who authors

- Primary owner: a design-systems lead (or the engineering lead in smaller teams).
- Co-authors: design (brand, visual, interaction), content (voice, terminology, microcopy), engineering (code style, API conventions, logging), accessibility lead (when present).
- [April](../agents/April.md) can elicit the initial draft, but DSD ownership tends to remain joint across design + engineering indefinitely.

## Authoring process

Approach in roughly this order; later sections may reveal earlier gaps.

1. **Scope & applicability.** Which product surfaces does the DSD govern (UI, CLI, API, logs, email, marketing site, internal tooling)? Which does it not? Declare these explicitly — silence is ambiguity.
2. **Conformance levels.** Explain how to read MUST / SHOULD / MAY before the first numbered rule so reviewers have shared semantics.
3. **Brand & voice.** Three to five brand attributes. One voice. A **tone matrix** — how voice flexes by context (onboarding, success, error, destructive confirmation, security, empty state).
4. **Content style.** Writing rules (case, person, tense, exclamation marks), terminology table (domain terms are the DDD's; product-surface terms are the DSD's), numbers / dates / units, microcopy patterns (empty state, destructive confirmation, recoverable error, success toast, loading), localization.
5. **Visual language.** Only if there's a visual surface. Treat the design tokens repo as authoritative; the DSD indexes it, doesn't duplicate values. Color semantics before palette; typography scale; spacing base; radius; elevation; motion tokens.
6. **Layout & components.** Breakpoints, grids, component inventory with status (GA / Beta / Deprecated). Canonical component library is authoritative; the DSD names rules of use.
7. **Interaction patterns.** Navigation, forms, feedback, destructive actions, empty / loading / error / populated states. Every data surface has all four states; name the rule.
8. **Motion.** Tokenised durations; `prefers-reduced-motion` respected; skip-long-animation rule.
9. **Accessibility.** Target level (WCAG 2.2 AA is a reasonable floor). Keyboard reachability, focus state, contrast, labels, alt text, semantic landmarks, live regions. All MUST by default.
10. **Code style.** Formatter / linter references (authoritative), naming rules beyond the formatter, boolean-as-predicate discipline.
11. **API conventions.** Endpoint paths, field casing (pick one, forever), timestamps (ISO-8601 UTC with Z), identifier formats (ULID / UUIDv7 / etc.), error-response shape, pagination.
12. **Log & telemetry style.** Structured logs, correlation IDs on every line, PII redaction via the logger factory (not per call site).
13. **Commits / branches / PRs.** Conventional commits (or project convention), branch name pattern, PR description requirements (traces-to upstream docs).
14. **Testing conventions.** Layout, naming, coverage expectations.
15. **Governance.** Change process, semver, review cadence, ownership per area.
16. **Privacy & safety guardrails.** Cross-cutting rules no slice can waive without security review.
17. **Do / don't.** A short list of the stuff that comes up most often in review. Keep tight.

**Number every rule** (`[DSD-001]`, `[DSD-002]`, …). Downstream agents cite by number; renumbering is a breaking change.

## What "good" looks like

- **Every rule is numbered and cite-able.** No unnumbered prose with enforceable intent.
- **Conformance level is explicit on every rule.** MUST / SHOULD / MAY, not implied by tone.
- **Tooling is named for every MUST.** If the rule cannot be enforced by a formatter, linter, test, or CI check, either add the tool or consider whether the rule is a MUST at all.
- **Applicability matrix is filled in.** Each product surface is either in scope or explicitly out.
- **Tokens / linters / component library are treated as authoritative.** The DSD **describes**; code **enforces**.
- **Brand voice and tone matrix are distinct.** Voice is constant; tone flexes by context.
- **Accessibility rules are MUST, with a target level named.**
- **API conventions commit to one choice per concern** — casing, timestamp, identifier, error shape, pagination. The DSD does not offer options; it picks.
- **Privacy guardrails are non-waivable without security review.** Say so explicitly.
- **Semver versioning is maintained.** Breaking changes bump MAJOR; additive rules bump MINOR; clarifications bump PATCH.
- **`Last reviewed` date is current.**

## Review checklist

- [ ] Is the applicability matrix complete (every surface marked Y / N with a reason when N)?
- [ ] Do conformance levels (MUST / SHOULD / MAY) precede the first numbered rule?
- [ ] Is every rule numbered with a stable `[DSD-###]` ID?
- [ ] Does every MUST have a tooling or review mechanism named?
- [ ] Are brand voice and tone matrix both present and distinct?
- [ ] Is the content style table internally consistent (case, tense, person)?
- [ ] Does the DDD ubiquitous language take precedence over the DSD terminology table for domain terms?
- [ ] Is the accessibility target named and set to MUST?
- [ ] Do API conventions commit to one choice per concern (casing, timestamp, ID format, error shape, pagination)?
- [ ] Is log structure defined with correlation-ID requirement and redaction?
- [ ] Is governance (change process, semver, review cadence, ownership) filled in?
- [ ] Are privacy guardrails marked non-waivable?
- [ ] Is the `Last reviewed` date within the project's cadence?
- [ ] Are tokens / linters / component library referenced as authoritative rather than duplicated inline?

## Common pitfalls

- **Vague rules.** *"Use clear names."* Remedy: specify the rule (boolean-as-predicate; no single-letter vars outside tight loops; DDD terms verbatim) and cite a linter where one exists.
- **No enforcement mechanism for MUSTs.** The rule is stated; nothing checks it. Remedy: add lint, test, or CI check; otherwise demote to SHOULD.
- **DSD as dumping ground.** Everything the team wants to codify lands here, including things that belong in ADR (technology choice) or ISC (invariant with failure mode). Remedy: route each rule to its correct home.
- **Duplicated token / lint values.** The DSD says *"primary is `#2563eb`"* and the tokens repo also does. The two drift. Remedy: DSD references; tokens own the value.
- **Options instead of choices.** *"Use camelCase or snake_case."* Remedy: pick one.
- **Accessibility treated as SHOULD.** If WCAG 2.2 AA is the floor, it's a MUST. Remedy: upgrade and enforce.
- **Rule IDs renumbered on refactor.** Downstream citations break silently. Remedy: additive only; deprecate with a pointer; never renumber.
- **No governance.** Who owns a rule? Who reviews changes? Unclear. Remedy: ownership table + change process + semver.
- **Versioning skipped.** Rules change; nothing records the break. Remedy: MAJOR / MINOR / PATCH on every change; change log updated.
- **Tone matrix missing.** Voice is constant; tone is not. An error message in playful voice is a trust problem. Remedy: a short tone matrix by context.

## Revision triggers

The DSD is the most frequently revised document. Reopen (and bump version) when:

- A new product surface is added (CLI, new API, new email template, internationalised UI).
- A design token changes (color, typography, spacing). MAJOR if semantics change; MINOR if additive.
- A new pattern is needed (e.g. first time a product has destructive confirmation; first time a product emits webhooks and needs signature-verification conventions).
- Accessibility floor changes (WCAG 2.2 → 2.3).
- A lint rule is added or removed.
- A recurring code-review comment can be codified as a rule — that is the point of the DSD.
- A privacy / compliance regime shift introduces new redaction or handling requirements.
- A downstream agent (Bishop, Tiger Claw, Tatsu) repeatedly flags an issue that could be prevented by a DSD rule — promote the pattern.

Every revision updates the change log with the version, the rule IDs affected, and the reason. Numbered IDs are additive: new rules get new numbers; deprecated rules are kept with a pointer to their replacement.
