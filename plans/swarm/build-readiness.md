# Build Readiness and Next-Work Handoff

Status: accepted
Owner: EffortlessMetrics
Created: 2026-09-08
Milestone: 0.5 preparation and post-release development
Linked proposal: docs/proposals/SHIPPER-PROP-0001-source-of-truth-and-release-evidence.md
Linked specs: docs/specs/SHIPPER-SPEC-0001-source-of-truth-stack.md
Linked ADRs: docs/adr/SHIPPER-ADR-0001-claims-become-checkable-state.md
Linked plan: plans/swarm/development-control-plane.md
Linked issues: #374; #251; #255; #262
Linked PRs:
Support-tier impact: no claim promotion; distinguish source, candidate, and public evidence
Policy impact: exact non-Rust receipt for this plan; no authority expansion
Proof commands: cargo xtask check-doc-contracts --mode advisory; cargo xtask check-file-policy --mode blocking-allowlist; cargo xtask policy-report; cargo fmt --all -- --check; git diff --check

## Objective and authority

Give the next builder a bounded issue, its governing contract, a meaningful
proof path, and an explicit place in the release sequence. Shipper's product
goal remains [safe-to-start and safe-to-rerun workspace publishing](../../MISSION.md),
with legible outcomes and recoverable durable evidence.

This plan is an audit and work-selection handoff. It does not replace the
[roadmap](../../ROADMAP.md), product specs,
[support tiers](../../docs/status/SUPPORT_TIERS.md), or active release owners:

- [Swarm #251](https://github.com/EffortlessMetrics/shipper-swarm/issues/251)
  owns the release program;
- [swarm #255](https://github.com/EffortlessMetrics/shipper-swarm/issues/255)
  owns queue disposition, exact candidate proof, and freeze;
- [swarm #262](https://github.com/EffortlessMetrics/shipper-swarm/issues/262)
  owns the deliberately deferred post-0.5 program.

The active manifest retains queue stewardship. Its planned reference to this
handoff does not activate deferred implementation or authorize a release.

## Audit boundary and refresh

The read-only audit inspected merged swarm source
`6463971cb32c945d9946ea4e09e91b511e5c61da` and the live boards in both
repositories on 2026-09-08 UTC. This was an unfrozen source snapshot, not a
candidate approval. The latest GitHub Release observed in the release authority
was `v0.4.0`. New source work and historical successful tests do not establish a
public 0.5 release.

Before claiming a slice, reread its issue body and latest human comments, open
PRs, branch/head/base, worktree ownership, review findings, and current checks.
Existing active writers retain their branches. After any head/base movement,
refresh the effective diff and affected proof. The table below is a dated
disposition, not a second live board.

```bash
gh issue view 255 --repo EffortlessMetrics/shipper-swarm --comments
gh pr list --repo EffortlessMetrics/shipper-swarm --state open
gh issue list --repo EffortlessMetrics/shipper-swarm --state open
gh pr list --repo EffortlessMetrics/shipper --state open
git status --short --branch
git worktree list --porcelain
```

## Preserve the current release lane

| Priority / owner | Audit finding and next bounded action | Proof and disposition boundary |
| --- | --- | --- |
| Before freeze: [#362](https://github.com/EffortlessMetrics/shipper-swarm/issues/362) / [PR #367](https://github.com/EffortlessMetrics/shipper-swarm/pull/367) | The documented `retry.per_error` contract needs execution, validation, cumulative resume ceilings, and event/rebuild consistency. Continue through its existing writer and substantive review. | Require behavior regressions, exact-head CI, and affected re-review; configuration parsing alone does not prove execution. |
| Before freeze: [#370](https://github.com/EffortlessMetrics/shipper-swarm/issues/370) / [PR #373](https://github.com/EffortlessMetrics/shipper-swarm/pull/373) | Broad-main jobs exhausted runner allocation without executing. The active PR owns an explicit hosted full-proof route. | Retain completed commands and artifacts on the relevant SHA. A dispatch, queued job, or successful required small gate does not establish broad-main proof. |
| Disposition before freeze: [#371](https://github.com/EffortlessMetrics/shipper-swarm/issues/371) | Classify the seven reported mutation survivors and add an observable oracle only where a surviving mutation changes required behavior. | Record equivalent/unobservable versus actionable findings individually. Do not claim seven defects or require impossible kills for equivalent mutants. #255 owns release disposition. |
| Revalidate after merge: [PR #372](https://github.com/EffortlessMetrics/shipper-swarm/pull/372) / [#369](https://github.com/EffortlessMetrics/shipper-swarm/issues/369) | The fuzz invocation repair is merged at the audited source SHA. | Merged toolchain/target correction is source evidence; successful instrumented campaign execution requires its own run evidence. |

Newly found correctness or operator-safety gaps must receive an explicit #255
disposition before freeze. Issue creation alone neither blocks nor waives a
release. A confirmed prerequisite should be fixed as a separate coherent PR;
unrelated improvements remain in the post-release queue.

After the last accepted candidate change, #255 requires a clean merged SHA/tree,
the complete candidate gate, package and compatibility proof, the cold operator
walkthrough, workflow checks, and exact-head hosted evidence. Its freeze record
lives outside the source tree so recording the SHA does not create a new SHA.
Follow the [release checklist](../../docs/release/release-preparation-checklist.md)
for commands and evidence fields instead of copying another full gate here.

## Release-authority chain

| Order | Owner and output |
| --- | --- |
| 1 | [swarm #255](https://github.com/EffortlessMetrics/shipper-swarm/issues/255): frozen merged swarm SHA/tree and candidate evidence; publication remains NO. |
| 2 | [shipper #475](https://github.com/EffortlessMetrics/shipper/issues/475): history-preserving promotion and tree equality. |
| 3 | [shipper #478](https://github.com/EffortlessMetrics/shipper/issues/478): intended tag date and approved release SHA/tree handoff. |
| 4 | [shipper #476](https://github.com/EffortlessMetrics/shipper/issues/476): exact-source rehearsal, binaries, compatibility, recovery, observed auth posture, and explicit GO/NO-GO. |
| 5 | [shipper #477](https://github.com/EffortlessMetrics/shipper/issues/477): only after explicit authorization, tag/publish/resume and verify the exact public artifacts. |
| 6 | [shipper #479](https://github.com/EffortlessMetrics/shipper/issues/479): public factual closeout, final source identity, history backfill, and reopening of normal swarm development. |

The [preparation control record](../../docs/release/0.5.0-preparation.md) remains
NOT FROZEN / publication NO. Its formerly open documentation and workflow-policy
prerequisites are complete. The
[August readiness evidence](../../docs/release/0.5.0-readiness.md) is historical;
the four binary artifacts from run `30867482676` expired on 2026-08-11 and cannot
serve as retained artifacts for a new candidate.

[Auth proof #105](https://github.com/EffortlessMetrics/shipper/issues/105) already
owns Trusted Publishing default evidence and the provenance decision. A token
mint, OIDC configuration, or local mock test cannot promote that claim. #476 may
record an explicitly approved fallback posture; lack of an unchosen signing or
SBOM format does not independently block 0.5.

## Confirmed gaps requiring bounded issue work

| Issue and observed seam | Acceptance and cheapest sufficient proof |
| --- | --- |
| [#375: generated Actions recipe](https://github.com/EffortlessMetrics/shipper-swarm/issues/375) — `run_ci` in `crates/shipper-cli/src/lib.rs` emits steps while the guide directs users to save a complete workflow; artifact actions, paths, and broad state restoration are stale. Child of #277; #259 can reuse its fixture. | Preserve snippet shape, document a valid wrapper, use portable paths, retain hidden evidence even on failure, and make recovery deliberate. Parse the actual generated YAML; lint the assembled wrapper; prove invocation/evidence behavior with fake Cargo and a mock registry. |
| [#376: documented OIDC fallback](https://github.com/EffortlessMetrics/shipper-swarm/issues/376) — `docs/how-to/run-in-github-actions.md` promises fallback after a mint step whose failure prevents publish from running. Related auth authority remains `shipper#105`. | Test minted-token success, deliberate fallback, no-token rejection, and absence of fallback with a successful mint against the actual example. Keep token values redacted and local branch proof separate from remote auth/registration proof. |
| [#377: checkpoint/file-sync contract](https://github.com/EffortlessMetrics/shipper-swarm/issues/377) — event flush and ignored projection file-sync results do not define one clear failure boundary across `state/events`, `state/execution_state`, and `engine/transition`. | Define process/runner/host-failure guarantees first; inject sync failure and prove the chosen error and event-before-projection ordering. Preserve corrupt-log rejection. This proves deterministic failure handling, not arbitrary hardware or power-loss durability. |
| [#378: proof-command accuracy](https://github.com/EffortlessMetrics/shipper-swarm/issues/378) — plain `cargo test` is mislabeled workspace proof; testing docs overstate platforms, triggers, and property intensity. | Compare Cargo default members, CI source, and nextest values; correct prose/comments without changing execution. Metadata and doc/file-policy checks suffice. Coordinate with #373 before editing CI documentation. |

#255 must explicitly classify all four as candidate prerequisites or bounded
follow-ups. The issues contain source links, accept/reject cases, commands,
non-goals, and rollback. This audit ran no new product behavior proof. Keep each
implementation in one review-forward PR; do not silently activate the deferred
#277/#259 parents or treat issue creation as release approval.

## Preserve the post-release sequence

The #255 disposition explicitly defers #258–#262 and #277. Select their next
slice only after the release authority reopens normal swarm development, unless
#255 explicitly adopts a narrow prerequisite:

| Existing owner | First bounded implementation decision | Acceptance / proof boundary |
| --- | --- | --- |
| [#259](https://github.com/EffortlessMetrics/shipper-swarm/issues/259) | Extend the release-blocking #272/#276 operator proof into one reproducible first-use-to-cross-process-recovery journey. | Use the just-built/installed CLI with a mock registry and controlled Cargo; prove human/JSON outcomes, exit codes, restart, no duplicate upload, and no secret-sentinel leak. |
| [#277](https://github.com/EffortlessMetrics/shipper-swarm/issues/277) | Select one secondary-command help, hint, reporter, or quiet-mode inconsistency. | Spawn the real CLI and assert the operator-visible behavior; avoid a broad output rewrite. |
| [#258](https://github.com/EffortlessMetrics/shipper-swarm/issues/258) | Define the smallest machine-readable extension beyond the approved-identity gate already landed in PR #263. | Bind source/tree/workflow/artifact identities and reject missing, mismatched, stale, or unapproved inputs; preserve the existing release authority. |
| [#260](https://github.com/EffortlessMetrics/shipper-swarm/issues/260) | Choose one expensive read-only proof whose reuse conditions can be proven. | Mutation-style fixtures must invalidate reuse on relevant tree/diff/config changes. Never reuse required candidate-head Rust or release-authority rehearsal. |
| [#261](https://github.com/EffortlessMetrics/shipper-swarm/issues/261) | Continue contract-dependency inversion from the current merged retry ownership; select one remaining dependency edge. | Prove both sides of the boundary, package surface, and compatibility without adding a public crate. |

Existing source-repository work remains linked rather than duplicated:
[embedding/notification #107](https://github.com/EffortlessMetrics/shipper/issues/107),
[direct core proof #426](https://github.com/EffortlessMetrics/shipper/issues/426),
[fuzz cadence #425](https://github.com/EffortlessMetrics/shipper/issues/425), and
[source-truth drift #430](https://github.com/EffortlessMetrics/shipper/issues/430).
Implement routine changes in swarm. Recheck each issue against current code;
historical percentages or unchecked boxes are not evidence of missing behavior.

## Handoff acceptance and proof

- Every selected seam has one live owner, source links, acceptance criteria,
  an observable regression or characterization, and an explicit release
  disposition. Existing overlapping PRs are reviewed or handed back first.
- Contributor navigation reaches the active development board and this plan;
  release-authority operations continue to point to their original owners.
- Preparation documents distinguish completed prerequisites, current proof
  gaps, historical artifacts, and public results. Support tiers are unchanged.
- The new plan has an exact non-Rust policy receipt. The plan and active
  manifest reference existing source-truth artifacts.
- The documentation PR passes the commands in this header, substantive review,
  the required current-head gate, and post-merge reconciliation. Audit source
  inspection is not a claim that Rust, release, or user-journey tests ran.

## Non-goals, rollback, and cleanup

This handoff does not implement product features, alter workflows, create a
second release umbrella, change support tiers, select a frozen candidate, or
authorize publication, tags, signing, deployment, or credentials. Deferred
architecture and ergonomics work remains deferred.

If an audit conclusion is disproved, correct the affected issue and plan claim
through review; preserve historical receipts and do not rewrite them as new
proof. The documentation-only change can be reverted as one PR without changing
runtime behavior.

For every later slice, use a dedicated branch/worktree when needed, one writer,
and named review/proof ownership. Before deleting lane-created worktrees or
branches, verify the resolved path, lock/status, untracked files, HEAD/upstream,
unique commits, and terminal PR state. Preserve ambiguous or user-owned work.
After merge, reconcile main proof, update the issue/plan as needed, remove only
verified lane-owned scratch/build artifacts, and record remaining work in the
owning issue. A silent agent or green CI is not a cleanup authorization.
