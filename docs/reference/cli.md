# CLI reference

**Canonical source:** `shipper --help` and `shipper <subcommand> --help`. Help
snapshots are the executable command-surface authority; this page organizes the
stable operator contracts around that surface.

This page is a topical map, not an exhaustive flag listing. For exhaustive flags, use `--help`.

## First-run command chain

Use the `shipper` facade binary for the user-facing workflow:

```bash
shipper doctor
shipper plan
shipper status
shipper preflight
```

`doctor` catches local setup blockers, `plan` shows what would publish,
`status` compares local versions to the registry, and `preflight` gives the
release-readiness verdict.

`status` is read-only. Its help shows only controls that affect inspection;
publish and resume controls remain accepted for command-line compatibility but
are intentionally omitted from that help surface.

The planning-only `plan-yank` and `fix-forward` commands follow the same rule:
their help focuses on receipt and planning inputs, not controls for executing a
publish or retrying one.

For CI, internal developer portals, or agent consumers, `shipper doctor`,
`shipper plan`, `shipper status`, `shipper preflight`, `shipper publish`,
and `shipper resume` support `--format json`. `publish` and `resume` emit
command-owned JSON envelopes with artifact paths and nested release receipt
evidence for each targeted registry.

## Commands at a glance

| Command | What it does | Writes state? |
|---|---|---|
| `shipper plan` | Compute and print the deterministic publish order | No |
| `shipper preflight` | Run safety checks without publishing | No (emits events) |
| `shipper publish` | Publish missing workspace versions; skip already-published `name@version` pairs | Yes |
| `shipper resume` | Continue from the last persisted state | Yes |
| `shipper rehearse` | Publish the plan to an alternate registry and verify it there | Yes (`rehearsal.json`, events) |
| `shipper status` | Compare local versions to the registry; `--watch` and `--durable` are separate local-evidence modes | No |
| `shipper doctor` | Environment / auth / connectivity diagnostics | No |
| `shipper inspect-events` | View the event log | No |
| `shipper inspect-receipt` | View the end-of-run receipt | No |
| `shipper clean` | Clean `.shipper/` state files | Yes (destructive) |
| `shipper yank` | Yank one `crate@version`, or execute a reviewed yank plan — containment, not undo | Yes (events; registry-mutating) |
| `shipper plan-yank` | Generate a reverse-topological yank plan from a receipt | No (prints the plan) |
| `shipper fix-forward` | Generate a supersession plan from a receipt marked compromised | No (prints the plan) |
| `shipper remediate` | Generate or execute a receipt-driven remediation plan | Yes (`remediation-plan.json`; `--execute-plan` yanks) |
| `shipper config init` | Generate a default `.shipper.toml` | No |
| `shipper config validate` | Validate an existing config | No |
| `shipper completion <shell>` | Generate shell completion scripts | No |
| `shipper ci <platform>` | Print a CI workflow template | No |

## Most-used flags

### Global

- `--config <path>` — path to a custom `.shipper.toml`
- `--manifest-path <path>` — path to the workspace `Cargo.toml`
- `--registry <name>` — Cargo registry name (default `crates-io`)
- `--state-dir <path>` — directory for `.shipper/` state
- `--format <text|json>` — output format for structured commands
- `--verbose`, `-q/--quiet` — verbosity controls

### Publish safety

- `--policy <safe|balanced|fast>` — verification posture
- `--verify-mode <workspace|package|none>` — dry-run granularity
- `--readiness-method <api|index|both>` — post-publish visibility check
- `--max-attempts <N>` — retry budget per crate (default 6)
- `--base-delay <duration>`, `--max-delay <duration>` — backoff envelope
- `--verify-timeout <duration>`, `--readiness-timeout <duration>` — verification budgets

### Preflight

- `--allow-dirty` — permit a dirty git working tree
- `--skip-ownership-check` — skip the owners preflight
- `--strict-ownership` — fail preflight on any ownership ambiguity

### Resume

- `--force-resume` — resume even if the computed plan differs from the state file (advanced; can cause duplicate publish attempts if misused)
- `--resume-from <crate>` — start from a specific crate

Completed resume output ends with one typed operator outcome. Human output
renders `Result`, `Safe to resume`, `Next`, and retained `Evidence`; JSON adds
the same object at `shipper.resume.v1.outcome` while preserving the existing
top-level fields. Ambiguous registry truth stops for reconciliation, uploaded
work waits for registry visibility, permanent failures require repair, and
only durable pending or retryable work recommends resume.

Resume's plan guard compares the stored and recomputed `plan_id`. That ID is a
hash of the registry API base and ordered package names/versions. It does not
prove source bytes, workspace path, or run identity; restore and independently
verify the intended checkout before recovery.

### Status modes

The three status modes have independent contracts:

| Mode | Reads | Schema | Registry access |
|---|---|---|---|
| `shipper status` | workspace plan and registry observations | `shipper.status.v1` | yes |
| `shipper status --watch` | local state and events | `shipper.status.watch.v1` | no |
| `shipper status --durable` | local events/state/receipt/reconciliation plus lock evidence | `shipper.status.durable.v1` | no |

`--watch` and `--durable` conflict. Durable mode also rejects the multi-registry
selectors because it must not guess which segregated state directory represents
the run; singular `--registry` remains available to select the matching plan.
Durable status reports one of `no_evidence`, `terminal`, `interrupted`,
`ambiguous`, `identity_mismatch`, `live`, `unknown`, or
`evidence_disagreement`. All but a coherent `interrupted` result are commandless
and fail closed. A `NotLive` process
observation is necessary but is not by itself permission to resume. Corrupt or
unreadable evidence is a command error (exit `1`, empty stdout, diagnostic on
stderr) before a durable envelope can be classified.

### Parallel

- `--parallel`, `--max-concurrent <N>` — parallelize within dependency levels
- `--per-package-timeout <duration>` — per-package timeout in parallel mode

## Policy matrix

| Policy | Verify mode | Readiness | Best for |
|---|---|---|---|
| `safe` (default) | `workspace` | `both` | Production releases |
| `balanced` | `package` | `api` | Regular releases when you want speed without skipping essentials |
| `fast` | `none` | none | Dev / sandbox registries only — not recommended for crates.io |

## Shared operator outcome vocabulary

Each primary command owns its typed outcome and compatibility-frozen fields.
Where a command supports both human and JSON output, the two views preserve the
same command-owned decision without implying one uniform cross-command schema.
Depending on what that command can actually establish, the outcome may carry:

- a typed result/status;
- failure class or finishability where the command can establish it;
- `safe_to_rerun` or `safe_to_resume`, including the reason rather than a bare
  Boolean;
- one typed next action and its reason;
- plan/package/registry identity that the command actually observed; and
- paths to evidence the command actually retained.

`doctor` summarizes passed, failed, skipped, and unknown checks, then chooses a
reason-bearing next action. `preflight` reports `PROVEN`, `NOT PROVEN`, or
`FAILED`; no label alone authorizes a live publish, so follow its typed next
action and the release-authority process. `publish`, `resume`, and durable
status render their human guidance from the same private outcome object used by
their JSON envelope. Non-watch status deliberately does not claim durable
evidence or rerun safety; plan likewise cannot invent execution evidence.
Human/JSON parity is semantic within each command, not byte-for-byte prose
identity or a shared wire shape.

## Exit codes

For canonical command-level behavior, `shipper --help` remains authoritative.
The table below documents the stable CI contract surfaces for idempotent
workspace publishing and recovery.

| Command | Scenario | Exit |
|---|---|---:|
| `shipper publish` | All package versions already exist | `0` |
| `shipper publish` | Mixed skipped-existing and successful publishes | `0` |
| `shipper publish` | Finalized permanent failure | `2` |
| `shipper publish` | Finalized retry budget exhaustion | `2` |
| `shipper publish` | Ambiguous cargo result reconciled to published | `0` |
| `shipper publish` | Ambiguous cargo result remains `StillUnknown` before receipt finalization | `1` |
| `shipper resume` | All packages already terminal | `0` |
| `shipper resume` | Plan/state mismatch | `1` unless forced |
| `shipper status` | Mixed registry state read succeeds | `0` |
| `shipper status` | Registry/query failure | `1` |
| `shipper status --durable` | Valid local evidence posture, including fail-closed classified outcomes | `0` |
| `shipper status --durable` | Malformed or unreadable evidence | `1` |

Non-watch `status` is a read-only registry observation. Its additive
`shipper.status.v1.outcome` reports `all_published`, `partially_published`,
`not_published`, or `no_publishable_packages`, states that no publication was
performed, and provides one commandless `none_complete`, `preflight`, or
`plan` posture. Human output renders the same Result and Next reason after the
existing plan and package lines. Because this command does not load durable
run evidence, it does not claim that publish or resume is safe and does not
invent state, event, or receipt evidence. Registry/query failures keep their
existing non-zero error path and do not emit a completed outcome.
Package observations are planned separately for each effective registry, so a
crate restricted with Cargo's `package.publish` allowlist is queried only for
registries that may receive it. Each registry row carries its authoritative
`plan_id`; the legacy top-level `plan_id` identifies the first effective
registry and remains in its existing field position. Consequently,
`all_published` means every registry-eligible selected version is visible in
its effective target registry or registries; it does not claim that ineligible
packages were queried everywhere.

### The publish / resume exit vocabulary

`publish` and `resume` distinguish their two failure shapes, so a CI job can
branch without parsing output:

| Code | Meaning | What CI should do |
|---:|---|---|
| `0` | Every package reached a successful terminal state (published or already present) | proceed |
| `2` | At least one package did not — the run finalized a receipt | follow the typed `outcome.next_action`; resume only when it says `resume` |
| `1` | The run failed before finalizing a receipt (auth, plan, lock, config) | fix the cause, rerun |

Exit `2` means a receipt exists and operator attention is required, *not* that
the invocation was rejected or that a blind retry is safe. In
`shipper.publish.v1`, the additive `outcome` field derives one next-action
posture from the completed receipt: terminal success is `none_complete`,
retryable unfinished work is `resume`, permanent failure is
`resolve_blockers`, an uploaded package awaiting visibility is
`wait_for_registry`, and an ambiguous outcome whose registry truth remains
unknown is `reconcile` without a fabricated command. Human output renders
`Result`, `Safe to rerun`, `Next`, and `Evidence` from that same typed outcome.

Exit-1 failures while building the `publish` plan or returned by the publish
engine before receipt finalization do not emit a partial `shipper.publish.v1`
document. With `--format json`, those two wrapped boundaries write a
`shipper.publish.error.v1` envelope to stderr and keep stdout empty. Its stable
category and summary do not expose the raw cause, `safe_to_rerun.value` is
`null` because no completed receipt proves that posture, its next action is
commandless, and its evidence list is empty. Human output retains the redacted
error chain and renders the same Result, Safe-to-rerun, Next, and Evidence
posture. Configuration/option validation errors outside those boundaries and
other commands keep their existing error output.

`StillUnknown` is the evidence-backed exception to that generic early-error
posture. Although the engine stops before finalizing a receipt, it has already
persisted authoritative reconciliation evidence. Structured output therefore
emits one uncontaminated `shipper.publish.error.v1` document with category
`ambiguous`, `safe_to_rerun.value: false`, a commandless `reconcile` action,
and the resolved `state.json`, `events.jsonl`, and `reconciliation.json` paths.
Human output derives the same posture from that evidence. No receipt is
claimed, and exit code `1` is unchanged.

Note that `2` covers "all packages failed" as well as "some failed":
finalization classifies every non-all-successful receipt as
`PartialFailure`, so `CompleteFailure` (`1`) is not reachable from a
finalized run today. Treat `1` as "the command errored out", not as "all
packages failed". Read `state.json` or `events.jsonl` if you need the
per-package breakdown.

With `--registries` / `--all-registries`, the exit code reflects the
**worst** outcome across every targeted registry, so a partial failure on
one registry is not masked by a success on another.

**Caveat:** argument-parsing errors (unknown flag, missing subcommand,
invalid `--format` value) also exit `2`, because that is clap's convention
and no work is performed.

The two are distinguishable by side effect: a usage error writes nothing,
so a partial failure always leaves a `state.json` and an `events.jsonl`
behind. Check the right directory, though — that is `<workspace_root>/.shipper/`,
where `workspace_root` comes from `--manifest-path` (default `Cargo.toml`)
and **not** from the current working directory, unless `--state-dir` was
given an absolute path. A CI step that looks for `./.shipper/` after
running Shipper from a different directory will mis-classify the exit code.

**Multi-registry runs write elsewhere again.** When more than one registry
is targeted, each run gets its own subdirectory — `<state_dir>/<registry-name>/`
— so the root `state_dir` holds no `state.json` at all. A CI step checking
only the root would read "no state file" and mis-classify a partial failure
as a usage error, which is the opposite of the truth. Inspect
`<state_dir>/<registry-name>/` for every targeted registry.

If your pipeline needs the codes to be unambiguous, validate flags in a
separate step, or pass an explicit absolute `--state-dir` and test for that
path.

## See also

- [Tutorial: First publish](../tutorials/first-publish.md)
- [How-to: Run in GitHub Actions](../how-to/run-in-github-actions.md)
- [`.shipper.toml` reference](../configuration.md)
- [Failure modes](../failure-modes.md)
