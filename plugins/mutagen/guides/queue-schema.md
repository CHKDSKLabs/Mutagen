# `slices/queue.json` — canonical queue format

The slice queue is authored by Shredder and executed by Karai.

The canonical, machine-readable form is **`slices/queue.json`**.
The human-facing rendering is **`slices/slicemap.md`**.
For compatibility with older Mutagen docs, `scripts/render_queue.sh` may also emit **`slices/queue.md`** as a shadow copy.

Commands read and mutate the JSON.
Humans review the slicemap.
If the artifacts drift, the JSON wins.

---

## Schema (`version: 1`)

```json
{
  "version": 1,
  "generated_at": "2026-04-22T12:00:00Z",
  "generated_by": "Shredder",
  "pipeline_mode": "full",
  "planning_advisories": [
    {
      "id": "ISC-012",
      "severity": "high",
      "summary": "Android offline cache writes plaintext locally.",
      "decision": "Proceed under the documented exception unless the user overrides.",
      "user_response_required": false,
      "references": ["ISC-012", "ADR-0006", "NFR-004"],
      "affects_slices": ["L1-Mobile-001", "L3-MobileSecurity-001"]
    }
  ],
  "slices": [
    {
      "id": "L1-Core-001",
      "title": "Bootstrap the service workspace",
      "phase": "Foundation",
      "status": "pending",
      "author_agent": "Krang",
      "layer": 1,
      "bounded_context": "Core",
      "target_loc": 300,
      "objective": "Stand up the repo scaffold, CI, and deploy-safe base config.",
      "context_to_update": "infrastructure_state.md",
      "implementation_details": [
        "Create the workspace layout and shared tooling config.",
        "Add CI steps for lint, tests, and release-safe checks.",
        "Wire environment examples for the chosen deploy target."
      ],
      "review_required": true,
      "attempts": 0,
      "micro_corrections_used": 0,
      "depends_on": [],
      "adjacent_scope_allowed": [],
      "write_set": [
        ".github/workflows/**",
        "Dockerfile",
        "infrastructure/**",
        ".env.example"
      ],
      "traces_to": {
        "prd": ["FR-001", "NFR-001"],
        "adr": ["ADR-0001"],
        "ddd": ["Core.Scaffold"],
        "isc": ["ISC-001"],
        "dsd": ["DSD-001"]
      },
      "verification_steps": {
        "acceptance": "pnpm install && pnpm -r typecheck",
        "isc_detection": "scripts/verify-isc-001.sh",
        "dsd_conformance": "pnpm lint"
      },
      "human_check_needed": {
        "required": false,
        "reason": "",
        "resolved_at": null
      }
    }
  ]
}
```

Runtime fields such as `verdicts`, `completed_at`, and `escalation_reason` are
part of the canonical schema, but the harness adds them later during execution.
Shredder does not need to pre-populate them just to feel busy.

---

## Top-level fields

| Field | Meaning | Written by |
|-------|---------|------------|
| `version` | schema version, currently `1` | Shredder |
| `generated_at` | ISO-8601 UTC timestamp | Shredder |
| `generated_by` | generator identity, usually `"Shredder"` | Shredder |
| `pipeline_mode` | `"full"` or `"lightweight"` | Shredder |
| `planning_advisories` | machine-readable slicing caveats or assumptions | Shredder |
| `slices` | canonical execution units | Shredder, then Karai mutates runtime state |

---

## Slice fields

### Required on author

| Field | Meaning |
|-------|---------|
| `id` | stable slice identifier, e.g. `L2-Orders-003` |
| `title` | concise human name |
| `status` | initial lifecycle state, usually `pending` |
| `author_agent` | assigned executor |
| `layer` | dependency layer, `1` through `6` |
| `bounded_context` | DDD context name |
| `target_loc` | intended size budget, default target `~300` |
| `objective` | one-sentence slice goal |
| `context_to_update` | `project_state.md` or `infrastructure_state.md` |
| `implementation_details` | structured technical instructions, not a prose blob |
| `review_required` | lightweight-mode review flag |
| `attempts` | initial retry counter, starts at `0` |
| `micro_corrections_used` | initial micro-fix counter, starts at `0` |
| `depends_on` | authoritative readiness edges |
| `adjacent_scope_allowed` | pre-approved retry-only scope expansion globs |
| `write_set` | authoritative write scope for scheduling and policy |
| `traces_to` | upstream evidence references |
| `verification_steps` | concrete acceptance and conformance commands |
| `human_check_needed` | execution gate for manual prerequisites |

### Optional on author

| Field | Meaning |
|-------|---------|
| `phase` | review-friendly label such as `Foundation` or `Release Prep` |

### Runtime-added later

| Field | Meaning | Written by |
|-------|---------|------------|
| `verdicts` | structural / review outcomes | Karai |
| `completed_at` | completion timestamp | Karai |
| `escalation_reason` | halt reason on blocked slices | Karai |

Example runtime verdict payload:

```json
{
  "verdicts": {
    "karai_structural": "pass",
    "bishop": "skip",
    "tiger_claw": "clean",
    "micro_correction": true,
    "micro_corrections_used": 1
  }
}
```

---

## Key contract rules

1. `depends_on` is authoritative for readiness.
2. `write_set` is authoritative for scheduling and scope policy.
3. `implementation_details` must be a list of actions, not a paragraph that wandered off.
4. `human_check_needed.required` is an execution gate, not decorative prose.
5. `target_loc` should default to `~300`. Anything above that should be deliberate, not lazy.
6. Every slice must carry real traceability. A citation-free slice is fan fiction.

---

## Status transitions

```
                    ┌─────────┐
                    │ pending │
                    └────┬────┘
                         │  /mutagen:execute-next
                         ▼
                    ┌──────────────┐
                    │ in_progress  │
                    └──────┬───────┘
                           │
     ┌──────────────┬──────┴─────┬──────────────┐
     │              │            │              │
     ▼              ▼            ▼              ▼
┌─────────┐   ┌─────────────┐ ┌──────────┐ ┌────────────┐
│completed│   │blocked_retry│ │escalated │ │  refused   │
└─────────┘   └──────┬──────┘ └──────────┘ └────────────┘
                     │
        re-dispatch author (retry)
                     │
                     ▼
               ┌──────────────┐
               │ in_progress  │
               └──────────────┘
```

`blocked_retry` is transient.
It means the slice is waiting for another author pass, not that the scheduler forgot how computers work.

---

## Rendering

`scripts/render_queue.sh` renders the human view from `slices/queue.json`.

- Primary output: `slices/slicemap.md`
- Compatibility shadow: `slices/queue.md`

Humans review the slicemap.
Machines read the JSON.

---

## Invariants

1. Only the orchestrator mutates status fields during execution.
2. Slice IDs are immutable once authored.
3. `slicemap.md` and `queue.md` are renderings; neither may contradict `queue.json`.
4. Re-slicing replaces the queue after explicit human confirmation.
