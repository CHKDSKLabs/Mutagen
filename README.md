# agentic_design_workflow — mutagen marketplace

A dual-harness plugin marketplace. Ships one plugin today — **`mutagen`** —
with both a Claude Code manifest and a Codex manifest under the same plugin
root, so one directory feeds both CLIs.

## Install

### Claude Code

```
/plugin marketplace add ObtuseAglet/agentic_design_workflow
/plugin install mutagen@mutagen-marketplace
```

Verify: `/plugin marketplace list` and `/plugin list`.

### Codex CLI

Clone this repo, then register the marketplace entry at
`~/.agents/plugins/marketplace.json` (or the repo-local
`.agents/plugins/marketplace.json` that ships here), pointing at the same
`plugins/mutagen/` folder. Export `MUTAGEN_ROOT` to that absolute path in
your shell rc so the skill bodies can resolve it:

```bash
export MUTAGEN_ROOT="/absolute/path/to/agentic_design_workflow/plugins/mutagen"
```

Codex discovers skills under `plugins/mutagen/skills/<name>/SKILL.md`
automatically. Invoke with `$mutagen-slice`, `$mutagen-execute-next`,
`$mutagen-status`, `$mutagen-amend-scope`, `$mutagen-elicit`, or
`$mutagen-setup-pushover`. All six skills are configured with
`allow_implicit_invocation: false` — mutagen is a workflow, not a helpful
tool, so explicit invocation is the only trigger.

**Known degradation on Codex:** the `codex_hooks` feature is still under
development and disabled on Windows, so the plugin does not ship
manifest-level hooks. Scope manifests are written between stages for audit
and visibility but enforcement is advisory (the agent is told its allowed
globs; nothing blocks it). Reviewers are the backstop.

## What's in this marketplace

| Plugin | Description |
|--------|-------------|
| [`mutagen`](plugins/mutagen/) | End-to-end agentic design workflow — thirteen personas, six commands/skills (elicit, slice, execute-next, amend-scope, status, setup-pushover), `PreToolUse` scope-enforcement hook *(Claude only)*, optional Pushover halt notifications, five-document upstream design bundle (PRD / ADR / DDD / ISC / DSD) with templates and authoring guides. |

See [`plugins/mutagen/README.md`](plugins/mutagen/README.md) for the full story.

## Repository layout

```
.
├── .claude-plugin/
│   └── marketplace.json                # Claude Code marketplace manifest
├── .agents/
│   └── plugins/
│       └── marketplace.json            # Codex marketplace registration
├── plugins/
│   └── mutagen/                        # dual-harness plugin root
│       ├── .claude-plugin/plugin.json  # Claude Code manifest
│       ├── .codex-plugin/plugin.json   # Codex manifest
│       ├── agents/                     # 13 Claude subagents + persona source-of-truth for Codex
│       ├── commands/                   # 6 Claude slash commands
│       ├── skills/                     # 6 Codex skills ($mutagen-*)
│       │   └── <skill>/SKILL.md + agents/openai.yaml
│       ├── bin/                        # agent.sh / agent.ps1 / agents-parallel.sh — host-aware persona launchers
│       ├── hooks/                      # Claude Code PreToolUse + PostToolUse
│       ├── scripts/                    # guard.sh, counter.sh, heartbeat.sh, render_queue.sh, notify.sh
│       ├── templates/                  # PRD / ADR / DDD / ISC / DSD templates
│       ├── guides/                     # authoring & review guides
│       └── README.md
└── README.md                           # this file
```

## Contributing a companion plugin

New plugins live under `plugins/<name>/` with their own manifest(s). For a
dual-harness plugin, ship both `.claude-plugin/plugin.json` and
`.codex-plugin/plugin.json`. Register in both marketplace files:

- [`.claude-plugin/marketplace.json`](.claude-plugin/marketplace.json) for Claude Code
- [`.agents/plugins/marketplace.json`](.agents/plugins/marketplace.json) for Codex

Bump the plugin's version on every release — clients cache.

## Validation

```bash
claude plugin validate .
```

Codex has no equivalent validator today; manually confirm
`plugins/mutagen/.codex-plugin/plugin.json` parses and that every skill
directory contains a `SKILL.md`.

## License

MIT.
