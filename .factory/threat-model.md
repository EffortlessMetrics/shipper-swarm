# Threat Model: EffortlessMetrics/shipper-swarm

**Generated:** 2026-05-25
**Repository:** EffortlessMetrics/shipper-swarm
**Technology Stack:** Rust (Edition 2024), three-crate architecture (shipper → shipper-cli → shipper-core)

---

## 1. Overview

Shipper is a cargo publish orchestration tool that automates the **plan → preflight → publish → resume** pipeline for Rust workspace publishing. It manages credentials, executes `cargo publish`, persists execution state to disk, and verifies registry visibility.

### Key Architectural Components

| Component | Responsibility | Security-Relevant Role |
|---|---|---|
| `shipper-core/src/ops/auth/` | Token resolution from env vars and credentials files | Handles registry authentication secrets |
| `shipper-core/src/ops/git/` | Git cleanliness checks, context capture | Validates git state before publishing |
| `shipper-core/src/ops/lock/` | Advisory file lock to prevent concurrent runs | Prevents race conditions in concurrent execution |
| `shipper-core/src/ops/process/` | Spawns `cargo publish` subprocess | Executes privileged cargo operations |
| `shipper-core/src/state/` | Persists state.json, events.jsonl, receipt.json | Stores sensitive execution metadata |
| `shipper-core/src/engine/` | Orchestrates the publish pipeline | Coordinates all security-relevant operations |

### State Files (`.shipper/` directory)

- `state.json` — Resumable execution state (schema-versioned)
- `receipt.json` — Audit receipt with evidence
- `events.jsonl` — Append-only event log (authoritative truth)
- `lock` — Advisory lock preventing concurrent publishes

---

## 2. STRIDE Threat Analysis

### 2.1 Spoofing

**Threat:** An attacker impersonates a legitimate user or process to obtain registry tokens or manipulate publish operations.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| Environment variable injection via symlink attack on CARGO_HOME | `ops/auth` | Low | High | Tokens resolved before exec; CARGO_HOME not directly user-controlled |
| Malicious `SHIPPER_GIT_BIN` override pointing to fake git binary | `ops/git` | Medium | High | `SHIPPER_GIT_BIN` respected in tests; production should use trusted PATH |
| Reading credentials.toml from a world-readable CARGO_HOME | `ops/auth` | Medium | High | File permissions depend on OS; cargo validates credentials format |
| Token theft via environment variable exposure in logs/debug output | `ops/auth` | Low | Critical | Tokens are opaque strings; masked in all logging via `mask_token()` |

**Details:**
- Token resolution order: `CARGO_REGISTRY_TOKEN` → `CARGO_REGISTRIES_<NAME>_TOKEN` → `$CARGO_HOME/credentials.toml` → legacy `$CARGO_HOME/credentials`
- Tokens are never logged; whitespace-trimmed and empty tokens treated as absent
- OIDC trusted publishing detection requires both `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN`

---

### 2.2 Tampering

**Threat:** An attacker modifies state files, lock files, or the git working tree to manipulate publish behavior.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| Corrupting `state.json` to force re-publishing already-published crates | `state/execution_state` | Medium | High | Schema version validation; corrupt JSON returns errors |
| Modifying `events.jsonl` to hide published crates from audit trail | `state/events` | Medium | Critical | Append-only design; events are authoritative truth over state.json |
| Tampering with lock file to steal publish slot from another process | `ops/lock` | Low | High | Lock contains PID/hostname/timestamp; stale lock detection via timeout |
| Modifying `.shipper.toml` config to redirect token or change registry | `shipper-config` | Medium | High | Config file parsed at startup; CLI flags override file values |
| Replay attack: replaying a stale lock to block legitimate publish | `ops/lock` | Low | Medium | Stale lock timeout (default 1h); `acquire_with_timeout` reclaims expired locks |
| Manipulating git working tree after cleanliness check but before publish | `ops/git` | Medium | High | Check-then-publish window; `--allow-dirty` bypasses check; not atomic |

**Details:**
- State writes use atomic write: temp file + fsync + rename (via `atomic_write_json`)
- Lock acquisition is advisory (check-then-create), not OS-level atomic — race conditions possible under heavy contention
- Lock file hash uses `DefaultHasher` (not guaranteed stable across Rust versions)
- Plan ID (SHA256-based) validates workspace match on resume

---

### 2.3 Repudiation

**Threat:** A user denies having published a specific version or performed a specific action.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| No cryptographic signature on published artifacts | `engine/publish` | High | High | No signature; relies on registry auth alone |
| Receipt lacks evidence of which user/token performed publish | `state/receipt` | Medium | Medium | Receipt includes git context, environment fingerprint, attempt evidence |
| events.jsonl can be deleted or truncated by operator | `state/events` | Medium | High | Append-only design; deleting events is possible but observable |
| No immutable audit log — events.jsonl is plaintext JSONL | `state/events` | Medium | Medium | File-based append; no cryptographic chaining or signing |

**Details:**
- `Receipt` includes: plan_id, registry, package receipts, git context, environment fingerprint, timestamps
- Event log records: PackageStarted, PackageAttempted, PackageOutput, PackagePublished, PackageFailed, RetryBackoffStarted
- Auth evidence collected: auth mode (Token, TrustedPublishing, Unknown), token detected flag, OIDC context presence

---

### 2.4 Information Disclosure

**Threat:** Sensitive data (tokens, credentials, workspace contents) is exposed to unauthorized parties.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| Registry token written to logs or events.jsonl | `ops/auth`, `state/events` | Low | Critical | Tokens are opaque strings; `mask_token()` used; stdout/stderr tails captured but tokens redacted in output |
| Sensitive data in state.json persisted to disk unencrypted | `state/execution_state` | Medium | High | State files written in plaintext; encryption support exists (`save_state_encrypted`) but not default |
| Token in CARGO_HOME credentials.toml readable by other users on multi-user system | `ops/auth` | Medium | High | Depends on OS file permissions; cargo does not enforce restrictive defaults |
| Credential file path information disclosed in error messages | `ops/auth/credentials` | Low | Low | Error messages include paths but not token content |
| OIDC token exposure via `ACTIONS_ID_TOKEN_REQUEST_TOKEN` env var | `ops/auth/oidc` | Medium | High | OIDC tokens are env vars; GitHub Actions handles these securely |
| `--output-lines` captures too much cargo output including warnings with token paths | `ops/process` | Low | Medium | `output_lines` configurable; tails of stdout/stderr captured in attempt evidence |

**Details:**
- `mask_token()` redacts token in diagnostic output; tokens never appear in debug/log output
- Credential file parsing: `credentials.toml` with `[registries.<name>]` and `token = "..."` format
- crates.io aliases handled: `crates-io`, `crates.io`, `crates_io`, nested `[registries.crates.io]`

---

### 2.5 Denial of Service

**Threat:** An attacker prevents legitimate publishing operations from completing.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| Lock file left by crashed/abandoned process blocking all publishes | `ops/lock` | Medium | High | `acquire_with_timeout` reclaims stale locks (default: 1 hour age) |
| Registry rate limiting (HTTP 429) causing publish retries and delays | `engine/retry` | High | Low | Registry-aware backoff; `is_new_crate` detection to avoid crates.io rate limits |
| Corrupting state.json to cause infinite retry loop | `state/execution_state` | Low | High | Schema validation on load; corrupt JSON causes error, not hang |
| Filling disk with events.jsonl entries (append-only) | `state/events` | Low | Medium | No rotation/compaction implemented; depends on operator cleanup |
| `cargo publish` subprocess left running consuming resources | `ops/process` | Low | Medium | Optional per-package timeout via `timeout` module; default: no timeout |

**Details:**
- Lock file stores: PID, hostname, acquired_at, optional plan_id
- Stale lock detection: compares `Utc::now() - acquired_at` against timeout (strictly greater than)
- Lock contention: `concurrent_acquire_only_one_succeeds` test shows at least one thread wins, but more may succeed in race
- Backoff: exponential backoff with jitter, max 60s default, configurable via `base_delay`/`max_delay`

---

### 2.6 Elevation of Privilege

**Threat:** An attacker gains capabilities beyond their intended role through the shipper tool.

| Threat | Affected Component | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| `SHIPPER_GIT_BIN` allows executing arbitrary git binary (not just path override) | `ops/git/bin_override` | Medium | High | Env var override respected for tests; production should use trusted PATH |
| Lock file allows writing arbitrary PID/hostname to disk | `ops/lock` | Low | Medium | JSON written atomically; PID from `std::process::id()` |
| Shipper runs with operator's cargo credentials — same permission level as cargo | `ops/process` | N/A | N/A | Shipper executes cargo; inherits operator's registry permissions |
| Configuration via `.shipper.toml` can specify arbitrary cargo flags | `shipper-config` | Medium | High | Config file in workspace; operator controls file contents |
| Webhook configuration can exfiltrate sensitive data to arbitrary URLs | `engine/webhook` | Low | High | Webhook events contain plan_id, package_name, error_class; no tokens |
| Parallel publishing shares state directory — potential for cross-workspace data leak | `engine/parallel` | Low | Medium | Parallel mode uses same state_dir; package isolation via BTreeMap |

---

## 3. Security-Relevant Data Flow

```
┌──────────────────────────────────────────────────────────────────────┐
│                         OPERATOR ENVIRONMENT                          │
│                                                                       │
│  CARGO_REGISTRY_TOKEN ─────┐                                         │
│  CARGO_REGISTRIES_<X>_TOKEN ┤                                         │
│  $CARGO_HOME/credentials.toml                                    │
│  ACTIONS_ID_TOKEN_REQUEST_* (OIDC)                                    │
└─────────────────────────────┼────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                       ops/auth (Token Resolution)                     │
│  resolve_token() → Option<String> (opaque token)                      │
│  detect_auth_type() → AuthType (Token | TrustedPublishing | Unknown) │
│  collect_auth_evidence() → AuthEvidence for receipts                 │
└─────────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                    shipper-registry (HTTP Client)                     │
│  version_exists(), check_new_crate(), is_version_visible_with_backoff │
└─────────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                    ops/process (cargo publish)                        │
│  cargo_publish(workspace_root, crate_name, registry, ...)             │
│  Runs: cargo publish -p <name> --registry <reg>                       │
│  Captures: stdout_tail, stderr_tail, exit_code, duration              │
└─────────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                   state/execution_state (Persistence)                 │
│  save_state() → atomic_write_json(state.json)                         │
│  write_receipt() → atomic_write_json(receipt.json)                    │
│  events::EventLog → append-only events.jsonl                          │
└─────────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│                    ops/lock (Advisory Lock)                           │
│  LockFile::acquire() → creates .shipper/lock with PID/host/timestamp  │
│  LockFile::acquire_with_timeout() → reclaims stale locks              │
└──────────────────────────────────────────────────────────────────────┘
```

---

## 4. Credential Handling Specifics

### Token Resolution Order (per `ops/auth/mod.rs`)

1. `CARGO_REGISTRY_TOKEN` env var (crates.io default only)
2. `CARGO_REGISTRIES_<NAME>_TOKEN` env var (registry-specific)
3. `$CARGO_HOME/credentials.toml` with registry aliases
4. Legacy `$CARGO_HOME/credentials` file

### Token Storage Formats

**credentials.toml:**
```toml
[registry]
token = "secret-token"

[registries.crates-io]
token = "secret-token"
```

**credentials (legacy):**
```toml
[registries.private-reg]
token = "secret-token"
```

### OIDC Trusted Publishing

Requires both:
- `ACTIONS_ID_TOKEN_REQUEST_URL`
- `ACTIONS_ID_TOKEN_REQUEST_TOKEN`

Detection precedence:
1. Explicit Cargo token → `AuthType::Token`
2. Full OIDC context → `AuthType::TrustedPublishing`
3. Partial OIDC context → `AuthType::Unknown`
4. No auth → `None`

---

## 5. State File Security

### Atomic Write Pattern

```rust
fn atomic_write_json(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let data = serde_json::to_vec_pretty(value)?;
    let mut f = File::create(&tmp)?;
    f.write_all(&data)?;
    f.sync_all().ok();
    fs::rename(&tmp, path)?;
    fsync_parent_dir(path);  // Best-effort on Windows
}
```

### Schema Versioning

- `state.json` → `shipper.state.v1`
- `receipt.json` → `shipper.receipt.v2` (migrated from v1)
- `events.jsonl` → append-only, no schema version header

### Encryption Support

- `save_state_encrypted()` / `load_state_encrypted()`
- `write_receipt_encrypted()` / `load_receipt_encrypted()`
- Uses `shipper_encrypt::StateEncryption` with configurable key

---

## 6. Git Integration Security

### Cleanliness Check Flow

1. `is_git_clean(repo_root)` → runs `git status --porcelain`
2. Any untracked, staged, or modified file → dirty
3. `ensure_git_clean()` → fail fast with error message

### Git Context Collection

- `collect_git_context()` → captures: commit, branch, tag, dirty status
- Short commit slicing: byte-based (not char-based)
- `SHIPPER_GIT_BIN` override for custom git binary

### Dirty Override

- `--allow-dirty` flag bypasses cleanliness check
- Allows publishing dirty working tree (e.g., with version bumps)

---

## 7. Lock Security Analysis

### Lock File Structure

```json
{
  "pid": 12345,
  "hostname": "build-server",
  "acquired_at": "2025-01-15T12:00:00Z",
  "plan_id": "abc123..."
}
```

### Lock Path Resolution

- Without workspace root: `.shipper/lock`
- With workspace root: `.shipper/lock_<hash>` (hash of workspace path)

### Stale Lock Recovery

```rust
// Lock age > timeout → considered stale
if age.num_seconds() > timeout.as_secs() {
    fs::remove_file(&lock_path)?;
    // Proceed with acquire
}
```

### Known Limitations

1. **Not atomic:** Check-then-create race; `concurrent_acquire_only_one_succeeds` test acknowledges this
2. **Best-effort Drop:** Lock release on drop silently succeeds if file already deleted
3. **Hash instability:** `DefaultHasher` not guaranteed stable across Rust versions

---

## 8. Registry Interaction Security

### Authentication Methods

| Method | Detection | Fallback |
|---|---|---|
| Cargo token | `CARGO_REGISTRY_TOKEN` / credentials.toml | None |
| OIDC trusted publishing | `ACTIONS_ID_TOKEN_REQUEST_URL` + `ACTIONS_ID_TOKEN_REQUEST_TOKEN` | Falls back to token |
| None | N/A | Fails with auth error |

### Version Existence Check

```rust
fn version_exists(&self, name: &str, version: &str) -> Result<bool> {
    // Queries registry API or sparse index
}
```

### Ambiguous Failure Handling

When `cargo publish` fails with ambiguous error (could have succeeded):
1. Record `PackageState::Ambiguous`
2. Run reconciliation against registry truth
3. Stop on `StillUnknown` instead of blind retry (per Reconcile competency)

### Readiness Verification

- `ReadinessConfig` controls verification behavior
- `prefer_index` and `index_path` are config-file-only (no CLI flags)
- Visibility check uses registry API or sparse index

---

## 9. High-Priority Security Considerations

### Critical

1. **Token confidentiality:** Tokens must never appear in logs, events.jsonl, or stdout/stderr captures. Current implementation masks tokens but relies on cargo not outputting tokens.

2. **State file integrity:** state.json and events.jsonl should be protected from tampering. Currently relies on filesystem permissions.

3. **Lock reliability:** Advisory lock is not a true mutex; concurrent processes may both acquire the lock under race conditions.

### High

4. **Git working tree manipulation:** Check-then-publish window allows modification between git cleanliness check and actual publish.

5. **Config file injection:** `.shipper.toml` can redirect operations or change registry targets.

6. **Credential file access:** Tokens in `$CARGO_HOME/credentials.toml` depend on OS-level file permissions.

7. **Parallel mode isolation:** Packages in parallel mode share state_dir; cross-package data leakage theoretically possible.

### Medium

8. **Receipt non-repudiation:** No cryptographic signature on receipts; relies on registry as source of truth.

9. **events.jsonl retention:** Append-only log has no rotation or compaction; can grow indefinitely.

10. **OIDC token handling:** `ACTIONS_ID_TOKEN_REQUEST_TOKEN` is an env var; could be exposed in process listing.

---

## 10. Threat Mitigation Summary

| Threat Category | Primary Mitigation | Secondary Mitigation |
|---|---|---|
| Spoofing | Token resolution from trusted sources only | Masking in all output |
| Tampering | Atomic file writes, schema validation | Stale lock detection |
| Repudiation | Receipt with git context + evidence | Append-only events |
| Information Disclosure | Token masking, no logging | Encryption at rest |
| Denial of Service | Stale lock recovery, rate limit backoff | Timeout on cargo |
| Elevation of Privilege | No privileged operations in shipper itself | Config validation |

---

## 11. Recommendations

1. **Add `--output redact` flag:** Automatically redact any token-like strings from cargo output before capture.

2. **Consider OS-level file locking:** Replace advisory lock with `flock()` or equivalent for true mutual exclusion.

3. **Cryptographic event signing:** Sign events.jsonl entries with a workspace-specific key to prevent tampering.

4. **State directory permissions:** Document and optionally enforce restrictive permissions on `.shipper/` directory.

5. **Parallel mode audit:** Review parallel publishing for potential cross-package state leakage.

6. **Config validation:** Add schema validation for `.shipper.toml` to catch redirect attempts early.

---

*Generated by Factory Droid threat-model-generation skill*
