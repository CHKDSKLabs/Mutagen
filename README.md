<p align="center">
  <img src="assets/mutagen.png" alt="Mutagen — Rust harness for Claude + Codex" width="320">
</p>

# Mutagen

A Rust harness that runs an agentic design workflow on top of Claude Code and
Codex CLI. Mutagen owns the things prompts can't be trusted with — queue
selection, stage transitions, scope enforcement, evidence bundling, retry
policy, and persisted verdicts — and hands the rest to a fixed cast of
personas (April → Shredder → Karai → Bebop / Baxter / Krang / Chaplin / Tatsu /
Metalhead → Bishop → Tiger Claw → Splinter). The pipeline turns a five-document
upstream design bundle (PRD / ADR / DDD / ISC / DSD) into dependency-ordered
slices, dispatches each slice to the right executor, and gates the result on
adversarial review before it touches the queue.

If a behavior matters, the harness enforces it or records it. If the only
control is "the prompt said pretty please," that is not a control plane.

## Install

Mutagen ships as a plugin so the same workflow drops into both supported hosts.

### Claude Code

```
/plugin marketplace add CHKDSKLabs/Mutagen
/plugin install mutagen@mutagen-marketplace
```

Verify with `/plugin marketplace list` and `/plugin list`.

### Codex CLI

Clone this repo, then register the marketplace entry at
`~/.agents/plugins/marketplace.json` (or use the repo-local
`.agents/plugins/marketplace.json` that ships here), pointing at
`plugins/mutagen/`. Export `MUTAGEN_ROOT` so skill bodies can resolve it:

```bash
export MUTAGEN_ROOT="/absolute/path/to/Mutagen/plugins/mutagen"
```

Codex auto-discovers skills under `plugins/mutagen/skills/<name>/SKILL.md`.
Invoke with `$mutagen-slice`, `$mutagen-execute-next`, `$mutagen-status`,
`$mutagen-amend-scope`, `$mutagen-elicit`, `$mutagen-consolidate-advisories`,
`$mutagen-setup-pushover`, `$mutagen-pause`, or `$mutagen-resume`. All nine
skills are configured with `allow_implicit_invocation: false` — Mutagen is a
workflow, not a helpful tool, so explicit invocation is the only trigger.

**Known degradation on Codex:** the `codex_hooks` feature is still under
development and disabled on Windows, so the plugin doesn't ship manifest-level
hooks. Scope manifests are written between stages for audit and visibility, but
enforcement is advisory (the agent is told its allowed globs; nothing blocks
it). Reviewers are the backstop.

## What you get

- **Thirteen personas** with bounded mandates — interviewer, architect,
  dispatcher, six executors split by layer, two reviewers, a doc author, and a
  scope guardian.
- **Nine commands / skills** matching across Claude Code and Codex (elicit,
  slice, execute-next, amend-scope, status, consolidate-advisories,
  setup-pushover, pause, resume).
- **A `PreToolUse` scope-enforcement hook** *(Claude Code only)* that blocks
  writes outside the active slice's manifest before they happen.
- **A canonical Rust runtime** (`mutagen-harness`) that owns queue mutation,
  evidence assembly, structural checks, retry policy, and verdict persistence —
  so behavior doesn't change just because you switched hosts.
- **Five upstream design templates** (PRD / ADR / DDD / ISC / DSD) plus
  authoring and review guides.
- **Optional Pushover notifications** for halts, scope violations, and
  retry-budget exhaustion.

For the full feature surface see [`plugins/mutagen/README.md`](plugins/mutagen/README.md).
For harness internals see [`harness/README.md`](harness/README.md).

For a populated reference workspace — five upstream design documents, a slice
queue, and a Tiger Claw review report in their canonical filesystem layout —
see [`examples/orders-demo/`](examples/orders-demo/).

## Repository layout

```
.
├── .claude-plugin/marketplace.json     # Claude Code marketplace manifest
├── .agents/plugins/marketplace.json    # Codex marketplace registration
├── plugins/mutagen/                    # the plugin (dual-host)
│   ├── .claude-plugin/plugin.json
│   ├── .codex-plugin/plugin.json
│   ├── agents/                         # 13 personas
│   ├── commands/                       # 9 Claude slash commands
│   ├── skills/                         # 9 Codex skills ($mutagen-*)
│   ├── bin/                            # host adapters + harness launcher
│   ├── hooks/                          # Claude PreToolUse + PostToolUse
│   ├── scripts/                        # wrappers, dispatch glue, notify
│   ├── templates/                      # PRD / ADR / DDD / ISC / DSD
│   └── guides/                         # authoring & review guides
├── harness/                            # mutagen-harness Rust crate
└── examples/orders-demo/               # populated reference workspace
```

## Validation

```bash
claude plugin validate .
```

Codex has no equivalent validator today; manually confirm
`plugins/mutagen/.codex-plugin/plugin.json` parses and that every skill
directory contains a `SKILL.md`.

## License

MIT.
