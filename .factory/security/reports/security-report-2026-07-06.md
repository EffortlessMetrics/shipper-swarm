# Security Scan Report

**Generated:** 2026-07-06
**Scan Type:** Weekly Scheduled
**Repository:** EffortlessMetrics/shipper-swarm
**Severity Threshold:** medium

## Executive Summary

| Severity | Count | Auto-fixed | Manual Required |
|----------|-------|------------|-----------------|
| CRITICAL | 0 | 0 | 0 |
| HIGH     | 0 | 0 | 0 |
| MEDIUM   | 0 | 0 | 0 |
| LOW      | 0 | 0 | 0 |

**Total Findings:** 0
**Auto-fixed:** 0
**Manual Review Required:** 0

The weekly scan of `droid/security-report-2026-07-06` over the last 7 days
(2026-06-29 through 2026-07-06) examined the single commit that landed in
the window: `eb18061 ci: harden self-hosted runner routing with groups,
guard, and actionlint config (#120)`. Although the commit is an "initial"
commit on the `main` branch (the prior history was pruned/restructured
before this scan window), the substantive changes it carries are exactly
those described in the PR title: scoped self-hosted runner routing, a
runner-routing guard, actionlint configuration, fork-PR guards on every
self-hosted job, and a check-out / cache version bump.

The commit is a **net defensive hardening** of the CI/CD surface. It does
not alter the `shipper-core`, `shipper-cli`, or `shipper` engine/library
crates, does not touch the auth/token-resolution path, the encryption path,
the state-persistence path, the subprocess invocation surface, or the
output-sanitizer path. It does not introduce a new dependency, a new
network egress, a new process invocation, or a new permission grant that
would constitute a finding at the configured `medium` threshold.

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

### OBS-1: `actions/checkout` and `actions/cache` are referenced by floating tags

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Tampering (supply chain) |
| **CWE** | CWE-1357 (Reliance on Untrusted Component) |
| **File** | `.github/workflows/*.yml` (`actions/checkout@v7.0.0`, `actions/cache@v6`) |
| **Status** | Accepted risk; tracked under OM-2 in threat model |

**Description:**
The CI workflows (`ci.yml`, `coverage.yml`, `droid.yml`, `droid-review.yml`,
`droid-security-scan.yml`, `em-ci-routed-rust.yml`, `fuzz.yml`,
`mutation.yml`, `release.yml`, `ripr.yml`, `architecture-guard.yml`,
`live-runner-interruption-rehearsal.yml`) reference `actions/checkout@v7.0.0`
and `actions/cache@v6` by floating tags rather than by commit SHA. This is
a v6->v7 bump for `checkout` and a v5->v6 bump for `cache` from the prior
state of the workflows. `dtolnay/rust-toolchain@stable` is also a
floating tag (long-standing pattern).

The highest-trust actions remain SHA-pinned:
`EffortlessMetrics/droid-action-safe@7c1377ccbacddc95560d1570547a5baa51de01ec`
and `oven-sh/setup-bun@0c5077e51419868618aeaa5fe8019c62421857d6`.

**Evidence:**
```yaml
- uses: actions/checkout@v7.0.0
...
- uses: actions/cache@v6
```

**Risk:** A compromise of the upstream `actions/checkout` or `actions/cache`
tag could push arbitrary code into CI. The job permissions are scoped
(e.g., `contents: read` for most jobs, `contents: write` for Droid PR-creating
jobs), and Dependabot is configured for the `github-actions` ecosystem.

**Mitigation already in place:**
- `.github/dependabot.yml` configures `package-ecosystem: "github-actions"`
  with weekly bumps and an `actions` group.
- The runner-routing guard (added in this commit) rejects bare
  `runs-on: self-hosted` declarations, restricting where these jobs can
  land.
- Fork-PR guards were added to every self-hosted job in this commit,
  so untrusted fork PRs cannot trigger job execution with secrets.
- The `trusted-pr` runner label restricts which runners are eligible.

**Recommended Hardening (Optional):**
Pin `actions/checkout` and `actions/cache` to commit SHAs (mirror the
pattern already used for `EffortlessMetrics/droid-action-safe` and
`oven-sh/setup-bun`). Dependabot's group updates will continue to bump
the SHA in lockstep. Trade-off: reduced upstream agility for tighter
supply-chain posture. Not required for this scan window.

### OBS-2: Runner labels add `rust-large` / `rust-medium` / `rust-16gb` without documented capacity mapping

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Denial of Service (resource contention) |
| **CWE** | CWE-400 (Uncontrolled Resource Consumption) |
| **File** | `.github/workflows/em-ci-routed-rust.yml`, `.github/actionlint.yaml` |
| **Status** | Operational follow-up; no security impact |

**Description:**
The `em-ci-routed-rust.yml` workflow routes between `cx43` (rust-medium),
`cpx42` (rust-16gb/rust-medium), and `cx53` (rust-large). The
`actionlint.yaml` and `runner-routing-guard.sh` accept these labels, but
the actual capacity mapping (RAM/CPU per label) is not declared in the
repo. The router order changed from `["cpx42", "cx43", "cx53"]` to
`["cx43", "cpx42", "cx53"]` in this commit.

**Risk:** If a runner with insufficient memory lands a heavy `rust-large`
job, OOM-kills are possible (out of scope of this scan, but the
`shipper-cargo-failure` crate ships OOM-kill snapshots in
`crates/shipper-cargo-failure/src/snapshots/`).

**Mitigation already in place:**
- `timeouts` per job (e.g., `timeout-minutes: 75` on the
  `rust_small_*` jobs).
- `cargo-mutants` and `cargo-fuzz` use dedicated large runners.
- The runner-routing guard rejects bare-label routing that lacks the
  capacity labels.

**Recommended Hardening (Optional):**
Document the capacity mapping (RAM/CPU/disk) for each
`rust-{tiny,medium,large,16gb}` label in `.github/runners.md` or
equivalent. Out of scope for this weekly security report.

### OBS-3: `live-runner-interruption-rehearsal.yml` uploads `.shipper/` directory as a workflow artifact

| Attribute | Value |
|-----------|-------|
| **Severity** | LOW (informational) |
| **STRIDE Category** | Information Disclosure |
| **CWE** | CWE-538 (Insertion of Sensitive Information into Externally-Accessible File or Directory) |
| **File** | `.github/workflows/live-runner-interruption-rehearsal.yml` |
| **Status** | Accepted risk; rehearsal-only |

**Description:**
The new `live-runner-interruption-rehearsal.yml` workflow runs
`cargo test -p shipper-cli --test e2e_rehearse` and uploads the resulting
`.shipper/` directory as a workflow artifact (`include-hidden-files: true`,
`retention-days: 30`). `.shipper/` contains `events.jsonl` and
`state.json` per the events-as-truth invariant.

**Risk:** In a real (non-rehearsal) run, `events.jsonl` may contain
redacted traces, plan IDs, package names/versions, and error class
classifications. The state sanitizer redacts `Authorization`, `token=`,
and `CARGO_REGISTRY_TOKEN=`, but the rehearsal environment is local-only
(`127.0.0.1:39197`) and uses mock data, so secrets are not exposed. The
artifact remains inside the same-repo GitHub Actions artifacts bucket
(30-day retention).

**Mitigation already in place:**
- The workflow runs on self-hosted runners (`group: em-ci-small`) and
  uses the rehearsal environment variables `SHIPPER_LIVE_REHEARSAL_ROOT`
  / `SHIPPER_LIVE_REHEARSAL_REGISTRY_ADDR` to force local-only behavior.
- The fork-PR guard (`github.event.pull_request.head.repo.full_name ==
  github.repository`) prevents untrusted forks from triggering this
  workflow.
- Output sanitization (`shipper-output-sanitizer`) is exercised by the
  underlying e2e test, which redacts `Authorization: Bearer ...` and
  similar patterns.

**Recommended Hardening (Optional):**
None. The artifact contents are already inside the repo's GitHub
Actions storage, and the rehearsal is gated by `workflow_dispatch` or a
PR path that touches only the rehearsal workflow itself plus the
matching test/engine/state source. Out of scope for this weekly security
report.

## Threat Model

- **Version:** carried forward from `2026-06-29` (still within the
  90-day regen window; today's date is `2026-07-06`)
- **Location:** `.factory/threat-model.md`
- **STRIDE coverage:** Spoofing, Tampering, Repudiation, Information
  Disclosure, Denial of Service, Elevation of Privilege
- **Trust boundaries enumerated:** 6 (TB-1 through TB-6)
- **Mitigations verified in code:** 10 (table in threat model)
- **Open risks tracked:** 3 (OM-1 Reconcile, OM-2 floating action
  versions, OM-3 output sanitizer OSC edge case)
- **Next regen due:** `2026-09-27` (90 days) or sooner on any change
  to TB-1 through TB-6.

The trust boundaries, mitigations, and open risks from the threat model
remain valid. The runner-routing hardening in this commit strengthens
TB-5 (Local repository <-> GitHub Actions / Droid workflow) by:

1. Replacing bare `runs-on: self-hosted` declarations with scoped
   `group:` / `labels:` routing, so jobs cannot land on an
   unintended runner that happens to carry the `self-hosted` label.
2. Adding a CI guard (`runner-routing-guard.yml` +
   `scripts/ci/no-bare-self-hosted.sh`) that fails the check if any
   future workflow regresses to bare routing.
3. Adding `if: github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository`
   to every self-hosted job, so untrusted fork PRs cannot trigger
   self-hosted runner execution with secrets (the prior release had
   this guard on some jobs but not all).
4. Adding `.github/actionlint.yaml` so the actionlint linter is aware
   of the valid `self-hosted-runner.labels` set.

## Scan Metadata

- **Commits scanned:** 1
- **Commit:** `eb18061 ci: harden self-hosted runner routing with groups,
  guard, and actionlint config (#120)`
- **Commit author:** Steven Zimmerman, CPA
  <15812269+EffortlessSteven@users.noreply.github.com>
- **Commit date:** 2026-07-04 22:16:07 -0400
- **Scan window:** 2026-06-29 to 2026-07-06 (last 7 days, UTC)
- **Scan duration:** < 1 minute
- **Branch:** `droid/security-report-2026-07-06`
- **Severity threshold:** medium
- **Skills used:** threat-model-generation, commit-security-scan,
  vulnerability-validation, security-review
- **Files inspected (security-sensitive surface):**
  - `.factory/threat-model.md` (carried forward, current)
  - `.github/actionlint.yaml` (new, 16 lines)
  - `.github/workflows/architecture-guard.yml` (modified)
  - `.github/workflows/ci.yml` (modified)
  - `.github/workflows/coverage.yml` (modified)
  - `.github/workflows/droid-review.yml` (modified)
  - `.github/workflows/droid-security-scan.yml` (modified)
  - `.github/workflows/droid.yml` (modified)
  - `.github/workflows/em-ci-routed-rust.yml` (modified)
  - `.github/workflows/fuzz.yml` (modified)
  - `.github/workflows/live-runner-interruption-rehearsal.yml` (new)
  - `.github/workflows/mutation.yml` (modified)
  - `.github/workflows/release.yml` (modified)
  - `.github/workflows/ripr.yml` (modified)
  - `.github/workflows/runner-routing-guard.yml` (new, 29 lines)
  - `scripts/ci/no-bare-self-hosted.sh` (new, 30 lines)
  - `docs/agent-context/review-invariants.md` (modified)
  - `Cargo.lock` (modified, dependency patch bumps only)
  - `SECURITY.md` (carried forward)

## Lenses Applied

Per the shipper review-invariants context:

1. **STRIDE** for the entire security-sensitive surface (see threat model).
2. **OWASP Top 10** for any web/CLI surface: A01 (Broken Access Control)
   verified by the new fork-PR guards and the new scoped runner routing;
   A02 (Cryptographic Failures) verified by the unchanged
   `shipper-encrypt` re-export and `pbkdf2/hmac/sha2/aes-gcm` ignore rules
   in `dependabot.yml`; A03 (Injection) verified by the lack of `sh -c`/
   shell pipelines in subprocess invocations and by the constrained
   `gh pr checkout` shim in `droid*.yml`; A04 (Insecure Design) verified
   by the events-as-truth invariant; A05 (Security Misconfiguration)
   verified by per-job `permissions:` blocks and the new
   `runner-routing-guard`; A06 (Vulnerable & Outdated Components)
   addressed by the actionlint config and dependabot bumps; A07
   (Identification and Authentication Failures) verified by token-source
   precedence and redaction; A08 (Software and Data Integrity Failures)
   verified by the receipt/events/state triplet; A09 (Security Logging
   and Monitoring Failures) verified by `events.jsonl` authoritative
   recording; A10 (SSRF) - the engine has no user-controlled URL
   construction outside the cargo crates.io domain and the user-
   configured registry base URL.
3. **OWASP LLM Top 10** is not directly applicable (no LLM boundary in
   the engine), but the Droid workflow layer is reviewed separately by
   `droid-review` and `droid-security-scan` automations. The PR guard
   `!startsWith(github.event.pull_request.head.ref, 'droid/security-report-')`
   in `droid-review.yml` continues to suppress auto-review on the
   weekly security-report PRs.

## Diff Coverage

The functional delta relative to the previous scan's last commit
(`3dc6bd2 ci(deps): bump codecov/codecov-action from 6 to 7`) is
contained in 19 files (600 insertions / 156 deletions):

```
.factory/security/reports/security-report-2026-06-29.md |  202 +++++++++++++++++++++
.factory/threat-model.md                           |   89 +++++++++
.github/actionlint.yaml                            |   16 ++
.github/workflows/architecture-guard.yml           |   18 +-
.github/workflows/ci.yml                           |  105 +++++++----
.github/workflows/coverage.yml                     |   17 +-
.github/workflows/droid-review.yml                 |    6 +-
.github/workflows/droid-security-scan.yml          |    6 +-
.github/workflows/droid.yml                        |    6 +-
.github/workflows/em-ci-routed-rust.yml            |   49 +++--
.github/workflows/fuzz.yml                         |   12 +-
.github/workflows/live-runner-interruption-rehearsal.yml | 101 ++++++++++
.github/workflows/mutation.yml                     |   19 +-
.github/workflows/release.yml                      |   54 ++++--
.github/workflows/ripr.yml                         |    9 +-
.github/workflows/runner-routing-guard.yml         |   29 +++
Cargo.lock                                         |   64 +++----
docs/agent-context/review-invariants.md            |    7 +-
scripts/ci/no-bare-self-hosted.sh                  |   30 +++
19 files changed, 600 insertions(+), 156 deletions(-)
```

No application code (`crates/shipper-core/src/**`,
`crates/shipper-cli/src/**`, `crates/shipper/src/**`,
`crates/shipper-cargo-failure/src/**`,
`crates/shipper-*/src/**`) was modified in this commit.

## References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://learn.microsoft.com/en-us/security/engineering/threat-modeling)
- [OWASP Top 10](https://owasp.org/Top10/)
- [docs/INVARIANTS.md](../../INVARIANTS.md) - events-as-truth contract
- [docs/status/SWARM_OPERATION.md](../../status/SWARM_OPERATION.md) -
  active-development / release-authority split
- [SECURITY.md](../../../SECURITY.md) - project security policy
- `.factory/threat-model.md` - carried-forward threat model
