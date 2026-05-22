# Non-Rust File-Policy Rollout

This is the archived tracker for the 12-PR rollout that turned Shipper's
planned file-policy infrastructure into a real, enforceable system. The
umbrella issue is [#180](https://github.com/EffortlessMetrics/shipper/issues/180);
each PR below has its own issue. Current policy status comes from
`cargo xtask policy-report`.

## Operating rules

- **Receipts, not burn-down.** Every current non-Rust file is allowed to remain if it has a receipt with `owner` and `reason`. The receipt is the contract, not the cleanup ticket.
- **Reason `"scheduled to be converted to Rust/xtask"` is acceptable** when the file exists for legacy compatibility or migration staging. Pair it with an `expires` date so the receipt does not silently outlive its rationale.
- **Strict default, owned exception.** The allowlist means "known surface, owner, reason, and current disposition" — never "approved forever."
- **One SRP PR per issue.** Do not bury docs, ledgers, enforcement, proposal generation, workflow receipts, and `xtask` scaffolding in one review.
- **Advisory before blocking.** Every new check ships in advisory mode and graduates only after the receipt set is in place.
- **No `blocking-strict` in this rollout.** Strict mode (fails on unused entries and stale review dates) waits until after a dedicated cleanup pass.

## Ladder

| PR | Issue | Title | Status | Depends on |
|---:|---|---|---|---|
| 1/12 | [#201](https://github.com/EffortlessMetrics/shipper/issues/201) | `docs(policy): clarify non-Rust allowlist rollout status` | landed | — |
| 2/12 | [#202](https://github.com/EffortlessMetrics/shipper/issues/202) | `chore(policy): add non-Rust policy allowlist ledgers` | landed | PR 1 |
| 3/12 | [#203](https://github.com/EffortlessMetrics/shipper/issues/203) | `chore(policy): receipt high-risk non-Rust surfaces` | landed | PR 2 |
| 4/12 | [#212](https://github.com/EffortlessMetrics/shipper/issues/212) | `feat(xtask): add non-Rust inventory command` | landed | — (relates to [#176](https://github.com/EffortlessMetrics/shipper/issues/176)) |
| 5/12 | [#204](https://github.com/EffortlessMetrics/shipper/issues/204) | `feat(policy): check non-Rust file allowlist` | landed | PR 2, PR 4 |
| 6/12 | [#205](https://github.com/EffortlessMetrics/shipper/issues/205) | `feat(policy): propose non-Rust allowlist entries` | landed | PR 2, PR 4, PR 5 |
| 7/12 | [#206](https://github.com/EffortlessMetrics/shipper/issues/206) | `feat(policy): check generated, executable, and dependency surfaces` | landed | PR 2, PR 4 |
| 8/12 | [#207](https://github.com/EffortlessMetrics/shipper/issues/207) | `feat(policy): check workflow, process, and network surfaces` | landed | PR 2, PR 3, PR 4 |
| 9/12 | [#208](https://github.com/EffortlessMetrics/shipper/issues/208) | `feat(policy): add unified policy report` | landed | PR 5–8 |
| 10/12 | [#209](https://github.com/EffortlessMetrics/shipper/issues/209) | `ci(policy): run file policy checks advisory` | landed | PR 5–9 |
| 11/12 | [#210](https://github.com/EffortlessMetrics/shipper/issues/210) | `ci(policy): require non-Rust file policy allowlist` | landed | PR 10 |
| 12/12 | [#211](https://github.com/EffortlessMetrics/shipper/issues/211) | `ci(policy): require process and network policy receipts` | landed | PR 10, PR 11 |

Tracked under the 0.4.0 quality rollout.

## Receipt schema

Every allowlist entry should answer four questions:

1. **What surface?** (`kind`, `surface`, `classification`)
2. **Who owns it?** (`owner`)
3. **Why does it exist?** (`reason`)
4. **What covers it?** (`covered_by` — tests, manual review, scheduled `xtask` check)

Plus the operational fields:

- `created` — when this entry was first added.
- `review_after` — when the receipt should be re-examined.
- `expires` — optional; when the entry must be removed or renewed.

### Examples

A durable receipt:

```toml
[[file]]
path = "codecov.yml"
kind = "coverage_config"
surface = "ci"
classification = "config"
owner = "release/ci"
reason = "Codecov status and reporting configuration."
covered_by = ["codecov upload workflows", "cargo xtask check-file-policy"]
created = "2026-05-11"
review_after = "2026-06-11"
```

A transitional receipt:

```toml
[[glob]]
pattern = "scripts/**/*.sh"
kind = "legacy_shell_tooling"
surface = "tooling"
classification = "tooling"
owner = "release/ci"
reason = "Legacy shell helper retained for current release workflow; scheduled to be converted to cargo xtask once the release path is stable."
covered_by = ["cargo xtask policy-report"]
created = "2026-05-11"
review_after = "2026-05-25"
expires = "2026-08-11"
```

Both are valid. Both are visible. Both have an owner.

## Definition of done for the completed ladder

The ladder is complete when these checks stay true:

- Every current non-Rust file is receipted.
- Every receipt has `owner` and `reason`.
- "Scheduled to be converted" entries are accepted but visible.
- `cargo xtask non-rust inventory` works.
- `cargo xtask non-rust propose` works.
- `cargo xtask check-file-policy --mode blocking-allowlist` works.
- Generated, executable, dependency, workflow, process, and network checks are clean.
- `cargo xtask policy-report` emits Markdown and JSON.
- CI runs the policy checks and uploads the policy report as an artifact.
- A new anonymous non-Rust file fails the blocking policy gate.

Current proof command:

```bash
cargo xtask policy-report
```
