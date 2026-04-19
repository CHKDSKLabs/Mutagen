# DSD: <Product or Initiative Name>

*Design Style Guide. The cross-cutting conventions — brand, content, visual, interaction, accessibility, and code — that every slice must conform to, regardless of which bounded context it lives in. A **living document**: versioned, continuously evolving, and binding on every surface the product exposes.*

## Metadata

| Field | Value |
|-------|-------|
| Document ID | DSD-NNNN |
| Version | MAJOR.MINOR (semver — breaking token changes bump MAJOR) |
| Status | Draft / In Review / Approved / Frozen |
| Owner | <design systems lead or equivalent> |
| Authors | <names> |
| Contributors | <names> |
| Created | YYYY-MM-DD |
| Last reviewed | YYYY-MM-DD |
| Next review due | YYYY-MM-DD |
| Related PRD | PRD-NNNN |
| Related ADRs | ADR-NNNN, ... |
| Related DDD | DDD-NNNN (ubiquitous language is canonical) |
| Related ISC | <link> |
| Authoritative sources | *design tokens repo, lint config, component library — the DSD describes, these enforce* |

## 1. Summary & Scope

*One paragraph. What product surfaces this guide governs (UI, CLI, API responses, docs, emails, logs). What it does **not** govern.*

### 1.1 Applicability

| Surface | In scope? | Notes |
|---------|-----------|-------|
| Web UI | Y / N | |
| Mobile UI | Y / N | |
| CLI output | Y / N | |
| API responses & errors | Y / N | |
| Log lines | Y / N | |
| Email / notifications | Y / N | |
| Marketing site | Y / N | |
| Internal tooling | Y / N | |

### 1.2 Conformance levels

*How to read the rules below.*

- **MUST / MUST NOT** — required. A slice that violates a MUST rule fails review.
- **SHOULD / SHOULD NOT** — recommended. Deviations require a documented justification in the slice PR.
- **MAY** — optional; provided for consistency when the decision arises.

*Rules are numbered (e.g. `[DSD-###]`) so slices, ISCs, and review checklists can cite them.*

## 2. Brand & Voice

### 2.1 Brand attributes

*Three to five adjectives that describe the product's personality. These are the anchors every downstream choice traces back to.*

- ...

### 2.2 Voice

*The product's constant personality — how it sounds regardless of context.*

### 2.3 Tone matrix

*How voice flexes by context. A product with a playful voice still gets serious during a security error.*

| Context | Tone | Example |
|---------|------|---------|
| First-run / onboarding | | |
| Success confirmation | | |
| Informational | | |
| Recoverable error | | |
| Destructive confirmation | | |
| Security / compliance | | |
| Empty state | | |

## 3. Content Style

### 3.1 Writing rules

- [DSD-001] MUST use sentence case for <headings / buttons / labels — pick one and commit>.
- [DSD-002] MUST use present tense and second person ("you") in user-facing copy.
- [DSD-003] MUST NOT use jargon not defined in the DDD ubiquitous language.
- [DSD-004] SHOULD prefer active voice over passive.
- [DSD-005] MUST avoid exclamation marks outside of celebratory success states.

### 3.2 Terminology

*The DDD is the source of truth for domain terms. This section lists product-surface vocabulary that isn't in the DDD and the canonical spelling/casing of terms that are.*

| Term | Use | Do not use | Notes |
|------|-----|------------|-------|

### 3.3 Numbers, dates, units

- [DSD-010] Dates in UI MUST render as `<format>` in the user's locale.
- [DSD-011] Durations MUST use `<format>`; never raw seconds.
- [DSD-012] Currency MUST render with explicit ISO code when ambiguous.

### 3.4 Microcopy patterns

*Canonical patterns for the copy that appears in the same place on every screen. Keep patterns short and copy-pasteable.*

| Pattern | Template | Example |
|---------|----------|---------|
| Empty state | `<headline> · <action>` | |
| Destructive confirmation | `Delete <thing>?` + consequence sentence | |
| Recoverable error | `<what happened> · <what to do>` | |
| Success toast | `<past-tense verb> <thing>` | |
| Loading | `<verb>ing <thing>…` | |

### 3.5 Localization

- [DSD-020] Strings MUST flow through the i18n layer; no hard-coded user-facing text.
- [DSD-021] Layouts MUST accommodate string expansion of at least <X%>.
- [DSD-022] RTL support: required / best-effort / out of scope.

## 4. Visual Language

*Delete this section if the product has no visual surface.*

### 4.1 Design tokens

*The token repo is authoritative. This table is the index; values live in code.*

| Token family | Examples | Source |
|--------------|----------|--------|
| Color — semantic | `bg.default`, `fg.muted`, `border.danger` | `<link>` |
| Color — palette | `brand.primary.500`, `neutral.900` | `<link>` |
| Typography | `type.body.md`, `type.display.lg` | `<link>` |
| Spacing | `space.1` … `space.12` (4-px base) | `<link>` |
| Radius | `radius.sm` … `radius.full` | `<link>` |
| Elevation / shadow | `elevation.1` … `elevation.4` | `<link>` |
| Motion | `motion.fast`, `motion.base`, `motion.slow` | `<link>` |

### 4.2 Color

- [DSD-100] MUST use semantic tokens (`bg.danger`) not palette tokens (`red.500`) in component code.
- [DSD-101] Foreground/background pairs MUST meet WCAG 2.2 AA contrast (4.5:1 text, 3:1 large text and non-text).
- [DSD-102] MUST NOT communicate state with color alone; pair with icon or label.

### 4.3 Typography

*Scale, weights, line-height ratios, fallback stacks.*

### 4.4 Iconography

- [DSD-110] Icons MUST come from the sanctioned set at `<path>`; no inline SVGs in feature code.
- [DSD-111] Icons MUST have an accessible name (visible label or `aria-label`).

### 4.5 Imagery & illustration

*Style, aspect ratios, licensing rules, alt-text requirements.*

## 5. Layout & Grid

*Breakpoints, grid, gutters, container widths, safe areas.*

| Breakpoint | Min width | Columns | Gutter |
|------------|-----------|---------|--------|

## 6. Components

*The component library is authoritative; this section is the index and the rules of use.*

### 6.1 Component inventory

| Component | Status (GA / Beta / Deprecated) | Replaces | Link |
|-----------|----------------------------------|----------|------|

### 6.2 Rules of use

- [DSD-200] New UI MUST compose library components before introducing primitives.
- [DSD-201] Bespoke variants MUST be proposed via the governance process (§11) before shipping.
- [DSD-202] Deprecated components MUST NOT appear in new slices.

### 6.3 Per-component anatomy

*For each component: purpose, anatomy, required states (default / hover / focus / active / disabled / loading / error / empty / selected), props, do's and don'ts, accessibility notes.*

## 7. Interaction Patterns

### 7.1 Navigation

*Top-level IA, back behavior, breadcrumbs, deep-linking, URL conventions.*

### 7.2 Forms & input

- [DSD-300] Labels MUST be persistent (no placeholder-as-label).
- [DSD-301] Validation MUST run on blur, not on every keystroke, except for format-constrained fields.
- [DSD-302] Error messages MUST state what is wrong and how to fix it.

### 7.3 Feedback & state

- [DSD-310] Every async action MUST show a loading state within 100ms.
- [DSD-311] Operations longer than 1s MUST show determinate progress where possible.
- [DSD-312] Success MUST be confirmed visibly; silent success is not success.

### 7.4 Destructive actions

- [DSD-320] Destructive actions MUST require explicit confirmation.
- [DSD-321] Confirmation copy MUST name the object and the consequence.
- [DSD-322] Where feasible, destructive actions MUST offer undo in addition to (or in place of) a confirm prompt.

### 7.5 Empty, loading, and error states

*Every data-bearing surface has four states: loading, empty, populated, error. All four MUST be designed — not just the happy path.*

## 8. Motion

- [DSD-400] Motion MUST use tokenized durations and easings; no ad-hoc timing.
- [DSD-401] Motion MUST respect `prefers-reduced-motion`.
- [DSD-402] Animations longer than 400ms MUST have a way to skip or reduce.

## 9. Accessibility

*Accessibility is a floor, not a feature. All rules here are MUST unless explicitly stated.*

- [DSD-500] Conformance target: WCAG 2.2 AA.
- [DSD-501] Every interactive element MUST be reachable and operable by keyboard alone.
- [DSD-502] Focus state MUST be visible and meet 3:1 contrast against adjacent colors.
- [DSD-503] Color contrast MUST meet WCAG AA (see [DSD-101]).
- [DSD-504] Every form control MUST have a programmatic label.
- [DSD-505] Every image and icon MUST have appropriate alt text or be marked decorative.
- [DSD-506] Dynamic content changes MUST be announced to assistive tech via appropriate live regions.
- [DSD-507] Page structure MUST use semantic landmarks and a single `h1`.

## 10. Code Style

### 10.1 Formatting & linting

*Authoritative config, not duplicated prose.*

| Language | Formatter | Linter | Config |
|----------|-----------|--------|--------|

- [DSD-600] Code MUST pass the formatter and linter in CI before merge.
- [DSD-601] Lint rules MUST NOT be disabled inline without a comment citing a DSD or ISC reference.

### 10.2 Naming

- [DSD-610] File names: `<case>`.
- [DSD-611] Types / classes: `<case>`.
- [DSD-612] Functions / variables: `<case>`.
- [DSD-613] Constants: `<case>`.
- [DSD-614] Boolean names MUST read as predicates (`isReady`, `hasAccess`), not nouns.
- [DSD-615] Domain terms in code MUST match the DDD ubiquitous language exactly.

### 10.3 API conventions

*Binds the ISC. Contracts defined here MUST be reflected in every externally visible API.*

- [DSD-620] Endpoint paths: `<convention>`.
- [DSD-621] Field casing in payloads: `<camelCase | snake_case>`. Pick one; do not mix.
- [DSD-622] Timestamps MUST be ISO 8601 UTC with explicit `Z`.
- [DSD-623] Identifiers crossing service boundaries MUST be <format, e.g. UUIDv7 / ULID>.
- [DSD-624] Error response shape MUST be `{ code, message, details? }`; `code` is a stable machine string.
- [DSD-625] Pagination: `<cursor | offset>`; parameters `<names>`.

### 10.4 Log & telemetry style

- [DSD-630] Logs MUST be structured (JSON), not free text.
- [DSD-631] Every log line MUST carry the trace/correlation ID.
- [DSD-632] User-identifying data MUST be redacted per the privacy rules in §12.

### 10.5 Commits, branches, PRs

- [DSD-640] Commit messages MUST follow `<Conventional Commits | project convention>`.
- [DSD-641] Branch names MUST match `<pattern>`.
- [DSD-642] PR descriptions MUST link the PRD / ADR / DDD / ISC / DSD items they touch.

### 10.6 Testing conventions

*Unit / integration / e2e split, naming, directory layout, coverage expectations.*

## 11. Governance

### 11.1 Change process

1. Propose change as a PR to this document and (where relevant) the token or component repo.
2. Design-system owner triage within <N> business days.
3. Breaking changes require a MAJOR version bump and a migration note.

### 11.2 Versioning

*Semver: MAJOR for breaking token/rule changes, MINOR for additive rules or components, PATCH for clarifications.*

### 11.3 Review cadence

*The DSD MUST be reviewed at least <quarterly>. The `Last reviewed` metadata field MUST be updated each cycle.*

### 11.4 Ownership

| Area | Owner |
|------|-------|
| Brand & voice | |
| Content style | |
| Visual tokens & components | |
| Accessibility | |
| Code style | |
| API / log conventions | |

## 12. Privacy & Safety Guardrails

*Cross-cutting rules that every slice inherits and that cannot be waived without security review.*

- [DSD-700] PII categories (name, email, phone, precise location, payment, biometric) MUST be redacted in logs and telemetry.
- [DSD-701] Secrets MUST NOT appear in source, logs, errors, or client bundles.
- [DSD-702] User-generated content rendered to other users MUST be sanitized at render time.

## 13. Do's and Don'ts

*A short, memorable list — the things that come up most often in review. Keep it tight.*

### Do
- ...

### Don't
- ...

## 14. Open Questions

| # | Question | Owner | Due | Blocks |
|---|----------|-------|-----|--------|

## 15. Change Log

*Every rule addition, change, or deprecation. Include the rule ID and version.*

| Date | Version | Author | Change |
|------|---------|--------|--------|
