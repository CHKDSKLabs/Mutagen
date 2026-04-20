# Authoring & Review Guides

Short, practical companions to the templates in [`../templates/`](../templates/). The templates define the **shape** of each document; these guides define **how to fill them well** and **how to review them**.

## When to read these

- You are about to author one of the five upstream documents (PRD, ADR, DDD, ISC, DSD) and the template alone leaves you wondering *"what makes this section good?"*.
- You are reviewing a draft and want a checklist.
- You are operating [April](../agents/April.md) and want to understand the quality bar she is working toward.
- You are deciding whether a change warrants reopening a document.

## The five guides

| Doc | Guide | Template |
|-----|-------|----------|
| PRD | [`PRD-guide.md`](PRD-guide.md) | [`templates/PRD-template.md`](../templates/PRD-template.md) |
| ADR | [`ADR-guide.md`](ADR-guide.md) | [`templates/ADR-template.md`](../templates/ADR-template.md) |
| DDD | [`DDD-guide.md`](DDD-guide.md) | [`templates/DDD-template.md`](../templates/DDD-template.md) |
| ISC | [`ISC-guide.md`](ISC-guide.md) | [`templates/ISC-template.md`](../templates/ISC-template.md) |
| DSD | [`DSD-guide.md`](DSD-guide.md) | [`templates/DSD-template.md`](../templates/DSD-template.md) |

## Shared principles

These apply to every document and are not repeated in each guide.

1. **Every document is a source of truth for something specific.** If two documents claim to be authoritative on the same concern, split the concern or rewrite one as a reference.
2. **Numbered IDs are contracts.** `[FR-*]`, `[NFR-*]`, `[INV-*]`, `[POL-*]`, `ISC-NNN`, `ADR-NNNN`, `[DSD-###]` — every downstream agent cites these verbatim. Never renumber casually. New IDs are additive; old IDs live on in a deprecation note.
3. **`<TBD>` is honest; fabrication is a hazard.** A document with accurate `<TBD>` markers is a better starting point than a document that quietly invents details.
4. **Status is part of the document.** `Draft` → `In Review` → `Approved` (or `Accepted` for ADR / ISC). A document without a status is a document a reader will misuse.
5. **Change log matters.** Every edit adds a line. The change log is the thinnest possible audit trail — do not skip it.
6. **Cross-references are first-class.** A PRD `[FR-7]` realized by `ADR-003` and backed by `ISC-012` is a three-way bond; keep it explicit. Broken cross-references are defects.
7. **Ubiquitous language comes from the DDD.** If the PRD and the DDD disagree on a term, the DDD wins after the first approval — update the PRD. Before the first DDD approval, the PRD's language seeds the DDD.
8. **The document exists to serve downstream agents.** If a section cannot be cited by Shredder, Karai, Bishop, Tiger Claw, or an execution agent, ask why it is there.

## A note on process

All five documents are authored under the same rhythm:

1. **Draft** — assemble enough content that a reviewer can meaningfully react. Gaps marked `<TBD>` with owners and due dates.
2. **In Review** — named reviewers walk the draft against this guide's review checklist.
3. **Approved** — the owner marks approval; the document is now citable downstream.
4. **Revision** — the document is reopened when a revision trigger fires (see per-guide triggers). Reopening does not delete history; it adds to it.

[April](../agents/April.md) can run this rhythm for you via interview. If you author without her, these guides are what she would have referenced.

---

**A document is not finished when it runs out of things to add. It is finished when every claim inside it is one a downstream agent could act on without asking.**
