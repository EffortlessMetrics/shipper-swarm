# Security Scan Report

**Generated:** 2026-05-25
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

## Scan Details

### Commits Scanned (Last 7 Days)

| Commit | Author | Description |
|--------|--------|-------------|
| 869473c08096a99b7b858d4c822ef619a0e6a4b9 | (GitHub Actions) | docs: add Droid Bun smoke checks (#106) |

### Files Changed in Scoped Commits

- `.cargo/config.toml` - Cargo alias configuration
- `.cargo/mutants.toml` - Mutation testing config
- `.config/nextest.toml` - Test runner config
- `.factory/rules/droid-review.md` - Droid review rules
- `.factory/skills/review-guidelines/SKILL.md` - Review skill guidelines

### Security Controls Verified

1. **`unsafe_code = "forbid"` enforced workspace-wide** — No unsafe blocks present
2. **Token handling** — Tokens are masked in all output via `mask_token()` function
3. **Secret management** — All GitHub Actions workflows properly use `secrets.` for sensitive data
4. **Workflow permissions** — Minimal permissions granted (`contents: read`, `pull-requests: write`, etc.)
5. **OIDC Trusted Publishing** — Proper fallback chain: OIDC token → secret token
6. **State file atomicity** — Uses `atomic_write_json` for safe disk writes
7. **gh shim** — Droid workflows use constrained `gh` shim that only supports `pr checkout`

### Threat Model Status

| Attribute | Value |
|-----------|-------|
| **Location** | `.factory/threat-model.md` |
| **Version** | 2026-05-25 (newly generated) |
| **Methodology** | STRIDE |
| **Last Updated** | Today |

## Verification Results

### Code Patterns Reviewed

| Pattern | Result | Notes |
|---------|--------|-------|
| `unsafe_` blocks | ✅ Not found | Workspace-wide forbid |
| `eval()`, `system()`, `exec()` | ✅ Not found | No shell injection vectors |
| Hardcoded secrets | ✅ Not found | Tokens resolved from env/files only |
| Insecure file permissions | ✅ Not found | Proper umask expected |
| SQL injection | ✅ Not applicable | No SQL database usage |

### GitHub Actions Security Review

| Workflow | Permissions | Secret Usage | Status |
|----------|-------------|--------------|--------|
| `ci.yml` | `contents: read` | None | ✅ Secure |
| `droid.yml` | `contents: read`, `id-token: write` | `MINIMAX_API_KEY`, `FACTORY_API_KEY` | ✅ Secure |
| `droid-review.yml` | `contents: write`, `id-token: write` | `MINIMAX_API_KEY`, `FACTORY_API_KEY` | ✅ Secure |
| `droid-security-scan.yml` | `contents: write`, `id-token: write` | `MINIMAX_API_KEY`, `FACTORY_API_KEY` | ✅ Secure |
| `release.yml` | `id-token: write` | `CARGO_REGISTRY_TOKEN`, OIDC token minting | ✅ Secure |

### Verified Mitigations from Threat Model

| Threat | Mitigation | Status |
|--------|------------|--------|
| Token theft via logs | `mask_token()` function | ✅ Verified |
| State file corruption | Atomic writes via `atomic_write_json` | ✅ Verified |
| Stale lock DoS | Timeout-based stale lock detection | ✅ Verified |
| Working tree manipulation | Git cleanliness check + `--allow-dirty` flag | ✅ Documented |
| Credential file access | Relies on OS file permissions | ⚠️ Acceptable risk |

## Low-Finding Observations

The following observations are **informational only** and do not meet the severity threshold:

1. **Lock file advisory nature** — Lock mechanism is advisory (check-then-create), not OS-level atomic. Acceptable for single-user/CI scenarios.

2. **events.jsonl retention** — Append-only log has no automatic rotation. Depends on operator cleanup. Low risk due to disk usage patterns.

3. **OIDC token in environment** — `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is an environment variable. GitHub Actions handles securely; standard practice.

4. **Config file trust** — `.shipper.toml` can redirect operations. Operator controls workspace files; acceptable.

## Appendix

### Threat Model
- **Version:** 2026-05-25 (newly generated)
- **Location:** `.factory/threat-model.md`
- **Methodology:** STRIDE (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege)

### Scan Metadata
- **Commits Scanned:** 1
- **Files Reviewed:** 5 (changed files in scoped commits)
- **Additional Review:** 7 GitHub Actions workflows, token handling, state management
- **Skills Used:** threat-model-generation (completed), commit-security-scan (not applicable - no code changes)

### References
- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
- [shipper-swarm threat model](./threat-model.md)
- [Security Audit (cargo audit) in CI](https://github.com/EffortlessMetrics/shipper-swarm/blob/main/.github/workflows/ci.yml#L268)

### Related Documentation
- [SECURITY.md](../../SECURITY.md) — Security policy and contact information
- [docs/INVARIANTS.md](../../docs/INVARIANTS.md) — Events-as-truth contract
- [ROADMAP.md](../../ROADMAP.md) — Nine competencies including Harden

---

*Report generated by Factory Droid scheduled security scan*
