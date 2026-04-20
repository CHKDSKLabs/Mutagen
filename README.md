# agentic_design_workflow — mutagen marketplace

A Claude Code plugin marketplace. Ships one plugin today — **`mutagen`** — with room for companions alongside it.

## Install

Inside a Claude Code session:

```
/plugin marketplace add ObtuseAglet/agentic_design_workflow
/plugin install mutagen@mutagen-marketplace
```

Verify:

```
/plugin marketplace list
/plugin list
```

## What's in this marketplace

| Plugin | Description | Path |
|--------|-------------|------|
| [`mutagen`](plugins/mutagen/) | End-to-end agentic design workflow — thirteen subagents, six slash commands (`/mutagen:elicit`, `/mutagen:slice`, `/mutagen:execute-next`, `/mutagen:amend-scope`, `/mutagen:status`, `/mutagen:setup-pushover`), a `PreToolUse` scope-enforcement hook, optional Pushover halt notifications, five-document upstream design bundle (PRD / ADR / DDD / ISC / DSD) with templates and authoring guides. | [`plugins/mutagen/`](plugins/mutagen/) |

Each plugin has its own README with install flow, session ritual, agent roster, and configuration. See [`plugins/mutagen/README.md`](plugins/mutagen/README.md) for the full story.

## Repository layout

```
.
├── .claude-plugin/
│   └── marketplace.json            # marketplace manifest
├── plugins/
│   └── mutagen/                    # the mutagen plugin
│       ├── .claude-plugin/
│       │   └── plugin.json
│       ├── agents/                 # 13 subagents
│       ├── commands/               # 6 slash commands
│       ├── hooks/                  # PreToolUse + PostToolUse hook wiring
│       ├── scripts/                # guard.sh, counter.sh, heartbeat.sh, render_queue.sh, notify.sh
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
      "name": "mutagen",
      "source": "./plugins/mutagen",
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
