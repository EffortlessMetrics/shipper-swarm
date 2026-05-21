# Spec style

The durable source-of-truth stack is separated by artifact role:

- proposal (why)
- spec (what)
- ADR (decision)
- implementation plan/tracker (how)
- proof and closeout (what happened)

Durable rails live in `.shipper-spec/`.

## External agent state

This repository may contain `.codex/`, `.claude/`, `.jules/`, or similar tool-specific directories.

Those directories are not the durable source of truth for this spec system.
Agents may read `.shipper-spec/` to decide what to do, but this system does not manage agent scratch state.

## Spec Kit coexistence

If `.spec/` exists, it is reserved for Spec Kit / speckit workflows.

The repo-native long-term spec rails live in `.shipper-spec/`.
This lane does not migrate, rewrite, validate, or depend on `.spec/`.
