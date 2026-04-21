# Changelog

## 0.1.2

Performance, ergonomics, and a couple of nasty Windows bugs.

### Fixed

- **guard.sh path normalization on Windows.** The PreToolUse guard stripped CWD from incoming paths but never normalized `\` → `/` before glob comparison, so every in-scope write got denied on Windows. Both `FILE_PATH` and `CWD` are now flattened before the prefix strip. (Symptom: the slice's own allowlist failing to match anything.)
- **WinGet jq CRLF contamination.** Windows-native jq 1.8.1 writes `\r\n` on stdout regardless of input line endings, so every `allowed_write_globs` entry was stored internally as `glob\r` and matched nothing. Strip `\r` from `author_agent` and from each glob during state-file reads.

### Changed

- **Runtime state moved out of `.claude/`.** All per-slice runtime state now lives under `.mutagen/state/**` instead of `.claude/state/**`. The `.claude/` directory triggers harness permission prompts even under bypass mode; `.mutagen/` doesn't. `.claude/workflow.json` (user config) stays put — it's touched only at setup. **Migration:** projects with an in-flight slice should `mkdir -p .mutagen && cp -r .claude/state .mutagen/` before resuming. New projects: add `.mutagen/` to `.gitignore`.
- **Per-agent model assignment.** Every agent now declares an explicit `model:`. Reasoning-heavy agents (April, Shredder, Tatsu, Baxter, Chaplin) stay on Opus; dispatch and review agents (Karai, Bishop, TigerClaw, Bebop, Krang, Metalhead, Splinter) drop to Sonnet; Traag drops to Haiku. The Bishop ∥ TigerClaw parallel review stage roughly halves in wall-clock as a result.
- **Per-agent tool restriction.** Every agent now declares an explicit `tools:`. Reviewers lose Bash/Edit; Traag is read-only.
- **Evidence Bundle pre-load in `/mutagen:execute-next`.** The orchestrator reads the upstream design bundle (PRD, ADRs, DDD, ISC, DSD) once per invocation and inlines a per-slice Evidence Bundle — verbatim excerpts of every `traces_to` citation — into every author/reviewer spawn prompt. Authors and reviewers no longer cold-load 5–14 design docs themselves; they receive the relevant fragments inline and are explicitly instructed not to re-read the bundle.
- **Agent descriptions trimmed.** Each agent's `description:` field cut by ~50–70%. The dispatcher's available-agents list is loaded on every routing decision; smaller descriptions = cheaper turns.

## 0.1.1

Plugin scaffolding, workflow phases, and the original PreToolUse scope guard. See git history for detail.

## 0.1.0

Initial plugin release.
