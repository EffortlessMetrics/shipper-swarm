# Security Scan Report

**Generated:** 2026-07-20
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/shipper-swarm
**Severity Threshold:** medium

## Executive Summary

| Severity | Count | Auto-fixed | Manual Required |
|----------|-------|------------|-----------------|
| CRITICAL | 0     | 0          | 0               |
| HIGH     | 0     | 0          | 0               |
| MEDIUM   | 0     | 0          | 0               |
| LOW      | 0     | 0          | 0               |

**Total Findings:** 0
**Auto-fixed:** 0
**Manual Review Required:** 0

The weekly scan of `droid/security-report-2026-07-20` over the last 7
days (2026-07-13 through 2026-07-20, UTC) examined the single commit
that landed on `main` in the window: `79bd268 policy(non-rust):
receipt for routed rust normalization script (#177)`. That commit is a
wholesale squash-merge of PR #177 — the policy allowlist rollout that
lands the file-policy, workflow/process/network allowlist ledgers,
the xtask check ladder, and the routed-Rust-small workflow that
introduces self-hosted runner selection with a GitHub-hosted
fallback lane. The commit carries 1,320 changed files / 186,362
insertions.

The functional delta in this commit is concentrated in three areas:

1. **Policy allowlist infrastructure.** New TOML ledgers under
   `policy/` (`workflow-allowlist.toml`, `process-allowlist.toml`,
   `network-allowlist.toml`, `non-rust-allowlist.toml`,
   `dependency-surface-allowlist.toml`, `executable-allowlist.toml`,
   `generated-allowlist.toml`) plus matching check subcommands in
   `xtask/` (`check-workflow-surfaces`, `check-process-policy`,
   `check-network-policy`, `check-file-policy`, `check-generated`,
   `check-executable-files`, `check-dependency-surfaces`). The
   checkers are pure-grep or AST-based (via `syn`/`toml`/`serde_json`)
   reads of tracked files plus the matching ledger; they emit
   `target/policy/` reports and never mutate workspace state.

2. **Routed Rust-small workflow.** New
   `.github/workflows/em-ci-routed-rust.yml` selects one of three
   self-hosted targets (`cx43`, `cpx42`, `cx53`) via a GitHub-API
   runner probe, with a constrained `gh pr checkout <numeric>` shim
   and a GitHub-hosted fallback lane for bot-authored PRs. The
   `runner-routing-guard.yml` workflow plus
   `scripts/ci/no-bare-self-hosted.sh` enforce the runner-label
   policy. `scripts/ci/normalize-routed-rust-result.py` collapses
   the routed result matrix into one blocking decision; it is
   pure-logic (env-var in, exit-code out) and has no subprocess or
   file I/O surface.

3. **Droid automation hardening.** `.github/workflows/droid.yml`,
   `droid-review.yml`, and `droid-security-scan.yml` install the
   Droid action by SHA, build a constrained `gh` shim that only
   accepts `gh pr checkout <numeric>`, configure the
   MiniMax-M3 BYOK customModel in `$HOME/.factory/settings.json`,
   and run `EffortlessMetrics/droid-action-safe@<sha>`. The PR
   routing job (`em-ci-routed-rust.yml`) refuses to dispatch
   self-hosted lanes on fork PRs
   (`if: github.event.pull_request.head.repo.full_name == github.repository`)
   and refuses to consume the runner-read token for bot-authored
   PRs (the documented trust-bootstrap condition routes them to
   `runs-on: ubuntu-latest`).

No application code in an auth, token-resolution, encryption,
state-persistence, or subprocess-invocation path was altered in a
way that introduces a finding at the configured `medium` severity
threshold.

The repository remains in a strong security posture: `unsafe_code =
"forbid"` is enforced workspace-wide, every event/state/receipt
triplet is preserved, the output sanitizer redacts tokens before
persistence, the encryption crate uses AES-256-GCM with PBKDF2
(100,000 iterations) and per-call random salt + nonce, the registry
HTTP client enforces timeouts, and the fuzzer corpus
(`fuzz/fuzz_targets/`) covers token resolution, encryption, output
redaction, plan building, retry, and webhook payload construction.

Build verification: `cargo build --workspace --all-targets` succeeds
against the commit under review. `cargo audit --json` against the
shipped `Cargo.lock` reports `vulnerabilities.found = false` for
all 377 dependencies.

## Critical Findings

None.

## High Findings

None.

## Medium Findings

None.

## Low Findings

None.

## Observations (Below Severity Threshold, Not Reported as Findings)

These are not findings under the configured `medium` threshold. They
are recorded for the next weekly scan and for engineering awareness;
no remediation is required for this report.

### OBS-1: Floating action tags in CI workflows (carried-over OM-2)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Tampering (supply chain) |
| **CWE** | CWE-1357 (Reliance on Untrusted Component) |
| **Files** | `.github/workflows/*.yml` (all 14 workflows) |
| **Status** | Accepted risk, tracked under OM-2 in `.factory/threat-model.md` |

**Description:**
Workflow references to third-party GitHub Actions use floating major
tags rather than commit SHAs. Examples observed across the scan
window: `actions/checkout@v7.0.0`, `dtolnay/rust-toolchain@stable`,
`dtolnay/rust-toolchain@nightly`, `dtolnay/rust-toolchain@v1`,
`taiki-e/install-action@v2`, `taiki-e/install-action@cargo-audit`,
`actions/cache@v6`, `actions/upload-artifact@v7`,
`actions/download-artifact@v8`, `codecov/codecov-action@v7`,
`softprops/action-gh-release@v3`,
`rust-lang/crates-io-auth-action@v1`. The Droid-related actions
(`EffortlessMetrics/droid-action-safe@7c1377c...`,
`oven-sh/setup-bun@0c5077e5...`) and the direct `actions/checkout`
reference in the Droid workflows are SHA-pinned.

**Risk:** A compromise of an upstream major tag could push arbitrary
code into CI. Most jobs hold `contents: read` and run on
self-hosted runners with fork-PR guards. The release workflow
(`release.yml`) uses `rust-lang/crates-io-auth-action@v1` to
exchange an OIDC token for a short-lived crates.io token, but that
path is gated by
`if: github.repository == 'EffortlessMetrics/shipper' && github.event_name == 'push'`
and therefore inert in `shipper-swarm` (the dev repo).

**Mitigation already in place:**
- Dependabot is configured (`dependabot.yml` -> `github-actions`
  ecosystem) to bump Actions weekly against this repo.
- The release workflow is gated to the release-authority repo
  (`EffortlessMetrics/shipper`).
- Fork-PR guards are added to every self-hosted job; untrusted fork
  PRs cannot trigger job execution with secrets.
- The runner-routing guard (`runner-routing-guard.yml` +
  `scripts/ci/no-bare-self-hosted.sh`) rejects bare
  `runs-on: self-hosted` declarations, restricting where these jobs
  can land.
- Per-job `permissions:` blocks scope each job to the least
  privilege it needs (most jobs hold `contents: read`).

**Recommended Hardening (Optional):**
Pin all third-party Actions to commit SHAs (mirror the pattern
already used for `EffortlessMetrics/droid-action-safe` and
`oven-sh/setup-bun`). Dependabot's group updates will continue to
bump the SHA in lockstep. Trade-off: reduced upstream agility for
tighter supply-chain posture. Not required for this scan window.

### OBS-2: Single commit in scope is a wholesale policy/receipt rollout

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Not applicable (informational) |
| **CWE** | Not applicable |
| **Files** | (commit scope) |
| **Status** | Migration artefact; informational only |

**Description:**
The single commit in scope is the wholesale squash-merge of
PR #177, the policy(non-rust) rollout. It carries 1,320 changed
files / 186,362 insertions. The functional changes introduced by
this commit are concentrated in (a) the `policy/*.toml` allowlist
ledgers and matching `xtask/src/*` checker subcommands, (b)
`.github/workflows/em-ci-routed-rust.yml` plus its routing-guard
companion and the `scripts/ci/normalize-routed-rust-result.py`
helper, and (c) the Droid workflow trio (`droid.yml`,
`droid-review.yml`, `droid-security-scan.yml`).

**Implication for the scan:** the threat model
(`.factory/threat-model.md`, generated 2026-06-29) applies to the
post-rollout tree as a whole. The functional delta relative to the
prior source of truth lies entirely within policy/receipt/Rust CI
plumbing; it does not touch the auth resolver, encryption crate,
HTTP client, output sanitizer, webhook transport, state store, or
subprocess invocation. No delta in any of those surfaces is in
scope for this weekly window.

### OBS-3: Whitespace-only token values are treated as valid credentials (carried-over OBS-3)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Spoofing / Tampering (low impact) |
| **CWE** | CWE-20 (Improper Input Validation), informational only |
| **File** | `crates/shipper-core/src/ops/auth/resolver.rs` |
| **Status** | Matches cargo behaviour; no fix recommended |

**Description:**
The `resolve_token` implementation treats an env var whose value is
a whitespace-only string as a valid token and returns it as the
resolved credential (the empty-check is `!token.is_empty()`, not
`!token.trim().is_empty()`). There is no upstream remediation
because cargo itself preserves whitespace-only tokens through
`cargo login` and through the env-var precedence chain. Behaviour
is pinned by the `snapshot_resolve_whitespace_token` insta test in
the same file.

**Risk:** minimal. A user who explicitly sets
`CARGO_REGISTRY_TOKEN="   "` in their environment gets a whitespace
token; the only consequence is that any subsequent publish attempt
will be rejected by crates.io with HTTP 403, which the existing
`ErrorClass::Permanent` classifier surfaces to the user. No token
is leaked, no privilege escalation is possible, and no other
caller is reachable from this state.

**Recommended action:** none. Documented for future auditability.

### OBS-4: Output sanitizer OSC without terminator consumes to EOF (carried-over OM-3)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Denial of Service (bounded) |
| **CWE** | CWE-400 (Uncontrolled Resource Consumption), informational only |
| **File** | `crates/shipper-output-sanitizer/src/lib.rs` (`strip_ansi`) |
| **Status** | Accepted behaviour, tracked under OM-3 in `.factory/threat-model.md` |

**Description:** `strip_ansi` consumes an unterminated OSC sequence
(`\x1b]...`) to EOF. There is no security impact (no panic, no
out-of-memory condition, no leaked token), but evidence truncation
could cause operator confusion during incident review. Behaviour is
pinned by the OSC-handler branch in the same file.

**Recommended action:** none. Documented under OM-3.

### OBS-5: `mask_token` exposes first and last four characters (carried-over OBS-5)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Information Disclosure (bounded) |
| **CWE** | CWE-200 (Exposure of Sensitive Information), informational only |
| **File** | `crates/shipper-core/src/ops/auth/resolver.rs` |
| **Status** | Standard display-masking pattern; no fix recommended |

**Description:** For tokens longer than 8 characters, `mask_token`
returns `<first 4>****<last 4>`. Tokens of 8 characters or fewer
are fully masked. This is the standard display-masking pattern used
by cargo, gh, and other registry CLIs.

**Risk:** minimal. The exposed prefix/suffix carries at most 8 ASCII
characters of token entropy, which is insufficient to recover a
crates.io style token. Display-masked tokens are not usable for
publishing.

**Recommended action:** none.

### OBS-6: Routed-Rust-small fallback lane consumes the same Rust-small gate on GitHub-hosted runners

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Tampering (cost vs coverage trade-off) |
| **CWE** | CWE-778 (Insufficient Logging), informational only |
| **File** | `.github/workflows/em-ci-routed-rust.yml` (`rust_small_github`) |
| **Status** | Documented design choice; informational only |

**Description:**
The `rust_small_github` fallback job runs the same Rust-small gate
as the self-hosted lanes (`cargo check --workspace --locked --all-targets`
+ `cargo nextest run --workspace --locked --all-targets --all-features --profile ci`
+ `cargo test --workspace --locked --doc` + CLI help smoke). It runs
on `ubuntu-latest` (GitHub-hosted, no repository secrets consumed)
and is reached only when (a) the routing decision is `github` and
`fallback_allowed == 'true'` (force-dispatch or bot-authored PR with
no runner-read token), or (b) the selected self-hosted lane was
`cancelled`. The previous tiny fallback (cargo check + `--help`
only) was the documented root cause of the #417 error-renderer
regression escaping detection for weeks.

**Risk:** minimal. The fallback lane is reachable only by bot-authored
PRs (Dependabot, factory-droid) whose diffs are trivially auditable
and whose checkout-time secret surface is empty (GitHub masks
repository secrets for bot users). The lane runs
`cargo nextest run --all-features` against the workspace, which is
the same test matrix the self-hosted lanes run, so any
green/false-negative on the fallback is also a green/false-negative
on the self-hosted lanes. The cost trade-off is real (Actions
minutes instead of self-hosted capacity) and is accepted.

**Recommended action:** none.

### OBS-7: `process-allowlist.toml` lists `sudo` for CI cross-compile prep

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Elevation of Privilege (bounded) |
| **CWE** | CWE-269 (Improper Privilege Management), informational only |
| **File** | `policy/process-allowlist.toml` |
| **Status** | Documented design choice; informational only |

**Description:**
The `ci` process profile enumerates `sudo` among its permitted
commands, with the `reason` field naming `apt-get install
gcc-aarch64-linux-gnu` cross-compile prep. This is allowed because
the self-hosted runners carry the `em-ci` group label and run as a
user with limited sudo scope; the network/process policy checker
(`xtask check-process-policy`) reconciles each workflow's actual
`run:` block against its declared process profile, and a workflow
introducing an unsanctioned `sudo` invocation will fail
`blocking-allowlist` mode.

**Risk:** minimal. The scope of `sudo` is bounded by (a) the
runner being a known self-hosted runner in the `em-ci` group, (b)
the workflow declaring the `ci` process profile, and (c) the
checker refusing any unsanctioned `sudo` invocation.

**Recommended action:** none. Documented for auditability.

### OBS-8: MiniMax-M3 customModel writes the resolved API key into `$HOME/.factory/settings.json`

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Information Disclosure (bounded) |
| **CWE** | CWE-312 (Cleartext Storage of Sensitive Information), informational only |
| **File** | `.github/workflows/droid.yml`, `.github/workflows/droid-review.yml`, `.github/workflows/droid-security-scan.yml` |
| **Status** | Standard BYOK pattern; no fix recommended |

**Description:**
The Droid workflow trio resolves `${{ secrets.MINIMAX_API_KEY }}`
into a `customModels[0].apiKey` field of a heredoc-written
`$HOME/.factory/settings.json`. The file is created on the runner
and lives only for the duration of the job; the runner's `actions`
permissions block scopes the job to the minimum required
(`contents: read` or `contents: write` for the review workflow, plus
`pull-requests: write` / `issues: write` / `id-token: write` /
`actions: read` as needed). The secret is never written to a
workflow `run.log` artifact beyond the secret-redaction boundary
applied by GitHub.

**Risk:** minimal. The customModel entry is consumed only by the
`EffortlessMetrics/droid-action-safe` step on the same runner, and
GitHub's secret-redaction keeps `MINIMAX_API_KEY` out of the
captured stdout/stderr. A runner-filesystem attacker would already
have full access to the runner's `secrets.*` material, so writing
the same value into a settings file does not expand the trust
boundary.

**Recommended action:** none.

## Threat Model

- **Version:** carried over from 2026-06-29
- **Location:** `.factory/threat-model.md`
- **Age at scan time:** 21 days (within the 90-day regen window)
- **STRIDE coverage:** Spoofing, Tampering, Repudiation, Information
  Disclosure, Denial of Service, Elevation of Privilege
- **Trust boundaries enumerated:** 6 (TB-1 through TB-6)
- **Mitigations verified in code:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case)
- **Next regen due:** 2026-09-27 (90 days) or sooner on any change
  to TB-1 through TB-6

The trust boundaries, mitigations, and open risks from the threat
model remain valid. The new TB-4 (Webhook receiver) and TB-5
(GitHub Actions / Droid workflow) surfaces introduced by the
policy/receipt rollout add two specific bindings already covered by
the existing mitigations:

- **TB-5 hardening in this commit.** The Droid workflow trio uses
  SHA-pinned actions for the highest-trust steps
  (`EffortlessMetrics/droid-action-safe@7c1377cc...`,
  `oven-sh/setup-bun@0c5077e5...`, the direct
  `actions/checkout@9c091bb...`). The PR-routing job refuses
  self-hosted dispatch on fork PRs
  (`if: github.event.pull_request.head.repo.full_name == github.repository`)
  and refuses to consume the runner-read token for bot-authored
  PRs (the documented trust-bootstrap condition routes them to
  `runs-on: ubuntu-latest`). The `gh` shim is constrained to
  `gh pr checkout <numeric>` with explicit numeric-only validation.
- **TB-4 hardening.** The webhook crate (`crates/shipper-webhook`)
  continues to use HMAC-SHA256 signing with `X-Hub-Signature-256`,
  rejecting empty/whitespace secrets before computing the signature.

No threat-boundary change warrants regenerating the threat model
under the 90-day window.

## Scan Metadata

- **Commits scanned:** 1
- **Commit:** `79bd268 policy(non-rust): receipt for routed rust
  normalization script (#177)`
- **Commit author:** Steven Zimmerman, CPA
  <15812269+EffortlessSteven@users.noreply.github.com>
- **Commit date:** 2026-07-18 19:31:22 -0400
- **Scan window:** 2026-07-13 to 2026-07-20 (last 7 days, UTC)
- **Scan duration:** ~10 minutes
- **Branch:** `droid/security-report-2026-07-20`
- **Severity threshold:** medium
- **Skills used:** threat-model check (carry-over),
  commit-security-scan, vulnerability-validation (against the
  auth/encryption/HTTP/webhook/sanitizer/process-policy surface),
  security-review
- **Build status:** `cargo build --workspace --all-targets`
  succeeds, `cargo audit --json` reports
  `vulnerabilities.found = false` across all 377 dependencies in
  `Cargo.lock`.
- **Test status:** not run end-to-end this week (test-file clippy
  warnings exist on the `engine/parallel/tests.rs` and
  `cli/output/progress/tests.rs` files but are stylistic lint
  issues, not security findings).
- **Files inspected (security-sensitive surface):**
  - `crates/shipper-core/src/ops/auth/resolver.rs`
  - `crates/shipper-core/src/ops/auth/credentials.rs`
  - `crates/shipper-core/src/ops/auth/oidc.rs`
  - `crates/shipper-core/src/ops/auth/mod.rs`
  - `crates/shipper-core/src/ops/cargo/mod.rs`
  - `crates/shipper-core/src/ops/git/bin_override.rs`
  - `crates/shipper-core/src/ops/git/cleanliness.rs`
  - `crates/shipper-core/src/ops/git/context.rs`
  - `crates/shipper-core/src/ops/git/mod.rs`
  - `crates/shipper-core/src/ops/process/cargo.rs`
  - `crates/shipper-core/src/ops/process/run/command_builder.rs`
  - `crates/shipper-core/src/ops/process/run/execution.rs`
  - `crates/shipper-core/src/ops/process/run/mod.rs`
  - `crates/shipper-core/src/ops/process/mod.rs`
  - `crates/shipper-core/src/state/store/fs.rs`
  - `crates/shipper-core/src/state/store/mod.rs`
  - `crates/shipper-core/src/state/events/mod.rs`
  - `crates/shipper-core/src/state/execution_state/mod.rs`
  - `crates/shipper-core/src/state/rebuild.rs`
  - `crates/shipper-core/src/engine/mod.rs`
  - `crates/shipper-core/src/engine/transition.rs`
  - `crates/shipper-core/src/engine/parallel/webhook.rs`
  - `crates/shipper-core/src/engine/publish/finalize.rs`
  - `crates/shipper-core/src/engine/retry.rs`
  - `crates/shipper-core/src/lib.rs`
  - `crates/shipper-encrypt/src/lib.rs`
  - `crates/shipper-registry/src/http.rs`
  - `crates/shipper-registry/src/context.rs`
  - `crates/shipper-webhook/src/lib.rs`
  - `crates/shipper-cargo-failure/src/lib.rs`
  - `crates/shipper-output-sanitizer/src/lib.rs`
  - `crates/shipper-cli/src/doctor/redaction.rs`
  - `crates/shipper-cli/src/doctor/checks/auth.rs`
  - `crates/shipper-cli/src/doctor/checks/encryption.rs`
  - `fuzz/fuzz_targets/auth_token_resolve.rs`
  - `fuzz/fuzz_targets/redact_output.rs`
  - `xtask/src/main.rs`
  - `xtask/src/check_file_policy.rs`
  - `xtask/src/file_policy.rs`
  - `xtask/src/workflow_checks.rs`
  - `xtask/src/no_panic.rs`
  - `xtask/src/package_surface.rs`
  - `xtask/src/doc_contracts.rs`
  - `xtask/src/ripr.rs`
  - `.github/workflows/droid.yml`
  - `.github/workflows/droid-review.yml`
  - `.github/workflows/droid-security-scan.yml`
  - `.github/workflows/release.yml`
  - `.github/workflows/ci.yml`
  - `.github/workflows/coverage.yml`
  - `.github/workflows/em-ci-routed-rust.yml`
  - `.github/workflows/mutation.yml`
  - `.github/workflows/fuzz.yml`
  - `.github/workflows/ripr.yml`
  - `.github/workflows/live-runner-interruption-rehearsal.yml`
  - `.github/workflows/runner-routing-guard.yml`
  - `.github/workflows/architecture-guard.yml`
  - `.github/dependabot.yml`
  - `.github/settings.yml`
  - `.github/CODEOWNERS`
  - `.github/copilot-instructions.md`
  - `.github/actionlint.yaml`
  - `policy/workflow-allowlist.toml`
  - `policy/process-allowlist.toml`
  - `policy/network-allowlist.toml`
  - `policy/non-rust-allowlist.toml`
  - `policy/dependency-surface-allowlist.toml`
  - `policy/executable-allowlist.toml`
  - `policy/generated-allowlist.toml`
  - `policy/no-panic-baseline.json`
  - `scripts/ci/normalize-routed-rust-result.py`
  - `scripts/ci/no-bare-self-hosted.sh`
  - `SECURITY.md`
  - `Cargo.toml`
  - `Cargo.lock`

## Lenses Applied

Per the shipper review-invariants context:

1. **STRIDE** for the entire security-sensitive surface (see threat
   model). All six categories checked against the auth, encryption,
   registry HTTP, webhook transport, state persistence, and
   subprocess invocation surfaces. The new policy/process/network
   ledgers and their `xtask` checkers are pure-read operations
   against tracked files plus the matching ledger; they do not
   mutate workspace state and do not consume user input beyond the
   CLI args.
2. **OWASP Top 10** for any web/CLI surface: A01 (Broken Access
   Control) verified by the advisory file lock + plan-ID validation;
   A02 (Cryptographic Failures) verified by `shipper-encrypt`
   (AES-256-GCM + PBKDF2 100k iterations + per-call random
   salt/nonce) and the `pbkdf2/hmac/sha2/aes-gcm` ignore rules in
   `dependabot.yml`; A03 (Injection) verified by the absence of
   `sh -c`/shell pipelines in subprocess invocations and by
   `taiki-e`/`std::process::Command` controlled-argument paths; A04
   (Insecure Design) verified by the events-as-truth invariant and
   the schema-versioned `state.json` projection; A05 (Security
   Misconfiguration) verified by per-job `permissions:` blocks in
   every workflow; A06 (Vulnerable & Outdated Components) verified
   by `cargo audit --json` reporting zero known vulnerabilities in
   `Cargo.lock`; A07 (Identification and Authentication Failures)
   verified by the cargo-conventional token precedence and the
   OIDC dual-env-var detection; A08 (Software and Data Integrity
   Failures) verified by the receipt/events/state triplet and the
   runner-routing guard (`scripts/ci/no-bare-self-hosted.sh`);
   A09 (Security Logging and Monitoring Failures) verified by
   `events.jsonl` authoritative recording; A10 (SSRF) verified by
   `https://crates.io` as the only first-party base URL and the
   `reqwest` default client builder with timeout, plus the routed
   runner probe being constrained to `https://api.github.com` via
   `policy/network-allowlist.toml`.
3. **OWASP LLM Top 10** is not directly applicable (no LLM boundary
   inside the engine). The Droid workflow layer is reviewed
   separately by `droid-review` and `droid-security-scan`
   automations; the BYOK `customModels[0]` write into
   `$HOME/.factory/settings.json` is documented under OBS-8.
4. **STRIDE spot-checks at the auth boundary:** `AuthInfo` carries
   `source: TokenSource` so diagnostic output never accidentally
   displays the token; `mask_token` is exercised by proptest with
   `[A-Za-z0-9]` ASCII + edge lengths (0, 1, 8, 9, 200, 500) and
   confirmed never to expose the middle of tokens longer than 8
   characters (see OBS-5).
5. **STRIDE spot-checks at the policy-boundary:** the
   `xtask check-process-policy` and `xtask check-network-policy`
   checkers are grep/AST-based reads of `.github/workflows/*.yml`
   and reconciliation against the matching ledger; they do not
   invoke network calls, do not invoke subprocesses, and do not
   write outside `target/policy/`.
6. **STRIDE spot-checks at the routed-CI boundary:** the routing
   logic in `em-ci-routed-rust.yml::route` accepts `force_route`
   as a workflow-dispatch input or `auto` for PR/push/merge_group
   events; the GitHub-API probe uses `urllib.request` with an
   explicit 10-second timeout, only consumes
   `EM_RUNNER_READ_TOKEN` (no write token), and emits its decision
   via `$GITHUB_OUTPUT` plus `set -euo pipefail`-style error
   handling. The fallback lane runs the same Rust-small gate as the
   self-hosted lanes and is unreachable by fork PRs.

## References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- [docs/INVARIANTS.md](../../../INVARIANTS.md) - events-as-truth contract
- [docs/status/SWARM_OPERATION.md](../../../status/SWARM_OPERATION.md) -
  active-development / release-authority split
- [SECURITY.md](../../../../SECURITY.md) - project security policy
- `.factory/threat-model.md` - carried-over threat model (2026-06-29)
- Previous weekly scan: `.factory/security/reports/security-report-2026-07-13.md`
- Earlier weekly scan: `.factory/security/reports/security-report-2026-06-29.md`
