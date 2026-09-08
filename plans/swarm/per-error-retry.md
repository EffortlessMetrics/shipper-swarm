# Per-error retry execution and recovery

Issue: [#362](https://github.com/EffortlessMetrics/shipper-swarm/issues/362)
Candidate: [#367](https://github.com/EffortlessMetrics/shipper-swarm/pull/367)

This repair completes the configured retry-class contract in the canonical
package executor. The package ceiling remains cumulative across publish and
resume. A configured class may narrow that ceiling; explicit CLI retry flags
overlay every configured class before the engine validates the effective policy.
The governing contracts are [events as truth](../../docs/INVARIANTS.md),
[failure modes](../../docs/failure-modes.md), and
[SPEC-0007](../../docs/specs/SHIPPER-SPEC-0007-idempotent-workspace-publish.md).

## Boundaries and implementation

1. Validate the flat and every configured class policy after CLI resolution,
   before publish/resume effects. Reject zero attempts, inverted delay bounds,
   and nonfinite or out-of-range jitter. Immediate zero delays remain valid.
   Apply matching class validation during config-file validation.
2. Admit retained failed packages against their effective cumulative class
   ceiling in deterministic selected-package order. A controlled NotPublished
   stop keeps its durable Failed(Retryable) identity but uses the originating
   Ambiguous policy. Preserve unresolved ambiguous registry reconciliation.
   Keep requested and effective ceilings distinct in human and additive JSON
   diagnostics.
3. Record effective classified attempt facts in authoritative events so rebuild
   matches persisted attempt history at final failure, retry scheduling, explicit
   permanent retry, and controlled NotPublished stop boundaries.
4. Align the generated config template, retry references, and Changie fragment
   with cumulative limits, complete class defaults, CLI precedence, and explicit
   permanent opt-in. Preserve existing direct doc repairs.

## Acceptance and proof

- Fake Cargo invocation logs prove a class ceiling of two stops the production
  executor after two cumulative attempts under a larger package ceiling.
- Events/rebuilt state preserve class ceiling, error class, backoff, and final
  attempt facts; permanent retries require explicit configuration.
- Ambiguous retry occurs only after NotPublished; StillUnknown stops safely.
- Resume at top=6/class=2/attempts=2 rejects before state, event, Cargo, or registry
  changes. An explicitly overlaid CLI ceiling of three permits the next attempt.
- Config and effective runtime tests cover invalid policy values and valid zero
  delays. Spawned CLI tests preserve human/JSON requested/effective parity.
- Run `cargo fmt --all -- --check`, focused types/config/core/CLI tests,
  production check/Clippy, then the required PR gate. Use locked dependencies,
  one shared lane target, `CARGO_INCREMENTAL=0`, and at most two compile jobs.

## Non-goals and cleanup

No new error class, counter reset, registry authority, package topology, release
workflow, tags, publishing, credentials, or signing changes. Adjacent work needs
a separate builder-ready issue. Rollback is a normal revert of the repair;
preserve branch history. After merge and post-merge proof, remove the lane-owned
worktree, branch, and generated target once their terminal state is verified.
