# STRIDE Threat Model: shipper-swarm

**Document:** STRIDE-based threat model for shipper-swarm
**Target:** EffortlessMetrics/shipper-swarm repository
**Date:** 2026-06-22
**Methodology:** STRIDE (Spoofing, Tampering, Repudiation, Information Disclosure, Denial of Service, Elevation of Privilege)

---

## 1. Spoofing (Authentication Threats)

Threats related to pretending to be someone else or something else.

### T1.1: Credential File Theft
- **Description:** An attacker with filesystem access steals registry tokens from `$CARGO_HOME/credentials.toml` or legacy `$CARGO_HOME/credentials` file
- **Affected Components:** `shipper-core/src/ops/auth/credentials.rs`
- **Impact:** Unauthorized publication to registries under victim's account
- **Likelihood:** Medium (depends on filesystem permissions)
- **Mitigation:** 
  - File permissions on credentials files should be restrictive (0600)
  - Documentation advises users on proper permissions
  - No token logging (already implemented via `mask_token()`)

### T1.2: Environment Variable Interception
- **Description:** An attacker reads registry tokens from environment variables (`CARGO_REGISTRY_TOKEN`, `CARGO_REGISTRIES_<NAME>_TOKEN`)
- **Affected Components:** `shipper-core/src/ops/auth/resolver.rs`
- **Impact:** Unauthorized publication to registries
- **Likelihood:** Low (requires access to running process environment)
- **Mitigation:** 
  - Tokens are opaque strings, never logged
  - Process isolation assumed as boundary

### T1.3: Trusted Publishing OIDC Token Theft
- **Description:** Attacker obtains GitHub OIDC tokens from environment variables (`ACTIONS_ID_TOKEN_REQUEST_URL`, `ACTIONS_ID_TOKEN_REQUEST_TOKEN`)
- **Affected Components:** `shipper-core/src/ops/auth/oidc.rs`
- **Impact:** Unauthorized publication via victim's trusted publishing path
- **Likelihood:** Low (requires CI environment access)
- **Mitigation:** 
  - OIDC tokens are short-lived and scoped to specific workflow
  - Environment variables used only for detection, not storage

### T1.4: Lock File Impersonation
- **Description:** An attacker creates a fake lock file to prevent legitimate publishes
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`
- **Impact:** Denial of service for publish operations
- **Likelihood:** Medium (lock file is world-readable)
- **Mitigation:** 
  - Lock contains PID, hostname, timestamp (identifies attacker)
  - Stale lock timeout allows recovery
  - Lock info includes plan_id for tracking

---

## 2. Tampering (Data Integrity Threats)

Threats related to modifying data or code without authorization.

### T2.1: State File Manipulation
- **Description:** Attacker modifies `.shipper/state.json` to alter publish progress or skip packages
- **Affected Components:** `shipper-core/src/state/store/fs.rs`, `shipper-core/src/state/execution_state/`
- **Impact:** Corrupted publish state, skipped packages, inconsistent registry state
- **Likelihood:** Medium (state directory may have permissive permissions)
- **Mitigation:** 
  - Events-as-truth invariant: `events.jsonl` is authoritative
  - Schema version validation on load
  - Atomic writes via temp file + rename
  - SHA256-based plan ID validation on resume

### T2.2: Event Log Injection
- **Description:** Attacker appends or modifies `events.jsonl` to falsify publish history
- **Affected Components:** `shipper-core/src/state/events/`
- **Impact:** False audit trail, potential for replay attacks
- **Likelihood:** Medium
- **Mitigation:** 
  - Events are append-only (no modification of existing entries)
  - Events contain hash of previous event (chain integrity)
  - Schema validation on event parsing

### T2.3: Receipt Forgery
- **Description:** Attacker modifies `receipt.json` to create false evidence of successful publish
- **Affected Components:** `shipper-core/src/state/execution_state/`
- **Impact:** False audit evidence, CI bypass
- **Likelihood:** Medium
- **Mitigation:** 
  - Receipt derived at end-of-run, not independently trusted
  - Events.jsonl remains authoritative
  - Exit codes and stdout/stderr captured (harder to fake)

### T2.4: Lock File Corruption
- **Description:** Attacker corrupts lock file to cause denial of service or confusion
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`
- **Impact:** Unpredictable lock behavior, potential race conditions
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - Corrupt lock files treated as stale and replaced
  - JSON parsing errors handled gracefully
  - Best-effort release on Drop

### T2.5: Configuration File Tampering
- **Description:** Attacker modifies `.shipper.toml` to change publish policy or registry settings
- **Affected Components:** `shipper-core/src/config.rs`
- **Impact:** Changed publish behavior, different registry targets
- **Likelihood:** Low-Medium (requires workspace write access)
- **Mitigation:** 
  - CLI flags override config file values
  - `config validate` command for integrity checks

### T2.6: Cargo Manifest Manipulation
- **Description:** Attacker modifies `Cargo.toml` to change package versions or dependencies before publish
- **Affected Components:** `shipper-core/src/plan/`, `shipper-core/src/ops/cargo/`
- **Impact:** Wrong versions published, dependency confusion
- **Likelihood:** Low (typically requires git access)
- **Mitigation:** 
  - Git cleanliness checks in preflight
  - Plan ID derived from workspace metadata

---

## 3. Repudiation (Non-Repudiation Threats)

Threats related to denying having performed an action.

### T3.1: No Cryptographic Signing
- **Description:** Publish events are not cryptographically signed, allowing operators to deny actions
- **Affected Components:** `shipper-core/src/state/events/`
- **Impact:** Lack of accountability, no verifiable audit trail
- **Likelihood:** High (current design limitation)
- **Mitigation:** 
  - Events include git context (commit, author)
  - CI environment fingerprint captured
  - Receipt includes environment fingerprint

### T3.2: Token Source Ambiguity
- **Description:** No clear attribution of which token was used for a publish operation
- **Affected Components:** `shipper-core/src/ops/auth/resolver.rs`
- **Impact:** Difficulty tracing which credential performed an action
- **Likelihood:** Medium
- **Mitigation:** 
  - `AuthInfo` records source (env var vs credentials file)
  - `shipper doctor` command shows current auth configuration

### T3.3: Events Not Timestamped with Trust Anchor
- **Description:** Event timestamps are local system time, not from a trusted source
- **Affected Components:** `shipper-core/src/state/events/`
- **Impact:** Timestamp manipulation possible
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - Chrono/UTC timestamps
  - Lock file includes `acquired_at` timestamp

---

## 4. Information Disclosure (Confidentiality Threats)

Threats related to exposing sensitive information to unauthorized parties.

### T4.1: Token Logging/Exposure
- **Description:** Registry tokens accidentally logged or exposed in output
- **Affected Components:** All modules handling tokens
- **Impact:** Token theft, unauthorized registry access
- **Likelihood:** Medium (common security issue)
- **Mitigation:** 
  - `mask_token()` function masks middle of tokens
  - Tokens are opaque strings, never logged as full value
  - Evidence files redact tokens

### T4.2: Token Stored in Memory After Resolution
- **Description:** Tokens remain in memory after use, potentially accessible via memory dumps
- **Affected Components:** `shipper-core/src/ops/auth/resolver.rs`
- **Impact:** Token extraction from process memory
- **Likelihood:** Low (requires memory access)
- **Mitigation:** 
  - Tokens cleared when AuthInfo goes out of scope
  - No persistent in-memory token cache

### T4.3: Credentials File Permissions Too Permissive
- **Description:** `$CARGO_HOME/credentials.toml` readable by other users on shared systems
- **Affected Components:** `shipper-core/src/ops/auth/credentials.rs`
- **Impact:** Token theft by other local users
- **Likelihood:** Low-Medium (depends on system)
- **Mitigation:** 
  - Documentation advises on file permissions
  - Tool does not control filesystem permissions

### T4.4: Command Line Arguments Visible
- **Description:** Tokens passed via CLI arguments visible in process list
- **Affected Components:** `shipper-cli`
- **Impact:** Token theft via process inspection
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - Tokens should use environment variables, not CLI args
  - No CLI flag for direct token (only config/env resolution)

### T4.5: State Directory Contents Exposed
- **Description:** `.shipper/` directory contents readable, exposing state and event history
- **Affected Components:** `shipper-core/src/state/store/fs.rs`
- **Impact:** Information disclosure about publish history, registry relationships
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - State directory should have restrictive permissions
  - Events.jsonl contains detailed publish history

### T4.6: Network Traffic Interception
- **Description:** Registry API communication intercepted (tokens, package data)
- **Affected Components:** Registry clients (crates.io API, sparse index)
- **Impact:** Token theft, package content modification
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - HTTPS required for all registry communication
  - crates.io API uses HTTPS by default

---

## 5. Denial of Service (Availability Threats)

Threats related to denying service to legitimate users.

### T5.1: Lock File DoS
- **Description:** Attacker creates persistent lock file preventing any publish operations
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`
- **Impact:** Complete publish blockage
- **Likelihood:** Medium (lock file is easy to create)
- **Mitigation:** 
  - `acquire_with_timeout` detects stale locks
  - Lock contains holder identity (PID, hostname)
  - Manual intervention possible to remove lock

### T5.2: Concurrent Publish Race Condition
- **Description:** Multiple shipper instances run simultaneously, causing publish conflicts
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`, `shipper-core/src/engine/publish/`
- **Impact:** Duplicate publish attempts, registry errors, corrupted state
- **Likelihood:** Low-Medium (lock is advisory, not atomic)
- **Mitigation:** 
  - Lock file mechanism provides coordination
  - Registry rejects duplicate version publishes
  - Idempotent design skips already-published packages

### T5.3: Registry Unreachable
- **Description:** Registry becomes unavailable during publish, causing indefinite hang or failure
- **Affected Components:** `shipper-core/src/engine/preflight/`, `shipper-core/src/engine/publish/`
- **Impact:** Publish interrupted, state preserved for resume
- **Likelihood:** Medium
- **Mitigation:** 
  - Retry logic with backoff for transient failures
  - HTTP 429 triggers rate limit backoff
  - Resume capability after interruption

### T5.4: State Directory Full
- **Description:** Disk space exhausted, preventing state/event writes
- **Affected Components:** `shipper-core/src/state/store/fs.rs`
- **Impact:** Publish cannot continue, state loss on interruption
- **Likelihood:** Low-Medium
- **Mitigation:** 
  - Atomic writes minimize corruption risk
  - Error propagates, stopping operation cleanly

### T5.5: Malicious Package Name Collision
- **Description:** Attacker publishes package with same name as legitimate workspace package to squat
- **Affected Components:** Registry interaction
- **Impact:** Legitimate publish fails or publishes to wrong owner
- **Likelihood:** Low (crates.io has ownership verification)
- **Mitigation:** 
  - crates.io ownership checks in preflight
  - Package ownership verification before publish

---

## 6. Elevation of Privilege (Authorization Threats)

Threats related to gaining capabilities or access without proper authorization.

### E1.1: Lock File TOCTOU Race
- **Description:** Time-of-check-time-of-use race in lock acquisition allows two processes to both acquire lock
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`
- **Impact:** Concurrent publishes, potential duplicate publishes
- **Likelihood:** Low-Medium (race window is small)
- **Mitigation:** 
  - Atomic rename after write
  - At least one process will succeed (worst case: both succeed under heavy contention)
  - Registry rejects duplicates anyway

### E1.2: Plan ID Collision
- **Description:** SHA256 plan ID collision allows resume of wrong plan
- **Affected Components:** `shipper-core/src/plan/build_pipeline.rs`
- **Impact:** Wrong packages published, state confusion
- **Likelihood:** Very Low (SHA256 collision resistance)
- **Mitigation:** 
  - SHA256-based plan ID has strong collision resistance
  - Plan ID validated on resume

### E1.3: Path Traversal via State Directory
- **Description:** Attacker provides malicious state directory path to access unintended files
- **Affected Components:** `shipper-core/src/state/store/fs.rs`
- **Impact:** Read/write of arbitrary files
- **Likelihood:** Very Low (path controlled by operator)
- **Mitigation:** 
  - State directory is operator-provided
  - No user input to path construction

### E1.4: Workspace Root Hash Collision
- **Description:** Two different workspace roots produce same hash for lock disambiguation
- **Affected Components:** `shipper-core/src/ops/lock/mod.rs`
- **Impact:** Lock path collision between workspaces
- **Likelihood:** Very Low (DefaultHasher has large output space)
- **Mitigation:** 
  - Different workspaces should have different state dirs
  - Hash collision extremely unlikely

### E1.5: Unsafe Deserialization
- **Description:** Malicious JSON in state/event files causes panic or code execution
- **Affected Components:** `shipper-core/src/state/`, `shipper-core/src/ops/lock/`
- **Impact:** Process crash, potential code execution
- **Likelihood:** Very Low (Rust's serde is safe)
- **Mitigation:** 
  - serde_json handles untrusted input safely
  - Error handling for malformed JSON
  - No unsafe code in workspace (`unsafe_code = "forbid"`)

---

## Summary: Risk Matrix

| ID | Category | Threat | Severity | Likelihood | Risk Score |
|----|----------|--------|----------|------------|------------|
| T1.1 | Spoofing | Credential file theft | High | Medium | **High** |
| T1.2 | Spoofing | Environment variable interception | High | Low | **Medium** |
| T1.3 | Spoofing | OIDC token theft | High | Low | **Medium** |
| T1.4 | Spoofing | Lock file impersonation | Low | Medium | **Low** |
| T2.1 | Tampering | State file manipulation | High | Medium | **High** |
| T2.2 | Tampering | Event log injection | Medium | Medium | **Medium** |
| T2.3 | Tampering | Receipt forgery | Medium | Medium | **Medium** |
| T2.4 | Tampering | Lock file corruption | Low | Low-Med | **Low** |
| T2.5 | Tampering | Configuration tampering | Medium | Low-Med | **Medium** |
| T2.6 | Tampering | Cargo manifest manipulation | High | Low | **Medium** |
| T3.1 | Repudiation | No cryptographic signing | Medium | High | **Medium** |
| T3.2 | Repudiation | Token source ambiguity | Low | Medium | **Low** |
| T3.3 | Repudiation | Event timestamps | Low | Low-Med | **Low** |
| T4.1 | Info Disclosure | Token logging | Critical | Medium | **High** |
| T4.2 | Info Disclosure | Token in memory | Medium | Low | **Low** |
| T4.3 | Info Disclosure | Credentials file permissions | High | Low-Med | **Medium** |
| T4.4 | Info Disclosure | CLI arguments visible | High | Low-Med | **Medium** |
| T4.5 | Info Disclosure | State directory exposure | Low | Low-Med | **Low** |
| T4.6 | Info Disclosure | Network interception | High | Low-Med | **Medium** |
| T5.1 | DoS | Lock file DoS | Medium | Medium | **Medium** |
| T5.2 | DoS | Concurrent publish race | Medium | Low-Med | **Low** |
| T5.3 | DoS | Registry unreachable | Medium | Medium | **Medium** |
| T5.4 | DoS | State directory full | Medium | Low-Med | **Low** |
| T5.5 | DoS | Package name collision | Low | Low | **Low** |
| E1.1 | Elevation | Lock file TOCTOU | Medium | Low-Med | **Low** |
| E1.2 | Elevation | Plan ID collision | Medium | Very Low | **Low** |
| E1.3 | Elevation | Path traversal | High | Very Low | **Low** |
| E1.4 | Elevation | Workspace root hash collision | Low | Very Low | **Low** |
| E1.5 | Elevation | Unsafe deserialization | Critical | Very Low | **Medium** |

---

## Top Priority Mitigations

1. **T4.1 Token Logging (High Priority)**
   - Status: Already mitigated via `mask_token()` implementation
   - Continue ensuring no full tokens in logs

2. **T1.1 Credential File Theft (High Priority)**
   - Add documentation warning about file permissions
   - Consider adding permission checks in `config init`

3. **T2.1 State File Manipulation (High Priority)**
   - Events-as-truth invariant already in place
   - Ensure schema validation is comprehensive

4. **T3.1 No Cryptographic Signing (Medium Priority)**
   - Consider adding optional GPG/PGP signing for events
   - Git commit signing as proxy for now

5. **T5.1 Lock File DoS (Medium Priority)**
   - Current mitigation (stale lock detection) is adequate
   - Consider adding audit logging for lock operations

---

## Component Security Notes

### shipper-core (Engine)
- No `unsafe` code (`unsafe_code = "forbid"`)
- Well-isolated `ops` layer with architecture guard
- Comprehensive error handling
- Property-based and snapshot tests for robustness

### shipper-cli (CLI Adapter)
- CLI arguments never directly accept tokens (uses env/resolver)
- Output sanitization for sensitive data

### shipper (Facade)
- Thin wrapper, no security-sensitive logic

---

*Document generated: 2026-06-22*
*Last reviewed: 2026-06-22*
