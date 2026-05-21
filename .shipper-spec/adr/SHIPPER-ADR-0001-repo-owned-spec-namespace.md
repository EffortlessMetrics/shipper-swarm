# SHIPPER-ADR-0001: Repo-owned spec namespace

Status: accepted
Date: 2026-05-21
Owner: repo-architecture
Linked proposal: SHIPPER-PROP-0001
Linked specs: SHIPPER-SPEC-0001

## Decision

Durable source-of-truth rails for proposal/spec/ADR/lane/closeout artifacts live in `.shipper-spec/`.

## Context

Agent and tool directories are useful execution surfaces but are not appropriate as long-term repository knowledge ownership boundaries.

## Consequences

The repo retains tool-neutral, auditable long-lived planning and specification memory.
