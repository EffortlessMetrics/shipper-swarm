# Rust 1.95 / 0.4.0 Quality Rollout

This document is the historical map for the Rust 1.95 and 0.4.0 release-quality rollout for `shipper`. The current claim-to-proof map lives in [docs/status/SUPPORT_TIERS.md](../status/SUPPORT_TIERS.md); use that file for live support-tier decisions.

## Current State

| Layer | Current | Target | Status |
|---|---|---|---|
| Edition | 2024 | 2024 | done |
| MSRV | 1.95 | 1.95.0 | done |
| Toolchain file | `rust-toolchain.toml` pinned to 1.95.0 | keep pinned | done |
| Release line | 0.4.0 | v0.4.0 published and proven | done |
| Clippy | workspace lint floor + staged lint ledger | keep policy-backed | stable/internal |
| No-panic | exact no-new-debt baseline | keep clean | stable/internal |
| Non-Rust file policy | allowlist + companion policies | keep clean | stable/internal |
| Coverage | advisory / routed | keep advisory / routed | present |
| ripr | advisory PR exposure lane | keep advisory | advisory |
| Mutation | weekly + label-gated PR lane | keep opt-in | advisory / opt-in |
| CI economics | full CI + routed Rust small lane | preserve cost discipline | present |
| Release proof | 0.4.0 readiness and publish proof | preserve release authority proof | stable |

## Why This Is a Minor Release

MSRV is part of the semver promise for a library/tool. Raising from 1.92 to 1.95 changed the supported consumer set. The policy rule was: MSRV increase -> minor version bump. Therefore `0.3.0-rc.2` advanced through the 0.4.0 line and was published as `v0.4.0`. No future work should reopen the old rc.2 line.

## What Rust 1.95 Adds for `shipper`

| Rust 1.95 item | `shipper` use |
|---|---|
| `Vec::push_mut` / `insert_mut` | Publish plan builders, event/receipt builders, retry histories, readiness reports, `.shipper` state summaries. |
| `if let` guards | Registry reconciliation, publish-state classification, retry/backoff classification, cargo-failure parsing, preflight outcome routing. |
| Atomic `update` / `try_update` | Reporter counters, warning-once state, retry telemetry, future concurrent publish metrics. |
| `cfg_select!` | Windows/Unix artifact names, shell behavior, path handling, binary install paths. |
| `cold_path` | Ambiguous-publish errors, fail-closed token handling, corrupted state, sparse-index/readiness failures. |
| Clippy 1.95 | `manual_checked_ops`, `manual_take`, `manual_pop_if`, `duration_suboptimal_units`, future `disallowed_fields`. |

## Existing Proof Lanes (Preserve)

The current CI already provides strong safety coverage. This rollout must not weaken any of these lanes.

| Lane | Trigger | Notes |
|---|---|---|
| fmt + clippy | Every PR / push | `RUSTFLAGS=-Dwarnings` and `-D warnings` — warns are errors |
| nextest (Linux/Windows/macOS) | Every PR / push | Three-OS matrix |
| MSRV gate | Every PR / push | Pinned to `1.95.0` |
| BDD smoke | Every PR / push | Cucumber feature specs in `features/` |
| Architecture guard | Every PR / push | Crate boundary enforcement |
| Security / audit | Every PR / push | `cargo audit` |
| Coverage | main / dispatch / `coverage` / `full-ci` | Codecov integration, advisory |
| Fuzz | Nightly schedule | Fuzz targets in `fuzz/` |
| Mutation | Nightly / labeled PRs | `.cargo/mutants.toml` |
| Release dogfood | `v*.*.*` tags / `workflow_dispatch` | Source repo release workflow remains the release authority; Trusted Publishing default is not promoted without short-lived-token proof |

## Gaps Closed Or Converted By This Rollout

The following were the pre-rollout gaps this ladder was designed to close or
turn into explicit advisory lanes:

- `rust-toolchain.toml` pins Rust 1.95.0.
- Workspace lint configuration and policy ledgers define the Rust/Clippy floor.
- `policy/` contains allowlists, ledgers, and generated baselines.
- `xtask/` owns Rust-native policy and evidence checks.
- No-panic tracking has an exact baseline and blocking check.
- Non-Rust file policy receipts workflows, docs, configs, and companion surfaces.
- ripr exists as an advisory PR-time exposure lane.
- Targeted mutation exists as a label-gated PR-time lane, with broader mutation outside ordinary PR cost.

## Historical Rollout Constraints

These governed the original rollout PR ladder. Current work should follow
`AGENTS.md`, the active goal, support tiers, and the live PR queue.

1. Do not mix this rollout into open PR #164 (Factory Droid review workflows).
2. Start every PR from clean `origin/main`.
3. Do not stack PRs unless the PR explicitly depends on prior policy/tooling work.
4. One PR per objective.
5. Open PRs as draft first.
6. Do not push main directly.
7. Do not force-push except to your own PR branch after rebase.
8. Do not merge while required checks are pending.
9. Do not claim green until post-merge main checks pass.
10. Do not add Clippy test carveouts or bare `#[allow(clippy::...)]`.
11. Do not reset the no-panic baseline except in the dedicated baseline PR (PR 8).
12. Do not make ripr branch-protection blocking (advisory only).
13. Do not put full mutation on ordinary PRs.
14. Do not weaken the release dogfood proof, Trusted Publishing behavior, or resume semantics.

## Historical PR Ladder

This ladder is historical context, not the active queue. Do not branch new work
from these branch names; branch from current `origin/main` and use support tiers
plus the active goal for current scope.

| PR | Branch | Title | Depends on |
|---|---|---|---|
| 1 | `docs/rust-1.95-rollout` | docs(policy): map Rust 1.95 and 0.4.0 quality rollout | none |
| 2 | `probe/rust-1.95-compat` | chore(msrv): probe Rust 1.95 compatibility | none |
| 3 | `chore/msrv-rust-1.95` | chore(msrv): raise workspace toolchain to Rust 1.95 | PR 2 |
| 4 | `chore/xtask-policy-foundation` | chore(xtask): add Rust-native policy runner | PR 3 |
| 5 | `policy/clippy-ledger` | policy(clippy): add strict lint ledger and checker | PR 4 |
| 6 | `policy/rust-1.95-lints` | policy(rust): enable Rust 1.95 compiler lint floor | PR 5 |
| 7 | `policy/clippy-rust-1.95-ratchets` | policy(clippy): activate Rust 1.95 lint ratchets | PR 6 |
| 8 | `policy/no-panic-baseline` | policy(panic): add exact no-panic baseline | PR 7 |
| 9 | `policy/file-allowlists` | policy(files): add non-Rust file allowlists | PR 8 |
| 10 | `ci/ripr-and-mutation-lanes` | ci: add ripr advisory and targeted mutation lanes | PR 9 |
| 11 | `ci/lane-policy` | ci: add lane policy and scoped PR routing | PR 10 |
| 12 | `refactor/rust-1.95-api-cleanups` | refactor: use Rust 1.95 APIs in publish and receipt paths | PR 11 |
| 13 | `policy/first-burndown` | policy: burn down first Clippy and panic-family debt | PR 12 |
| 14 | `release/0.4.0-prep-rust-1.95` | release: prepare 0.4.0 for Rust 1.95 | PR 13 |
| 15 | `release/0.4.0-dry-run` | release: validate 0.4.0 publish readiness | PR 14 |

## Historical Acceptance Gates per PR

### PR 1 (this PR — docs only)
```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
git diff --check
```

### PR 2 (compat probe)
```bash
rustup override set 1.95.0
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

### PR 3 (toolchain/MSRV bump)
```bash
rustup override set 1.95.0
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo test --workspace --doc --locked
git diff --check
```

### PR 4 (xtask foundation)
```bash
cargo check -p xtask --locked
cargo test -p xtask --locked
cargo xtask package-surface
cargo xtask policy-report
```

### PR 5 (Clippy ledgers)
```bash
cargo test -p xtask --locked
cargo xtask check-lint-policy
cargo xtask check-clippy-exceptions
cargo xtask policy-report
```

### PR 6 (rustc lint floor)
```bash
cargo check --workspace --all-targets --all-features --locked
cargo xtask check-lint-policy
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

### PR 7 (Clippy ratchets)
```bash
cargo xtask check-lint-policy
cargo xtask check-clippy-exceptions
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

### PR 8 (no-panic baseline)
```bash
cargo test -p xtask no_panic --locked
cargo xtask no-panic check --mode blocking
cargo xtask policy-report
git diff --check
```

### PR 9 (file allowlists)
```bash
cargo xtask non-rust inventory
cargo xtask check-file-policy --mode advisory
cargo xtask policy-report
```

### PR 10 (ripr + mutation lanes)
```bash
cargo test -p xtask ripr mutation --locked
cargo xtask ripr-pr || true
cargo xtask mutants-pr --changed --dry-run
```

### PR 11 (CI lane policy)
```bash
cargo xtask ci plan --base origin/main --json-out target/ci/ci-plan.json
cargo xtask policy-report
```

### PR 12 (API cleanups)
```bash
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo xtask no-panic check --mode blocking
cargo xtask policy-report
git diff --check
```

### PR 13 (debt burndown)
```bash
cargo test -p <touched-crate> --locked
cargo clippy -p <touched-crate> --all-targets --all-features --locked -- -D warnings
cargo xtask no-panic check --mode blocking
cargo xtask no-panic baseline
cargo xtask policy-report
```

### PR 14 (0.4.0 prep)
```bash
cargo xtask package-surface
cargo xtask policy-report
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
git diff --check
```

### PR 15 (dry-run proof)
```bash
cargo xtask package-surface
cargo xtask policy-report
cargo xtask check-lint-policy
cargo xtask check-clippy-exceptions
cargo xtask no-panic check --mode blocking
cargo xtask check-file-policy --mode blocking-allowlist
cargo publish --dry-run -p shipper-duration   # ... all 13 crates in publish order
```

## Bot / CI Response Rules

For every failing CI run:
1. Identify the first real failing command.
2. Reproduce locally where possible.
3. Fix only that failure.
4. Rerun the matching local gate.
5. Push.
6. Check bot comments again.

For bot comments:
- Real defect → fix.
- False positive → reply with evidence and reasoning.
- Style-only but cheap and in scope → fix.
- Out of scope for the PR → defer with a follow-up issue.
- Stale comment → verify current HEAD and mark stale.

## Self-Review Checklist

Before marking any PR ready for merge:

```markdown
## Self-review

- Scope matches PR title:
- Files touched are expected:
- No unrelated cleanup:
- Policy changes are intentional:
- No Clippy test carveouts added:
- No bare `#[allow(clippy::...)]` added:
- No-panic baseline handling is scoped:
- Non-Rust allowlist changes are narrow:
- Release dogfood proof preserved:
- Trusted Publishing/resume behavior unchanged unless scoped:
- Local validation:
- CI status:
- Bot comments addressed:
- Follow-ups:
```

If any item is not true, do not merge.
