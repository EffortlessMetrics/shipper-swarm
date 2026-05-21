# Rails framework

`.rails/` is the durable Rails knowledge base for this repository.

## Ownership boundaries

Rails owns the durable framework artifacts under `.rails/`:

- proposals (`.rails/proposals/`)
- specs (`.rails/specs/`)
- ADRs (`.rails/adr/`)
- lane trackers (`.rails/lanes/`)
- templates (`.rails/templates/`)
- closeouts (`.rails/closeouts/`)
- support maps (`.rails/support/`)
- policy references (`.rails/policy/`)
- receipts (`.rails/receipts/`)
- schemas (`.rails/schemas/`)

`docs/` explains Rails to humans.

Rails does not own external awareness-only namespaces:

- `.codex/` (Codex execution state)
- `.spec/` (Spec Kit / speckit state)
- `.claude/` and `.jules/` (external agent/session state)

## Source of truth

Artifacts are indexed in `.rails/index.toml`.

Hard rule: no Rails-owned artifact path may live under `.codex/`, `.spec/`, `.claude/`, or `.jules/`.
