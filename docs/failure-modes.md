# Failure modes and how shipper handles them

Publishing a Rust workspace is an **irreversible, non-atomic workflow**: once a
version is uploaded to a registry it cannot be re-published. Shipper is designed
around this constraint — every step is persisted, classified, and recoverable.

---

## Error classification

When `cargo publish` fails, Shipper inspects the combined stdout/stderr output
and classifies the error into one of three classes
(see `crates/shipper-cargo-failure`):

| Class | Meaning | Action taken |
|---|---|---|
| **Retryable** | Transient — likely to succeed on retry | Retry with backoff |
| **Permanent** | Requires human intervention | Stop retrying, record failure |
| **Ambiguous** | Outcome unclear (upload may have succeeded) | Reconcile against registry truth; accept `Published`, retry only a proved `NotPublished`, or stop on `StillUnknown` |

### Retryable patterns

Matched case-insensitively in cargo output:

`too many requests`, `429`, `timeout`, `timed out`, `connection reset`,
`connection refused`, `connection closed`, `dns`, `tls`,
`temporarily unavailable`, `failed to download`, `failed to send`,
`server error`, `500`, `502`, `503`, `504`, `broken pipe`,
`reset by peer`, `network unreachable`

### Permanent patterns

`failed to parse manifest`, `invalid`, `missing`, `license`, `description`,
`readme`, `repository`, `could not compile`, `compilation failed`,
`failed to verify`, `package is not allowed to be published`,
`publish is disabled`, `yanked`,
`forbidden`, `permission denied`, `not authorized`, `unauthorized`,
`version already exists`, `is already uploaded`, `token is invalid`,
`invalid credentials`, `checksum mismatch`

### Ambiguous fallback

If **no** pattern matches, the error is classified as **Ambiguous**. Shipper
then checks the registry to determine whether the version was actually
uploaded. It never converts an inconclusive `StillUnknown` result into a blind
retry.

---

## Retry behavior

Retries are handled by `crates/shipper-retry`. By default Shipper uses
exponential backoff with jitter (the `Default` policy).

### Default retry policy

| Parameter | Default | Description |
|---|---|---|
| `strategy` | `exponential` | `base_delay × 2^(attempt-1)` |
| `max_attempts` | **6** | Total tries (first attempt + 5 retries) |
| `base_delay` | **2 s** | Initial delay |
| `max_delay` | **120 s** (2 min) | Delay cap |
| `jitter` | **0.5** | ±50 % random variation on each delay |

### Predefined policies

| Policy | Strategy | Max attempts | Base delay | Max delay | Jitter |
|---|---|---|---|---|---|
| **default** | exponential | 6 | 2 s | 120 s | 0.5 |
| **aggressive** | exponential | 10 | 500 ms | 30 s | 0.3 |
| **conservative** | linear | 3 | 5 s | 60 s | 0.1 |

### Delay calculation example (default policy, no jitter)

| Attempt | Delay |
|---|---|
| 1 | 2 s |
| 2 | 4 s |
| 3 | 8 s |
| 4 | 16 s |
| 5 | 32 s |
| 6 | 64 s |

With jitter of 0.5, each delay is multiplied by a random factor in `[0.5, 1.5]`.

### Per-error-class overrides

In `.shipper.toml` you can set different retry parameters for each error class:

```toml
[retry]
policy = "custom"
strategy = "exponential"
max_attempts = 8
base_delay = "3s"
max_delay = "90s"
jitter = 0.4

[retry.per_error.retryable]
max_attempts = 10
base_delay = "1s"

[retry.per_error.ambiguous]
max_attempts = 4
base_delay = "5s"
```

### CLI overrides

```bash
shipper publish --max-attempts 10 --base-delay 5s --max-delay 5m
```

---

## State file format (`state.json`)

Shipper persists progress to `.shipper/state.json` after every package
completes. This file enables `shipper resume` to skip already-published crates.

```json
{
  "state_version": "shipper.state.v1",
  "plan_id": "a1b2c3d4",
  "registry": {
    "name": "crates-io",
    "api_base": "https://crates.io",
    "index_base": "https://index.crates.io"
  },
  "created_at": "2025-06-01T12:00:00Z",
  "updated_at": "2025-06-01T12:05:30Z",
  "packages": {
    "my-core@0.3.0": {
      "name": "my-core",
      "version": "0.3.0",
      "attempts": 1,
      "state": { "state": "published" },
      "last_updated_at": "2025-06-01T12:02:00Z"
    },
    "my-cli@0.3.0": {
      "name": "my-cli",
      "version": "0.3.0",
      "attempts": 0,
      "state": { "state": "pending" },
      "last_updated_at": "2025-06-01T12:00:00Z"
    }
  }
}
```

### Key fields

| Field | Purpose |
|---|---|
| `state_version` | Schema version (`shipper.state.v1`); used for forward-compatibility |
| `plan_id` | Deterministic hash of the registry API base and ordered package names/versions; resume refuses a stored/current mismatch, but this is not source or workspace provenance |
| `registry` | Target registry name, API base URL, and optional sparse-index URL |
| `packages` | `BTreeMap<"name@version", PackageProgress>` — one entry per planned crate |

### Package states

Each package in the state file has one of these states:

| State | Meaning |
|---|---|
| `pending` | Not yet attempted |
| `uploaded` | `cargo publish` exited 0 but readiness not yet confirmed |
| `published` | Confirmed visible on the registry — **terminal success** |
| `skipped` | Intentionally skipped (e.g. already on registry), includes `reason` |
| `failed` | Permanently failed, includes `class` (`retryable`/`permanent`/`ambiguous`) and `message` |
| `ambiguous` | Outcome unclear, includes `message` |

---

## Failure mode: Partial publish

**Scenario:** A workspace has crates `core`, `macros`, and `cli`. Publishing
`core` succeeds, then the network drops during `macros`.

**What happens:**
1. `core@0.3.0` is marked `published` in `state.json`.
2. `macros@0.3.0` fails — Shipper classifies as `Retryable`, retries up to
   `max_attempts`. If all retries fail, the state is saved as `failed`.
3. `cli@0.3.0` remains `pending`.

**Recovery:** inspect the typed local posture before choosing a command:
```bash
shipper status --durable
shipper inspect-events
shipper inspect-receipt --format json
```

Resume only when the durable outcome reports a coherent interrupted run and its
typed next action recommends `resume`. Shipper then confirms the current
`plan_id`, skips terminal packages, and continues from eligible pending or
retryable work. A different checkout can produce the same plan ID, so verify
the restored source independently.

---

## Failure mode: Ambiguous upload result

**Scenario:** `cargo publish -p macros` exits unsuccessfully after an upload,
but its output does not establish whether the registry accepted the crate.

**What happens:**
1. No retryable/permanent pattern matches the output — classified as **Ambiguous**.
2. Shipper queries the registry API: does `macros@0.3.0` exist?
3. **If `Published`:** marks `published`, continues to the next crate.
4. **If `NotPublished`:** the proved-absent upload may retry under policy.
5. **If `StillUnknown`:** persists reconciliation evidence and stops before
   another Cargo attempt.

**Recovery:**
```bash
shipper status --durable
shipper inspect-events
```

Do not resume while registry truth is inconclusive. Follow the typed
`reconcile` posture and retain `reconciliation.json` for diagnosis.

**Inspect evidence:**
```bash
shipper inspect-events     # chronological event log
shipper inspect-receipt    # structured receipt with attempt details
```

---

## Failure mode: Rate limiting (HTTP 429)

**Scenario:** crates.io returns `429 Too Many Requests` during a batch publish.

**What happens:**
1. Shipper matches `429` / `too many requests` in stderr → **Retryable**.
2. Applies exponential backoff: 2 s → 4 s → 8 s → 16 s → 32 s → 64 s (defaults).
3. Each retry is logged to `.shipper/events.jsonl`.
4. After 6 attempts (default), the package is marked `failed` with class
   `retryable` and the run continues with remaining crates.

**Recovery:** after the rate limit clears, classify the retained run first:
```bash
shipper status --durable
```

Run `shipper resume` only when the returned next action recommends it; a
retryable label or elapsed delay alone is not authorization.

**Tuning for large workspaces:**
```bash
# More retries with longer backoff
shipper publish --max-attempts 10 --base-delay 5s --max-delay 5m
```

---

## Failure mode: CI cancellation

**Scenario:** A GitHub Actions job is cancelled (timeout, manual cancel, or a
new push) after `core` and `macros` are published but before `cli`.

**What happens:**
1. The process is killed. `.shipper/state.json` reflects the last persisted
   state (state is saved after each crate completes).
2. `core` and `macros` show `published`; `cli` shows `pending`.

**Recovery:** restore the complete `.shipper/` artifact into the exact intended
checkout, use the matching candidate binary, then classify the run before
dispatch:
```bash
shipper status --durable
```

Only a coherent `interrupted` outcome can recommend `shipper resume`. Missing,
legacy, cross-host, corrupt, or mismatched identity evidence remains fail
closed; cancellation by itself does not prove that the old process is gone.

### Lock file safety

Shipper writes `.shipper/lock` to prevent concurrent runs. On supported Linux
hosts, current locks may carry `/proc`-derived process identity evidence. Other
platforms, legacy or missing/corrupt locks, and cross-host evidence remain
`Unknown`. Age alone — including the configured timeout — does not prove that a
process is dead.

```bash
# Inspect the correlated local evidence first
shipper status --durable

# Configure the age threshold used by lock acquisition; this is not liveness proof
shipper publish --lock-timeout 30m
```

Current publish lock acquisition can replace a lock older than
`--lock-timeout` using age alone; it does not consult durable status's stronger
process-liveness classification. Treat a reduced timeout as a destructive
operational choice, not proof of interruption. `--force` sets that threshold to
zero and is an even stronger override. Use either path only after an independent
operational decision establishes that no concurrent publisher can still mutate
the release.

---

## CI-specific guidance

Use [Run a release in GitHub Actions](how-to/run-in-github-actions.md) for the
maintained CI sequence, artifact boundaries, typed exit handling, and
release-authority fence. Do not copy an old “retry the job and it resumes”
snippet: CI must retain the complete evidence set and classify it from the
matching workspace before choosing a recovery command.

---

## Evidence and debugging

### Event log (`.shipper/events.jsonl`)

Append-only, one JSON object per line:

```json
{"timestamp":"2025-06-01T12:00:00Z","event_type":{"type":"execution_started"},"package":""}
{"timestamp":"2025-06-01T12:01:00Z","event_type":{"type":"package_started","name":"my-core","version":"0.3.0"},"package":"my-core@0.3.0"}
{"timestamp":"2025-06-01T12:01:05Z","event_type":{"type":"package_attempted","attempt":1,"command":"cargo publish -p my-core"},"package":"my-core@0.3.0"}
{"timestamp":"2025-06-01T12:01:20Z","event_type":{"type":"package_published","duration_ms":15000},"package":"my-core@0.3.0"}
```

### Receipt (`.shipper/receipt.json`)

Written only after the run finalizes. Contains per-package evidence (every attempt's
command, exit code, stdout/stderr tail, and timing) plus git context and
environment fingerprint. See [the types in `crates/shipper-types`](../crates/shipper-types/src/lib.rs)
for the full schema.

### Inspection commands

```bash
shipper inspect-events                 # human-readable event timeline
shipper inspect-receipt                # formatted receipt summary
shipper inspect-receipt --format json  # machine-readable for scripts
shipper status                         # compare local versions vs registry
shipper status --watch                 # watch local persisted progress
shipper status --durable               # correlate retained recovery evidence
shipper doctor                         # check environment, auth, tools
```

`inspect-events` is authoritative history, not process-liveness proof.
`inspect-receipt` fails when no finalized receipt exists. Durable status is the
read-only local classifier; malformed evidence exits `1` before it can render a
durable result.

### Cleaning up

```bash
shipper clean                 # remove all state files
shipper clean --keep-receipt  # keep receipt.json for auditing
```

---

## Getting help

If you encounter a failure not covered above:

1. Run `shipper doctor` to check your environment.
2. Inspect `.shipper/events.jsonl`, durable status, and any finalized receipt.
3. Redact workspace/package-sensitive context as required, then file an issue
   with the relevant evidence paths and typed outcome. Never attach registry
   tokens, passphrases, credentials, or unreviewed raw environment data.
