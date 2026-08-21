# State files reference — `.shipper/`

One-page cheat sheet. For the full contract see [INVARIANTS.md](../INVARIANTS.md); for triage recipes see [inspect-a-stalled-run.md](../how-to/inspect-a-stalled-run.md).

## Authority order

**`events.jsonl` > `state.json` > `receipt.json`**

When they disagree, events win. `state.json` and `receipt.json` are projections/summaries derived from events. An end-of-run consistency check emits `StateEventDriftDetected` if drift is found.

That authority order applies to execution state. Other `.shipper/` artifacts
answer narrower questions: auth posture, reconciliation evidence, remediation
planning, or captured plan/preflight output.

## Per-file summary

| File | Authority | Purpose | When written | Format |
|---|---|---|---|---|
| `events.jsonl` | **Truth** (append-only) | Every state transition with timestamp | Per event | JSONL (one event per line) |
| `state.json` | Projection | Serialized `ExecutionState` for fast resume | Per package state change | JSON |
| `receipt.json` | Summary | End-of-run audit artifact with evidence | Once, at run completion | JSON |
| `lock` | — | Concurrent-publish guard and optional process-identity evidence | Held during the run | JSON-compatible `LockInfo` metadata plus optional process identity on supported Linux hosts; legacy text remains readable but cannot prove liveness |

Additional evidence artifacts may appear when the related command or workflow
runs:

| File | Authority | Purpose | When written | Format |
|---|---|---|---|---|
| `auth-evidence.json` | Auth evidence | Observed Trusted Publishing/fallback context without token values | Release workflow auth setup | JSON |
| `reconciliation.json` | Ambiguity evidence | Registry-truth evidence for ambiguous publish outcomes | When ambiguous cargo output is reconciled | JSON |
| `remediation-plan.json` | Remediation plan | Dry-run containment and fix-forward plan derived from a receipt | `shipper remediate --dry-run` | JSON |
| `plan.txt` | Captured output | Plan JSON captured for workflow artifacts | Release workflow plan stage | Text containing JSON |
| `preflight_workspace_verify.txt` | Captured output | ANSI-stripped Cargo workspace dry-run output | Preflight workspace verification | Text |

## Compatibility note

0.5 artifacts are rebuildable for every field promised by the event vocabulary.
0.4 events, state, and receipts remain readable and safely resumable, but
fields introduced after 0.4 are unknown when the old artifact cannot provide
evidence unless their schema defines an explicit compatibility default. The
current exception is `Receipt.execution_result`: a missing value in a legacy
all-published receipt deserializes as `success`, as preserved by
`receipt_without_execution_result_defaults_to_success`. Consumers must not
invent defaults for other absent evidence. See
[INVARIANTS.md](../INVARIANTS.md) for the full compatibility and event-first
contract.

## Which file for which question?

| Question | File |
|---|---|
| What happened, in order? | `events.jsonl` |
| What's the current state (fast lookup)? | `state.json` |
| Did the whole release succeed, and what's the audit trail? | `receipt.json` |
| What would `shipper resume` skip? | `state.json` (packages with `state.state == "published"`) |
| What's the truth when they disagree? | `events.jsonl` |
| Which auth path was observed? | `auth-evidence.json` |
| How did Shipper resolve ambiguity? | `events.jsonl`; `reconciliation.json` if present |
| What would remediation do? | `remediation-plan.json` |
| Is a retained run terminal, live, interrupted, or unsafe to classify? | `shipper status --durable` over the correlated local evidence set; no single file proves this |

## Key field paths

### `events.jsonl` (one JSON object per line)

```json
{
  "timestamp": "2026-04-17T...",
  "event_type": {"type": "package_published", "duration_ms": 3400},
  "package": "shipper-types@0.3.0-rc.1"
}
```

Common event types:
- `plan_created` — beginning
- `preflight_started`, `preflight_workspace_verify`, `preflight_complete`
- `package_started`, `package_attempted`, `package_uploaded`, `package_published`, `package_failed`, `package_skipped`
- `retry_backoff_started` — added in [#91](https://github.com/EffortlessMetrics/shipper/issues/91); carries attempt N/M, delay, reason, next-attempt time
- `publish_reconciling`, `publish_reconciled` — added in [#99](https://github.com/EffortlessMetrics/shipper/issues/99); registry-truth resolution of ambiguous outcomes
- `state_event_drift_detected` — added in [#93](https://github.com/EffortlessMetrics/shipper/issues/93); end-of-run consistency check
- `execution_started`, `execution_finished`, `execution_stopped`

`execution_stopped` is a nonterminal, no-receipt marker written while the
publish lock is still held. Its current reason,
`not_published_retry_budget_exhausted`, records that registry truth proved the
selected package absent after Cargo's ambiguous exit and the retry budget was
exhausted. The marker authorizes nothing by itself: status and resume also
require matching plan, registry, state, ordered events, and reconciliation
evidence. An unfinished run without this exact coherent marker keeps missing,
legacy, cross-host, and otherwise inconclusive lock evidence fail-closed.
For a coherent controlled stop, an absent lock or an affirmatively `NotLive`
holder permits recovery. A matching `Live` holder still blocks recovery, and
an `Unknown` observation (including corrupt, legacy, or cross-host lock
evidence) remains fail-closed. The public raw-event API therefore includes the
`ExecutionStopped` event variant and its `ControlledStopReason`; exhaustive
consumers must handle that compatibility surface.

### `state.json`

```json
{
  "state_version": "...",
  "plan_id": "23ff8f85...",
  "registry": {"name": "crates-io", "api_base": "https://crates.io"},
  "attempt_history": [
    {
      "package": "shipper-types",
      "version": "0.3.0-rc.1",
      "attempt": 1,
      "max_attempts": 3,
      "started_at": "...",
      "ended_at": "...",
      "error_class": "retryable",
      "next_attempt_at": "...",
      "redacted_message": "rate limited"
    }
  ],
  "packages": {
    "shipper-types@0.3.0-rc.1": {
      "name": "shipper-types",
      "version": "0.3.0-rc.1",
      "attempts": 1,
      "state": {"state": "published"},
      "last_updated_at": "..."
    }
  }
}
```

**Field path caveat**: package state lives at `.packages[].state.state` (nested), **not** `.packages[].status`. Common misread.

`plan_id` binds the registry API base and ordered package names/versions. It
does not bind source bytes, workspace path, or a unique execution. Resume uses
it as one guard, not as provenance for the checkout restored by an operator.

Affirmative lock-holder liveness currently depends on Linux `/proc` process
identity. Unsupported platforms, legacy locks, missing/corrupt locks, and
cross-host evidence remain `Unknown`; PID or lock age alone is not equivalent
proof.

`attempt_history` is the per-attempt projection replayed from `events.jsonl` and is used by diagnostics and recovery workflows after interruption.

Per-attempt recovery fields are under `.attempt_history[]`, keyed by:

- `package`, `version`
- `attempt`, `max_attempts`
- `started_at`, `ended_at`
- `error_class`, `next_attempt_at`, `redacted_message`

### `receipt.json`

```json
{
  "receipt_version": "shipper.receipt.v2",
  "plan_id": "...",
  "registry": {...},
  "started_at": "...",
  "finished_at": "...",
  "packages": [
    {
      "name": "shipper-types",
      "version": "0.3.0-rc.1",
      "attempts": 1,
      "state": {"state": "published"},
      "started_at": "...",
      "finished_at": "...",
      "duration_ms": 3400,
      "evidence": {...}
    }
  ],
  "event_log_path": ".shipper/events.jsonl",
  "git_context": {...},
  "environment": {...},
  "execution_result": "success"
}
```

`execution_result` is the aggregate receipt outcome: `"success"`,
`"partial_failure"`, or the compatibility-retained `"complete_failure"` value.
Current finalized publish/resume runs emit `success` (exit `0`) or
`partial_failure` (exit `2`); `complete_failure` is not currently reachable
from receipt finalization. Exit `1` means the command errored before a receipt
was finalized, so do not infer it from `receipt.json`. The field is
`#[serde(default)]` — receipts written before it existed deserialize as
`"success"`.

## jq one-liners

```bash
# All packages that published successfully
jq -r 'select(.event_type.type == "package_published") | .package' .shipper/events.jsonl | sort -u

# Last durable event (this does not prove that the process is live)
jq -c '.' .shipper/events.jsonl | tail -1

# Package states from state.json
jq -r '.packages[] | "\(.name): \(.state.state)"' .shipper/state.json

# Plan ID for comparison across runs
jq -r '.plan_id' .shipper/state.json

# Reconciliation outcomes
jq -c 'select(.event_type.type == "publish_reconciled") | .event_type' .shipper/events.jsonl

# Drift (should be empty on a healthy run)
jq -c 'select(.event_type.type == "state_event_drift_detected")' .shipper/events.jsonl
```

## See also

- [INVARIANTS.md](../INVARIANTS.md) — truth/projection/summary contract (normative)
- [how-to/inspect-a-stalled-run.md](../how-to/inspect-a-stalled-run.md) — triage recipes
- [how-to/inspect-state-and-receipts.md](../how-to/inspect-state-and-receipts.md) — post-run inspection
- [explanation/why-shipper.md](../explanation/why-shipper.md) — why the three-file split exists
