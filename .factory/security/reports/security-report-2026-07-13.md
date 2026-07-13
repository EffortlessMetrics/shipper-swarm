# Security Scan Report

**Generated:** 2026-07-13
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/shipper-swarm
**Severity Threshold:** medium

## Executive Summary

| Severity | Count | Auto-fixed | Manual Required |
|----------|-------|------------|-----------------|
| CRITICAL | 0 | 0 | 0 |
| HIGH | 0 | 0 | 0 |
| MEDIUM | 0 | 0 | 0 |
| LOW | 0 | 0 | 0 |

**Total Findings:** 0
**Auto-fixed:** 0
**Manual Review Required:** 0

The weekly scan of `droid/security-report-2026-07-13` over the last 7 days
(2026-07-06 through 2026-07-13) examined the only commit that landed in
the window on `main`: `aca135e fix(engine): project pending reconciliation
state (#160)`. This commit is the initial migration commit of the
`EffortlessMetrics/shipper-swarm` repository, re-homing the existing
`EffortlessMetrics/shipper` codebase into the active-development repository
per `docs/status/SWARM_OPERATION.md`. It carries 1,319 files changed /
184,746 insertions, but the functional delta that is *new code* (rather
than a copy of pre-existing security-reviewed code) is the engine
reconciliation module (`crates/shipper-core/src/engine/parallel/reconcile.rs`),
which is the only file the commit's message describes as the actual fix.

The reconcile module:

- accepts `crate_name` and `version` from the plan (cargo-validated,
  already in the registry-trust boundary TB-1);
- wraps the existing `readiness::is_version_visible_with_backoff`
  polling loop, which already enforces bounds on retries, jitter, and
  total wait time;
- produces one of three `ReconciliationOutcome` variants
  (`Published` / `NotPublished` / `StillUnknown`) without invoking
  any new subprocess, opening any new socket, persisting any token,
  or mutating any filesystem path beyond the existing `.shipper/` state
  directory;
- carries no secrets, no user-controlled URLs, no shell construction,
  and no unsafe code (the workspace still forbids unsafe).

No application code, auth/token-resolution path, encryption path,
state-persistence path, webhook path, or subprocess invocation
surface was altered in a way that introduces a security finding at
the configured severity threshold.

## Critical Findings

None.

## High Findings

None.

## Medium Findings

None.

## Low Findings

None.

## Observations (Below Severity Threshold, Not Reported as Findings)

These are not findings under the configured `medium` threshold. They are
recorded for the next weekly scan and for engineering awareness; no
remediation is required for this report.

### OBS-1: Initial commit is a wholesale migration

**Description:**
The single commit in scope is the initial commit of the `shipper-swarm`
working repository. It carries 1,319 changed files / 184,746 insertions,
which is the migrated contents of the original `EffortlessMetrics/shipper`
release-authority repository. The functional code change introduced by
this commit, beyond the migration, is the new reconciliation module at
`crates/shipper-core/src/engine/parallel/reconcile.rs`. The remaining
deltas are re-homing existing, security-reviewed code into the dev
repository (per `docs/status/SWARM_OPERATION.md`).

**Implication for the scan:**
- The threat model is applied to the migrated tree as a whole (see
  `docs/INVARIANTS.md` and the threat model at `.factory/threat-model.md`).
- The previous weekly scan (`security-report-2026-06-29.md`) applied
  the same lens to the original source-of-truth repo at this exact
  commit hash family; no new code surface is added beyond the
  reconcile module.
- No engine (`shipper-core`), CLI (`shipper-cli`), or facade
  (`shipper`) crate delta other than the reconcile module is in
  scope for security review in this weekly window.

### OBS-2: Reconciliation module is well-bounded (STRIDE pass)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | All |
| **CWE** | n/a (no finding) |
| **File** | `crates/shipper-core/src/engine/parallel/reconcile.rs` |
| **Status** | Code committed; no mitigation required |

**Description:**
The new `reconcile_ambiguous_upload` function is a thin wrapper that
translates the output of `readiness::is_version_visible_with_backoff`
into a three-outcome `ReconciliationOutcome` enum. STRIDE pass:

- **Spoofing**: The registry client is constructed upstream (engine
  bootstrap) with no new hostname injection in this module.
- **Tampering**: No file is written; only `PublishEvent`s are emitted
  via the existing event log (events-as-truth contract).
- **Repudiation**: The module returns a per-call `Vec<ReadinessEvidence>`
  that the caller attaches to the receipt; nothing is hidden.
- **Information Disclosure**: No tokens, paths, or sensitive fields are
  copied into the new `ReconciliationOutcome` variants.
- **Denial of Service**: Bounded by `config.max_total_wait` from the
  existing readiness config; the module adds no new sleep/channels.
- **Elevation of Privilege**: No new subprocess, no new filesystem
  path, no new secret source.

**Why this is not a finding:** The module exercises code paths that
were already in the readiness module (which is fuzz-tested and
proptest-covered). The functional change is purely an enum translation.

### OBS-3: Float → integer conversion in readiness scheduler is safe

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Denial of Service (capped) |
| **CWE** | CWE-190 (Integer overflow), but capped |
| **File** | `crates/shipper-core/src/engine/parallel/readiness.rs:127` |
| **Status** | Existing behavior, no change in this commit |

**Description:**
The backoff computation in `is_version_visible_with_backoff_and_events`
multiplies `base_delay * 2^attempt` and then multiplies by a jitter
factor in milliseconds, casting to `u64`. `saturating_sub` clamps the
`pow` exponent at 16 to avoid overflow. This is unchanged from the
prior snapshot; the new reconcile module does not exercise a fresh
path through this code. Recorded here for the next scan to re-confirm
when `ReadinessConfig` evolves.

**Mitigation already in place:** `saturating_pow`. max_delay cap.
jitter_factor is configuration-controlled and validated by
`shipper-config`'s runtime validation.

### OBS-4: Floating action tags (carried over from OM-2)

**Description:**
Several GitHub Action references in this commit use floating tags
rather than commit SHAs:

- `actions/checkout@v7.0.0` (`coverage.yml`, `ci.yml`, `release.yml`,
  `droid.yml`, `droid-review.yml`, `droid-security-scan.yml`, `fuzz.yml`,
  `architecture-guard.yml`, `mutation.yml`, `live-runner-interruption-rehearsal.yml`,
  `ripr.yml`, `runner-routing-guard.yml`, `em-ci-routed-rust.yml`,
  `dependabot.yml`)
- `taiki-e/install-action@v2` (CI install jobs)
- `softprops/action-gh-release@v3` (release train)
- `codecov/codecov-action@v7` (coverage)
- `rust-lang/crates-io-auth-action@v1` (release OIDC)
- `dtolnay/rust-toolchain@stable` and `@nightly` (CI)
- `actions/cache@v6`, `actions/upload-artifact@v7`,
  `actions/download-artifact@v8` (artifacts)

Actions that are SHA-pinned:

- `EffortlessMetrics/droid-action-safe@<sha>` (Droid executor)
- `oven-sh/setup-bun@<sha>` (Bun installer)

**Status:** Accepted risk, tracked as `OM-2` in `.factory/threat-model.md`.
No remediation required for this scan window.

## Appendix

### Threat Model

- **Version:** 2026-06-29 (14 days old at scan time)
- **Location:** `.factory/threat-model.md`
- **Status:** Current — generated on 2026-06-29 by
  `droid/security-report-2026-06-29`. Next scheduled regen is
  2026-09-27 (90-day cadence) or earlier on any material change to
  trust boundaries TB-1 through TB-6.
- **Mitigations verified:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case). All three remain
  tracked at the same status.

### Scan Metadata

- **Commits scanned:** 1
- **Commit:** `aca135e fix(engine): project pending reconciliation state (#160)`
- **Scan window:** 2026-07-06 to 2026-07-13 (last 7 days, UTC)
- **Scan duration:** < 5 minutes (single commit, no new surface beyond
  the reconcile module)
- **Branch:** `droid/security-report-2026-07-13`
- **Severity threshold:** medium
- **Skills used:** threat-model-generation (cached, not regenerated),
  commit-security-scan, vulnerability-validation, security-review
- **Files inspected (security-sensitive surface):**
  - `crates/shipper-core/src/engine/parallel/reconcile.rs` (NEW — the
    commit's stated fix)
  - `crates/shipper-core/src/engine/parallel/readiness.rs` (re-read —
    direct dependency of the reconcile module)
  - `crates/shipper-core/src/ops/cargo/mod.rs` (re-read — subprocess
    path that reconcile may resolve into)
  - `crates/shipper-core/src/ops/process/run/command_builder.rs` (re-read
    — confirms `Command::new` + `args()` is the only spawn primitive;
    no `sh -c`, no shell expansion)
  - `crates/shipper-core/src/ops/auth/resolver.rs` (re-read — token
    resolution precedence chain)
  - `crates/shipper-core/src/ops/auth/credentials.rs` (re-read — TOML
    parsing)
  - `crates/shipper-core/src/ops/auth/oidc.rs` (re-read — OIDC detection)
  - `crates/shipper-output-sanitizer/src/lib.rs` (re-read — redaction)
  - `crates/shipper-encrypt/src/lib.rs` (re-read — AES-256-GCM + PBKDF2)
  - `crates/shipper-webhook/src/lib.rs` (re-read — HMAC signature, URL
    scheme validation)
  - `crates/shipper-registry/src/http.rs` (re-read — `reqwest` defaults,
    rustls only)
  - `crates/shipper-cli/src/doctor/redaction.rs` (re-read — query +
    userinfo redaction)
  - `crates/shipper-config/src/runtime_options/secrets.rs` (re-read —
    webhook/encryption overrides)
  - `.github/workflows/droid-security-scan.yml` (re-read — confirms
    scope of this scan)
  - `.github/workflows/release.yml` (re-read — OIDC mint, fallback
    secrets, environment binding)
  - `.github/workflows/ci.yml` (re-read — least-privilege
    `permissions:` blocks)
  - `.github/workflows/coverage.yml` (re-read — `CODECOV_TOKEN` scope)
  - `.github/workflows/fuzz.yml` (re-read — fuzz targets, no network)
  - `.github/workflows/droid-review.yml`, `.github/workflows/droid.yml`
    (re-read — `MINIMAX_API_KEY` is a custom-model BYOK key, not a
    privileged token; SHA-pinned action)
  - `.github/dependabot.yml` (re-read — weekly Rust + GitHub Actions
    bumps)
  - `SECURITY.md` (re-read — public disclosure policy, token-handling
    table)

### Lenses Applied

Per the shipper review-invariants context:

1. **STRIDE** for the entire security-sensitive surface (see threat
   model). Special attention on **I-1 token leakage via logs** (no
   new emission paths) and **E-1 code execution via workspace** (no
   new subprocess paths).
2. **OWASP Top 10** for any web/CLI surface:
   - A01 Broken Access Control — still verified by the advisory lock
     and plan ID validation.
   - A02 Cryptographic Failures — verified by `shipper-encrypt`
     re-export and `pbkdf2/hmac/sha2/aes-gcm` pinned at digest
     0.10 (per `dependabot.yml` carve-out).
   - A03 Injection — verified by `Command::new` + `args(&[...])`
     (no `sh -c` in `ops::process::run::command_builder`).
   - A04 Insecure Design — events-as-truth invariant applies
     unchanged to the new reconcile module.
   - A05 Security Misconfiguration — per-job `permissions:` blocks
     unchanged.
   - A06 Vulnerable & Outdated Components — `Cargo.lock` carried
     over from the upstream migration; dependabot still updates
     weekly.
   - A07 Identification & Authentication Failures — token-source
     precedence (`env_default > env_registry > credentials_file`)
     unchanged.
   - A08 Software & Data Integrity Failures — receipt/events/state
     triplet unchanged.
   - A09 Security Logging and Monitoring Failures — `events.jsonl`
     authoritative recording unchanged.
   - A10 SSRF — the engine has no user-controlled URL construction
     outside the cargo crates.io domain and the user-configured
     registry base URL. The new reconcile module does not change
     URL handling.
3. **OWASP LLM Top 10** is not directly applicable (no LLM boundary
   in the engine), but the Droid workflow layer is reviewed
   separately by `droid-review` and `droid-security-scan` automations.

### Reconciliations Against the Previous Scan

| Item | Previous scan (2026-06-29) | This scan (2026-07-13) | Net change |
|------|----------------------------|------------------------|------------|
| Threat model version | newly generated (2026-06-29) | reused (14 days old) | None — within 90-day window |
| CRITICAL findings | 0 | 0 | 0 |
| HIGH findings | 0 | 0 | 0 |
| MEDIUM findings | 0 | 0 | 0 |
| OM-1 (Reconcile) | Tracked | Partially addressed (the new code is the in-scope answer) | Improved |
| OM-2 (floating action tags) | Tracked | Tracked; no new tags introduced | No change |
| OM-3 (output sanitizer OSC) | Tracked | Tracked; no change | No change |

### References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- [docs/INVARIANTS.md](../../INVARIANTS.md) — events-as-truth contract
- [docs/status/SWARM_OPERATION.md](../../status/SWARM_OPERATION.md) —
  active-development / release-authority split
- [SECURITY.md](../../../SECURITY.md) — project security policy
- `.factory/threat-model.md` — reused threat model (cached)
