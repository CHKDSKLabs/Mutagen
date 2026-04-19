---
description: Run April to interview the user and author the five upstream design documents (PRD, ADR, DDD, ISC, DSD).
---

# Elicit — run April on the upstream design bundle

The user has invoked `/shredder:elicit`. You are orchestrating the April subagent to produce or iterate the five upstream design documents.

## Preflight

1. Ensure the project's state directory exists: `mkdir -p .claude/state`.
2. Detect whether any upstream documents already exist. Check these paths and report what is present:
   - `docs/PRD.md` or `PRD.md`
   - `docs/ADR/**` or `docs/ADR-*.md` or repo-root `ADR-*.md`
   - `docs/DDD.md` or `DDD.md`
   - `docs/ISC.md` or `ISC.md`
   - `docs/DSD.md` or `DSD.md`
3. Decide the mode:
   - **Kickoff** — none of the five exist yet.
   - **Gap-fill** — some exist with `<TBD>` markers or missing sections.
   - **Iteration** — user has a specific document or section they want to revise (ask if unclear from `$ARGUMENTS`).
4. Write the active-slice state file so the PreToolUse guard allows April's writes:

   ```json
   {
     "slice_id": "elicit-{YYYY-MM-DD-HHMM}",
     "author_agent": "April",
     "mode": "kickoff | gap-fill | iteration",
     "allowed_write_globs": [
       "docs/PRD*",
       "docs/PRD/**",
       "docs/ADR*",
       "docs/ADR/**",
       "docs/DDD*",
       "docs/DDD/**",
       "docs/ISC*",
       "docs/ISC/**",
       "docs/DSD*",
       "docs/DSD/**",
       "PRD*.md",
       "ADR*.md",
       "DDD*.md",
       "ISC*.md",
       "DSD*.md",
       "design/**",
       ".claude/state/**"
     ]
   }
   ```

   Write it to `.claude/state/active-slice.json`. The guard.sh hook reads this file on every Write / Edit.

## Dispatch

Spawn the April subagent via the Agent tool with:

- `subagent_type`: `April` (the plugin provides this subagent).
- A prompt that (a) names the detected mode, (b) passes `$ARGUMENTS` as user intent if provided, (c) lists the documents currently present with their status, (d) reminds April to use the templates at `${CLAUDE_PLUGIN_ROOT}/templates/` as scaffolds and the guides at `${CLAUDE_PLUGIN_ROOT}/guides/` as the quality bar, and (e) asks her to produce the opening interview turn, or — in iteration mode — to act directly on the user's stated change.

## After April returns

1. Surface April's **Readiness Brief** (or interview turn) to the user verbatim.
2. **Do not advance** to Shredder without explicit user instruction. April hands to the user; the user hands to Shredder.
3. Leave `.claude/state/active-slice.json` in place — a follow-up April turn in the same session reuses it. When the user says the bundle is ready to slice, `/shredder:slice` will overwrite the state file with Shredder's scope.

## Reminders

- Never invent domain details the user has not given you. Every unknown is `<TBD>` with an owner and a due date.
- Preserve the user's exact vocabulary — ubiquitous language starts here.
- Cross-check every turn: does what the user just said contradict an earlier statement or a drafted document? If so, surface the mismatch.
- Templates live at `${CLAUDE_PLUGIN_ROOT}/templates/`; never mutate them.
- April reads the repo; she writes only the instantiated design docs.

$ARGUMENTS
