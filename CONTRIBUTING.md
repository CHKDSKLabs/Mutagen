# Contributing

Thanks for considering a contribution. The mutagen plugin is small,
opinionated, and aims to stay that way. The contributor bar is the same as
the runtime bar: be precise, cite sources, and don't widen scope silently.

## Before you open a PR

1. **File an issue first if the change is non-trivial.** Drive-by
   architectural rewrites land slowly. A two-line discussion in an issue
   saves both of us a 400-line dead-end PR.
2. **One concern per PR.** Mixing a typo fix with a behaviour change makes
   the diff hard to review and hard to revert.
3. **Run the harness tests locally.**
   ```bash
   cd harness
   cargo fmt --check
   cargo clippy --all-targets
   cargo test
   ```
   Any failure is grounds for a request to fix before merge.
4. **Run the shell wrappers' syntax check.**
   ```bash
   bash -n plugins/mutagen/scripts/*.sh
   bash -n plugins/mutagen/bin/*.sh
   ```
5. **Update the relevant doc.** If you change a CLI flag, update both the
   wrapper script's usage and the matching command/skill markdown. If you
   change the queue schema, update `harness/schemas/queue.schema.json` plus
   the user-facing guide at `plugins/mutagen/guides/queue-schema.md`.

## Style

- Rust: match the existing style. `cargo fmt` is the source of truth. New
  modules go under `harness/src/` and are exposed through `lib.rs`.
- Bash: `set -euo pipefail` unless there's an explicit reason not to (the
  `counter.sh` / `heartbeat.sh` / `status.sh` exceptions are documented in
  those files).
- Markdown: keep the existing voice in user-facing docs (RUNBOOK, command
  files, agent personas). Drop the voice in machine-readable spec files
  (QUEUE_SCHEMA, ARTIFACT_SCHEMAS, SLICEMAP_SPEC) — those are contracts.

## What is in scope

- Bug fixes in the harness binary, the shell wrappers, or the plugin
  metadata.
- New agent personas for new domains (ask in an issue first — adding a
  persona is not free, it expands the routing surface).
- New host adapters (Codex and Claude Code today). A new host needs a
  `bin/agent.sh` branch, a clap `HostKind` variant, and a host profile
  resolver.
- Documentation cleanup, including stale claims, broken links, missing
  examples.

## What is out of scope

- Resurrecting the embedded HTTP dashboard. That was retired deliberately;
  the operator surface is the CLI. A new UI layer is welcome but it should
  consume the JSON snapshot from `project dashboard`, not be embedded.
- Anything that bypasses scope enforcement (the PreToolUse hook). The
  whole point of the plugin is that scope is enforced.
- Anything that adds a hard dependency on a paid / proprietary service.

## Reporting bugs

Use the GitHub issue tracker. Please include:

- Mutagen plugin version (`plugins/mutagen/.claude-plugin/plugin.json`).
- Host (`claude` or `codex`) and host version.
- The harness binary's output (`mutagen-harness --version` if available, or
  the cargo manifest commit if running from source).
- The exact command you ran and the JSON payload you got back.

## Reporting security issues

See [SECURITY.md](SECURITY.md). Don't file public issues for vulnerabilities.

## Code of Conduct

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
