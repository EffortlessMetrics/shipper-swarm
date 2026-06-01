# Security Scan Report

**Generated:** 2026-06-01
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

No security vulnerabilities at or above MEDIUM severity were identified in this scan.

### Areas Scanned

1. **Rust Source Code** (crates/shipper-core/src/, crates/shipper-cli/src/, crates/shipper/src/)
   - Auth module (token resolution, credential handling)
   - Lock module (advisory file locking, stale lock detection)
   - Process module (command execution wrappers)
   - Cargo module (publish/dry-run/yank operations)
   - State module (atomic JSON persistence)
   - Config parsing with TOML validation
   - Git operations

2. **GitHub Actions Workflows** (.github/workflows/)
   - CI pipeline
   - Security scanning workflows
   - Release workflows
   - Review workflows

### Security Controls Verified

- unsafe_code = "forbid" enforced workspace-wide
- Tokens are opaque strings, never logged (masked via mask_token())
- redact_sensitive() applied to all cargo output before logging
- Atomic writes via temp file + rename for state files
- Command arguments passed as slices, not shell-expanded (no injection vectors)
- Workflow secrets referenced via ${{ secrets.X }} syntax (no hardcoded tokens)
- External actions pinned to commit SHAs
- Least-privilege permissions on workflow jobs
- Heredoc injection mitigation in droid workflows

## Appendix

### Threat Model
- **Version:** 2026-06-01 (newly generated)
- **Location:** .factory/threat-model.md

### Scan Metadata
- **Commits Scanned:** 1
- **Files Analyzed:** ~50 Rust source files, 12 GitHub workflow files
- **Scan Duration:** <5 minutes
- **Skills Used:** threat-model-generation, commit-security-scan, vulnerability-validation

### Threat Model Summary

The newly generated STRIDE threat model covers:

- **Spoofing:** Token theft, credential injection, OIDC token exposure
- **Tampering:** State file manipulation, events.jsonl tampering, plan injection
- **Repudiation:** Denying publishes, auth mode misrepresentation
- **Information Disclosure:** Token leakage, webhook secret exposure, passphrase exposure
- **Denial of Service:** Rate limiting, lock file orphans, disk exhaustion
- **Elevation of Privilege:** Scoped token misuse, supply chain attacks, config injection

### References
- [CWE Database](https://cwe.mitre.org/)
- [STRIDE Threat Model](https://docs.microsoft.com/en-us/azure/security/develop/threat-modeling-tool-threats)
