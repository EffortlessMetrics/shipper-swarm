# .shipper-spec

`.shipper-spec/` is the durable, repo-owned source-of-truth namespace for Shipper's specification rails.

It is used for long-term artifacts such as roadmap milestones, proposals, specs, ADRs, lane trackers, implementation plans, support claim maps, policy references, and closeouts.

## Ownership boundaries

This namespace owns durable spec artifacts.

It does **not** own tool/session state directories such as:

- `.codex/`
- `.spec/`
- `.claude/`
- `.jules/`

Those directories may exist and may read from `.shipper-spec/`, but they are awareness-only for this system.

## Chain of evidence

Shipper's durable spec chain is:

`roadmap -> proposal -> spec -> ADR -> lane tracker -> implementation plan -> PRs/issues -> proof -> support/policy updates -> closeout`

## Index

All durable artifacts tracked by this system must be listed in `.shipper-spec/index.toml`.
