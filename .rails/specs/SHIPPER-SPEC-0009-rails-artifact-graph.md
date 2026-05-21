# SHIPPER-SPEC-0009: Rails artifact graph contract

Status: accepted
Owner: docs
Created: 2026-05-21
Linked proposal: SHIPPER-PROP-0003
Linked ADRs: SHIPPER-ADR-0003
Linked lane: rails-adoption
Linked issues:
Linked PRs:
Support-tier impact: documentation
Policy impact: none

## Problem

Without a single indexed artifact graph, durable artifacts are harder to discover and validate consistently.

## Behavior

- Rails artifacts are indexed in `.rails/index.toml`.
- Rails-owned artifact paths must live under `.rails/`.
- External namespaces may be declared for awareness only and cannot own Rails artifacts.
- Specs define behavior and evidence, not PR sequence.
- Lane trackers define focused implementation sequencing.

## Non-goals

- Managing `.codex/`, `.spec/`, `.claude/`, or `.jules/` contents.

## Required evidence

- Repository diff passes `git diff --check`.
- Indexed paths exist on disk.

## Acceptance examples

- Proposal, spec, ADR, and lane entries exist and resolve through `.rails/index.toml`.

## Test mapping

- `git diff --check`

## Implementation mapping

- `.rails/index.toml`
- `.rails/proposals/`
- `.rails/specs/`
- `.rails/adr/`
- `.rails/lanes/`

## CI proof

- `git diff --check`

## Metrics / promotion rule

Promote once a dedicated validator is added and run in CI.

## Failure modes

- Artifact IDs collide.
- Indexed paths are missing.
- Owned artifacts point outside `.rails/`.
