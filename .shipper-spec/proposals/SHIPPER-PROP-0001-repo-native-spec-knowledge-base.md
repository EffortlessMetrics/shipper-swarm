# SHIPPER-PROP-0001: Repo-native spec knowledge base

Status: proposed
Owner: repo-architecture
Created: 2026-05-21
Target milestone: Spec rails bootstrap
Linked specs: SHIPPER-SPEC-0001
Linked ADRs: SHIPPER-ADR-0001
Linked lanes: spec-system

## Problem

Durable planning and specification context can drift into agent-owned namespaces, which weakens long-term maintainability and tool neutrality.

## Users and surfaces

Contributors, maintainers, and automation that need durable linkage between proposals, specs, ADRs, trackers, and closeouts.

## Success criteria

A complete, durable spec namespace exists under `.shipper-spec/`, with contributor docs that explain boundaries and ownership.

## Proposed shape

Adopt `.shipper-spec/` as the durable control plane and treat `.codex/`, `.spec/`, `.claude/`, and `.jules/` as awareness-only.
