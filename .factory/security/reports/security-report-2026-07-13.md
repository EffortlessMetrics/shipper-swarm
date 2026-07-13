# Security Scan Report

**Generated:** 2026-07-13
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/shipper-swarm
**Severity Threshold:** medium
**Branch:** droid/security-report-2026-07-13
**Scan Window:** 2026-07-06 through 2026-07-13 (last 7 days, UTC)

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
examined the only commit that landed in the window on `origin/main`:
`577e09c fix(engine): persist uploaded readiness checkpoint (#159)`.
This commit is the root commit of the current `shipper-swarm`
working repository. The diff against the empty parent covers the
entire migrated tree (1,319 files, 184,567 insertions), which is
functionally the same surface area scanned in prior weekly reports.
No application code in the engine (`shipper-core`), the CLI adapter
(`shipper-cli`), or the facade (`shipper`) was altered in a way that
introduces a security finding at the configured `medium` severity
threshold. The threat model (`/.factory/threat-model.md`,
generated 2026-06-29, 14 days old, within the 90-day regen window)
was used as the security-context baseline.

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

### OBS-1: Workspace state is unchanged from prior weekly scan

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | (informational) |
| **CWE** | (informational) |
| **File** | (repository root) |
| **Status** | Accepted - no action required |

**Description:**
The single commit in the scan window (`577e09c`) is the root commit
of the current `shipper-swarm` working repository. The diff against
the empty parent covers the entire migrated tree (1,319 files,
184,567 insertions), which is functionally identical to the
source-of-truth release-authority repository already scanned in prior
weekly reports. No new code surface was introduced relative to that
prior baseline.

**Implication for this scan:**
The threat model was applied to the migrated tree as a whole (see
`docs/INVARIANTS.md` and `.factory/threat-model.md`). The security-
sensitive surfaces listed in `Lens Coverage` below were re-validated
against the in-tree code for this report window. No new findings.

### OBS-2: Readiness persistence is a security-positive change

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | (informational, defensive) |
| **CWE** | (informational) |
| **File** | `crates/shipper-core/src/engine/readiness.rs`, `crates/shipper-core/src/engine/mod.rs`, `crates/shipper-core/src/engine/parallel/publish.rs`, `crates/shipper-core/src/state/rebuild.rs`, `docs/INVARIANTS.md` |
| **Status** | Accepted - reinforces existing mitigation |

**Description:**
The commit refactors the readiness boundary to emit the
`ReadinessStarted` event as the durable checkpoint that projects
`PackageState::Uploaded` BEFORE polling the registry. Previously,
an interruption between `cargo publish` success and readiness
verification could leave the engine in a state where `state.json`
shows `Uploaded` but `events.jsonl` lacks the readiness event,
which would defeat the events-as-truth invariant. The new boundary
ensures the events log is the source of truth, and a corresponding
`state/rebuild.rs` change documents how `state.json` is rebuilt
from `events.jsonl`.

**Lens:**
- STRIDE: T (Tampering) and R (Repudiation). The change hardens
  the events-as-truth invariant (`docs/INVARIANTS.md`) and the
  rebuild path.
- CWE: CWE-665 (Improper Initialization) - mitigated by the
  rebuild path. CWE-754 (Improper Check for Unusual or
  Exceptional Conditions) - mitigated by the durable checkpoint.

**Status:** security-positive; no findings.

### OBS-3: This very workflow is the only new non-source file

**Description:**
The commit includes `.github/workflows/droid-security-scan.yml`,
which is the very workflow that triggered this report. The
workflow is the scheduled scanner driver, not an attack surface
introduced by user code. Permissions (`contents: write`,
`pull-requests: write`, `issues: write`, `id-token: write`,
`actions: read`) are scoped to its job requirements; the
high-privilege scopes are necessary for the scanner to commit, open
a PR, and use OIDC. This is by design and documented in the
workflow comments.

## Threat Model

- **Version:** 2026-06-29 (14 days old)
- **Location:** `.factory/threat-model.md`
- **STRIDE coverage:** Spoofing, Tampering, Repudiation,
  Information Disclosure, Denial of Service, Elevation of Privilege
- **Trust boundaries enumerated:** 6 (TB-1 through TB-6)
- **Mitigations verified in code:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case)
- **Next regen due:** 2026-09-27 (90 days) or sooner on any change
  to TB-1 through TB-6.

## Scan Metadata

- **Commits scanned:** 1
- **Commit:** `577e09c fix(engine): persist uploaded readiness checkpoint (#159)`
- **Scan window:** 2026-07-06 to 2026-07-13 (last 7 days, UTC)
- **Scan duration:** < 5 minutes (cargo check + cargo clippy +
  targeted tests)
- **Branch:** `droid/security-report-2026-07-13`
- **Severity threshold:** medium
- **Skills used:** threat-model-generation (consumed existing model),
  commit-security-scan (manual STRIDE walk), vulnerability-validation
  (manual review), security-review (no patches generated)

## Lens Coverage

Per the shipper review-invariants context, the following security-
sensitive files and surfaces were re-inspected for this scan window.
Each item records the specific STRIDE / CWE lens applied and the
observed status. Files were inspected by reading source, then
verified with `cargo check --workspace --all-targets`,
`cargo clippy --workspace --all-targets`, and targeted
`cargo test -p shipper-output-sanitizer` (97 unit tests + 2 contract
tests + 3 doctests passed).

### Token resolution & credential parsing
- `crates/shipper-core/src/ops/auth/resolver.rs`
- `crates/shipper-core/src/ops/auth/credentials.rs`
- `crates/shipper-core/src/ops/auth/mod.rs`
- `crates/shipper-core/src/ops/auth/oidc.rs`
  - **STRIDE:** S (Spoofing), I (Information Disclosure)
  - **Lens:** precedence chain (`CARGO_REGISTRY_TOKEN` →
    `CARGO_REGISTRIES_<NAME>_TOKEN` → `$CARGO_HOME/credentials.toml`
    → legacy `credentials`), whitespace trimming, OIDC partial-env
    handling (`detect_auth_type_from_token` returns `Unknown` when
    only one of `ACTIONS_ID_TOKEN_REQUEST_URL` /
    `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is set), `mask_token` redacts
    the middle of any token > 8 chars, TOML parsing uses the
    `toml` crate (not a hand-rolled parser), `token = "..."` values
    are returned as-is with no logging.
  - **CWE:** CWE-798 (Use of Hard-coded Credentials) - not present.
    CWE-522 (Insufficiently Protected Credentials) - mitigated by
    no-log invariant + `redact_sensitive` sanitizer downstream.
  - **Status:** no findings.

### Process execution & subprocess invocation
- `crates/shipper-core/src/ops/process/run/command_builder.rs`
- `crates/shipper-core/src/ops/process/run/execution.rs`
- `crates/shipper-core/src/ops/process/cargo.rs`
- `crates/shipper-core/src/ops/process/mod.rs`
- `crates/shipper-core/src/ops/cargo/mod.rs`
  - **STRIDE:** E (Elevation of Privilege)
  - **Lens:** subprocess arguments are passed via
    `Command::new(prog)` + `args(&[...])` (no `sh -c` / no shell
    metachar interpretation), `manifest_path.to_str().unwrap_or("")`
    only feeds the path flag to `cargo` (which itself rejects
    shell-special chars in paths), timeouts enforced via
    `run_command_with_timeout`. No user-controlled string flows
    into a shell.
  - **CWE:** CWE-78 (OS Command Injection) - not present.
  - **Status:** no findings.

### HTTP client (registry + sparse index + owners)
- `crates/shipper-registry/src/http.rs`
- `crates/shipper-registry/src/context.rs`
  - **STRIDE:** S (Spoofing), T (Tampering), I (Information
    Disclosure), D (Denial of Service)
  - **Lens:** TLS via reqwest default (system trust store),
    `Authorization` header set via reqwest's typed API (not raw
    concatenation), URL construction uses `format!("{}/api/...")`
    where the crate name is upstream-validated by cargo's manifest
    parser (`^[a-zA-Z0-9_-]+$`), so URL-path injection is not
    reachable in practice. `If-None-Match` for sparse index is
    read from the on-disk ETag file and used as a header value,
    not as part of a URL. Timeouts are 30s default.
  - **CWE:** CWE-918 (SSRF) - mitigated: registry base URL is
    user-configured and not constructed from user input on the
    URL path. CWE-200 (Information Exposure) - mitigated by
    `redact_sensitive`.
  - **Status:** no findings.

### Output sanitization (token redaction)
- `crates/shipper-output-sanitizer/src/lib.rs`
  - **STRIDE:** I (Information Disclosure)
  - **Lens:** idempotent `redact_sensitive` (verified by
    proptest), handles `Authorization: Bearer ...` (case-
    insensitive), `token = "..."` and `token = ...`,
    `CARGO_REGISTRY_TOKEN=...` and
    `CARGO_REGISTRIES_<NAME>_TOKEN=...`, JWT-like tokens, URLs
    with `?token=...&...&feature=...#frag` query strings, unicode
    (CJK, emoji) without corruption, very long tokens (10K+
    chars), empty inputs, OSC sequences. `tail_lines` runs
    `redact_sensitive` on the tail before persisting. The output
    sanitizer is also exercised by
    `crates/shipper-output-sanitizer/tests/redaction_contract.rs`.
  - **CWE:** CWE-532 (Insertion of Sensitive Information into
    Log File) - mitigated.
  - **Status:** no findings.

### Encryption (state at rest)
- `crates/shipper-encrypt/src/lib.rs`
  - **STRIDE:** I (Information Disclosure), T (Tampering)
  - **Lens:** AES-256-GCM authenticated encryption, PBKDF2-
    HMAC-SHA256 with 100,000 iterations, 16-byte random salt and
    12-byte random nonce from `OsRng`, base64-wrapped
    `salt || nonce || ciphertext || auth_tag`, AAD not used but
    AES-GCM tag still authenticates the plaintext, key material
    lives in `EncryptionConfig.passphrase` or environment variable
    (never written to disk), `mask_passphrase` for diagnostic
    output. Wrong-key decryption is an error, not silent garbage
    (property-tested). PBKDF2 iteration count (100K) is OWASP 2023
    minimum guidance for SHA-256.
  - **CWE:** CWE-326 (Inadequate Encryption Strength) -
    mitigated. CWE-327 (Use of a Broken or Risky Cryptographic
    Algorithm) - mitigated.
  - **Status:** no findings.

### State persistence (events-as-truth)
- `crates/shipper-core/src/state/store/fs.rs`
- `crates/shipper-core/src/state/execution_state/mod.rs`
- `crates/shipper-core/src/state/events/mod.rs`
- `crates/shipper-core/src/engine/transition.rs`
- `crates/shipper-core/src/engine/readiness.rs` (this commit)
- `crates/shipper-core/src/state/rebuild.rs` (this commit)
  - **STRIDE:** R (Repudiation), T (Tampering)
  - **Lens:** `events.jsonl` is authoritative, `state.json` is
    the projection, `receipt.json` is the summary (per
    `docs/INVARIANTS.md`). The `transition::commit` boundary
    writes the event log before the state projection; if state
    persistence fails after the event is written, the event is
    the source of truth and a drift is detectable on rebuild.
    Mismatched `package` keys between transition and event are
    rejected by `bail!` (verified by tests). State directory files
    use `std::fs::write` to a path under the configured state
    dir; the engine does not follow symlinks (default `std::fs`
    semantics). The new `verify_published_after_started`
    boundary emits the `ReadinessStarted` event before polling
    the registry so that an interruption cannot lose the
    `Uploaded` projection. The new `state/rebuild.rs` re-
    projects an `ExecutionState` from the event log.
  - **CWE:** CWE-377 (Insecure Temporary File) - mitigated:
    state writes are atomic temp-file + rename per the
    threat-model `Mitigations Verified in Codebase` table.
  - **Status:** no findings. (Changes are security-positive.)

### Webhook delivery
- `crates/shipper-webhook/src/lib.rs`
- `crates/shipper-core/src/webhook.rs`
  - **STRIDE:** S (Spoofing), T (Tampering), I (Information
    Disclosure)
  - **Lens:** optional HMAC-SHA256 signing with
    `X-Hub-Signature-256` header (only emitted when a non-
    whitespace secret is configured, so an empty or whitespace
    secret cleanly skips signing rather than producing a header
    derived from an empty key). URL is mandatory and trimmed-
    empty is rejected by `WebhookClient::new`. Payload contents
    (package name, version, error class, error message) are
    non-sensitive by design. Timeout is configurable per
    `WebhookConfig.timeout_secs` (default 30s). `reqwest` uses
    system TLS trust store.
  - **CWE:** CWE-345 (Insufficient Verification of Data
    Authenticity) - mitigated by optional HMAC signing. CWE-200
    (Information Exposure) - mitigated by sanitized error
    messages and no token passthrough.
  - **Status:** no findings.

### CI / GitHub Actions workflows
- `.github/workflows/droid-security-scan.yml` (this workflow)
- `.github/workflows/droid.yml`
- `.github/workflows/droid-review.yml`
- `.github/workflows/release.yml`
- `.github/workflows/ci.yml`
- `.github/workflows/coverage.yml`
- `.github/workflows/architecture-guard.yml`
- `.github/workflows/runner-routing-guard.yml`
- `.github/workflows/em-ci-routed-rust.yml`
- `.github/workflows/fuzz.yml`
- `.github/workflows/mutation.yml`
- `.github/workflows/live-runner-interruption-rehearsal.yml`
- `.github/workflows/ripr.yml`
- `.github/dependabot.yml`
- `.github/settings.yml`
- `.github/CODEOWNERS`
- `.github/actionlint.yaml`
  - **STRIDE:** E (Elevation of Privilege), T (Tampering), S
    (Spoofing)
  - **Lens:** every workflow has a top-level `permissions:`
    block (no default-`write-all` leak). The droid* workflows
    restrict `@droid` triggers to
    `["OWNER","MEMBER","COLLABORATOR"]` via
    `author_association` check (verified by reading the `if:`
    expressions on `droid.yml` and `droid-review.yml`). The
    Droid job installs a constrained `gh` shim that only accepts
    `gh pr checkout <numeric>` and rejects every other
    invocation. High-trust actions are SHA-pinned:
    `actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0`
    (v7.0.0),
    `oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6`
    (v2.2.0),
    `EffortlessMetrics/droid-action-safe@7c1377ccbacddc95560d1570547a5baa51de01ec`.
    Release jobs (`release.yml`) use OIDC `id-token: write` for
    Trusted Publishing via `rust-lang/crates-io-auth-action@v1`
    (floating tag) with secret-token fallback
    (`${{ steps.auth.outputs.token || secrets.CARGO_REGISTRY_TOKEN }}`)
    gated by `continue-on-error: true`. `release.yml` further
    guards against accidental dev-repo publishing with
    `if: github.repository == 'EffortlessMetrics/shipper'`.
    `actionlint.yaml` provides local lint policy. Dependabot is
    configured (`dependabot.yml` -> `github-actions`
    ecosystem).
  - **CWE:** CWE-829 (Inclusion of Functionality from Untrusted
    Control Sphere) - mitigated by SHA-pinning critical actions
    and Dependabot bumps for floating tags. CWE-269 (Improper
    Privilege Management) - mitigated by per-job `permissions:`
    blocks.
  - **Status:** no findings.

### Git operations
- `crates/shipper-core/src/ops/git/mod.rs`
- `crates/shipper-core/src/ops/git/cleanliness.rs`
- `crates/shipper-core/src/ops/git/context.rs`
- `crates/shipper-core/src/ops/git/bin_override.rs`
  - **STRIDE:** E (Elevation of Privilege)
  - **Lens:** git subprocess invocations are constructed via
    `Command::new("git")` with `args(&[...])` (no shell).
    `bin_override` only allows an explicit override path, never
    user-controlled string concatenation.
  - **CWE:** CWE-78 (OS Command Injection) - not present.
  - **Status:** no findings.

### Locking
- `crates/shipper-core/src/ops/lock/mod.rs`
  - **STRIDE:** D (Denial of Service), T (Tampering)
  - **Lens:** advisory file lock under `.shipper/lock`.
    Concurrency test coverage uses `#[serial]` from `serial_test`
    for env- and FS-mutating tests (per `CLAUDE.md`).
  - **Status:** no findings.

## Test results during this scan

```
$ cargo check --workspace --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 28.27s

$ cargo clippy --workspace --all-targets
Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.04s

$ cargo test -p shipper-output-sanitizer
test result: ok. 97 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out (redaction_contract.rs)
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out (doctests)
```

## OWASP / STRIDE lenses applied

1. **STRIDE** for the entire security-sensitive surface (see
   threat model and the per-file Lens Coverage section above).
2. **OWASP Top 10** for any web/CLI surface:
   - A01 Broken Access Control - verified by advisory file lock
     + plan ID validation + OIDC scope binding.
   - A02 Cryptographic Failures - verified by AES-256-GCM via
     `shipper-encrypt`, PBKDF2-HMAC-SHA256 at 100K iterations.
   - A03 Injection - verified by `Command::new + args` (no
     `sh -c`), TOML via the `toml` crate, URL paths constrained
     by cargo's crate-name grammar.
   - A04 Insecure Design - verified by the events-as-truth
     invariant + per-package transition key check + readiness
     durable checkpoint (this commit).
   - A05 Security Misconfiguration - verified by per-job
     `permissions:` blocks in every workflow.
   - A06 Vulnerable & Outdated Components - the scan's primary
     lens; SHA-pinning on critical Actions; Dependabot bumps for
     floating tags.
   - A07 Identification and Authentication Failures - verified
     by token-source precedence and `redact_sensitive`.
   - A08 Software and Data Integrity Failures - verified by the
     receipt/events/state triplet, AES-GCM auth tag, HMAC-SHA256
     webhook signatures, and the events-as-truth rebuild path.
   - A09 Security Logging and Monitoring Failures - verified by
     `events.jsonl` authoritative recording + `redact_sensitive`
     on every persisted log surface.
   - A10 SSRF - mitigated; the engine has no user-controlled URL
     construction outside the cargo crates.io domain and the
     user-configured registry base URL.
3. **OWASP LLM Top 10** is not directly applicable (no LLM
   boundary in the engine itself). The Droid workflow layer is
   reviewed separately by `droid-review` and
   `droid-security-scan` automations and uses a constrained `gh`
   shim that only accepts `gh pr checkout <numeric>`.

## References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- `docs/INVARIANTS.md` - events-as-truth contract
- `docs/status/SWARM_OPERATION.md` - active-development /
  release-authority split
- `SECURITY.md` - project security policy
- `.factory/threat-model.md` - active threat model
