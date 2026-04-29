# Changelog

## 0.3.0

Public-readiness pass. Harness dispatch hardening, the embedded HTTP dashboard
retired, and everything a public OSS repo needs (LICENSE, CONTRIBUTING,
SECURITY, CODE_OF_CONDUCT, cross-platform release workflow). The plugin
surface is unchanged for anyone already on `0.2.x` other than the dashboard
removal and two new commands; consumer workflows that drove `/mutagen:execute-next`
keep working.

### Added

- **`/mutagen:pause` and `/mutagen:resume` (Claude) plus `$mutagen-pause` and `$mutagen-resume` (Codex).** Stage-boundary pause for the execute-next loop via a `.mutagen/state/pause.json` sentinel. Resume is the operator counterpart that handles the four-step manual recovery (structural-check → update-queue → transition-active-slice → dispatch-stage) in one call after a hand-repaired author output. Brings the plugin to nine commands and nine skills, with full host parity.
- **`bin/claude-harness.sh` non-interactive Claude wrapper.** Wraps `claude --print --permission-mode bypassPermissions` so a Rust-harness dispatch never stalls on a permission prompt. `harness_runtime.sh` defaults `CLAUDE_BIN` to it when present; `agent.sh` calls it directly when `--host claude` is selected.
- **`examples/orders-demo/`.** A populated reference workspace — five upstream design docs, a slice queue with two pending slices, and a Tiger Claw review report — laid out exactly the way a real consumer workspace looks. Useful for new users and for plugin contributors who need a fixture to exercise the pipeline against.
- **Release infrastructure.** `.github/workflows/release.yml` cross-compiles the harness on tag push (`v*`) for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`, attaches archives plus `.sha256` checksums to the matching GitHub Release, and auto-generates release notes.
- **Standard public-OSS files.** Top-level `LICENSE` (MIT), `.gitignore`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`. The plugin claimed MIT in its manifest but had no LICENSE file in the repo before this.

### Changed

- **The embedded HTTP dashboard is retired.** ~2,900 lines of `dashboard_server.rs`, plus `scripts/dashboard_dev.sh`, `scripts/dev_console.sh`, `commands/dashboard.md`, and the `tiny_http` dependency are gone. Operator control is the CLI surface (`/mutagen:execute-next`, `/mutagen:status`, `/mutagen:pause`, `/mutagen:resume`, `/mutagen:amend-scope`). The read-only `project dashboard` JSON snapshot subcommand is still available for any future UI layer.
- **Persona body parser fixed in `agent.sh`.** Previously toggled on every `---`, so a Markdown horizontal rule inside a persona body got treated as frontmatter and corrupted the prompt. Now strips only the first YAML frontmatter block.
- **`finalize_slice` gates on `human_check_needed`.** When a slice declares `human_check_needed.required: true` and `resolved_at` is empty, finalize bails instead of silently completing. `update-slice` gains `--resolve-human-check` (stamps now), `--human-check-resolved-at <ISO>`, and `--clear-human-check-resolved-at`. Replaces the older advisory-only behaviour where the gate was documented but unenforced.
- **`SliceStatus` CLI normalisation.** The clap `ValueEnum` derive now uses `rename_all = "snake_case"` to match the on-disk format. CLI accepts `--status in_progress` (the same form the queue stores), no longer the historical kebab-case shadow.
- **Stronger Baxter output discipline.** First non-blank stdout line must be the execution header; State Update is a fenced markdown block; success closes with one canonical completion marker. Prevents the partial-artifact dispatches that surfaced on `L4-World-004` in the previous run.
- **Zero clippy warnings.** Six remaining lints (`large_enum_variant` on the two `Ready` result enums, `too_many_arguments` on four function signatures) cleaned via boxed-flattened structs and bundled-arg context structs. CI now runs `cargo clippy --all-targets -- -D warnings`; any future warning fails the build.
- **`harness_runtime.sh` resolution chain documented and clarified.** The 47 MB precompiled Linux x86_64 binary previously committed at `plugins/mutagen/bin/mutagen-harness` is gone — it was wrong-architecture for half the audience and bloated clones. The plugin's `.gitignore` covers the path so a local `build_harness_binary.sh --release` doesn't accidentally re-track it. Pre-built binaries for all five supported targets ship as Release assets going forward.
- **Plugin identity moved to CHKDSK Labs for the public release.** `author`, `homepage`, and `repository` fields across `.claude-plugin/marketplace.json`, `plugins/mutagen/.claude-plugin/plugin.json`, and `plugins/mutagen/.codex-plugin/plugin.json` now point at `CHKDSK Labs` / `https://github.com/CHKDSKLabs/Mutagen`. `LICENSE` copyright, `SECURITY.md` and `CODE_OF_CONDUCT.md` reporting addresses, and the marketplace install commands in both READMEs follow. The `interface.developerName` field on the Codex side was already `CHKDSK Labs`. Internal-development releases tracked under `ObtuseAglet/agentic_design_workflow` remain reachable for history.

### Fixed

- **`slice_loc.sh` on greenfield repos.** Previously fell back to `HEAD^` unconditionally, so a freshly-`git init`-ed workspace with no commits reported `added: 0` for every slice. Now walks a fallback chain: saved start-of-slice ref → `HEAD^` (if a parent commit exists) → empty-tree object (with a sweep over untracked in-scope files). Reports `base_mode` so the caller knows whether the LOC delta was measured against a real base or the empty tree.
- **Stale documentation.** README claims of "six slash commands" and "all six skills" were two minor versions stale by the time we got to them. `/mutagen:consolidate-advisories` was implemented but missing from the command table. Six markdown links pointed at hardcoded `/mnt/c/Users/spork/...` paths from somebody's WSL workspace. All swept in this release.

### Removed

- `harness/src/dashboard_server.rs` and the `tiny_http` dependency.
- `plugins/mutagen/scripts/dashboard_dev.sh`, `plugins/mutagen/scripts/dev_console.sh`.
- `plugins/mutagen/commands/dashboard.md` and `/mutagen:dashboard`.
- `project dashboard-serve` CLI subcommand. (`project dashboard` for the JSON snapshot is unchanged.)
- Empty stub file `.codex` at the repo root (no documented purpose; verified via repo-wide grep).
- The Linux-x86_64-only precompiled `mutagen-harness` binary that was previously tracked under `plugins/mutagen/bin/`.

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
