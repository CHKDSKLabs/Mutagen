# agentic_design_workflow — shredder marketplace

A Claude Code plugin marketplace. Ships one plugin today — **`shredder`** — with room for companions alongside it.

## Install

Inside a Claude Code session:

```
/plugin marketplace add ObtuseAglet/agentic_design_workflow
/plugin install shredder@shredder-marketplace
```

Verify:

```
/plugin marketplace list
/plugin list
```

## What's in this marketplace

| Plugin | Description | Path |
|--------|-------------|------|
| [`shredder`](plugins/shredder/) | End-to-end agentic design workflow — thirteen subagents, four slash commands (`/shredder:elicit`, `/shredder:slice`, `/shredder:execute-next`, `/shredder:status`), a `PreToolUse` scope-enforcement hook, five-document upstream design bundle (PRD / ADR / DDD / ISC / DSD) with templates and authoring guides. | [`plugins/shredder/`](plugins/shredder/) |

Each plugin has its own README with install flow, session ritual, agent roster, and configuration. See [`plugins/shredder/README.md`](plugins/shredder/README.md) for the full story.

## Repository layout

```
.
├── .claude-plugin/
│   └── marketplace.json            # marketplace manifest
├── plugins/
│   └── shredder/                   # the shredder plugin
│       ├── .claude-plugin/
│       │   └── plugin.json
│       ├── agents/                 # 13 subagents
│       ├── commands/               # 4 slash commands
│       ├── hooks/                  # PreToolUse hook wiring
│       ├── scripts/                # guard.sh (scope enforcer)
│       ├── templates/              # PRD / ADR / DDD / ISC / DSD templates
│       ├── guides/                 # authoring & review guides
│       └── README.md
└── README.md                       # this file
```

## Contributing a companion plugin

New plugins live under `plugins/<name>/` with their own `.claude-plugin/plugin.json`. Add an entry to [`.claude-plugin/marketplace.json`](.claude-plugin/marketplace.json):

```json
{
  "plugins": [
    {
      "name": "shredder",
      "source": "./plugins/shredder",
      "version": "0.1.0"
    },
    {
      "name": "your-plugin",
      "source": "./plugins/your-plugin",
      "version": "0.1.0"
    }
  ]
}
```

Keep the marketplace manifest versioned in semver; bump the plugin's version field when publishing changes or clients won't notice the update.

## Validation

```bash
# Validate the marketplace manifest and every plugin under plugins/
claude plugin validate .
```

## License

MIT.
