---
name: finish-pr
description: Converge an implementation-complete pull request through challenge, substantive review, live CI verification, repair/re-review, and merge reconciliation. Use when asked to finish, land, carry through, get ready, or merge a PR.
---

# Finish pull request

## Trigger point

Use this skill when the candidate implementation and focused local proof are assembled, or whenever the user asks to finish, land, carry through, prepare, or merge a PR. This is the normal convergence entry point; it is not a synonym for “wait for CI.”

## Route

Resolve the current PR and exact live identities, then follow:

```text
candidate assembled
→ final-challenge
→ no useful current substantive review? review-pr
→ REVIEW_CURRENT
→ verify-live-ci
→ INTEGRATION_READY
→ merge-reconcile
```

Green CI, `mergeable: true`, zero unresolved threads, a bot summary, an approval, or the author saying the PR was reviewed cannot bypass `final-challenge` and the provider-native `review-pr` skill.

## Procedure

1. Reload repository, PR, head, base, merge-base, controlling authority, candidate diff, current checks, review threads, and draft/merge state.
2. Ensure the PR body accurately states the cumulative claim, non-goals, semantic owners, risk, proof, and limitations.
3. Invoke the provider-native `final-challenge` skill.
4. Determine whether a substantive review is current for the exact effective subject. If absent, stale, unavailable, shallow, or invalidated, invoke the provider-native `review-pr` skill.
5. If the result is `CHANGES_REQUIRED`, give one writer the consolidated repair packet. After repair, rerun affected proof, invoke `final-challenge` for changed semantic subjects, and invoke `review-pr` for affected findings and dimensions.
6. Proceed only on `REVIEW_CURRENT`. Invoke `verify-live-ci`; do not collapse pending integration into review judgment.
7. Proceed to `merge-reconcile` only on `INTEGRATION_READY` and only when merge was authorized by the request or standing repository workflow.

Stop and report the exact blocker for `NOT_PROVEN`, `BLOCKED_BY_PREREQUISITE`, `SUPERSEDED_OR_CLOSE`, `PR_IN_FLIGHT`, or `MERGE_BLOCKED`. Do not create no-op commits merely to manufacture evidence.

## Stacks and campaigns

Each PR receives its own challenge, substantive review, and integration posture. A campaign summary may be produced only after those child results exist. Respect dependency order; never let a parent or fan-in outrun an unreviewed child.

## Authority boundary

Normal `shipper-swarm` development PRs squash-merge. History-preserving swarm/source synchronization follows its separate merge-commit contract. This skill never authorizes tags, crates.io publication, GitHub Release mutation, signing, deployment, credential movement, or release-authority changes.
