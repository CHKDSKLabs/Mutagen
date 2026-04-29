# Dev Deployment

This is the boring, useful path for running the harness as a development
console instead of a loose pile of commands.

## Target Shape

The first deployment target is intentionally small:

- one harness process
- one workspace per process
- local machine first
- dashboard bound to localhost by default
- plugin scripts stay the main entrypoint

That keeps the runtime honest while we learn where the sharp edges actually are.

## What Gets Installed

For local development, we ship four things together:

1. the Rust harness binary
2. the plugin wrapper scripts
3. the workspace under active development
4. the local dashboard UI served by `project dashboard-serve`

## Runtime Contract

The development deployment expects:

- workspace state under `.mutagen/`, `docs/`, and `slices/`
- the harness binary at `plugins/mutagen/bin/mutagen-harness` when packaged
- the fallback source manifest at `harness/Cargo.toml` when running from a checkout
- `bash`, `git`, and `jq` on `PATH`
- Rust installed when the packaged binary is absent and the wrapper needs to build one

The dashboard defaults live in [config/dev.toml](/mnt/c/Users/spork/dev/agentic_design_workflow/harness/config/dev.toml).

## Launch Flow

The blessed local entrypoint is:

```bash
bash plugins/mutagen/scripts/dev_console.sh --workspace-root /path/to/workspace
```

That wrapper does the small amount of housekeeping we actually want:

- runs `doctor_dev.sh` first
- resolves the workspace root
- verifies `.mutagen/project.json` exists
- builds a packaged harness binary when one is missing
- reads default bind/port/host values from `harness/config/dev.toml`
- launches `project dashboard-serve`

If you want the bare launcher without the preflight pass:

```bash
bash plugins/mutagen/scripts/dashboard_dev.sh --workspace-root /path/to/workspace
```

The companion environment check is:

```bash
bash plugins/mutagen/scripts/doctor_dev.sh --workspace-root /path/to/workspace
```

## Config Precedence

The dev launcher resolves configuration in this order:

1. CLI flags
2. environment variables
3. `harness/config/dev.toml`
4. built-in defaults

Supported environment variables:

- `MUTAGEN_WORKSPACE_ROOT`
- `MUTAGEN_DASHBOARD_BIND`
- `MUTAGEN_DASHBOARD_PORT`
- `MUTAGEN_HOST_KIND`

## Common Commands

Build a packaged binary:

```bash
bash plugins/mutagen/scripts/build_harness_binary.sh --debug
```

Launch the dashboard:

```bash
bash plugins/mutagen/scripts/dev_console.sh --workspace-root /path/to/workspace
bash plugins/mutagen/scripts/dashboard_dev.sh --workspace-root /path/to/workspace
```

Run the dev doctor:

```bash
bash plugins/mutagen/scripts/doctor_dev.sh --workspace-root /path/to/workspace
```

Launch manually without the wrapper:

```bash
bash plugins/mutagen/scripts/project.sh dashboard-serve \
  --workspace-root /path/to/workspace \
  --bind 127.0.0.1 \
  --port 7799 \
  --host stub
```

## Local Smoke Checklist

Before calling a local deployment healthy, verify:

1. the plugin wrapper launches without path errors
2. `/healthz` returns `ok`
3. `/api/dashboard` returns a project snapshot
4. `/api/activity-feed` returns a valid payload even when sparse
5. `Run Doctor` works from the UI
6. `Repair Scaffold` works from the UI when a managed file is missing
7. preview/build controls report truthful failures when tooling is missing

## Troubleshooting

### Port already in use

Launch on another port:

```bash
bash plugins/mutagen/scripts/dashboard_dev.sh \
  --workspace-root /path/to/workspace \
  --port 7801
```

### Packaged binary missing

The launcher will try to build one automatically. If Rust is missing, install
it or point `MUTAGEN_HARNESS_BIN` at an already-built binary.

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

That means the dashboard is being truthful, which is rude but useful. Install
the missing stack toolchain or use a stack whose requirements already exist on
the machine.

## Shared Dev Later

Once the local loop feels boring in the best possible way, the next step is a
single shared internal dev instance:

- one Linux VM or dev box
- one workspace mounted on disk
- one dashboard process
- reverse proxy in front
- VPN-only or basic auth access

Keep it single-workspace and single-operator-biased at first. Multi-tenant
mutation is where software starts writing checks the team did not mean to cash.

## CI Skeleton

The repo now includes a starter workflow at
[.github/workflows/harness-dev.yml](/mnt/c/Users/spork/dev/agentic_design_workflow/.github/workflows/harness-dev.yml).

It does four useful things:

1. checks Rust formatting
2. syntax-checks the plugin shell wrappers
3. runs the full harness test suite
4. creates a scratch workspace, launches the dashboard, and hits the health and JSON endpoints
