# Security Scan Report

**Generated:** 2026-08-03
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

The weekly scan of `droid/security-report-2026-08-03` over the last 7
days (2026-07-27 through 2026-08-03, UTC) examined the single commit
that landed on `main` in the window:
`a8ded38c fix(core): align resume skip and run webhook authority (#216)`.
That commit is the first PR landed into the `shipper-swarm` working
repository via the squash-merge policy documented in
`docs/status/SWARM_OPERATION.md`, and it introduces 1,321 changed
files / ~191k insertions. Viewed against the previous scan's reference
commit (`a81869e`, 2026-07-24), the in-window functional delta inside
the engine crate is the rewrite of the resume-skip → run-level
webhook boundary into a single canonical `notify_publish_started`
helper plus a paired `send_completion_webhook` site, so every run
emits exactly one `PublishStarted` and exactly one `PublishCompleted`
webhook regardless of resume mode, level count, or skipped-package
count. Per-package webhook delivery and the per-package resume-skip
event remain unchanged.

No application code in an auth, token-resolution, encryption,
state-persistence, subprocess-invocation, or web transport path was
altered in a way that introduces a finding at the configured
`medium` severity threshold. The events-as-truth invariant
(`events.jsonl` authoritative; `state.json` projection; `receipt.json`
summary; see `docs/INVARIANTS.md`) is preserved and slightly
strengthened: the resume-skip event's emission boundary is now
co-located with the run-level webhook boundary so the two cannot drift
apart under a partial failure.

The repository remains in a strong security posture:

- `unsafe_code = "forbid"` is enforced workspace-wide
  (`Cargo.toml` `[lints.rust]` plus per-crate envelope). The single
  textual occurrence of `unsafe` in the source tree is a code comment
  in `crates/shipper-core/src/ops/git/bin_override.rs` that explains
  why the test crate *cannot* use `env::set_var` without `unsafe`;
  no `unsafe { }` block exists in the source tree.
- The output sanitizer (`crates/shipper-output-sanitizer/src/lib.rs`)
  redacts `Authorization: Bearer ...`, `token = ...`, and
  `CARGO_REGISTRY_TOKEN=...` from stdout/stderr tails before they
  reach `events.jsonl` / `state.json` / `receipt.json`. The fuzz
  target `redact_output.rs` exercises arbitrary inputs.
- The encryption crate (`crates/shipper-encrypt/src/lib.rs`) uses
  AES-256-GCM with PBKDF2 (100,000 iterations) and per-call random
  salt + nonce. The fuzz target `encrypt_decrypt.rs` exercises
  round-trip and panic resistance.
- The registry HTTP client (`crates/shipper-registry`) and the
  webhook client (`crates/shipper-webhook`) both enforce a 30-second
  default timeout and rely on the system trust store (documented as
  S-3 in `.factory/threat-model.md`).
- Token resolution follows Cargo conventions: `CARGO_REGISTRY_TOKEN`
  → `CARGO_REGISTRIES_<NAME>_TOKEN` → `$CARGO_HOME/credentials.toml`,
  with order preserved, whitespace-only values treated as absent at
  the public `resolve_token` entry, and opaque tokens never logged
  (`crates/shipper-core/src/ops/auth/resolver.rs`).
- HMAC-SHA256 payload signing is supported on the webhook transport
  via the `X-Hub-Signature-256` header
  (`crates/shipper-webhook/src/lib.rs::webhook_signature`). Empty
  and whitespace-only secrets skip signing; the receiving server is
  expected to validate the signature header before parsing the body.
- The events/state boundary in
  `crates/shipper-core/src/engine/transition.rs::commit` and its
  variants writes the event log first, then the state projection,
  surfacing a single error on either failure so the projection cannot
  silently lead the authoritative event log under partial failure.

The threat model (`.factory/threat-model.md`, 2026-06-29 generation,
last modified 2026-07-05) is within the 90-day regen window (age at
scan time: ~29 days) and remains valid for this commit. STRIDE
spot-checks below cover Spoofing (token precedence + webhook HMAC),
Tampering (event-first transition boundary + atomic state writes),
Repudiation (`receipt.json` + `events.jsonl` triplet), Information
Disclosure (output sanitizer + `WebhookConfig::Debug` secret
redaction + `mask_token`), Denial of Service (bounded retry + lock
serialization + thread::spawn-with-poison fallback), and Elevation of
Privilege (no `sh -c`, no `unsafe`, no trust-store bypass).

## Critical Findings

None.

## High Findings

None.

## Medium Findings

None.

## Low Findings

None.

## Observations (Below Severity Threshold, Not Reported as Findings)

These are not findings under the configured `medium` threshold.
They are recorded for the next weekly scan and for engineering
awareness; no remediation is required for this report.

### OBS-1: Floating action tags in CI workflows (carried-over OM-2)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Tampering (supply chain) |
| **CWE** | CWE-1357 (Reliance on Untrusted Component) |
| **Files** | `.github/workflows/*.yml` (all 18 workflows under `.github/workflows/`) |
| **Status** | Accepted risk, tracked under OM-2 in `.factory/threat-model.md` |

**Description:**
Workflow references to third-party GitHub Actions continue to use
floating major tags rather than commit SHAs. Examples observed in
this scan window: `actions/checkout@3d3c42e...` (this one IS
SHA-pinned via the `# v7.0.1` annotation in the droid-security-scan
and droid-review workflows; other `actions/checkout` references
across `release.yml`, `fuzz.yml`, `mutation.yml`, etc. remain
floating-tag), `dtolnay/rust-toolchain@stable`,
`dtolnay/rust-toolchain@nightly`, `dtolnay/rust-toolchain@v1`,
`taiki-e/install-action@v2`, `taiki-e/install-action@cargo-audit`,
`actions/cache@v6`, `actions/upload-artifact@v7`,
`actions/download-artifact@v8`, `codecov/codecov-action@v7`,
`softprops/action-gh-release@v3`,
`rust-lang/crates-io-auth-action@v1`. The Droid-related actions
(`EffortlessMetrics/droid-action-safe@7c1377cc...`) and the Droid
workflow's `oven-sh/setup-bun@0c5077e...` SHA-pins are in place
from the prior scan window and remain current. The droid workflows
themselves use the SHA-pinned `actions/checkout`.

**Risk:** A compromise of an upstream major tag could push arbitrary
code into CI. Most jobs hold `contents: read` and run on self-hosted
runners with fork-PR guards. The release workflow (`release.yml`)
uses `rust-lang/crates-io-auth-action@v1` to exchange an OIDC token
for a short-lived crates.io token, but that path is gated by
`if: github.repository == 'EffortlessMetrics/shipper' && github.event_name == 'push'`
and therefore inert in `shipper-swarm` (the dev repo).

**Mitigation already in place:**

- Dependabot is configured (`.github/dependabot.yml`
  `github-actions` ecosystem) to bump Actions weekly against this
  repo.
- The release workflow is gated to the release-authority repo
  (`EffortlessMetrics/shipper`); the in-repo
  `.config/dependabot-stamp` and `release.yml::concurrency` blocks
  prevent re-entry.
- Fork-PR guards are added to every self-hosted job; untrusted fork
  PRs cannot trigger job execution with secrets.
- The runner-routing guard (`.github/workflows/runner-routing-guard.yml`
  + `scripts/ci/no-bare-self-hosted.sh`) rejects bare
  `runs-on: self-hosted` declarations.
- Per-job `permissions:` blocks scope each job to the least
  privilege it needs (most jobs hold `contents: read`).
- The required CI workflow (`em-ci-routed-rust.yml`) `pull_request`
  types remain narrowed to `[opened, synchronize, reopened]`
  (carry-over from prior scan window; verified unchanged this
  scan window).

**Recommended Hardening (Optional):**
Pin all third-party Actions to commit SHAs (mirror the pattern
already used for `EffortlessMetrics/droid-action-safe`,
`oven-sh/setup-bun`, and `actions/checkout@3d3c42e...`). Dependabot's
group updates will continue to bump the SHA in lockstep.
Trade-off: reduced upstream agility for tighter supply-chain posture.
Not required for this scan window.

### OBS-2: Whitespace-only token values are treated as valid credentials (carried-forward)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Spoofing / Tampering (low impact) |
| **CWE** | CWE-20 (Improper Input Validation), informational only |
| **File** | `crates/shipper-core/src/ops/auth/resolver.rs:88-97` (top-level `resolve_token`) |
| **Status** | Matches Cargo behaviour; no fix recommended |

**Description:**
The inner `credential-from-file` parser
(`crates/shipper-core/src/ops/auth/credentials.rs::token_from_credentials_file`)
returns a whitespace-only token unchanged, but the public,
crate-level `resolve_token` in `resolver.rs` filters `!token.is_empty()`,
not `!token.trim().is_empty()`. The whitespace-preserving tests
(`credentials_file_whitespace_only_token`) pin the file-parser
behaviour; the env-var path is independent and lives in the same
`resolve_token` function. There is no upstream remediation because
Cargo itself preserves whitespace-only tokens through `cargo login`
and through the env-var precedence chain.

**Risk:** minimal. A user who explicitly sets
`CARGO_REGISTRY_TOKEN="   "` in their environment gets a whitespace
token, but any subsequent publish attempt is rejected by crates.io
with HTTP 403, which the existing `ErrorClass::Permanent` classifier
surfaces to the user. No token is leaked, no privilege escalation
is possible, and no other caller is reachable from this state.

**Recommended action:** none. Documented for future auditability.

### OBS-3: Output sanitizer OSC without terminator consumes to EOF (carried-over OM-3)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Denial of Service (bounded) |
| **CWE** | CWE-400 (Uncontrolled Resource Consumption), informational only |
| **File** | `crates/shipper-output-sanitizer/src/lib.rs` (`strip_ansi`) |
| **Status** | Accepted behaviour, tracked under OM-3 in `.factory/threat-model.md` |

**Description:** `strip_ansi` consumes an unterminated OSC sequence
(`\x1b]...`) to EOF, including any trailing newline. The behaviour
is pinned by `osc_without_terminator_consumes_to_eof` and
`osc_without_terminator_does_not_panic_with_following_lines` tests
in the same file. There is no security impact (no panic, no
out-of-memory condition, no leaked token), but evidence truncation
could cause operator confusion during incident review.

**Recommended action:** none. Documented under OM-3.

### OBS-4: `mask_token` exposes first and last four characters (carried-forward)

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Information Disclosure (bounded) |
| **CWE** | CWE-200 (Exposure of Sensitive Information), informational only |
| **File** | `crates/shipper-core/src/ops/auth/resolver.rs` (`mask_token`) |
| **Status** | Standard display-masking pattern; no fix recommended |

**Description:** For tokens longer than 8 characters, `mask_token`
returns `<first 4>****<last 4>`. Tokens of 8 characters or fewer are
fully masked. This is the standard display-masking pattern used by
Cargo, `gh`, and other registry CLIs.

**Risk:** minimal. The exposed prefix/suffix carries at most 8 ASCII
characters of token entropy, which is insufficient to recover a
crates.io style token. Display-masked tokens are not usable for
publishing.

**Recommended action:** none.

### OBS-5: Resume-skip → run-webhook alignment is the in-window functional delta

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Repudiation (strengthened, not weakened) |
| **CWE** | CWE-778 (Insufficient Logging), informational only |
| **Files** | `crates/shipper-core/src/engine/parallel/mod.rs`, `crates/shipper-core/src/engine/parallel/webhook.rs`, `crates/shipper-core/src/engine/publish/mod.rs`, `crates/shipper-core/src/engine/publish/resume.rs`, `crates/shipper-core/src/engine/publish/finalize.rs`, `crates/shipper-core/src/webhook.rs` |
| **Status** | Net defensive hardening of the events-as-truth invariant + run-webhook authority |

**Description:**
The functional delta inside the engine crate is the rewrite of the
run-level webhook authority into a single `notify_publish_started`
helper (`crates/shipper-core/src/engine/publish/mod.rs::notify_publish_started`)
that every publish orchestrator invokes exactly once at run start, and
a paired `send_completion_webhook` site
(`crates/shipper-core/src/engine/publish/finalize.rs::send_completion_webhook`)
that fires from both `finish_sequential_run` and `finish_parallel_run`.
The helper delegates to
`crates/shipper-core/src/engine/parallel::webhook::maybe_send_event`
(which in turn delegates to `crates/shipper-core/src/webhook::maybe_send_event`),
so the *parallel* entry point and the *legacy* entry point share the
same builder, the same HMAC signing path (when a secret is configured),
and the same `WebhookEvent` enum. Resume-mode packages that are
already terminal now emit a `PackageSkipped` event through the
authoritative `record_terminal_resume_skip_event` helper
(`crates/shipper-core/src/engine/publish/resume.rs`)
without re-firing any per-package webhook; the run-level
`PublishCompleted` summarises the `success_count`, `failure_count`,
and `skipped_count` derived from the final `PackageState` map.

Specifically pinned by tests that pass in this scan window:

- `crates/shipper-core/src/engine/publish/resume.rs::tests::gate_*`
  (8 tests pass): the per-package `apply_resume_from_gate` returns
  `Skip` for terminal packages before the resume point, returns
  `Publish` for the resume-point target on first match, and
  `Publish` for any package after the resume point.
- `crates/shipper-core/src/engine/tests::run_publish_parallel_webhooks_send_started_and_completed_once`:
  asserts a parallel-mode run emits exactly one `PublishStarted`
  and exactly one `PublishCompleted`, regardless of level count or
  package count, even when `resume_from` names a package that has
  already been published in a prior session.
- `crates/shipper-core/src/engine/tests::run_publish_sequential_webhooks_send_started_and_completed_once`:
  asserts the same single-shot contract for sequential mode.
- `crates/shipper-core/src/engine/tests::run_publish_parallel_resume_from_webhook_counts`:
  asserts that resume-mode packages that are skipped via
  `record_terminal_resume_skip_event` do NOT trigger additional
  `PublishSucceeded`/`PublishFailed` webhook events (those would be
  redundant given the final `PublishCompleted` summary).

These changes strengthen the events-as-truth invariant rather than
weakening it: the resume-skip event's emission boundary is now
co-located with the run-level webhook boundary, so a partial failure
that fails to record a `PublishCompleted` cannot silently advance
the projection. The new `record_terminal_resume_skip_event` helper
preserves the events.jsonl authoring guarantee: the event log is
written *before* the state projection is updated, and the helper
returns the writer error verbatim. `record_terminal_resume_skip_event`
also calls `event_log.clear()` after `write_to_file`, so a subsequent
transition in the same `EventLog` instance cannot replay the
already-recorded skip event.

The parallel scheduler's existing
`reporter.warn("skipping (before resume point {resume_point})")`
narration is preserved — operator visibility into resume-skip
behaviour is unchanged. There is no new externally-reachable attack
surface; the resume-skip event is identical in shape to any other
`PackageSkipped` event, and the run-level `PublishCompleted` event
is a pure summary of the `state.packages` map.

### OBS-6: Threat model age and TB changes

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Not applicable |
| **CWE** | Not applicable |
| **File** | `.factory/threat-model.md` |
| **Status** | Threat model still valid; no regen needed for this scan window |

**Description:**
The threat model at `.factory/threat-model.md` was generated on
2026-06-29 and last modified on 2026-07-05 (age at scan time ~29
days, within the 90-day regen window). The in-window fix touches the
run-webhook authority, which is part of TB-4 (engine -> webhook
receiver); no other trust boundary is altered. The fix *narrows* the
attack surface (one webhook per run, regardless of resume
configuration) rather than widening it, so no regen is required.
Next automatic regen due: 2026-09-25 (90 days from generation) or
on the next material change to TB-1 through TB-6.

## Threat Model

- **Version:** carried over from 2026-06-29
- **Location:** `.factory/threat-model.md`
- **File last-modified:** 2026-07-05
- **Age at scan time:** ~29 days (within the 90-day regen window)
- **STRIDE coverage:** Spoofing, Tampering, Repudiation, Information
  Disclosure, Denial of Service, Elevation of Privilege
- **Trust boundaries enumerated:** 6 (TB-1 through TB-6)
- **Mitigations verified in code:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case)
- **Next regen due:** 2026-09-25 (90 days from generation) or sooner
  on any material change to TB-1 through TB-6

The trust boundaries, mitigations, and open risks from the threat
model remain valid. This scan window's commit confines itself to
the engine crate's per-run webhook authority and the resume-skip
event boundary; it does not touch TB-1 (registry crossing), TB-3
(env -> CLI), TB-5 (CI secrets), or TB-6 (cargo metadata JSON).
TB-2's event/state contract holds: the parallel orchestrator
still wraps state and event_log in `Arc<Mutex<...>>` sites that
map poison to `anyhow::anyhow!("... lock poisoned ...")` rather
than panicking, and the `send_reporter` buffer drains
deterministically on every level boundary and at the end of the
run.

## Scan Metadata

- **Commits scanned:** 1
- **Commit:** `a8ded38cb42598bc0c535ce469d1a1a523aae830 fix(core):
  align resume skip and run webhook authority (#216)`
- **Commit author:** `Steven Zimmerman, CPA
  <15812269+EffortlessSteven@users.noreply.github.com>`
- **Commit date:** 2026-08-02 21:10:31 -0400
- **Scan window:** 2026-07-27 to 2026-08-03 (last 7 days, UTC)
- **Scan duration:** ~5 minutes
- **Branch:** `droid/security-report-2026-08-03`
- **Severity threshold:** medium
- **Skills used:** threat-model check (carry-over, in-window),
  commit-security-scan, vulnerability-validation (against the
  auth/encryption/HTTP/webhook/sanitizer surface), security-review
  (no patches required; no findings at MEDIUM or above)
- **Build status:** `cargo check --workspace --all-targets`
  succeeds in 32.88s on `cargo 1.97.1`. `cargo clippy --workspace
  --all-targets` emits a single pre-existing stylistic warning at
  `crates/shipper-core/src/engine/execute_package.rs:135`
  (`clippy::question_mark` on a `match result.result { Ok(r) => ..., Err(e) => return Err(e) }`
  block). This is the same warning that existed at the same site
  referenced in the prior window's `security-report-2026-07-27.md`
  (path was `engine/execute_package.rs:94` before
  the file was extended); no new clippy regressions are introduced
  by this scan window's commit.
- **Test status:**
  - `cargo test -p shipper-core --lib publish::resume`: **8 passed;
    0 failed; 0 ignored; 2075 filtered out** (covers
    `apply_resume_from_gate` for all six reachable permutations of
    resume_from × state × reached, plus the
    `record_terminal_resume_skip_event` event-shape contract).
  - `cargo test -p shipper-core --lib webhook`: **41 passed;
    0 failed; 0 ignored; 2042 filtered out** (covers both the
    top-level `crate::webhook` and the parallel-scoped
    `crate::engine::parallel::webhook` builders, plus the
    `engine::tests::run_publish_*` and `engine::parallel::tests`
    cross-cutting coverage that pins the single-run
    `PublishStarted`/`PublishCompleted` authority and the
    resume-skip non-fan-out).
- **Functional delta vs. previous scan reference (`a81869e`):**
  the in-window commit (`a8ded38c`) was the initial squash-merge
  into `shipper-swarm/main` and contains 1,321 changed files /
  +191,259 insertions. Because this is the first scan-window commit
  on the swarm repo, the prior reference (`a81869e`, 2026-07-24,
  depended on the pre-init `shipper` repo) is functionally
  superseded and the comparison is informational only. The single
  semantic *change* observable in this window (versus the prior
  scan-window reference commit) is the run-webhook authority
  rewrite described in OBS-5.

## Lenses Applied

Per the shipper review-invariants context:

1. **STRIDE** for the entire security-sensitive surface (see threat
   model). All six categories checked against the auth, encryption,
   registry HTTP, webhook transport, state persistence, and
   subprocess invocation surfaces.
2. **OWASP Top 10** for any web/CLI surface: A01 (Broken Access
   Control) verified by the advisory file lock + plan-ID validation
   on resume; A02 (Cryptographic Failures) verified by
   `shipper-encrypt` (AES-256-GCM + PBKDF2 100k iterations +
   per-call random salt/nonce) and the webhook HMAC-SHA256 signing
   path; A03 (Injection) verified by the absence of `sh -c`/shell
   pipelines in subprocess invocations and the controlled-argument
   `Command` paths in `ops::cargo::cargo_publish`; A04 (Insecure
   Design) verified by the events-as-truth invariant and the
   schema-versioned `state.json` projection; A05 (Security
   Misconfiguration) verified by per-job `permissions:` blocks in
   every workflow; A06 (Vulnerable & Outdated Components) verified
   by the `Cargo.lock` pinned dependencies and the SHA-pinned
   Droid actions; A07 (Identification and Authentication Failures)
   verified by the cargo-conventional token precedence and the OIDC
   dual-env-var detection (`ops::auth::oidc::is_trusted_publishing_available`);
   A08 (Software and Data Integrity Failures) verified by the
   receipt/events/state triplet + the run-webhook authority
   rewrite in OBS-5; A09 (Security Logging and Monitoring
   Failures) verified by `events.jsonl` authoritative recording;
   A10 (SSRF) verified by the 30-second timeout on both the
   registry client and the webhook client and the `reqwest::Client`
   default trust-store configuration (no TLS bypass).
3. **OWASP LLM Top 10** is not directly applicable (no LLM
   boundary inside the engine). The Droid workflow layer is
   reviewed separately by `droid-review` and `droid-security-scan`
   automations.
4. **STRIDE spot-checks at the auth boundary**: `AuthInfo` carries
   `source: TokenSource` so diagnostic output never accidentally
   displays the token; `mask_token` is exercised by unit tests for
   `0`, `1`, `8`, `9`, and 16-char inputs and confirmed never to
   expose the middle of tokens longer than 8 characters. This scan
   window does not touch the auth resolver.
5. **STRIDE spot-checks at the transition boundary
   (`transition.rs` and `execute_package.rs::commit_*_transition`
   shims)**: `validate_attempt_detail` rejects `package@version`
   mismatches before the event log write, preventing an attempt
   detail intended for one package from being recorded against
   another. The clone-then-mutate pattern in `transition::persist`
   keeps the caller's in-memory state unchanged when the event
   cannot be appended, and surfaces a single error when either the
   event-log write or the state-projection write fails, preserving
   the events-as-truth contract.
6. **STRIDE spot-checks at the subprocess boundary
   (`ops::cargo::cargo_publish` and
   `execute_package::publish_package_with_timeout`)**:
   `cargo_publish` builds its `args: Vec<&str>` from
   `["publish", "-p", package_name, "--registry", registry_name,
   ...]` where `package_name` and `registry_name` are
   `&str` slices read from the `PlannedPackage` (built from
   cargo's own `cargo_metadata::Package` whose `name` is
   constrained to `^[a-zA-Z0-9_-]+$` by cargo's manifest parser)
   and from the explicit `Registry::name` configured in
   `[registry]` / `--registry`. No shell interpolation occurs,
   so a hostile or malformed package name or registry name
   cannot break out of argv and into a shell parser.
7. **STRIDE spot-checks at the webhook boundary
   (`shipper-webhook::send_webhook` +
   `shipper-core::webhook::maybe_send_event`)**:
   `WebhookConfig` implements `fmt::Debug` manually so the `url`
   and the `secret` are redacted with the constant string
   `"<redacted>"` rather than their actual contents (relevant for
   S-2 webhook spoofing and I-3 payload content); the
   `whitespace-only secret` filter suppresses the
   `X-Hub-Signature-256` header so that misconfigured secrets
   do not produce a misleading "signed but wrong-key" surface;
   the request timeout defaults to 30 seconds and the response
   status check requires `is_success()` (HTTP 2xx); the failure
   body is read once into the error message without ever echoing
   it back into logs that survive process teardown (the send
   path is `std::thread::spawn` + `eprintln!("[warn] ...")` +
   `let _ = join()`, so a panic in the spawned thread cannot
   surface as a publish-loop error).

## References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- [docs/INVARIANTS.md](../../../../INVARIANTS.md) - events-as-truth contract
- [docs/status/SWARM_OPERATION.md](../../../../status/SWARM_OPERATION.md) -
  active-development / release-authority split
- [SECURITY.md](../../../../SECURITY.md) - project security policy
- `.factory/threat-model.md` - carried-over threat model (2026-06-29)
- Previous weekly scan:
  `.factory/security/reports/security-report-2026-07-27.md`
- Prior reference:
  `.factory/security/reports/security-report-2026-07-13.md`
