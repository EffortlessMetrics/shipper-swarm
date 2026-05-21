# .shipper-spec

This namespace is the durable, repo-owned source of truth for spec rails in `shipper-swarm`.

## Ownership boundary

`./shipper-spec` owns long-lived artifacts for:

- roadmap slices
- proposals (PRD-style problem framing)
- behavior specs
- ADRs
- lane trackers and implementation plans
- support claim maps and policy references
- closeouts

This namespace does **not** own agent or tool execution state.

## External namespaces (awareness-only)

The repository may include:

- `.codex/`
- `.spec/`
- `.claude/`
- `.jules/`

Those directories are external/tool-specific state. They are not part of the durable spec rails managed here.
