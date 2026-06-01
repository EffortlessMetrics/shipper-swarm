# STRIDE Threat Model: shipper-swarm

**Document Version:** 1.0  
**Date:** 2026-06-01  
**Project:** EffortlessMetrics/shipper-swarm  
**Architecture:** Rust workspace with shipper-core (engine), shipper-cli (CLI adapter), shipper (install façade), shipper-webhook, shipper-registry, shipper-encrypt, shipper-types

---

## 1. Overview and Scope

Shipper is an idempotent, resumable publishing tool for Rust workspaces. It publishes crates to registries (primarily crates.io) by orchestrating `cargo publish` invocations, verifying registry visibility, and recording evidence for CI/CD pipelines.

**In Scope:**
- Token resolution and authentication (`ops/auth/`)
- Git context capture (`ops/git/`)
- Cargo command execution (`ops/cargo/`)
- Registry HTTP client (`shipper-registry/`)
- State file persistence (`state/execution_state/`, `state/events/`, `state/store/`)
- Webhook notifications (`shipper-webhook/`)
- State file encryption (`shipper-encrypt/`)
- CLI argument parsing (`shipper-cli/`)
- Engine orchestration (`engine/`)

**Out of Scope:**
- The bundled `cargo` binary itself (upstream trust)
- The Rust toolchain
- External registry infrastructure (crates.io, etc.)

---

## 2. System Architecture and Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         shipper-cli                               │
│  (clap derive, subcommand dispatch, progress rendering)           │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                    ┌──────▼──────┐
                    │ shipper-core │
                    │  (engine)    │
                    └──────┬──────┘
           ┌───────────────┼────────────────┐
    ┌──────▼──────┐ ┌──────▼──────┐ ┌──────▼──────┐
    │   ops/auth  │ │   ops/git   │ │  ops/cargo  │
    │   (token)   │ │   (context) │ │ (subprocess)│
    └──────┬──────┘ └──────┬──────┘ └──────┬──────┘
           │               │               │
    ┌──────▼───────────────▼───────────────▼──────┐
    │              shipper-registry                  │
    │          (HTTP client, sparse index)          │
    └──────────────────────────┬───────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
       ┌──────▼──────┐  ┌──────▼──────┐  ┌──────▼──────┐
       │  crates.io  │  │   Other    │  │   Webhook   │
       │   (API)      │  │ Registries │  │ Delivery   │
       └─────────────┘  └─────────────┘  └──────┬──────┘
                                               │
                                        ┌──────▼──────┐
                                        │shipper-webhook│
                                        │(HMAC signing) │
                                        └─────────────┘

State Files (.shipper/):
  ├── state.json       (ExecutionState projection)
  ├── events.jsonl     (Append-only event log - authoritative)
  ├── receipt.json     (Final receipt summary)
  ├── reconciliation.json
  ├── auth-evidence.json
  └── remediation-plan.json
```

---

## 3. STRIDE Threat Analysis

### 3.1 Spoofing (Identity Impersonation)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **S-1** | Malicious actor obtains `CARGO_REGISTRY_TOKEN` env var and publishes unauthorized crates | `ops/auth/` | Medium | Critical |
| **S-2** | Attacker modifies credentials.toml to inject their own token | `ops/auth/` | Low | Critical |
| **S-3** | OIDC token theft in GitHub Actions environment via `ACTIONS_ID_TOKEN_REQUEST_TOKEN` | `ops/auth/` | Low | High |
| **S-4** | Token logged or exposed in CI logs due to misconfiguration | `ops/auth/` | Medium | High |
| **S-5** | Webhook URL hijacked via DNS or MITM attack to receive publish notifications | `shipper-webhook/` | Low | Medium |

**Mitigations in place:**
- Tokens are opaque strings and never logged (`mask_token` function)
- Token resolution follows Cargo conventions (env → credentials.toml → legacy file)
- Whitespace-trimmed empty tokens treated as absent
- Auth evidence records auth mode without storing tokens
- HMAC-SHA256 webhook signatures verify sender authenticity

---

### 3.2 Tampering (Unauthorized Modification)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **T-1** | State file (.shipper/state.json) tampered to skip publishing specific crates | `state/execution_state/` | Low | High |
| **T-2** | events.jsonl modified to hide failed publish attempts | `state/events/` | Low | High |
| **T-3** | Receipt.json modified to show fake success for failed publishes | `state/execution_state/` | Low | High |
| **T-4** | Plan file manipulated to reorder crates in a way that breaks dependency resolution | `plan/` | Low | High |
| **T-5** | Cargo metadata cache poisoned to hide version conflicts | `ops/cargo/` | Low | Medium |
| **T-6** | Sparse index cache corrupted to prevent visibility verification | `shipper-registry/` | Low | Medium |
| **T-7** | Webhook payload tampered in transit (without HMAC check) | `shipper-webhook/` | Low | Medium |

**Mitigations in place:**
- Events are append-only (events.jsonl is authoritative)
- State files use 0600 permissions (user read/write only)
- plan_id SHA256 checksum validates plan integrity
- Reconciliation check against registry truth for ambiguous outcomes
- Sparse index ETag caching prevents stale/malicious cache

---

### 3.3 Repudiation (Denial of Actions)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **R-1** | User claims they did not publish a specific version when they did | Global | Medium | High |
| **R-2** | CI pipeline claims failure was due to network issues when registry actually returned 403 | `ops/auth/` | Low | High |
| **R-3** | User denies making changes to credentials.toml that caused unauthorized publish | `ops/auth/` | Medium | Medium |
| **R-4** | Attacker claims events.jsonl was corrupted by filesystem error, not deliberate deletion | `state/events/` | Low | Medium |

**Mitigations in place:**
- GitContext captures commit hash, branch, tag for receipt evidence
- AuthEvidence records auth mode (Token, TrustedPublishing, OIDC context)
- events.jsonl is append-only with timestamp + EventType
- Receipt contains stdout/stderr, exit codes, git context, environment fingerprint
- Reconciliation against registry truth for ambiguous cargo publish outcomes

---

### 3.4 Information Disclosure (Confidentiality Breach)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **I-1** | Token value exposed in process list on multi-user system | `ops/auth/` | Medium | Critical |
| **I-2** | Token written to disk in state files or logs | `ops/auth/`, `state/` | Low | Critical |
| **I-3** | Webhook secret exposed in config file or environment variable | `shipper-webhook/` | Low | High |
| **I-4** | Credential file (.cargo/credentials.toml) permissions too permissive | `ops/auth/` | Low | High |
| **I-5** | Package names/versions exposed in events.jsonl or state files (low severity) | `state/` | Medium | Low |
| **I-6** | Git remote URL exposed in receipts (may contain auth tokens) | `ops/git/` | Low | High |
| **I-7** | Encryption passphrase exposed in logs or config | `shipper-encrypt/` | Medium | Critical |
| **I-8** | Sparse index cache leaks crate dependency information | `shipper-registry/` | Low | Low |

**Mitigations in place:**
- `mask_token()` function ensures tokens never appear in logs
- State files explicitly exclude tokens
- Webhook `X-Hub-Signature-256` header uses HMAC, not token in URL
- `CARGO_HOME/credentials.toml` has user-only permissions
- Encryption config supports env var (`SHIPPER_ENCRYPT_KEY`) to avoid passing passphrase via CLI
- `mask_passphrase()` masks all but first/last char in display output

---

### 3.5 Denial of Service (Availability)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **D-1** | Registry returns 429 Too Many Requests, blocking publish | `shipper-registry/`, `engine/` | High | Medium |
| **D-2** | .shipper/lock file not cleaned up after crash, blocking future runs | `ops/lock/` | Medium | Medium |
| **D-3** | events.jsonl grows without bound, consuming all disk space | `state/events/` | Low | Medium |
| **D-4** | Sparse index unreachable, preventing visibility checks | `shipper-registry/` | Medium | Medium |
| **D-5** | Webhook endpoint unreachable, blocking publish completion | `shipper-webhook/` | Medium | Low |
| **D-6** | Git operations fail due to network issues, preventing preflight | `ops/git/` | Medium | Low |
| **D-7** | Malicious .shipper/state.json causes infinite retry loop | `engine/` | Low | Medium |

**Mitigations in place:**
- Retry with exponential backoff for HTTP 429 (rate limit)
- `registry_aware_backoff()` respects Retry-After headers
- Advisory lock with cleanup on exit
- Plan validation prevents malformed state loading
- Webhook failures do not block publish (non-blocking delivery)
- Preflight-only mode for isolated checks without full publish

---

### 3.6 Elevation of Privilege (Unauthorized Access)

| Threat | Description | Affected Component | Likelihood | Impact |
|--------|-------------|-------------------|------------|--------|
| **E-1** | Attacker gains access to CI environment with scoped token, publishes malicious crate version | `ops/auth/` | Low | Critical |
| **E-2** | Local user on multi-user system reads tokens from process environment | `ops/auth/` | Medium | High |
| **E-3** | Malicious configuration file (.shipper.toml) injects arbitrary cargo flags | `shipper-config/` | Low | High |
| **E-4** | Shipper binary replaced with malicious version via supply chain attack | Supply chain | Low | Critical |
| **E-5** | OIDC token with excessive permissions used for trusted publishing | `ops/auth/` | Low | High |
| **E-6** | Arbitrary file write via path traversal in cache directory | `shipper-registry/` | Low | High |

**Mitigations in place:**
- Scoped tokens recommended (crates.io supports fine-grained scopes)
- Credential store preferred over env vars for local development
- Config file validated before use
- `cargo install --locked` ensures reproducible installs
- Sparse index cache path is hardcoded based on crate name (no arbitrary path)
- Architecture guard enforces layer boundaries (ops cannot import from engine/plan/state)

---

## 4. Component-Specific Threat Details

### 4.1 shipper-core::ops::auth

**Purpose:** Cargo registry token resolution from env vars and credentials files.

**Key Files:**
- `ops/auth/mod.rs` - Token resolution, auth detection
- `ops/auth/credentials.rs` - credentials.toml parsing
- `ops/auth/resolver.rs` - Env var resolution
- `ops/auth/oidc.rs` - Trusted publishing detection

**Resolution Order:**
1. `CARGO_REGISTRY_TOKEN` env var
2. `CARGO_REGISTRIES_<NAME>_TOKEN` env var
3. `$CARGO_HOME/credentials.toml`
4. Legacy `$CARGO_HOME/credentials` file

**Threats:**
- Token leakage via process list (S-1)
- Token exposure via verbose logging or error messages (I-1)
- credentials.toml file permission issues (I-4)
- Malicious credential injection (S-2)

**Security Controls:**
- Tokens never logged (enforced by mask_token)
- Empty/whitespace tokens treated as absent
- OIDC detection requires both `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN`

---

### 4.2 shipper-core::ops::git

**Purpose:** Git repository introspection for cleanliness checks and context capture.

**Key Files:**
- `ops/git/context.rs` - Commit/branch/tag/changed-files/remote queries
- `ops/git/cleanliness.rs` - Porcelain cleanliness checks

**Threats:**
- Git command injection via paths (unlikely, uses Command API)
- Remote URL exposure containing credentials (I-6)
- Git binary path override via `SHIPPER_GIT_BIN` env var (E-1)

**Security Controls:**
- Uses std::process::Command directly (no shell injection)
- `SHIPPER_GIT_BIN` override is explicit env-based, not user-input
- GitContext fields are optional (None when git operations fail)

---

### 4.3 shipper-core::state::events

**Purpose:** Append-only JSONL event log for publish operations.

**Key Files:**
- `state/events/mod.rs` - EventLog type

**Threats:**
- Event log tampering (T-2)
- Disk space exhaustion (D-3)
- Log injection via malicious crate names (low severity)

**Security Controls:**
- Append-only (OpenOptions with append=true)
- File created with 0600 permissions
- events.jsonl is authoritative truth (state.json is projection)

---

### 4.4 shipper-webhook

**Purpose:** Webhook notifications for publish events (Slack, Discord, generic).

**Key Files:**
- `crates/shipper-webhook/src/lib.rs`

**Threats:**
- Webhook URL interception (S-5)
- Payload tampering without HMAC verification (T-7)
- Webhook secret exposure (I-3)
- Endpoint availability (D-5)

**Security Controls:**
- HMAC-SHA256 signature in `X-Hub-Signature-256` header
- Whitespace-only secrets are ignored (no signature sent)
- Configurable timeout (default 30s, prevents hanging)
- Non-blocking delivery (webhook failure does not block publish)

---

### 4.5 shipper-encrypt

**Purpose:** AES-256-GCM encryption for state files with PBKDF2 key derivation.

**Key Files:**
- `crates/shipper-encrypt/src/lib.rs`

**Threats:**
- Passphrase exposure via CLI arguments (I-7)
- Weak passphrase selection
- Encrypted data tampering (authenticated encryption prevents this)
- Key derivation failure with weak passphrases

**Security Controls:**
- PBKDF2 with 100,000 iterations
- Random salt (16 bytes) per encryption
- Random nonce (12 bytes) per encryption
- AES-256-GCM (authenticated encryption)
- Passphrase via env var preferred over CLI
- `mask_passphrase()` for display purposes

---

### 4.6 shipper-registry

**Purpose:** Lightweight HTTP client for registry operations.

**Key Files:**
- `crates/shipper-registry/src/http.rs` - HttpRegistryClient
- `crates/shipper-registry/src/sparse_index.rs` - Sparse index handling

**Threats:**
- Network interception (MITM) on HTTP traffic
- DNS spoofing for registry endpoints
- Sparse index cache poisoning (T-6)
- Path traversal in cache directory (E-6)

**Security Controls:**
- HTTPS only for crates.io (TLS)
- ETag-based caching prevents stale responses
- Cache path derived from crate name (hardcoded, no user input)
- Timeout configured (default 30s)

---

### 4.7 shipper-core::engine

**Purpose:** Orchestration of plan → preflight → publish → resume pipeline.

**Key Files:**
- `engine/mod.rs` - Main entry points (run_preflight, run_publish, run_resume)
- `engine/publish/` - Publish execution with retry
- `engine/preflight/` - Preflight checks

**Threats:**
- Infinite retry loop with malformed state (D-7)
- State file tampering to skip crates (T-1)
- Git cleanliness bypass via --allow-dirty (misuse)

**Security Controls:**
- Plan ID SHA256 checksum validates plan integrity
- ErrorClass classification (Retryable, Permanent, Ambiguous) prevents infinite retry of permanent errors
- Reconciliation against registry truth before retrying ambiguous outcomes

---

## 5. Trust Boundaries

```
┌──────────────────────────────────────────────────────────────────┐
│                        TRUSTED ZONE                               │
│                                                                  │
│  ┌─────────────┐    ┌──────────────┐    ┌───────────────────┐   │
│  │  Developer  │    │  CI/CD Pipe  │    │   Local Machine   │   │
│  │  Workstation │    │  (GitHub     │    │   (User's shell)  │   │
│  │             │    │   Actions)   │    │                   │   │
│  └──────┬──────┘    └──────┬───────┘    └───────┬───────────┘   │
│         │                  │                    │               │
│         ▼                  ▼                    ▼               │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │  shipper executable (installed via cargo install --locked) │ │
│  └────────────────────────────────────────────────────────────┘ │
│                              │                                    │
└──────────────────────────────┼──────────────────────────────────┘
                               │
              ┌────────────────┴────────────────┐
              │         UNTRUSTED ZONE            │
              │                                  │
    ┌─────────▼─────────┐        ┌──────────────▼──────────┐
    │   crates.io API    │        │  Third-party Registry  │
    │   (HTTPS only)     │        │  (user-configured)     │
    └───────────────────┘        └─────────────────────────┘
              │
    ┌─────────▼─────────┐
    │  Webhook Endpoints │
    │  (user-configured) │
    └───────────────────┘
```

**Trust assumptions:**
- Developer machine is trusted (no local privilege escalation by design)
- CI/CD environment credentials are trusted
- Cargo credentials store is trusted
- Shipper binary is trusted (supply chain integrity verified by cargo install --locked)
- crates.io API is trusted (authentic registry responses)

---

## 6. Security Recommendations

### High Priority

1. **Token Protection in CI/CD**
   - Use `CARGO_REGISTRY_TOKEN` via GitHub Secrets, not inline
   - Consider OIDC-based trusted publishing to eliminate long-lived tokens
   - Enable IP allowlisting for token scope when available

2. **Supply Chain Integrity**
   - Verify crate checksums after `cargo install`
   - Use `--locked` flag for reproducible builds
   - Monitor for anomalous crate versions published from your account

3. **State File Integrity**
   - Enable encryption for sensitive environments
   - Use file integrity monitoring (e.g., AIDE, OSSEC) for `.shipper/` directory
   - Verify `receipt.json` matches events after each run

### Medium Priority

4. **Webhook Security**
   - Rotate webhook secrets periodically
   - Validate webhook signatures before processing
   - Use HTTPS webhook URLs only

5. **Credential File Permissions**
   - Verify `~/.cargo/credentials.toml` has 0600 permissions
   - Include permission checks in `shipper doctor`

6. **Sparse Index Cache Hardening**
   - Clear cache periodically or on security events
   - Verify cache integrity with ETag validation

### Low Priority

7. **Process Isolation**
   - Run shipper in dedicated CI job, not shared runner
   - Use container isolation to limit token exposure
   - Consider seccomp profiles for shipper process

8. **Logging and Monitoring**
   - Enable structured logging for security audit trail
   - Monitor for repeated 403/401 responses (token issues)
   - Alert on unexpected reconciliation outcomes

---

## 7. Threat Mitigation Summary

| Threat ID | Threat | Mitigation | Status |
|-----------|--------|-------------|--------|
| S-1 | Token theft | Mask tokens, never log | Implemented |
| S-2 | Credential injection | Parse validation, no shell | Implemented |
| S-3 | OIDC token theft | OIDC env var pairing required | Implemented |
| S-4 | Token exposure in logs | `mask_token()` enforced | Implemented |
| T-1 | State tampering | SHA256 plan checksum | Implemented |
| T-2 | Event log tampering | Append-only, 0600 perms | Implemented |
| R-1 | Repudiation of publish | GitContext + timestamp evidence | Implemented |
| R-2 | Repudiation of auth | AuthEvidence mode recording | Implemented |
| I-1 | Token in process list | Credential store preferred | Implemented |
| I-3 | Webhook secret exposure | Env var support | Implemented |
| I-7 | Encryption passphrase in logs | Env var support, `mask_passphrase()` | Implemented |
| D-1 | Rate limiting | Exponential backoff, Retry-After | Implemented |
| D-2 | Lock file orphan | Advisory lock with cleanup | Implemented |
| E-1 | Scoped token misuse | Scoped token recommendation | Documented |
| E-3 | Malicious config | Config validation | Implemented |

---

## 8. References

- [SECURITY.md](../../SECURITY.md) - Official security policy
- [docs/INVARIANTS.md](../../docs/INVARIANTS.md) - Events-as-truth contract
- [docs/specs/SHIPPER-SPEC-0004-json-evidence-contracts.md](../../docs/specs/SHIPPER-SPEC-0004-json-evidence-contracts.md) - Evidence format
- [docs/specs/SHIPPER-SPEC-0006-release-auth-evidence-and-trusted-publishing.md](../../docs/specs/SHIPPER-SPEC-0006-release-auth-evidence-and-trusted-publishing.md) - Auth evidence
- [crates/shipper-core/src/ops/auth/AGENTS.md](../../crates/shipper-core/src/ops/auth/AGENTS.md) - Token handling invariants
- [crates/shipper-encrypt/src/lib.rs](../../crates/shipper-encrypt/src/lib.rs) - Encryption implementation
- [crates/shipper-webhook/src/lib.rs](../../crates/shipper-webhook/src/lib.rs) - Webhook implementation

---

*This threat model was generated for security review purposes. It should be reviewed and updated periodically, especially after significant architectural changes or new feature additions.*
