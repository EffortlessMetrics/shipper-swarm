# Shipper Roadmap

> See [MISSION.md](MISSION.md) for the mission, vision, audience, and beliefs that produce the priorities below.

## Where we are

**v0.4.0 shipped 2026-05-20.** Thirteen public crates are live on crates.io, and `cargo install shipper --locked` is the stable install path. The 0.4.0 release line made Shipper an idempotent, evidence-backed workspace publisher: it plans missing `name@version` pairs, preflights proof and pacing, reconciles ambiguous Cargo outcomes against registry truth, resumes from durable state, and records release evidence.

The post-v0.3.0 retrospective organized the product thesis around nine competencies. Issues #100–#109 are the historical implementation ledger for that model; completed competencies are no longer open planning queues. This document is the current roadmap.

## 0.5.0 release-line status

The 0.5.0 line is the capstone for the current execution and security work. `shipper-swarm` is the development and proof surface; the separate `EffortlessMetrics/shipper` repository remains the release authority for tags, crates.io publication, GitHub Release creation, and final release evidence.

The stabilized boundaries are one per-package execution authority shared by both schedulers, event-first transitions with rebuild evidence, one readiness polling kernel, validated registry destinations and redirects, context-aware authorization redaction, versioned KDF compatibility, and blank-value-safe OIDC diagnostics. A green swarm branch is not itself a release: promotion still requires one exact candidate SHA and release-authority evidence. Release credentials must not move into this repository.

The active release sequence is explicit:

1. freeze and review one swarm candidate;
2. promote that history to the source-authority repository;
3. rehearse the exact promoted SHA and record GO or NO-GO;
4. publish only after explicit authorization;
5. verify public artifacts and backfill the final authority history into swarm.

## Five existential pillars (the safety claim)

Cargo stabilized multi-package workspace publishing. “Publish several crates at once” is not enough differentiation. Shipper is worth existing only if it owns five guarantees Cargo does not give the operator together:

| Pillar | Question it answers | Current state |
|---|---|---|
| **Prove** | Can I show this release is safe *before* the irreversible step? | Implemented base — deterministic plan, structured preflight, package dry-run, ownership evidence where applicable, registry pacing, policy reports, rehearsal, and versioned JSON evidence. Exact release-candidate proof is repeated for every release. |
| **Dispatch** | Is publication executed in a registry-aware, paced way? | Implemented for the evidence-backed crates.io profile — authoritative publish regimes, first-publish burst/refill behavior, `Retry-After` floors, readiness checks, and duration estimates. Additional registry profiles require their own evidence. |
| **Reconcile** | When the result is ambiguous, do I check registry truth before retrying? | Implemented — ambiguous exits resolve to Published, NotPublished, or StillUnknown before any safe retry. StillUnknown stops rather than guessing. |
| **Recover** | If the runner dies mid-train, can I converge from durable state without losing or duplicating work? | Stable/internal proof — events are authoritative, state can be rebuilt, terminal packages are skipped on resume, and a two-job interruption artifact handoff is proven with fake Cargo and a mock registry. The release candidate repeats this proof on its exact tree. |
| **Remediate** | If a partial release goes bad, can I contain or fix-forward it mechanically? | Bounded implementation — receipt-driven yank/fix-forward planning, dry-run artifacts, guarded yank execution, and compromise evidence exist. Live registry mutation remains an explicit operator decision. |

The engine therefore has the release-closure shape. The remaining work is not to reimplement these pillars under stale “partial” labels; it is to preserve their evidence while finishing the 0.5 source-authority release, close direct coverage gaps, and prove the actual auth and integration contracts.

## Nine competencies (the implementation ledger)

The five pillars cover the safety story. Narrate, Harden, Integrate, and Ergonomics make the tool legible, securable, embeddable, and approachable.

| # | Competency | Definition | Current state | Issue |
|---|---|---|---|---|
| 1 | **Prove** | Establish before the irreversible step that publication can succeed | Implemented base; repeated per release candidate | [#100](https://github.com/EffortlessMetrics/shipper/issues/100) |
| 2 | **Survive** | Recover from interruption without losing or duplicating work | Implemented | [#101](https://github.com/EffortlessMetrics/shipper/issues/101) |
| 3 | **Reconcile** | Close ambiguous outcomes against registry truth | Implemented | [#102](https://github.com/EffortlessMetrics/shipper/issues/102) |
| 4 | **Narrate** | Tell the operator what is happening live, not only afterward | Implemented | [#103](https://github.com/EffortlessMetrics/shipper/issues/103) |
| 5 | **Remediate** | Mechanically contain or fix-forward bad partial outcomes | Bounded implementation | [#104](https://github.com/EffortlessMetrics/shipper/issues/104) |
| 6 | **Harden** | Default to the proved auth posture and bound supply-chain evidence | Active — exact-source Trusted Publishing proof and provenance decision remain | [#105](https://github.com/EffortlessMetrics/shipper/issues/105) |
| 7 | **Profile** | Encode evidence-backed registry constraints and regimes | Implemented for crates.io | [#106](https://github.com/EffortlessMetrics/shipper/issues/106) |
| 8 | **Integrate** | Expose reliable machine and embedding boundaries | Active — observable notification delivery and one compile-tested embedding proof remain | [#107](https://github.com/EffortlessMetrics/shipper/issues/107) |
| 9 | **Ergonomics** | Keep first-run, diagnosis, and recovery friction low | Implemented base | [#108](https://github.com/EffortlessMetrics/shipper/issues/108) |

The historical master scorecard is [#109](https://github.com/EffortlessMetrics/shipper/issues/109). It is superseded by this roadmap and the current release program; it should not be used to infer present implementation status.

The important unresolved distinctions are narrow:

- Trusted Publishing is supported, but 0.4.0 selected `fallback_secret`; the default claim waits for exact-source rehearsal evidence.
- Webhooks exist and preserve sequential/parallel event parity, but delivery is still best-effort and not durably observable.
- The crates.io profile is implemented; another registry does not get a profile until its pacing and propagation semantics are evidenced.
- A completed competency does not waive release-candidate proof. Every irreversible release still re-establishes source identity, package surface, compatibility, auth, recovery, and public-result evidence.

## Design principles

### Reliability over speed

Default behaviors verify, log, and provide evidence. Faster paths are explicit opt-ins. The default publish policy (`safe`) includes the required verification.

### Determinism

Publish order is reproducible. Plan IDs are SHA-256 identities of the workspace plan and stable across equivalent environments. The same release inputs must produce the same plan identity.

### Events are truth; state is a projection

Per [docs/INVARIANTS.md](docs/INVARIANTS.md), `events.jsonl` is authoritative and append-only. `state.json` is a projection for resume convenience. `receipt.json` is a summary derived at end of run. Event/state/receipt drift is a product defect, not an acceptable reporting mismatch.

### Engine is library; CLI is an adapter

Release behavior lives in `crates/shipper-core` and its contract/adapter crates. `crates/shipper-cli` parses arguments and renders output. `crates/shipper` is the install facade and curated product-name re-export. Other frontends consume supported public boundaries rather than importing CLI internals.

### Evidence controls release authority

A green branch, merged PR, configuration value, or successful token mint is not publication authorization. The release issue must name one approved SHA/tree and one observed auth path. Public claims follow retained evidence, not intended configuration.

### Forbid unsafe; respect MSRV

`unsafe_code = "forbid"` is workspace-wide. Edition 2024 and MSRV 1.95 are policy boundaries, with repository checks preventing silent regression.

## Now / Next / Later

### Now — close the 0.5 line

1. **Release candidate and authority chain** — finish the swarm candidate, promote it without rewriting history, run the full exact-source gate, record GO/NO-GO, and publish only through [#475–#479](https://github.com/EffortlessMetrics/shipper/issues/475).
2. **[#105 Harden](https://github.com/EffortlessMetrics/shipper/issues/105)** — prove which auth source the release actually selects; keep fallback explicit until the short-lived path is proven across the full train; define provenance from a threat model and consumer rather than a tool checklist.
3. **[#107 Integrate](https://github.com/EffortlessMetrics/shipper/issues/107)** — choose an observable webhook-delivery contract and add one compile-tested embedding consumer using supported public APIs.
4. **Coverage and evidence maintenance** — compile every fuzz target, keep scheduled campaign failures honest, add direct tests at high-value policy seams, and repair source-of-truth drift without relabeling advisory evidence as release proof.

### Next — extend only from evidence

1. Add a second registry profile only after its documented and observed pacing, propagation, ambiguity, and auth semantics can be encoded and tested.
2. Exercise bounded remediation against a deliberate non-production registry or equally strong harness before promoting live-registry automation.
3. Tighten document-contract checks after the current status/link drift is repaired; do not make a known-broken projection blocking first.
4. Improve notification replay or external adapters only after the chosen webhook/embedding contract is stable.

### Later — concrete consumers, not generic integration theatre

- IDP adapters for Backstage, Port, Cortex, or another platform when an actual consumer supplies the required workflow and compatibility surface.
- HTTP or service APIs only when process embedding and versioned CLI/event contracts are insufficient for a real deployment.
- Additional provenance formats only when an independent verifier consumes them.
- Performance work after correctness evidence identifies a material operator cost.

## Explicit non-goals

The Shipper product does not own these concerns merely because the repository release workflow may orchestrate them around a release:

| Concern | Primary owner |
|---|---|
| Version bumping | release-plz, cargo-release, or an explicit repository process |
| Changelog generation | Changie/release tooling |
| Git tag creation | release-authority workflow or operator |
| GitHub Release publication | release-authority workflow / GitHub tooling |
| crates.io team management | `cargo owner` |
| Dependency updates | Cargo/Dependabot and reviewed maintenance PRs |

**Shipper focuses on safe workspace publication and evidence-backed convergence.** Repository automation may call adjacent tools, but those calls do not become engine responsibilities.

## Contributing

Work from the current open issue and PR queues, not the historical #100–#109 scorecard alone.

Prioritize changes by:

1. fit with the release-closure model;
2. whether they close a real proof, operator-confidence, or maintainability gap;
3. whether the claim can be challenged by tests or retained evidence;
4. maintenance burden relative to value.

A good PR is bounded, names the governing contract, carries direct tests or a reason they are inapplicable, updates the relevant policy/document projection, and does not claim publication authority.

## Version history

| Version | Date | Theme |
|---|---|---|
| v0.4.0 | 2026-05-20 | Stable release-closure line; idempotent workspace publish, JSON evidence envelopes, registry-truth reconciliation, resume proof, auth evidence, and bounded remediation surfaces |
| v0.3.0-rc.1 | 2026-04-16 | First crates.io publish; 12 crates live; deterministic plan, retry absorption, and evidence trail proven under real rate limits |
| v0.2.0 | 2026-02-14 | Evidence and verification: event log, receipts, readiness checks, publish policies |
| v0.1.0 | — | Initial release |
