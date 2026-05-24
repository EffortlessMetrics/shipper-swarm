# Codex CI-Efficiency Compatibility Invariants

This section is **hard compatibility**, not optional tuning. CI-efficiency changes must preserve EffortlessMetrics queue semantics and classification boundaries.

> Do not optimize CI by blindly canceling active work or by routing metadata edits through Rust. Optimize by classifying changes correctly, keeping one active run, one pending replacement slot, and making default PR paths tiny.

## 1) Concurrency semantics (heavy/core workflows)

For heavy/core PR workflows, do **not** set `cancel-in-progress: true`.

Desired behavior is **single active run + single pending replacement slot**:

- If one run is executing, it continues.
- If a newer commit arrives, queue the newer run.
- If an even newer commit arrives while one is pending, replace the older pending run.
- When active run completes, run the latest pending one.

Required pattern:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: false
```

Rationale: canceling a near-complete heavy run wastes self-hosted runner time and cache progress and can increase queue churn.

## 2) Change classification requirements

Do **not** treat all changed files as Rust-code input.

Metadata/control-plane changes must stay on docs/policy/light paths unless mixed with real Rust/build/test changes. Examples:

- `docs/**`
- `*.md`
- `README*`, `CHANGELOG*`, `SECURITY*`, `CONTRIBUTING*`
- `policy/**`
- `plans/**`
- `badges/**`
- `AGENTS.md`
- `.github/CODEOWNERS`
- `.github/dependabot.yml`
- `.github/pull_request_template.md`
- `.github/PULL_REQUEST_TEMPLATE/**`
- `.codex/campaigns/**`
- `docs/tracking/**`
- `ci/hardware/**` receipt files
- `.rails/**`
- `.uselesskey/**`

Workflow files are special:

- `.github/workflows/**` are **not** docs-light.
- Route workflow edits to a minimal hosted workflow-validation/safety lane, not full Rust CI unless explicitly required.

## 3) Default PR routing policy

Default PR CI must classify first, then choose the **cheapest truthful lane**:

- docs/control-plane only → no Rust compile
- workflow-only → hosted YAML/workflow validation, no full Rust
- Rust source/build/test touched → Rust-small (self-hosted)
- hardware/GPU/receipt-only → syntax/receipt validation only
- unknown or mixed changes → Rust-small (not full CI)

Full CI should require explicit trigger (label, manual dispatch, main push, release, schedule, merge queue, or equivalent repo policy).

## 4) Hosted fallback policy

Do **not** silently replace a self-hosted Rust-small lane with a full GitHub-hosted equivalent.

- Fork PRs may use a tiny hosted safe lane.
- Missing runner readiness, token issues, or no idle self-hosted runner must not auto-trigger a long hosted fallback.
- Expensive hosted fallback must be explicitly gated (e.g. `full-ci`, `allow-github-hosted`, `ci-budget-ack`, or equivalent).

## 5) Artifact policy

Do not upload receipts/JUnit/log artifacts with `if: always()` on default PR lanes unless required by merge policy and kept tiny.

- Prefer upload-on-failure.
- Use short retention (3–7 days) for failure diagnostics.
- Avoid artifact uploads for docs/control-plane-only paths when possible.

## 6) Required checks for CI-only PRs

Every CI-efficiency PR must include:

1. `git diff --check`
2. YAML parse check for each edited workflow
3. Classification dry-run or shell/unit coverage for:
   - docs-only
   - `.rails/**`
   - `.uselesskey/**`
   - workflow-only
   - Rust file change
   - mixed docs + Rust
4. Explicit confirmation that heavy/core concurrency did not regress to `cancel-in-progress: true` unless intentionally documented

## “Do not do this” checklist

Reject CI-efficiency changes that do any of the following by default:

- flip heavy/core Rust CI to `cancel-in-progress: true`
- treat `.rails/**`, `.uselesskey/**`, `.codex/campaigns/**`, `docs/tracking/**`, `policy/**`, or receipt-only paths as Rust source changes
- route workflow edits through docs-light
- replace self-hosted Rust lanes with broad hosted equivalents when runners are busy
- add broad hosted fallback under the name “rust-small”
- upload artifacts on every PR run without branch-protection necessity
- add default PR matrix expansion that increases baseline runner burn
- run deny/fuzz/mutants/docs/examples/release/GPU/hardware suites on default PR paths without explicit classification and budget

## Reviewer gate (must answer yes)

When reviewing CI-efficiency PRs, require explicit answers:

1. Does this preserve `cancel-in-progress: false` for heavy/core CI?
2. Does this avoid Rust CI for metadata/control-plane-only edits?
3. Does this keep workflow changes out of docs-light?
4. Does this avoid silent expensive hosted fallback?
5. Does this reduce actual billable work instead of moving cost elsewhere?
