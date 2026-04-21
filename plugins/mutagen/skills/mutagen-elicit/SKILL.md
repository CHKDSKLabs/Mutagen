---
name: mutagen-elicit
description: Explicit-only skill. Run April to interview the user and author the five upstream design documents (PRD, ADR, DDD, ISC, DSD). Invoke only when the user explicitly says $mutagen-elicit. Do not trigger on mentions of "design document", "PRD", etc.
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
3. Decide mode:
   - **Kickoff** — none of the five exist yet.
   - **Gap-fill** — some exist with `<TBD>` markers or missing sections.
   - **Iteration** — user has a specific document or section to revise.
4. Write the active-slice state file (advisory — no hook enforces it, but
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
Detected mode: kickoff | gap-fill | iteration

User intent (from prompt args): <paste if any>

Documents currently present with status:
  <enumerate>

Templates: $MUTAGEN_ROOT/templates/   (scaffolds — never mutate)
Guides:    $MUTAGEN_ROOT/guides/       (quality bar)

Tasks:
- Kickoff: open the interview.
- Gap-fill: name the top-priority gap and ask about it.
- Iteration: act directly on the stated change.

Whenever the bundle is at or near handoff condition, include your
**Readiness Brief** per your Output Format so we can persist it.

Your advisory write scope: the `allowed_write_globs` in
.mutagen/state/active-slice.json. Do not write outside those globs.
PROMPT
)"
```

## After April returns

1. Surface April's Readiness Brief (or interview turn) to the user verbatim.
2. **Persist the Readiness Brief** whenever April produced one:
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
   If April produced only an interview turn, leave the state files untouched.
3. **Do not advance to Shredder** without explicit user instruction. April
   hands to the user; the user hands to Shredder.
4. Leave `.mutagen/state/active-slice.json` in place — a follow-up April
   turn reuses it. When the user says the bundle is ready,
   `$mutagen-slice` will overwrite the state file with Shredder's scope.

## Reminders

- Never invent domain details. Every unknown is `<TBD>` with an owner and
  a due date.
- Preserve the user's exact vocabulary — ubiquitous language starts here.
- Cross-check every turn for contradictions with earlier statements or
  drafted documents.
- Templates live at `$MUTAGEN_ROOT/templates/`; never mutate them.
- April reads the repo; she writes only the instantiated design docs.
