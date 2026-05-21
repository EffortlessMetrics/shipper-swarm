# Spec style and ownership model

This repository separates durable product knowledge from tool-specific execution state.

## Durable rails

Durable specification rails live in `.shipper-spec/`.

Use `.shipper-spec/` for:

- roadmap direction and milestones
- proposals (why, user value, alternatives, success criteria)
- behavior specs and evidence requirements
- architecture decisions (ADRs)
- lane trackers and implementation plans
- support-tier claim mapping and policy references
- closeouts that record what landed and what remains

## Human-facing docs

`docs/` remains the human-facing explanation and contributor guidance surface.

## Policy ledgers

Live enforcement ledgers remain in `policy/*.toml`.
The `.shipper-spec/` namespace may reference ledgers, but should not duplicate enforcement logic.

## External/tool-specific state

This repository may include `.codex/`, `.spec/`, `.claude/`, `.jules/`, or similar directories.

Those directories are not the durable source of truth for this spec system. They are external tool/session state.
