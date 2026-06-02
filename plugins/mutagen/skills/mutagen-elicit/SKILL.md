---
name: mutagen-elicit
description: Explicit invocation only. Run April to interview the user and author the five upstream design documents (PRD, ADR, DDD, ISC, DSD).
---

# $mutagen-elicit — run April on the upstream design bundle

Orchestrate the April persona to produce or iterate the five upstream
design documents.

## Preflight

1. `mkdir -p .mutagen/state`.
2. Detect which upstream documents already exist. Report what's present:
   - `docs/PRD.md` or `PRD.md`
   - `docs/ADR/**` or `docs/ADR-*.md` or repo-root `ADR-*.md`
   - `docs/DDD.md` or `DDD.md`
   - `docs/ISC.md` or `ISC.md`
   - `docs/DSD.md` or `DSD.md`
3. Read the elicitation checkpoint at `.mutagen/state/elicitation.jsonl` if it
   exists. The presence of this file means a prior April turn was checkpointed
   and a fresh April instance must resume from it rather than re-interview.
   Compute:
   - `last_turn` — the highest `turn` number on file.
   - `mode_history` — the `mode` value per turn, in order.
   - `open_tbds` — union of `open_tbds` from the latest record.
   - `unanswered_questions` — questions present in any prior `questions_asked`
     but absent from any subsequent `answers_recorded`.
   - `last_user_message_summary` — from the most recent record.
4. Decide mode:
   - **Resume** — `.mutagen/state/elicitation.jsonl` exists and has at least
     one record. This wins over the document-presence heuristics below; the
     checkpoint is canonical for "where we left off."
   - **Kickoff** — no checkpoint AND none of the five docs exist yet.
   - **Gap-fill** — no checkpoint, some docs exist with `<TBD>` or missing
     sections.
   - **Iteration** — user has a specific document or section to revise (with
     or without a checkpoint; iteration is intent-driven, not state-driven).
5. Write the active-slice state file (advisory — no hook enforces it, but
   `$mutagen-status` and `$mutagen-amend-scope` read it):

   ```json
   {
     "slice_id": "elicit-{YYYY-MM-DD-HHMM}",
     "author_agent": "April",
     "active_agent": "April",
     "stage": "elicit",
     "mode": "kickoff | gap-fill | iteration",
     "attempts": 0,
     "allowed_write_globs": [
       "docs/PRD*", "docs/PRD/**",
       "docs/ADR*", "docs/ADR/**",
       "docs/DDD*", "docs/DDD/**",
       "docs/ISC*", "docs/ISC/**",
       "docs/DSD*", "docs/DSD/**",
       "PRD*.md", "ADR*.md", "DDD*.md", "ISC*.md", "DSD*.md",
       "design/**",
       ".mutagen/state/**"
     ]
   }
   ```

## Dispatch

```bash
bash "$MUTAGEN_ROOT/bin/agent.sh" April "$(cat <<'PROMPT'
Detected mode: kickoff | gap-fill | iteration | resume

User intent (from prompt args): <paste if any>

Documents currently present with status:
  <enumerate>

Checkpoint state (.mutagen/state/elicitation.jsonl):
  - last_turn: <N or "none">
  - mode_history: <list or "none">
  - last_user_message_summary: <one line or "none">
  - unanswered_questions: <list or "none">
  - open_tbds: <list or "none">
  Full file path: .mutagen/state/elicitation.jsonl
  (Read every line yourself before deciding what to do this turn — this
   summary is a hint, not a substitute. If mode=resume, treat the
   checkpoint as canonical for prior-turn state.)

Templates: $MUTAGEN_ROOT/templates/   (scaffolds — never mutate)
Guides:    $MUTAGEN_ROOT/guides/       (quality bar)

Tasks:
- Kickoff: open the interview.
- Gap-fill: name the top-priority gap and ask about it.
- Iteration: act directly on the stated change.
- Resume: read every line of .mutagen/state/elicitation.jsonl, acknowledge
  the resume in one line, then continue in the appropriate sub-mode
  (gap-fill or iteration) — do not re-interview on already-answered
  questions.

Append exactly one JSON record to .mutagen/state/elicitation.jsonl as the
last action of this turn. Schema is in your persona's Checkpoint Discipline
section. Turn number = (highest existing turn) + 1, or 1 if file is empty.

Whenever the bundle is at or near handoff condition, include your
**Readiness Brief** per your Output Format so we can persist it, and set
`readiness_brief_emitted: true` on this turn's checkpoint record.

Your advisory write scope: the `allowed_write_globs` in
.mutagen/state/active-slice.json. Do not write outside those globs.
PROMPT
)"
```

## After April returns

1. Surface April's Readiness Brief (or interview turn) to the user verbatim.
2. **Verify the checkpoint was appended.** Read the last line of
   `.mutagen/state/elicitation.jsonl` and confirm its `turn` is greater than
   the `last_turn` you computed at preflight. If April did not append a
   record, surface this to the user as a recoverability gap before
   continuing — the next April spawn will not be able to resume cleanly.
3. **Persist the Readiness Brief** whenever April produced one:
   - `.mutagen/state/readiness-brief.md` — full markdown verbatim.
   - `.mutagen/state/readiness-brief.json` — structured summary:
     ```json
     {
       "date": "YYYY-MM-DD",
       "generated_by": "April",
       "mode": "kickoff | gap-fill | iteration",
       "documents": {
         "prd": { "status": "Draft|In Review|Approved|Missing", "tbd_count": 0, "blocking": false },
         "adr": { "status": "...", "tbd_count": 0, "blocking": false },
         "ddd": { "status": "...", "tbd_count": 0, "blocking": false },
         "isc": { "status": "...", "tbd_count": 0, "blocking": false },
         "dsd": { "status": "...", "tbd_count": 0, "blocking": false }
       },
       "cross_consistency": [
         { "docs": ["PRD", "DDD"], "summary": "..." }
       ],
       "shredder_readiness": {
         "prd": "green|yellow|red",
         "adr": "green|yellow|red",
         "ddd": "green|yellow|red",
         "isc": "green|yellow|red",
         "dsd": "green|yellow|red"
       },
       "recommendation": "Ready for Shredder | Ready after N items | Not yet — ..."
     }
     ```
   If April produced only an interview turn, leave the readiness-brief state
   files untouched. The elicitation checkpoint (`elicitation.jsonl`) is
   updated every turn regardless.
4. **Do not advance to Shredder** without explicit user instruction. April
   hands to the user; the user hands to Shredder.
5. Leave `.mutagen/state/active-slice.json` and
   `.mutagen/state/elicitation.jsonl` in place — a follow-up April turn
   reuses both. When the user says the bundle is ready, `$mutagen-slice`
   will overwrite the active-slice file with Shredder's scope; the
   elicitation checkpoint is preserved as historical record.

## Reminders

- Never invent domain details. Every unknown is `<TBD>` with an owner and
  a due date.
- Preserve the user's exact vocabulary — ubiquitous language starts here.
- Cross-check every turn for contradictions with earlier statements or
  drafted documents.
- Templates live at `$MUTAGEN_ROOT/templates/`; never mutate them.
- April reads the repo; she writes only the instantiated design docs.
