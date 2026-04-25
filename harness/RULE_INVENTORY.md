# Harness Rule Inventory

This inventory records rules that used to live mainly in command or skill
prose and where the harness now enforces or records them.

| Rule | Runtime Owner | Enforcement |
| --- | --- | --- |
| Select only ready slices | `runtime::prepare_next`, `selected_slice::prepare_selected_slice`, `cohort::prepare_cohort` | Dependencies must be completed before claim. |
| Queue JSON is authoritative | `validation`, shell queue-ready wrappers | Execution reads `slices/queue.json`; prose maps are renderings only. |
| Preserve active slice state | `state`, `state_transition` | Stage, agent, counters, scope, host, and degraded capabilities are persisted. |
| Build evidence from traces | `evidence`, `runtime`, `selected_slice` | Missing citations fail slice preparation. |
| Scope writes by stage | `policy`, `state_transition`, `amend_scope` | Allowed globs are produced per stage and amended only through policy. |
| Report host degradation | `adapter`, `runtime`, `cohort` | Host profile records advisory scope, serial fallback, and missing host capabilities. |
| Structural gate before review | `structural` | Required sections, trace drift, State Update block, and LOC overrun produce fail reports. |
| Record Tiger Claw verdicts | `review_record` | QA report and latest convenience copy must agree before queue mutation. |
| Decide retry path from state | `review` | Retry, micro-correction, blocked retry, and retry-budget escalation are computed from persisted counters and QA contract. |
| Apply State Update centrally | `state_update`, `finalize`, `cohort_apply` | Agents emit blocks; the harness applies them to context files. |
| Finalize successful slices | `finalize` | Queue status, summary, dispatch log, completion marker, active-state cleanup, and layer notifications are written together. |
| Normalize scope violations | `scope_violation` | Traag reports are enriched, queue state is escalated when possible, and notifications are planned. |
| Select bounded cohorts safely | `cohort` | Same-layer ready siblings with non-conflicting write sets are selected in queue order. |
| Isolate cohort work | `cohort_worktree` | Each selected sibling runs in a dedicated managed worktree. |
| Dispatch cohort members | `cohort_dispatch` | The harness spawns member runners and collects member results. |
| Reconcile cohort members | `cohort_reconcile`, `cohort_apply` | Imports are filtered by slice scope, merged in order, and halted on conflicts. |
| Emit notification intents | `notifications`, `runtime`, `review`, `structural`, `scope_violation`, `finalize` | The runtime emits canonical notification payloads; shell only transports them. |

Remaining host glue is allowed in shell when it launches processes, renders
compatibility markdown, or relays notifications. Anything that changes queue
truth belongs in the harness.
