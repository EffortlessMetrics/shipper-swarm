# Historical Handoff

Status: historical

This file used to contain a one-time February 2026 handoff for the pre-0.4
Shipper workspace. It described the old `next-pr` branch, Rust 1.92,
`v0.2.0` release artifacts, and the pre-split crate layout. Those details are
not authoritative for current work and should not guide new agents.

Use the current source-of-truth surfaces instead:

- [AGENTS.md](../AGENTS.md) - active repository role, architecture, and agent
  operating rules.
- [ROADMAP.md](../ROADMAP.md) - product sequencing for Shipper as release
  closure infrastructure.
- [docs/status/SUPPORT_TIERS.md](status/SUPPORT_TIERS.md) - claim-to-proof map.
- [.shipper-meta/goals/active.toml](../.shipper-meta/goals/active.toml) -
  current active execution goal.
- [docs/status/SWARM_OPERATION.md](status/SWARM_OPERATION.md) - active
  development repo and source/sync policy.
- [docs/release/0.4.0-readiness.md](release/0.4.0-readiness.md) - published
  0.4.0 release evidence.

If a future handoff is needed, write it as a dated closeout or readiness
artifact that points back to these source-of-truth documents instead of
replacing them.
