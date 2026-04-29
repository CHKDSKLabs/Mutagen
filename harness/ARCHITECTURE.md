# Harness Architecture

## Core rule

The harness owns truth. Prompt text, plugin manifests, persona files, and host adapters are inputs to the runtime, not the runtime itself.

## Layers

### 1. Core engine

Owns queue loading, ready-slice selection, stage transitions, retry accounting, escalation, and completion.

### 2. Policy engine

Owns path scopes, deny rules, amendment rules, and degraded-mode reporting when a host cannot enforce a decision directly.

### 3. Evidence builder

Owns citation resolution from PRD, ADR, DDD, ISC, and DSD into a slice-scoped evidence bundle. Missing citations fail closed.

### 4. Gate runner

Owns deterministic checks such as structural conformance, LOC telemetry, schema validation, and any future machine-checkable gate.

### 5. Host adapter

Owns persona dispatch, tool restrictions, host-specific prompt framing, and translation of host events back into harness events.

### 6. Persistence

Owns queue state, active slice state, evidence bundles, review reports, summaries, dispatch logs, and telemetry.

## Initial runtime objects

- `WorkflowConfig`
- `SliceQueue`
- `Slice`
- `ActiveSlice`
- `StageManifest`
- `EvidenceBundle`
- `GateVerdict`
- `ScopeDecision`
- `DispatchRecord`
- `RunSummary`

## Host adapter contract

A host adapter must be able to:

- dispatch a named stage actor with a bounded task
- pass the evidence bundle path and stage scope to that actor
- persist or return the actor's artifacts
- report a terminal outcome for the stage
- declare what the host can actually do
- resolve a canonical execution profile from host capabilities plus workflow config

Capability flags should include:

- `can_enforce_pre_write`
- `can_isolate_worktree`
- `can_stream_tool_events`
- `can_restrict_tools_per_stage`
- `can_interrupt_running_stage`

If a host lacks a capability, the harness should downgrade explicitly and record the downgrade in state. Silent degradation is how we end up writing README fan fiction.

The adapter's execution profile is the canonical answer to questions like:

- hard scope enforcement vs. advisory scope
- serial-only vs. bounded-parallel dispatch
- isolated worktree vs. shared-workspace execution
- streaming vs. polling telemetry
- interrupt-capable vs. manual-only stop behavior

## Execution loop

1. Load workflow config, queue, and any active run state.
2. Select the next ready slice from the queue.
3. Build the stage manifest and active-slice state.
4. Build or refresh the evidence bundle for the slice.
5. Dispatch the author stage through the host adapter.
6. Run deterministic gates.
7. Dispatch review stages required by policy.
8. Persist verdicts and either complete, retry, or escalate.
9. Emit a summary and continue until a stop condition fires.

## Migration rule

`plugins/mutagen/**` should become a client of this harness, not the place where the canonical runtime lives.

## First executable target

The first code we write under `harness/` should not be another prompt bundle. It should be a small runtime skeleton with:

- queue loading
- ready-slice selection
- active-slice state writing
- evidence bundle writing
- a single deterministic gate
- a host adapter interface with one stub implementation
