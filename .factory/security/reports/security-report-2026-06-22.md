# Security Scan Report

**Generated:** 2026-06-22
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

## Scan Results

No security vulnerabilities at or above the medium severity threshold were identified in this scan.

### Analysis Coverage

The scan analyzed the following security-critical components:

- **Authentication & Authorization** (crates/shipper-core/src/ops/auth/)
  - Token resolution and management
  - Credentials file handling
  - OIDC/trusted publishing authentication
  - All findings (if any) validated as false positives due to existing mitigations

- **Lock Management** (crates/shipper-core/src/ops/lock/)
  - Lock file acquisition and release
  - Race condition analysis (CWE-362)
  - Finding: Advisory-lock design is documented; not an OS-enforced mutex

- **CLI Security** (crates/shipper-cli/src/)
  - Secret handling in CLI arguments
  - Finding: SHIPPER_ENCRYPT_KEY env var is recommended alternative; process listing exposure is an ergonomic concern, not a security vulnerability

- **Git Operations** (crates/shipper-core/src/ops/git/)
  - Binary override analysis
  - Finding: SHIPPER_GIT_BIN is intentional test infrastructure, not attacker-controlled input

- **Webhook Delivery** (crates/shipper-core/src/webhook.rs)
  - Non-blocking delivery analysis
  - Finding: Failures logged to stderr; non-blocking design is intentional

- **State Management** (crates/shipper-core/src/state/)
  - File system storage security
  - Encryption handling
  - Events-as-truth invariant validation

## Threat Model Status

| Attribute | Value |
|-----------|-------|
| **Version** | 2026-06-22 (newly generated) |
| **Location** | .factory/threat-model.md |
| **Threats Identified** | 30 (across STRIDE categories) |
| **High Priority Mitigations** | Already in place |

### Key Mitigations Verified

1. **Token Masking** - mask_token() function prevents token exposure in logs
2. **Events-as-truth** - events.jsonl append-only log provides authoritative audit trail
3. **Atomic Writes** - State files use temp file + rename pattern
4. **SHA256 Plan IDs** - Collision-resistant plan identification
5. **Stale Lock Detection** - Timeout-based lock recovery
6. **No Unsafe Code** - unsafe_code = "forbid" enforced workspace-wide

## Commits Scanned

| Date | Commit | Description |
|------|--------|-------------|
| 2026-06-21 | a7b41a4 | Merge pull request #435 - sync/shipper-swarm-2026-06-21 |

**Note:** Only 1 commit in the last 7 days, which was a sync merge from shipper repository. No new security-sensitive code changes were introduced.

## Appendix

### Threat Model Summary

The generated threat model identifies 30 threats across STRIDE categories:

| Category | Threats | High Priority |
|----------|---------|--------------|
| Spoofing | 4 | T1.1 Credential File Theft (mitigated) |
| Tampering | 6 | T2.1 State File Manipulation (mitigated) |
| Repudiation | 3 | T3.1 No Cryptographic Signing (design limitation) |
| Information Disclosure | 6 | T4.1 Token Logging (mitigated) |
| Denial of Service | 5 | T5.1 Lock File DoS (mitigated) |
| Elevation of Privilege | 5 | E1.5 Unsafe Deserialization (mitigated by serde) |

### Scan Metadata

| Attribute | Value |
|-----------|-------|
| Commits Scanned | 1 |
| Files Analyzed | ~30 security-critical Rust source files |
| Skills Used | threat-model-generation, commit-security-scan, vulnerability-validation |

### References

- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- shipper-swarm Security Policy (../../SECURITY.md)
- shipper-swarm Threat Model (../threat-model.md)
