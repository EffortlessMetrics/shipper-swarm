# SHIPPER-SPEC-0001: Repo-native spec rails

Status: draft
Owner: repo-architecture
Created: 2026-05-21
Linked proposal: SHIPPER-PROP-0001
Linked ADRs: SHIPPER-ADR-0001
Linked lane: spec-system
Support-tier impact: none
Policy impact: references-only

## Problem

The repository needs durable, repo-owned spec rails that are distinct from external agent/tool state.

## Behavior

- Durable artifacts for proposal/spec/ADR/lane/closeout live under `.shipper-spec/`.
- Contributor guidance for this model lives under `docs/`.
- External namespaces (`.codex`, `.spec`, `.claude`, `.jules`) are awareness-only and not owned artifacts.
