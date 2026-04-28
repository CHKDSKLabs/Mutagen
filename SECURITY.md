# Security Policy

## Supported Versions

Only the latest minor version on `main` is supported. Older versions on
historical tags will not receive security fixes; upgrade.

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Reporting a Vulnerability

**Do not open a public GitHub issue for a security vulnerability.**

Email a description of the issue to the address listed on the maintainer's
GitHub profile (https://github.com/ObtuseAglet). Please include:

- A description of the vulnerability and its impact.
- Steps to reproduce, including a minimal example if possible.
- The plugin version and host (Claude Code / Codex) you observed it on.
- Whether you have a suggested fix.

You should expect an initial acknowledgement within seven days. A fix
timeline depends on severity and complexity, but the maintainers will
coordinate disclosure with you before any public announcement.

## Scope

In scope:

- The Rust harness binary under `harness/`.
- The shell wrappers under `plugins/mutagen/scripts/` and
  `plugins/mutagen/bin/`.
- The PreToolUse scope-enforcement hook.
- The agent persona prompts (insofar as a malformed persona could escalate
  scope).

Out of scope:

- Issues in upstream dependencies (Claude Code, Codex, the Anthropic /
  OpenAI APIs themselves) — report those to the relevant vendor.
- Issues that require an attacker to already have local filesystem access
  to the workspace state directory (`.mutagen/state/`). That directory is
  trusted by design.
- Theoretical attacks that require modifying the agent persona files
  themselves. Those files are part of the plugin's trusted code path.

## What counts as a vulnerability

- Scope-enforcement bypass — a slice writes outside its declared write
  globs without triggering the PreToolUse hook.
- Prompt injection that causes an agent to skip a verification step,
  silently widen scope, or skip a structural check.
- Path traversal in any of the script wrappers (`agent.sh`,
  `dispatch_stage.sh`, `run_execute_next.sh`, etc.).
- Command injection through queue contents, slice IDs, agent names, or
  any other field that the harness consumes.
- Disclosure of credentials (Pushover token, host API keys) through
  log output, error messages, or persisted state.
