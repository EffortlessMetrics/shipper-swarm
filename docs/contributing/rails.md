# Contributing: Rails artifacts

Use this guide when adding or updating the Rails framework artifacts.

## Durable footprint

Rails lives under `.rails/` and uses `.rails/index.toml` as the canonical artifact graph.

Every new proposal, spec, ADR, and lane tracker must be linked through `.rails/index.toml`.

## Directory ownership

- `.rails/` is the durable repo knowledge base.
- `docs/` is the human-facing explanation and adoption surface.
- `.codex/`, `.spec/`, `.claude/`, and `.jules/` are awareness-only external namespaces.

Do not migrate, rewrite, validate, or otherwise own external namespaces from a Rails lane.

## Authoring rules

1. Keep proposal/spec/ADR/lane detail in `.rails/` artifacts.
2. Use focused lane trackers under `.rails/lanes/`; do not create one giant active queue.
3. Keep owned artifact paths inside `.rails/`.
4. Record proof commands in lane work items and closeouts.
5. Update `.rails/index.toml` in the same change as new artifacts.
