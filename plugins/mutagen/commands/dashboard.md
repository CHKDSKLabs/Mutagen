---
description: Launch the Mutagen local development console for the current workspace.
---

# Dashboard

The user has invoked `/mutagen:dashboard`.

This command is the front door for the local Mutagen harness console. Use the
packaged wrapper instead of manually reassembling the launch flow.

## Run

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/dev_console.sh" --workspace-root "$PWD"
```

If the current working directory is not the intended workspace root, substitute
the correct path explicitly.

## What It Does

The wrapper:

1. runs the local deployment doctor
2. checks the workspace capsule
3. builds a packaged harness binary when one is missing
4. launches `project dashboard-serve` with the configured defaults

Treat the wrapper output as authoritative. If it says the workspace is missing
`.mutagen/project.json`, believe it. The harness is not playing hard to get.
