# Layer: `ops::process` (process execution)

**Position:** Layer 1 (ops). Crate-private submodule of `shipper-core`.

## Single responsibility

Cross-platform command execution: spawn child processes, capture stdout/stderr,
apply optional timeouts, and shell out to `cargo`. Used by `ops::cargo` to
invoke `cargo publish`, `cargo package`, and friends with retry-friendly
result types.

## What lives here

- `types` — `CommandResult` and `CommandOutput` result types (serde-friendly).
- `run` — Basic command runners: `run_command`, `run_command_in_dir`,
  `run_command_with_env`, `run_command_streaming`, `run_command_simple`.
- `timeout` — `run_command_with_timeout` which polls the child and kills it
  if it exceeds the deadline.
- `capture` — full capture or opt-in publish diagnostic tails. Publish readers
  use 8 KiB chunks, a 64 KiB partial-line limit, a 256 KiB retained/rendered
  payload limit per stream, and at most 4096 entries. Never allocate from an
  unchecked caller line count. Normal last-N eviction is not loss; capacity/read
  loss is sticky and clears fragments. Frame CR/LF and UTF-8 boundaries before
  rendering. The Cargo adapter suppresses both streams on either loss.
- `which` — `command_exists`/`which` helpers delegating to the `which` crate.
- `cargo` — `run_cargo`, `run_cargo_in_dir`, `cargo_dry_run`, `cargo_publish`
  convenience wrappers.

## Import rules

`ops::process` is Layer 1:
- MUST NOT import from `crate::engine`, `crate::plan`, `crate::state`, `crate::runtime`.
- MAY import from `crate::types` and external crates.

## Visibility

Everything in this subsystem is `pub(crate)`. None of it is part of shipper-core's
public API — it's an internal implementation detail that callers (primarily
`ops::cargo`) consume via the `ops::process` facade.

`run_command_with_timeout` keeps complete stdout/stderr. Only the explicit
`run_command_with_publish_capture` entry point bounds retention, including when
timeout is None. Timed generic execution and all bounded publish execution share
the supervisor and owned reader joins; generic timeout=None keeps the existing
run_command_in_dir full-output route. Do not
implement bounded capture by reading everything and truncating afterward, and
do not route metadata or generic structured stdout through diagnostic tails.
Inherited descendant EOF/cleanup is not bounded by this retention policy.

## History

Absorbed from the standalone `shipper-process` microcrate (1948 LOC) in the
Phase 2 decrating effort. See `docs/decrating-plan.md` §6.
