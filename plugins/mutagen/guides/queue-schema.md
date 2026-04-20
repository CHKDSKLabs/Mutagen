# `slices/queue.json` — canonical queue format

The slice queue is authored by Shredder and driven by Karai. The canonical, machine-readable form is **`slices/queue.json`**. The human-readable rendering is **`slices/queue.md`**, regenerated from the JSON on every Shredder run and after every status transition.

Commands read and mutate the JSON. The markdown is strictly for humans.

---

## Schema (`version: 1`)

```json
{
  "version": 1,
  "generated_at": "2026-04-20T14:30:00Z",
  "generated_by": "Shredder",
  "pipeline_mode": "full",
  "slices": [
    {
      "id": "L1-Core-001",
      "title": "Project scaffold and CI",
      "status": "pending",
      "author_agent": "Krang",
      "layer": 1,
      "bounded_context": "Core",
      "target_loc": 300,
      "review_required": true,
      "traces_to": {
        "prd":  ["FR-001", "NFR-001"],
        "adr":  ["ADR-0001"],
        "ddd":  ["Core.Scaffold"],
        "isc":  ["ISC-001"],
        "dsd":  ["DSD-001"]
      },
      "context_to_update": "infrastructure_state.md",
      "objective": "Stand up the repo scaffold, Docker, CI workflows.",
      "implementation_details": [
        "Create pnpm workspace with apps/web and packages/core.",
        "Add Dockerfile and docker-compose.dev.yml.",
        "Wire GitHub Actions CI for lint + typecheck + test."
      ],
      "verification_steps": {
        "acceptance":      "pnpm install && pnpm -r typecheck",
        "isc_detection":   "scripts/verify-isc-001.sh",
        "dsd_conformance": "pnpm lint"
      },
      "human_check_needed": { "required": false, "reason": "" },
      "attempts": 0,
      "verdicts": {
        "karai_structural": null,
        "bishop":           null,
        "tiger_claw":       null
      },
      "completed_at":      null,
      "escalation_reason": null
    }
  ]
}
```

### Field semantics

| Field | Values | Written by |
|-------|--------|------------|
| `version` | `1` (integer) | Shredder |
| `generated_at` | ISO-8601 UTC | Shredder (on author), Karai (on any status mutation; keep original `generated_at`, bump a sibling `last_mutated_at` if you want — v1 doesn't require it) |
| `generated_by` | `"Shredder"` | Shredder |
| `pipeline_mode` | `"full"` \| `"lightweight"` | Shredder (copied from `.claude/workflow.json`) |
| `slices[].id` | e.g. `L{layer}-{BoundedContext}-{NNN}` | Shredder |
| `slices[].status` | `pending` \| `in_progress` \| `completed` \| `refused` \| `escalated` \| `blocked_retry` | Karai (via `/mutagen:execute-next`) |
| `slices[].author_agent` | one of Bebop / Baxter / Chaplin / Metalhead / Splinter / Tatsu / Krang | Shredder |
| `slices[].layer` | 1–6 | Shredder |
| `slices[].bounded_context` | DDD context name, or `"Core"` / `"Infra"` for cross-cutting Layer 1 | Shredder |
| `slices[].target_loc` | integer, ≤ 500 net-new | Shredder |
| `slices[].review_required` | boolean — meaningful only in `lightweight` mode | Shredder |
| `slices[].traces_to` | object of `{prd, adr, ddd, isc, dsd}` arrays of string IDs | Shredder |
| `slices[].context_to_update` | `"project_state.md"` \| `"infrastructure_state.md"` | Shredder |
| `slices[].objective` | one-sentence goal | Shredder |
| `slices[].implementation_details` | array of concrete technical instructions | Shredder |
| `slices[].verification_steps` | object of `{acceptance, isc_detection, dsd_conformance}` strings | Shredder |
| `slices[].human_check_needed` | `{required: boolean, reason: string}` | Shredder |
| `slices[].attempts` | integer, incremented by the re-review loop | Karai |
| `slices[].verdicts` | `{karai_structural, bishop, tiger_claw}` — values below | Karai |
| `slices[].completed_at` | ISO-8601 or `null` | Karai (on `completed`) |
| `slices[].escalation_reason` | string or `null` — populated on `refused` / `escalated` / `blocked_retry` | Karai |

### Verdict values

- `karai_structural`: `null` \| `"pass"` \| `"fail"`
- `bishop`: `null` \| `"clean"` \| `"advisory"` \| `"block"` \| `"skip"`
- `tiger_claw`: `null` \| `"clean"` \| `"gap"` \| `"defect"` \| `"skip"`

### Status transitions

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

`blocked_retry` is the transient state while the author iterates on Bishop/Tiger Claw findings. After `MAX_RETRIES` exhaust, the slice transitions to `escalated`.

---

## Rendering to `slices/queue.md`

The markdown rendering is optional for machines but required for humans. Shredder (or any command that mutates the queue) re-emits `slices/queue.md` from the JSON. Suggested layout — one section per slice, same ID headings Shredder's Output Protocol already uses. A minimal renderer is acceptable; a reader should never need to parse the JSON to answer "what's next."

---

## Invariants

1. **Only Karai mutates status.** Shredder authors the queue; `/mutagen:execute-next` (Karai's driver) is the only thing that flips statuses. Humans may edit the JSON by hand to unstick a slice, but that is a manual intervention, not part of the flow.
2. **Slice IDs are immutable.** A re-slice by Shredder produces a new queue (new file, or overwrites after human confirmation — see `/mutagen:slice`). Do not renumber in place.
3. **Traces-to is sacred.** Every slice has a non-empty citation in `prd` OR `nfr` (via `prd`) AND in `adr` AND `ddd`. `isc` and `dsd` may be empty arrays when genuinely N/A, but prefer to cite at least one.
4. **`queue.md` never contradicts `queue.json`.** If they drift, the JSON wins. Regenerate the markdown.
