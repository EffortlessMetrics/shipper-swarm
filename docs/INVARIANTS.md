# Invariants

## Truth, projection, summary

Three files in `.shipper/` carry execution data. They have a strict ordering of authority.

### `events.jsonl` — truth
Append-only. Every state transition emits exactly one event with timestamp, package context, and event-type-specific payload. Never replayed-and-rewritten. Never compacted (events are bounded by package count × attempt count, both small).

### `state.json` — projection
A serialized view of the current `ExecutionState`, rewritten after every package state change. Equivalent to a fold over `events.jsonl` followed by snapshotting. Used by `shipper resume` for fast recovery without replaying the full event log.

### `receipt.json` — summary
Written once at end-of-run. Summarizes packages, plan, registry, environment, and event log location. Intended for CI artifacts and audit consumers.

## The invariant

> The set of `package_published` events in `events.jsonl` MUST equal the set of packages with `state.state == "published"` in `state.json`.

Drift between events and state is a bug. Per [#93](https://github.com/EffortlessMetrics/shipper/issues/93), an end-of-run consistency check enforces this and emits `state_event_drift_detected` on mismatch.

## Why it matters

`shipper resume` reads `state.json` to decide which packages to skip. If state.json drifts from events, resume could either re-publish duplicates (state under-reports success) or refuse to continue valid work (state over-reports success).

The contract guarantees: even if `state.json` is corrupted or deleted, the run can be reconstructed from `events.jsonl` alone (per [#101](https://github.com/EffortlessMetrics/shipper/issues/101)'s state-rebuild capability).

## Field-name caveat

In `state.json`, package status is at `.packages[].state.state`, not `.packages[].status`. The original v0.3.0-rc.1 retrospective briefly misread the file by querying the wrong path. The invariant above is what makes that ambiguity recoverable: events are the truth, the projection's exact field path is implementation detail.

## Practical guidance for tooling

If you are writing tooling that consumes Shipper output:

- **For per-event audit / streaming**: read `events.jsonl`. Each line is one JSON object.
- **For "what's the current state"**: read `state.json`. Treat it as a cache; reconcile against events if you suspect drift.
- **For "did this release succeed and what was published"**: read `receipt.json`.

Never derive critical decisions from CLI stdout alone. Stdout is a human-facing rendering of the events; structured consumers should always go to the JSON files.

## Uploaded recovery checkpoint

`EventType::PackageUploaded` is emitted only after Cargo has accepted an
upload. It is now the durable checkpoint for `PackageState::Uploaded`:
state rebuild maps that event to `uploaded`, and a later `package_published`
event advances the projection to `published`. `EventType::ReadinessStarted`
continues to map to `uploaded` for backward compatibility with historical
event logs.

## Process timeout enters reconciliation

The canonical package executor treats `CargoOutput::timed_out` as
`ErrorClass::Ambiguous` before inspecting output text. Killing Cargo cannot
establish whether the registry accepted the upload. This holds when retained
tails contain Retryable/Permanent patterns and when the configured tail is
empty. Both schedulers use this decision and the same three-way registry
reconciliation. Ordinary non-timeout Cargo exits with complete tails retain their output-based
classification. This rule does not change process capture, memory limits, or
descendant cleanup guarantees.

## Publish diagnostic capture is bounded and loss is explicit

Cargo publication, including rehearsal, opts into bounded diagnostic retention.
Each stream retains at most 256 KiB of payload, a 64 KiB partial line, and 4096
line entries while reading fixed 8 KiB chunks. Normal eviction outside the
requested last-N-lines window is intentional tailing. Capacity or read loss is
sticky: both diagnostic streams are suppressed in favor of a fixed omission
summary, so discarded authorization context cannot leave a credential suffix or
continuation in evidence. UTF-8 decoding and redaction also obey the final payload
limit. `output_lines=0` intentionally returns empty tails; it is not capacity loss.

The canonical executor consumes typed completeness, not the summary text. An
unsuccessful incomplete result enters Ambiguous reconciliation after the typed
timeout check. Cargo success still establishes Uploaded and proceeds through
readiness. Attempt counts and registry outcome authority do not change. Rehearsal
reports incomplete failure as Ambiguous without adding a retry policy. Public
CargoOutput fields remain compatible.

Generic complete-output capture and structured metadata remain complete. Yank,
install and dry-run capture are unchanged. This contract bounds retained publish
diagnostics, not descendant lifetimes or inherited-pipe EOF; cleanup bounds remain
separate acceptance in #412.

## Readiness evidence matches readiness events

`ReadinessEvidence.delay_before` is the delay that was **actually slept**
immediately before that poll — not an independently recomputed backoff
sample. It equals the `delay_ms` of the `ReadinessPollScheduled` event that
announced the same attempt, so the receipt's readiness evidence and the event
log can never disagree about how long a run waited.

This holds for the first attempt too: when `readiness.initial_delay` is
configured, the loop emits `ReadinessPollScheduled { attempt: 1 }` and sleeps,
and attempt 1's evidence reports that same `initial_delay`. It reports
`0` only when nothing was slept before the first poll.

There is exactly one implementation of this loop —
`shipper_registry::RegistryClient::is_version_visible_with_backoff_and_events`
(see [#202](https://github.com/EffortlessMetrics/shipper/issues/202)). The
engine wraps it with the `ReadinessStarted` / `ReadinessComplete` /
`ReadinessTimeout` / `ReadinessError` envelope but does not re-implement the
polling or the evidence. `ReadinessTimeout` means the wait budget ended with a
conclusive negative observation. `ReadinessError` means the result is
inconclusive or polling aborted, and records elapsed time rather than the
configured budget. An inconclusive final probe can therefore produce
`ReadinessError` at budget exhaustion as well as before it.

### Unknown observations cannot authorize another upload

A positive API or valid sparse-index observation establishes publication.
With readiness enabled, negative reconciliation evidence requires a successful
final observation from every configured authority: API `404`, sparse-index `404`, or a fully readable
index containing valid Cargo semantic versions but not the requested version. In
`Both` mode both probes must establish absence in the same final poll;
`prefer_index` changes query order only. Either probe may still establish
presence while the other is unavailable. Disabled readiness retains its legacy
single API observation, irrespective of the configured polling method.

Connection errors, request timeouts, authentication/rate-limit/server errors,
empty or malformed index responses, an unreadable local index, and a `304`
without a sent validator and readable cached content remain unknown. A
configured local index is an explicitly selected observation source; Shipper
does not independently prove that an operator-managed mirror is fresh. Cached
HTTP content is subject to the same parsing rules. A cached positive record
can confirm presence. A `304` with no valid positive record triggers one
unconditional refresh before absence can be claimed: cached bytes and ETags
are stored separately and a partial cache write must not authorize a retry.

The polling loop may recover from transient errors. If its final poll is
unknown, an earlier negative poll cannot authorize `NotPublished`: the engine
records `StillUnknown`, retains ambiguous state, and stops before another
Cargo invocation. Failed probes are not emitted as `ReadinessPoll(false)` or
stored as negative `ReadinessEvidence`. Conclusive evidence keeps its actual
poll number, so gaps in attempt numbers are expected after failed probes.

The public polling signatures remain `Result<(bool, Vec<ReadinessEvidence>)>`;
the additive `shipper_registry::ReadinessObservationError` retains the total
poll count and earlier conclusive evidence for consumers that downcast it.
`check_index_visibility` now returns errors for unknown observations rather
than collapsing them to `false`. No event or artifact schema changes are
required. The permissive `shipper_sparse_index::contains_version` search helper
retains its existing contract; registry observations use `observe_version`.

Attempt details are appended to `state.json` through the same event-first
transition boundary as the matching terminal, reconciliation, or retry event.
This prevents a scheduler from independently persisting the attempt timeline
and package state. State rebuild now projects all event-vocabulary-carrying
attempt metadata into `attempt_history`, including retry backoff timestamps.
Remaining edge cases are only where an event cannot describe the same detail
that a runtime-only attempt transition could infer locally.

Parallel package workers own the explanatory events for a transition until its
projection is ready. The canonical executor validates the entire package batch,
then holds the shared event-log mutex across appending those events and the
event-first state/attempt-detail commit. Another worker may append or flush
before that boundary, but cannot replace its pending tail during the commit.
Lock ordering is event log, then execution state; Cargo, registry queries,
reporting and retry sleeps remain outside that critical section. This is an
in-process concurrency guarantee, not an all-or-nothing filesystem transaction
or a host/power-loss durability claim.

Reconciliation report generation uses that same event-log mutex across the
complete durable-event read, record derivation, and report replacement. Package
workers cannot overwrite newer reconciliation evidence with an older snapshot
or interfere with another worker's `reconciliation.tmp`. The report path takes
only the event-log mutex; package transitions retain the event-log-then-state
lock order. Cargo, registry polling, backoff, and report warnings remain outside
this report persistence boundary.

Post-join finalization derives the report after workers finish, while the
invocation still owns its publication key. A failed report read or write remains
observable, and the complete event/state/report consistency check owns recovery
authorization. A worker's local result cannot substitute for that check. The
low-level report writer requires caller-provided exclusion for its whole call;
this is an engine concurrency guarantee, not a new generic storage transaction,
cross-process locking primitive, or fsync/power-loss guarantee.

## Artifact compatibility across the 0.4 to 0.5 line

The compatibility promise is asymmetric:

- **0.5 artifacts:** new events contain enough information to rebuild every
  field the project claims is reconstructible. State and receipt fields are
  projections/summaries, not a second source of truth.
- **0.4 artifacts:** old event, state, and receipt files remain readable and
  safely resumable. A field absent from the old vocabulary is `unknown`; it
  must not be fabricated as zero or inferred without evidence.

Encrypted payloads use a versioned KDF header for new writes. KDF v1
unversioned payloads remain readable at their legacy PBKDF2 cost, while KDF v2
is the new stronger default. Unsupported or unreasonable parameters are
rejected rather than accepted as an implicit fallback.

Security evidence follows the same non-disclosure rule as execution evidence:
complete authorization header values and bounded `Token=`/`ApiKey=` values are
redacted, ordinary prose is preserved, and webhook configuration secrets do
not appear in events, state, receipts, diagnostics, structured output, or
logs. OIDC availability requires both request variables to be present and
nonblank; blank diagnostics never include their values.

## Containment effects and local evidence

Single yank, yank-plan execution, and guarded remediation prepare their shared
event destination before invoking Cargo. Each observed result must append its
`PackageYanked` event before the command counts success or starts another yank.
A returned append error stops the remaining plan and produces a nonzero,
essential diagnostic even under `--quiet`. The diagnostic preserves the
observed Cargo result without treating missing local evidence as proof that no
registry effect occurred.

A requested receipt amendment is checked against the selected registry and
package before the effect and re-read after the event is recorded. Failure to
load, match, or write that amendment is an incomplete operation, not a successful
containment command. Existing event bytes are preserved. Direct and yank-plan
reasons retain their intentional
recording contract; guarded remediation retains its redacted reason placeholder.

These checks handle ordinary returned filesystem errors. They do not make
preflight a transaction or add a host/power-loss durability guarantee. An
incomplete-evidence diagnostic calls for registry and artifact reconciliation,
not an automatic repeat of the remote operation.
