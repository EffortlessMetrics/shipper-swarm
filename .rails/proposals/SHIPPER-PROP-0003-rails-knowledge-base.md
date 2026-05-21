# SHIPPER-PROP-0003: Rails durable knowledge base

Status: accepted
Owner: docs
Created: 2026-05-21
Target milestone: docs foundation
Linked specs: SHIPPER-SPEC-0009
Linked ADRs: SHIPPER-ADR-0003
Linked lanes: rails-adoption

## Problem

Durable planning and contract artifacts are distributed across docs areas and can be conflated with agent/tool-specific execution state.

## Users and surfaces

Release operators, maintainers, and contributors who need a stable artifact graph for proposals, specs, ADRs, lane sequencing, support claims, and closeouts.

## Success criteria

A portable `.rails/` framework exists with explicit ownership boundaries, index-linked artifacts, and lane-focused sequencing.

## Proposed shape

Adopt `.rails/` as the long-term repo knowledge base and keep `.codex/`, `.spec/`, `.claude/`, and `.jules/` as awareness-only external namespaces.

## Alternatives considered

- Keep repo-specific naming like `.<repo>-spec/` (rejected: reduces portability and brand consistency).
- Place durable artifacts in agent directories (rejected: ownership confusion and lifecycle mismatch).

## Specs to create or update

- SHIPPER-SPEC-0009

## Architecture decisions needed

- SHIPPER-ADR-0003

## Implementation campaign shape

1. Add framework footprint and docs.
2. Add templates and first artifact graph.
3. Add lane tracker and implementation plan.

## Evidence plan

- `git diff --check`

## Risks

Potential duplication with existing docs unless links and ownership are clear.

## Non-goals

Migrating external agent/tool state or rewriting existing spec-kit assets.

## Exit criteria

`.rails/` exists with index-linked core artifacts and contributor guidance.
