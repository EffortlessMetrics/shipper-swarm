# SHIPPER-ADR-0003: Rails framework footprint

Status: accepted
Date: 2026-05-21
Owner: docs
Linked proposal: SHIPPER-PROP-0003
Linked specs: SHIPPER-SPEC-0009

## Decision

Long-term proposal/spec/ADR/lane/closeout Rails artifacts live under `.rails/`. Agent and tool specific state remains external and awareness-only.

## Context

The repository needs a portable, branded, durable framework footprint that is consistent across adopting repositories and separate from session-state namespaces.

## Consequences

- Durable artifact ownership is explicit.
- External namespaces are referenced but not owned by Rails.
- Future validators and portals can consume a single indexed graph.

## Alternatives considered

- Repo-specific framework directory names.
- Durable artifacts in agent-specific directories.

## Follow-up specs / plans

- SHIPPER-SPEC-0009
- `.rails/lanes/rails-adoption/tracker.toml`
