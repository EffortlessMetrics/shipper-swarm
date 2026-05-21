# Contributing guide: spec rails

When adding or updating durable planning/specification artifacts, use `.shipper-spec/`.

## Rules

1. Keep durable proposal/spec/ADR/lane/closeout artifacts in `.shipper-spec/`.
2. Keep explanation and contributor-oriented guidance in `docs/`.
3. Keep live policy ledgers in `policy/*.toml`; reference them from `.shipper-spec/` as needed.
4. Do not store durable artifacts in `.codex/`, `.spec/`, `.claude/`, or `.jules/`.
5. Ensure durable artifacts are indexed in `.shipper-spec/index.toml`.

## Recommended chain

`roadmap -> proposal -> spec -> ADR (when needed) -> lane tracker -> implementation plan -> proof -> support/policy references -> closeout`

## Minimal coexistence language

If you mention agent/spec-tool directories in docs, keep wording minimal:

- they are external/tool-specific state,
- they may read durable artifacts,
- they are not owned or managed by this spec-rails system.
