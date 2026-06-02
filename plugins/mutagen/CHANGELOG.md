# Changelog

## 0.4.0

The interactive service layer. Up to now the only way to drive the pipeline was a human at a CLI (or an orchestrator shelling out to one). 0.4.0 adds `mutagen-service` — an axum HTTP + WebSocket front door that lets a remote GUI register projects, read workflow state, issue Workflow Commands, and run a live elicitation Session over a socket — without giving up the per-project exclusion guarantees the CLI already had. The monolithic `harness/` crate is carved into `core` / `cli` / `service` so the service and the CLI link the same brains and can't drift on the state machine.

This is additive for anyone on `0.3.x`: the CLI surface, the slash commands, and the skills are unchanged. If you never start the service, nothing about your workflow changes except that State Update records now carry an `origin` field (see below).

### Added

- **`mutagen-service` — axum 0.7 HTTP/WS service.** New binary that binds a listener, serves the workflow over REST + WebSocket, and drains in-flight requests on SIGINT/SIGTERM. Fail-closed start: refuses to bind without a resolved listen address and secret source. (`L1-Infra-001`, `L1-Infra-005`)
- **Three-crate workspace carve.** `harness/` is now a Cargo workspace: `mutagen-core` (the library — runner, dispatch, finalize, state machine, structural-check, project lock/registry, sessions), `mutagen-harness` (the CLI binary), and `mutagen-service`. Plus the `xtask` crate for build tooling. CLI and service link the same `core`, so there is exactly one implementation of the state machine. (`L1-Infra-001`, ADR-0003)
- **Service config loader.** Runtime config (listen address, log level, secret source) from `<project_root>/.mutagen/service.toml` with `MUTAGEN_SERVICE_{LISTEN,LOG_LEVEL,SECRET_PATH}` env overrides. Env beats file; a missing required field is an `Err`, never a silent permissive default. (`L1-Infra-002`)
- **Shared-secret bearer auth.** Every route except the `{/health, /version, /openapi.json}` allowlist requires a bearer token. `BearerToken` and `Secret` are value objects wrapping `Vec<u8>` with private fields and redacting `Debug` — direct equality doesn't compile, and the secret can't accidentally land in a log line. The verifier returns an `AuthOutcome`, never a bare `bool`. (`L3-Auth-001`, `L3-Auth-002`)
- **Project Registry.** Multiple projects behind one service. On-disk TOML at `<service_data>/projects.toml` with canonical-path enforcement and duplicate-root rejection, exposed over `POST/GET/DELETE /projects`. (`L2-Project-001`, `L4-Project-001`)
- **Project Lock — cross-process exclusion.** File-based advisory lock at `<root>/.mutagen/state/project.lock` with PID-based crash detection and a holder-identity payload. The CLI and the service are now symmetric: whoever grabs the lock for a project wins; the other backs off. Per-command acquisition on the write endpoints means a long-running `dispatch-next` holds the lock for its full duration by design. (`L2-Workflow-001`, `L6-Release-001`)
- **Workflow read endpoints.** `GET /projects/{id}/status`, `/slices`, `/state-log` over `core`'s read side. (`L4-Workflow-001`)
- **Workflow Command write endpoints.** `POST .../dispatch-next`, `.../slices/{id}/accept`, `.../escalate`, `.../finalize`, `.../resume` — each acquires the Project Lock, writes a State Update tagged with its origin, and returns `202 Accepted`. (`L4-Workflow-002`)
- **Live elicitation Sessions over WebSocket.** `/projects/{project_id}/session` upgrades to a WS connection (through the auth middleware), enforces one active Session per project (a second attempt gets `409` with the live `session_id`), and carries the Question Envelope / Answer round-trip so a GUI can run April's interview over the socket. Workflow Commands issued from a Session get two-stage `command.accepted` feedback, and `slice.transitioned` / `cohort.*` events broadcast to every Session on the project. (`L4-Session-001/002/003`)
- **`GET /version`.** Unauthenticated. Reports service + harness + chat-protocol versions so a GUI can negotiate compatibility before authing. (`L4-Edge-001`)
- **Error Envelope + DTO module.** Canonical error shape and the `dto/` module that every handler serializes through. (`L4-Edge-001`)
- **OpenAPI spec + drift gate.** `docs/openapi.json` is generated from utoipa annotations via `cargo xtask openapi`; a CI test fails the build if the committed spec drifts from the derived output, and a smoke step confirms the spec is parseable by `openapi-typescript` so the downstream GUI repo can codegen against it. WebSocket message shapes (which utoipa can't fully describe) are hand-authored in `harness/service/docs/websocket.md` with an `externalDocs` pointer from the stub path. (`L1-Infra-003`, `L1-Infra-003r`, `L6-Release-002`)
- **Structured logging.** `tracing-subscriber` JSON output, a request span topology, and a `request_id` tower middleware so every line out of a handler carries its `request_id`. Secret-named fields are filtered. (`L1-Infra-005`)
- **Workspace lints + CI matrix.** `unwrap`/`expect`/`panic` are denied in `harness/service/src/**` via clippy config, and the OpenAPI-drift and CLI/service-exclusion tests are wired into the Linux/macOS/Windows CI matrix. (`L1-Infra-004`)

### Changed

- **State Update records carry a mandatory `origin` field.** Every State Update now records who wrote it — `cli:<pid>` from the CLI, `service:<session-or-request-id>` from the service. The append-only writer, the replay validator, and an `Origin` value object (whose constructor forbids empty strings) all live in `core`. **Backward-compat:** pre-0.4.0 records missing `origin` are tolerated on replay; post-0.4.0 records missing it fail closed. A user who upgrades mid-run is fine; a user who *downgrades* will hit a parser that doesn't recognize the field. (`L2-Workflow-002`, `L4-Workflow-001`, ISC-007)
- **Service config resolves DSD §14 Q2 in-slice** as TOML + env overrides (ADV-002). CLI logging adoption (Q1) stays deferred — the CLI keeps its current `eprintln!` shape until 0.5.0.

### Fixed

Seven harness self-healing fixes, every one a real incident the execution loop tripped over while building this release and then patched so it can't recur:

- **`finalize_slice` now verifies `write_set` artifacts exist on disk.** The 2026-05-05 `L1-Infra-003` incident finalized clean — TigerClaw clean, queue marked complete — with *zero* files actually written; the whole xtask crate existed only inside the author-output markdown. Finalize now fails closed with `write_set_artifacts_missing` if any declared path is absent. `--accept-missing-artifacts` overrides it but records an auditable `slice.finalize_artifacts_overridden` State Update. (`L1-Harness-002`; this is also why `L1-Infra-003r` exists — the re-land of the artifacts that never persisted.)
- **TigerClaw verdict parser accepts any heading depth.** It required exactly `#### Verdict`; TigerClaw kept emitting `## Verdict`, so the parser never entered section mode and the harness errored out — three times in one cohort, each needing a manual `update-slice --tiger-claw clean`. Now any-depth `# Verdict` anchors the section. (`L1-Harness-003`)
- **`review-decision` honors a fresh QA verdict over a stale cached one.** A retry could re-review `clean` while the queue's cached `defect` from the prior run still won, leaving the slice wrongly escalated. The freshly-parsed verdict now wins and updates the queue. (`L1-Harness-004`)
- **Structural-check accepts `**Bold Heading**` as equivalent to `## Heading`.** An author emitted complete, contract-faithful content under bold section markers and got six bogus "missing required heading" findings. The parser is the lower-friction fix site, not the persona doc. (`L1-Harness-005`)
- **State Update fenced-block parser tolerates one metadata line before the slice marker.** A `key: value` line ahead of the `### <slice-id> — <date>` marker no longer rejects the whole block; the marker may be the first non-blank line or the second if the first is a single metadata pair. Marker-uniqueness still enforced. (`L1-Harness-006`)
- **Persona-drift gate regression-tested for the zero-byte and unbordered-content paths.** Pins the gate's behavior for a literal empty author file and for output that exceeds the char threshold with zero canonical headings — the shape behind the 2026-05-12 `L4-Workflow-001` escalation that surfaced 10 contract-violation findings on a recently-empty file. (`L1-Harness-001`)
- **Reconciled a stale structural-check test left behind by `L1-Harness-005`.** That slice loosened the required-section gate to accept `**Bold**` markers but never removed an older test asserting a bold `**State Update**` marker must fail *that* gate — so the test went red the moment the slice landed and rode the tree until a full `cargo test --workspace` caught it. Resolution: the rejection guarantee is real but it now belongs to the State Update *block parser* (which still demands a `#`-prefixed heading + slice marker), not the required-section gate. Test re-pointed at the `state_block` finding; the bold marker is asserted to satisfy `required_section` so L1-Harness-005 can't silently regress.

### Notes

- **No UI in this repo** (Constraint C2). The downstream GUI repo owns the interface and codegens its client against the committed `docs/openapi.json` + `websocket.md`. 0.4.0 ships the contract and the server; the GUI is a separate deliverable.
- **One release-criteria test is still `#[ignore]`d:** `cli_exits_78_with_documented_message_when_service_holds_lock`. The exclusion *primitive* is shipped and tested; un-ignoring it waits on CLI lock acquisition wiring through `transition_active_slice` / `apply_state_update_for_slice`. Tracked, not forgotten.
- **Known platform gap:** the project-*preview* lifecycle tests in `core` spawn `bash -c` and don't pass on a native-Windows host (no regression — they predate the carve and only moved crates). They run green in the Linux/macOS CI legs.

## 0.3.3

April-resumability patch. A fresh April spawn can now reconstruct the full elicitation trail from disk instead of trusting the parent agent to re-pass conversational context. Fixes the leaky-abstraction case where a parent dropped April's `agentId`, the harness happily started a fresh instance, and April re-interviewed the user from kickoff because no continuity primitive survived the dispatch boundary.

### Added

- **Elicitation checkpoint at `.mutagen/state/elicitation.jsonl`.** Append-only JSONL — one record per April turn. Schema: `ts`, `turn`, `mode`, `user_message_summary`, `drafted_paths`, `defaults_filled`, `questions_asked`, `answers_recorded`, `open_tbds`, `consistency_flags`, `readiness_brief_emitted`. Turn numbers are monotonic. April writes one line per turn as her last action; a fresh April reads every line on entry and reconstructs the room from it.
- **Fourth April mode: `resume`.** Distinct from kickoff / gap-fill / iteration. Triggered when a checkpoint exists. April reads the full trail, opens with a one-line acknowledgement (*"Picking up from turn N — last we left it, …"*), and continues in the appropriate sub-mode without re-interviewing on already-answered questions.
- **`$mutagen-status` reports checkpoint state.** New "Elicitation checkpoint" block surfaces total turns, last turn / mode / timestamp, last user message summary, unanswered questions (set diff of `questions_asked` minus `answers_recorded[].q` across all turns), open `<TBD>`s from the latest record, whether the latest turn emitted a Readiness Brief, and a malformed-line count if any line failed to parse. JSON output gains an `elicitation_checkpoint` object.
- **Malformed-checkpoint warning in `next_actions`.** If `status.sh` finds unparseable lines, the next-actions list flags it as a recoverability gap that must be repaired before the next April turn — the next fresh spawn cannot resume cleanly through corrupt records.

### Changed

- **April's frontmatter `description` advertises statelessness.** Now explicitly says: stateless across invocations; continuity comes from `.mutagen/state/elicitation.jsonl`; the parent must either pass full context every call or trust the checkpoint. Stops parent agents from assuming subagent identity persists across dispatches.
- **`$mutagen-elicit` preflight reads the checkpoint first.** Computes `last_turn`, `mode_history`, `unanswered_questions`, `open_tbds`, `last_user_message_summary` and passes the summary into April's framing prompt. Mode decision now elevates `resume` above the document-presence heuristics — checkpoint wins when it exists.
- **`$mutagen-elicit` post-return verifies the checkpoint was appended.** If April returned without writing a new record, the skill surfaces it to the user as a recoverability gap rather than silently leaving the next spawn unable to resume.

### Notes

- This is the on-disk durability half of the fix. The other half — a real `SendMessage`/`ResumeAgent` tool surface in the parent runtime that re-enters the same agent instance with preserved context — lives upstream of this repo. The checkpoint makes a fresh April spawn behave indistinguishably from a resumed instance, which is the user-visible property that matters.
- The checkpoint pattern is currently April-only. If long-lived state matters for other agents (Shredder mid-validation, Karai mid-cohort), the same JSONL-append discipline can be lifted out of April's persona section into a shared contract — deferred until a second agent demonstrably needs it.

## 0.3.2

UX/install patch. End users no longer need a Rust toolchain on a fresh host, and Windows clones stop mangling shell scripts into CRLF.

### Added

- **Auto-provisioned harness binary.** New `plugins/mutagen/scripts/fetch_harness_binary.sh` (POSIX) and `fetch_harness_binary.ps1` (PowerShell) detect the host triple via `uname -sm` (or `RuntimeInformation.OSArchitecture`), pull the matching per-target archive plus `.sha256` from the GitHub Release, verify the checksum, and extract `mutagen-harness(.exe)` into `plugins/mutagen/bin/`. Idempotent against a `.harness-version` sidecar so a second invocation is a no-op. Pure curl/tar/unzip on POSIX and `Invoke-WebRequest`/`Get-FileHash`/`Expand-Archive` on Windows — no jq, cargo, python, or any other new runtime dependency.
- **Background fetch on plugin load.** A new `SessionStart` hook (`scripts/session_start_fetch_harness.sh`) backgrounds the fetch script when the binary is missing so the user's first real harness call doesn't pay the download latency. Non-blocking, swallows errors, opt-out via `MUTAGEN_NO_AUTOFETCH=1`.
- **Repo-wide `.gitattributes`.** Forces `*.sh`/`*.bash` to LF and `*.ps1`/`*.cmd`/`*.bat` to CRLF on checkout, so Windows clones with `core.autocrlf=true` no longer turn shell scripts into CRLF and break hook execution under git-bash.

### Changed

- **`harness_runtime.sh` falls forward to auto-fetch.** When the binary is missing AND no `harness/Cargo.toml` is reachable AND cargo isn't on PATH, the runtime now invokes `fetch_harness_binary.sh` once before giving up. The "harness unavailable" error message now points users at the fetch script and at `MUTAGEN_HARNESS_BIN` for offline installs.
- **PostToolUse hook drops the `sh ` wrapper.** `hooks/hooks.json` now invokes `counter.sh` directly through its shebang. Sidesteps a Windows hook-runner edge case that produced `/usr/bin/sh: /usr/bin/sh: cannot execute binary file` on every tool call.

### Notes

- **No Rust toolchain required for end users.** A fresh Linux / macOS / Windows host with only curl/tar/unzip (or pwsh built-ins) can install and run the plugin end-to-end. Rust is still needed for plugin contributors who want to rebuild the harness from source.

## 0.3.1

Field-feedback patch release. Eight defects from the first external 0.2.3 run, fixed against the 0.3.x line.

### Added

- **`/mutagen-harness resume-slice` (and `plugins/mutagen/scripts/resume_slice.sh`).** Force-reset the active slice to a given slice id and stage. Rebuilds `active-slice.json` and the evidence bundle from the queue row, claims status to `in_progress`, and pivots the orchestrator to the requested stage. Refuses on terminal statuses (`completed`, `escalated`, `refused`) so a closed slice can't be silently reopened. Surfaces the prior active slice id and stage in the JSON result.

### Changed

- **Citation resolver loosened against descriptive prefixes and parentheticals.** Strips role-prefixes (`Cross-cutting:`, `NFR:`), leading section markers (`§4`, `§4.2`), and trailing parentheticals (`(§4 note: …)`). Tries literal then canonical forms. Heading match is bidirectional with a 2-word floor on the reverse direction, so single-word headings can't grab unrelated long citations. Errors include the canonical form when it differs from the original.
- **ADR resolution falls back to a consolidated `docs/ADR.md` (or top-level `ADR.md`).** When no per-file `ADR-*.md` candidate matches, the resolver now extracts the section by heading the same way DDD/ISC/DSD already do.
- **Author dispatch prompt now embeds the persona's structural-check contract verbatim.** `required_sections_for_author` in `structural.rs` is now the single source of truth; `render_author_prompt` consumes it. Prevents the failure mode where an author produces reasonable-looking output that the structural check then rejects for missing a marker nobody told them about.
- **State Update marker errors surface a worked example and diagnose three common shapes.** Marker placed before the fenced block, marker prefixed with `+`/`-`/`@@` from a unified-diff render, marker buried after narrative inside the fence. Replaces the previous one-line "must start with a slice marker" rejection.
- **`update_queue_slice.sh` and `run_execute_next.sh` forward harness errors instead of swallowing them.** When the harness exits non-zero with structured JSON, that JSON is forwarded; non-JSON stderr is wrapped into a `detail` field. Same pattern applied to the `render_queue` failure branch.
- **`run_execute_next.sh` auto-reruns the validator on `queue_validation_stale`.** Capped at 2 retries. Writes the fresh report to `.mutagen/state/queue-validation.json` and continues. If the validator itself fails or rejects the queue, that surfaces as `queue_validation_rerun_failed` with the validator's actual output embedded — no more loop-on-stale-forever.

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
- **Plugin identity moved to CHKDSK Labs for the public release.** `author`, `homepage`, and `repository` fields across `.claude-plugin/marketplace.json`, `plugins/mutagen/.claude-plugin/plugin.json`, and `plugins/mutagen/.codex-plugin/plugin.json` now point at `CHKDSK Labs` / `https://github.com/CHKDSKLabs/Mutagen`. `LICENSE` copyright, `SECURITY.md` and `CODE_OF_CONDUCT.md` reporting addresses, and the marketplace install commands in both READMEs follow. The `interface.developerName` field on the Codex side was already `CHKDSK Labs`. Internal-development releases tracked under `CHKDSKLabs/Mutagen` remain reachable for history.

### Fixed

- **`slice_loc.sh` on greenfield repos.** Previously fell back to `HEAD^` unconditionally, so a freshly-`git init`-ed workspace with no commits reported `added: 0` for every slice. Now walks a fallback chain: saved start-of-slice ref → `HEAD^` (if a parent commit exists) → empty-tree object (with a sweep over untracked in-scope files). Reports `base_mode` so the caller knows whether the LOC delta was measured against a real base or the empty tree.
- **Stale documentation.** README claims of "six slash commands" and "all six skills" were two minor versions stale by the time we got to them. `/mutagen:consolidate-advisories` was implemented but missing from the command table. Six markdown links pointed at hardcoded absolute paths from somebody's local WSL workspace. All swept in this release.

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
