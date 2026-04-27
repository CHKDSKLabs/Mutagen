# Dev Deployment

This is the boring, useful path for running the harness as a development
console instead of a loose pile of commands.

The embedded HTTP dashboard (`project dashboard-serve`, the
`scripts/dashboard_dev.sh` and `scripts/dev_console.sh` wrappers,
`/mutagen:dashboard`) has been retired. Operator control runs through the
CLI surface: `/mutagen:execute-next`, `/mutagen:status`, `/mutagen:pause`,
`/mutagen:resume`, and `/mutagen:amend-scope`. The JSON snapshot
`project dashboard` is still available for any future UI layer.

## Target Shape

The first deployment target is intentionally small:

- one harness process
- one workspace per process
- local machine first
- plugin scripts stay the main entrypoint

That keeps the runtime honest while we learn where the sharp edges actually are.

## What Gets Installed

For local development, we ship three things together:

1. the Rust harness binary
2. the plugin wrapper scripts
3. the workspace under active development

## Runtime Contract

The development deployment expects:

- workspace state under `.mutagen/`, `docs/`, and `slices/`
- the harness binary at `plugins/mutagen/bin/mutagen-harness` when packaged
- the fallback source manifest at `harness/Cargo.toml` when running from a checkout
- `bash`, `git`, and `jq` on `PATH`
- Rust installed when the packaged binary is absent and the wrapper needs to build one

## Common Commands

Build a packaged binary:

```bash
bash plugins/mutagen/scripts/build_harness_binary.sh --debug
```

Run the dev doctor (checks workspace + tooling):

```bash
bash plugins/mutagen/scripts/doctor_dev.sh --workspace-root /path/to/workspace
```

Load a project dashboard JSON snapshot:

```bash
bash plugins/mutagen/scripts/project.sh dashboard --workspace-root /path/to/workspace
```

Run the workflow loop:

```bash
bash plugins/mutagen/scripts/run_execute_next.sh --workspace-root /path/to/workspace --host claude
```

Pause / resume the loop at a stage boundary:

```bash
bash plugins/mutagen/scripts/pause.sh on  --reason "investigating L4-World-004"
bash plugins/mutagen/scripts/pause.sh off
```

## Troubleshooting

### Packaged binary missing

`build_harness_binary.sh` will build one. If Rust is missing, install it or
point `MUTAGEN_HARNESS_BIN` at an already-built binary.

### Workspace not initialized

Create the project capsule first:

```bash
bash plugins/mutagen/scripts/project.sh init \
  --workspace-root /path/to/workspace \
  --name demo-app \
  --stack vite-express-sqlite \
  --design-system plain-css
```

### Doctor says tools are missing

That means the doctor is being truthful, which is rude but useful. Install the
missing stack toolchain or pick a stack whose requirements already exist on the
machine.

## CI Skeleton

The repo includes a starter workflow at
[.github/workflows/harness-dev.yml](/mnt/c/Users/spork/dev/agentic_design_workflow/.github/workflows/harness-dev.yml).
It does three useful things:

1. checks Rust formatting
2. syntax-checks the plugin shell wrappers
3. runs the full harness test suite
