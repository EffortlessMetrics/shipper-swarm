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
   automatic permanent retry opt-in. Preserve existing direct doc repairs.

## Acceptance and proof

- Fake Cargo invocation logs prove a class ceiling of two stops the production
  executor after two cumulative attempts under a larger package ceiling.
- Events/rebuilt state preserve class ceiling, error class, backoff, and final
  attempt facts; automatic permanent retries require explicit configuration.
- Ambiguous retry occurs only after NotPublished; StillUnknown stops safely.
- Resume at top=6/class=2/attempts=2 rejects before state, event, Cargo, or registry
  changes. An explicitly overlaid CLI ceiling of three permits the next attempt.
- Config and effective runtime tests cover invalid policy values and valid zero
  delays. Spawned CLI tests preserve human/JSON requested/effective parity.
- Run `cargo fmt --all -- --check`, focused types/config/core/CLI tests,
  production check/Clippy, then the required PR gate. Use locked dependencies,
  one shared lane target, `CARGO_INCREMENTAL=0`, and at most two compile jobs.

## Follow-up review repairs at `25e3d517`

- Share retained-state admission between deliberate `publish` and `resume`
  invocations before bootstrap effects. Both must reject exhausted class and
  package ceilings; unresolved Ambiguous and Uploaded packages must retain their
  registry-only reconciliation opportunity.
- Repeat the same retained-state validation after acquiring the publish lock
  and loading the current state. A stale pre-lock observation cannot authorize
  another Cargo attempt. Skip external lock liveness refusal only while the
  observed record matches the captured publisher/process/plan/acquisition
  identity; reject missing, corrupt, foreign, or changed records. Retain
  controlled-stop identity and event consistency checks. This bounded check
  does not make the existing advisory lock an atomic mutex; the acquisition and
  release ownership follow-up is [#383](https://github.com/EffortlessMetrics/shipper-swarm/issues/383).
- Apply the same runtime and retained-state admission to the public lower-level
  `run_publish_parallel` wrapper before its webhook or worker effects. Preserve
  the caller-supplied state and registry interface and caller responsibility for
  bootstrap/locking; direct callers receive the same class-budget diagnostics.
- Preserve historical manual recovery after a permanent cause is repaired.
  The permanent override controls automatic retries within one invocation;
  a fresh deliberate invocation may retry below the cumulative ceiling.
- Treat wall-clock timestamps as observations, allowing clock rollback during
  or after Cargo. Use the recorded attempt end as the completion event timestamp
  and retain identity/domain validation without imposing wall-clock ordering.
- Keep omitted class attempts defaulting to six, but allow invalid zero values
  through diagnostic-only deserialization. Validated config loading and resolved
  runtime execution still reject them with class-specific diagnostics.
- Refresh the generated config snapshot and replace zero-attempt partial-result
  fixtures with valid positive-budget retained interruption fixtures. Keep the
  human/JSON parity and multi-registry worst-result assertions.

Proof adds sequential/parallel repeat-publish refusal with unchanged Cargo,
registry, state, and event evidence; explicit raised ceilings and repaired
permanent-cause recovery; reversed-clock completion round trips; diagnostic
loader accept/validate-reject cases; and the three exact hosted regressions.
A stale-precheck fixture advances state through a real publish invocation before
calling bootstrap, proving its under-lock reload rejects without new effects.

### Local follow-up repair proof

The follow-up dirty tree based on `25e3d517` passed:

- `cargo nextest run --locked -p shipper-core --lib -E 'test(retry_regressions) | test(completion) | test(transition) | test(mode_parity)' --test-threads 2 --status-level fail --final-status-level fail`: 70 passed, 2081 skipped; run `4cab986a-cca4-4ec1-8143-26c759b88143`.
- `cargo clippy --locked -j 2 -p shipper-types -p shipper-config -p shipper-core -p shipper-cli --all-targets --all-features -- -D warnings`: pass.
- `cargo fmt --all -- --check` and `git diff --check`: pass.

An earlier combined selection passed 709 tests, including config/types and the
repaired CLI snapshots/partial-result fixtures. A later full selected run exposed
an intermittent pre-existing parallel event-buffer/transition race in
`parallel_incomplete_reconciliation_never_authorizes_resume`; it failed safely
with a package identity mismatch instead of the expected controlled-stop result.
That failure remains unresolved by this retry repair and requires its separate
atomic-transition prerequisite before full integration proof is repeated.
Staged-index `cargo precommit` passed with Changie v1.25.1 on all 18 repair
paths. Exact committed-head integration proof remains for the coordinator
after the prerequisite lands. These results are not release evidence.

## Non-goals and cleanup

No new error class, counter reset, registry authority, package topology, release
workflow, tags, publishing, credentials, or signing changes. Adjacent work needs
a separate builder-ready issue. Rollback is a normal revert of the repair;
preserve branch history. After merge and post-merge proof, remove the lane-owned
worktree, branch, and generated target once their terminal state is verified.
