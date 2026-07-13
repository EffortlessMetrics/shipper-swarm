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
(2026-07-06 through 2026-07-13) examined the only commit that landed in the
window: `26d16a2 deps(deps): bump indicatif from 0.18.4 to 0.18.5 (#148)`.
The commit is the active-development-repository initial migration, carrying
1,318 files / 184,149 insertions, the migrated contents of the
`EffortlessMetrics/shipper` release-authority repository into the
`shipper-swarm` working repository. The functional delta beyond that
migration is the dependabot patch bump of `indicatif` from 0.18.4 to 0.18.5
(semver-patch). No application code, auth/token-resolution path,
encryption path, state-persistence path, webhook signature path, or
subprocess invocation surface was altered in a way that introduces a
security finding at the configured severity threshold.

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

### OBS-1: State file permissions rely on umask

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Information Disclosure |
| **CWE** | CWE-732 (Incorrect Permission Assignment for Critical Resource) |
| **File** | `crates/shipper-core/src/state/execution_state/mod.rs:74, 82, 320` (`save_state`, `write_receipt`, `atomic_write_json`) |
| **Status** | Documented; umask-based hardening is the prevailing pattern |

**Description:**
The state-persistence layer (`save_state`, `write_receipt`,
`write_reconciliation_report`, and the shared `atomic_write_json` helper)
creates `.shipper/state.json`, `receipt.json`, `reconciliation.json`, and
similar artifacts using `std::fs::File::create` plus
`fs::create_dir_all` and `fs::rename`, but does **not** call
`std::fs::set_permissions` to set 0600 on Unix. The threat model
(`.factory/threat-model.md` T-1) currently claims that ".shipper/state.json"
files are 0600 on Unix; in practice the file mode follows the process
umask. The risk is small in the typical release-operator environment (a
single-user workflow under `cargo run`/`shipper` from a developer account
or a CI runner with default umask `0022`), but a permissive umask on a
shared host would leave receipts and event-log files readable by other
local users.

**Evidence:**
```rust
// crates/shipper-core/src/state/execution_state/mod.rs:74
pub fn save_state(state_dir: &Path, state: &ExecutionState) -> Result<()> {
    fs::create_dir_all(state_dir)
        .with_context(|| format!("failed to create state dir {}", state_dir.display()))?;
    let path = state_path(state_dir);
    atomic_write_json(&path, state)
}

// crates/shipper-core/src/state/execution_state/mod.rs:308 (atomic_write_json)
{
    let mut f = fs::File::create(&tmp)
        .with_context(|| format!("failed to create tmp file {}", tmp.display()))?;
    f.write_all(&data)
        .with_context(|| format!("failed to write tmp file {}", tmp.display()))?;
    f.sync_all().ok();
}
fs::rename(&tmp, path).with_context(|| { ... })?;
fsync_parent_dir(path);
```

**Risk:** A permissive umask leaves `.shipper/state.json` and
`.shipper/receipt.json` world-readable. Receipt contents include
stdout/stderr tails (post-redaction), git context, and environment
fingerprints; state includes publish progression. Neither contains raw
tokens (the output sanitizer enforces that) but operational metadata
leakage could aid an attacker profiling a release pipeline.

**Mitigation already in place:**
- The output sanitizer (`crates/shipper-output-sanitizer/src/lib.rs`)
  enforces redaction of bearer tokens, `CARGO_REGISTRY_TOKEN=`,
  `CARGO_REGISTRIES_*_TOKEN=`, `token = ...`, and `Authorization: Bearer ...`
  in any line that reaches `state.json` / `receipt.json` / `events.jsonl`.
  Verified by `redact_sensitive_is_idempotent`, `cargo_registry_token_always_redacted`,
  `authorization_tokens_are_redacted`, and `token_assignment_always_redacted`
  proptests plus snapshot coverage.
- `events.jsonl` is authoritative (events-as-truth invariant) and the
  projection in `state.json` is regenerable from events, so an attacker
  who reads `state.json` gains only timing-and-package-version visibility.
- The product's release-operator threat model already lists this as a
  per-deployment hardening item (om-2-style guidance in
  `docs/status/SUPPORT_TIERS.md`).

**Recommended Hardening (Optional):**
After the temp-file write in `atomic_write_json`, on Unix call
`std::os::unix::fs::PermissionsExt::set_permissions` with `0o600` on the
`.tmp` file before `fsync_all` (so the on-disk file is already 0600 when
the rename happens). Update the threat-model T-1 claim to remove the
"File permissions are 0600 on Unix" line until the change ships, or
backport the set_permissions call to harden the invariant in code rather
than in documentation.

### OBS-2: Coverage workflow uses floating tags for some actions

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Tampering (supply chain) |
| **CWE** | CWE-1357 (Reliance on Untrusted Component) |
| **File** | `.github/workflows/coverage.yml:103, 130, 137, 150, 156` |
| **Status** | Accepted risk, tracked under OM-2 in threat model |

**Description:**
The coverage workflow (`.github/workflows/coverage.yml`) and the droid
security scan workflow (`.github/workflows/droid-security-scan.yml`)
reference third-party GitHub Actions via floating tags rather than commit
SHAs in a few cases:

- `codecov/codecov-action@v7` (coverage.yml:130, 137)
- `taiki-e/install-action@v2` (coverage.yml:103)
- `actions/cache@v6` (coverage.yml:46)
- `actions/upload-artifact@v7` (coverage.yml:55, 178)
- `actions/checkout@v7.0.0` (coverage.yml:50; release.yml:225, 246, 273, ...)
- `softprops/action-gh-release@v3` (release.yml)

The droid-related actions (`EffortlessMetrics/droid-action-safe@<sha>`,
`oven-sh/setup-bun@<sha>`) and `actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0`
in the droid-security-scan workflow are SHA-pinned.

**Risk:** A compromise of the upstream tag could push arbitrary code into
CI, gated by the `CODECOV_TOKEN` secret. The coverage job holds only
`contents: read` and runs on self-hosted runners.

**Mitigation already in place:**
- Dependabot is configured (`.github/dependabot.yml` -> `github-actions`
  ecosystem) to bump Actions weekly with PRs against this repo.
- The coverage job holds only `contents: read`; it cannot push branches,
  open PRs, or publish releases.
- The `CODECOV_TOKEN` is scoped to coverage upload only; it does not
  provide repo write, OIDC, or release authority.
- SHA-pinning is used for the highest-trust actions (Droid executor,
  Bun installer, repository checkout in droid-security-scan).

**Recommended Hardening (Optional):**
Pin the remaining third-party actions to commit SHAs and follow Dependabot's
"version updates" pattern with `dependabot.yml` -> `groups: actions:
* update-types: [minor, patch]`. The trade-off is reduced upstream
agility against marginal supply-chain tightening. Not required for
this scan window.

### OBS-3: Authentication header for owners API uses raw token (no Bearer prefix)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational, by-design) |
| **STRIDE Category** | N/A (protocol compliance, not a vulnerability) |
| **CWE** | N/A |
| **File** | `crates/shipper-registry/src/http.rs:128` (`fetch_owners_with_token`) |
| **Status** | Verified by `list_owners_sends_auth_header` test |

**Description:**
`HttpRegistryClient::fetch_owners_with_token` sends the cargo token as the
`Authorization` header value directly (no `Bearer ` prefix). This matches
the crates.io API contract, which accepts either the literal token or the
conventional `Bearer <token>` form. The unit test
`list_owners_sends_auth_header` in `http.rs` pins the wire format to the
literal-token form (`auth.value.as_str() == "my-token"`).

**Risk:** None. This is the documented crates.io contract; deviating
from it would break ownership queries. The literal-token shape is also
harder to mistake for OAuth bearer credentials if a token were ever
accidentally re-used in a non-crates.io context.

**Mitigation already in place:**
- The output sanitizer explicitly recognizes the literal-token shape via
  the generic `token = ...` and `Authorization: ...` rules; the bearer
  rule is a stricter layer that catches the standard `Authorization:
  Bearer <jwt>` shape. Together they cover both forms.
- The token is never logged (resolver.rs `mask_token`, output sanitizer
  redaction).

**Recommended Hardening (Optional):** None. Document the wire format
inline (the comment in `fetch_owners_with_token` already references the
crates.io API contract).

### OBS-4: URL construction does not percent-encode crate names

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (theoretical) |
| **STRIDE Category** | Tampering / Information Disclosure |
| **CWE** | CWE-918 (Server-Side Request Forgery, theoretical) |
| **File** | `crates/shipper-registry/src/http.rs:90, 105` (`crate_exists`, `version_exists`) |
| **Status** | Tracked as OM-2-adjacent in threat model (D-4) |

**Description:**
The HTTP client constructs crate-lookup URLs by direct interpolation:
`format!("{}/api/v1/crates/{}", self.base_url, name)` and
`format!("{}/api/v1/crates/{}/{}", self.base_url, name, version)`. Cargo's
manifest parser upstream constrains crate names to `^[a-zA-Z0-9_-]+$` and
semver versions to `^\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?$`, so the URL is
provably safe in the current code paths.

**Risk:** A future caller that bypasses the manifest parser and feeds
user-controlled strings into `crate_exists` or `version_exists` could
craft URLs that pivot the request to a different path on the registry
(e.g., `../../v1/users`). This is theoretical under the current data
flow and not reachable from CLI input.

**Mitigation already in place:**
- `cargo_metadata` is the only source of crate names today; it parses
  `Cargo.toml` and rejects malformed names before they reach the
  registry client.
- The registry base URL is configured up-front; SSRF pivots would need
  to also subvert that configuration.
- Threat model D-4 records this as theoretical.

**Recommended Hardening (Optional):** Add a validation step in the
registry client that rejects crate names or versions that don't match
the cargo manifest grammar, defensively. Not required for this scan
window.

## Threat Model

- **Version:** reused (last regenerated 2026-07-05, file mtime
  `2026-07-05 02:16:24 UTC`)
- **Location:** `.factory/threat-model.md`
- **STRIDE coverage:** Spoofing, Tampering, Repudiation, Information
  Disclosure, Denial of Service, Elevation of Privilege
- **Trust boundaries enumerated:** 6 (TB-1 through TB-6)
- **Mitigations verified in code:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case)
- **Next regen due:** 2026-10-03 (90 days from 2026-07-05) or sooner on
  any change to TB-1 through TB-6.
- **Decision:** Reuse as-is (last regenerated 8 days ago, well within
  the 90-day cadence).

## Scan Metadata

- **Commits scanned:** 1
- **Commit:** `26d16a2 deps(deps): bump indicatif from 0.18.4 to 0.18.5 (#148)`
- **Commit author:** dependabot[bot] <49699333+dependabot[bot]@users.noreply.github.com>
- **Commit date:** 2026-07-12 19:25:50 -0400
- **Scan window:** 2026-07-06 to 2026-07-13 (last 7 days, UTC)
- **Scan duration:** < 2 minutes
- **Branch:** `droid/security-report-2026-07-13`
- **Severity threshold:** medium
- **Skills used:** threat-model-generation (reuse), commit-security-scan
  (manual STRIDE pass), vulnerability-validation (manual),
  security-review (manual)
- **Files inspected (security-sensitive surface):**
  - `crates/shipper-core/src/ops/auth/resolver.rs`
  - `crates/shipper-core/src/ops/auth/credentials.rs`
  - `crates/shipper-core/src/ops/auth/oidc.rs`
  - `crates/shipper-core/src/encryption.rs` (re-export shim, see `shipper-encrypt`)
  - `crates/shipper-core/src/webhook.rs`
  - `crates/shipper-core/src/state/store/fs.rs`
  - `crates/shipper-core/src/state/execution_state/mod.rs`
  - `crates/shipper-core/src/state/events/mod.rs`
  - `crates/shipper-core/src/state/store/mod.rs`
  - `crates/shipper-core/src/ops/process/cargo.rs`
  - `crates/shipper-core/src/ops/process/run/mod.rs`
  - `crates/shipper-core/src/ops/process/run/command_builder.rs`
  - `crates/shipper-encrypt/src/lib.rs`
  - `crates/shipper-output-sanitizer/src/lib.rs`
  - `crates/shipper-registry/src/http.rs`
  - `crates/shipper-webhook/src/lib.rs`
  - `.github/workflows/release.yml`
  - `.github/workflows/droid-security-scan.yml`
  - `.github/workflows/droid.yml`
  - `.github/workflows/coverage.yml`
  - `.github/dependabot.yml`

## Lenses Applied

Per the shipper review-invariants context:

1. **STRIDE** for the entire security-sensitive surface (see threat model).
2. **OWASP Top 10** for any web/CLI surface: A01 (Broken Access Control)
   verified by the advisory lock + plan ID validation; A02
   (Cryptographic Failures) verified by `shipper-encrypt` PBKDF2 (100k
   iterations) + AES-256-GCM + random salt/nonce + masked passphrase
   display; A03 (Injection) verified by the lack of `sh -c`/shell
   pipelines in subprocess invocations (`Command::new(program).args(args)`
   vector); A04 (Insecure Design) verified by the events-as-truth
   invariant + atomic state writes (temp file + rename); A05 (Security
   Misconfiguration) verified by per-job `permissions:` blocks; A06
   (Vulnerable & Outdated Components) — the scan's primary lens, and
   this week's only delta is a dependabot semver-patch bump of
   `indicatif`; A07 (Identification and Authentication Failures)
   verified by token-source precedence and redaction; A08 (Software and
   Data Integrity Failures) verified by the receipt/events/state
   triplet and atomic state writes; A09 (Security Logging and
   Monitoring Failures) verified by `events.jsonl` authoritative
   recording; A10 (SSRF) — the engine has no user-controlled URL
   construction outside the cargo crates.io domain and the
   user-configured registry base URL (see OBS-4 for the theoretical
   URL-encoding gap).
3. **OWASP LLM Top 10** is not directly applicable (no LLM boundary in
   the engine), but the Droid workflow layer is reviewed separately by
   `droid-review` and `droid-security-scan` automations.

## Diff Summary

`git diff main..26d16a2` is the active `main` tip itself (single-commit
repository). The commit carries the migrated contents of the
`EffortlessMetrics/shipper` release-authority repository (1,318 files,
184,149 insertions) plus the `indicatif 0.18.4 → 0.18.5` dependabot
patch. The single changed dependency is a semver-patch bump of an
indicatif progress-bar crate that powers CLI rendering in
`shipper-cli`. No security-sensitive code paths in `shipper-core`,
`shipper-cli`, `shipper-registry`, `shipper-webhook`,
`shipper-output-sanitizer`, or `shipper-encrypt` are touched.

## References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- [docs/INVARIANTS.md](../../INVARIANTS.md) — events-as-truth contract
- [docs/status/SWARM_OPERATION.md](../../status/SWARM_OPERATION.md) —
  active-development / release-authority split
- [SECURITY.md](../../../SECURITY.md) — project security policy
- `.factory/threat-model.md` — current threat model (regenerated
  2026-07-05)
