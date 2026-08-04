# Shipper release operator runbook

Status: active

This runbook is the ordered procedure for preparing, promoting, rehearsing, publishing, and closing a Shipper release. Use it with a copied [release preparation checklist](release/release-preparation-checklist.md); the checklist is the durable evidence record, while this document explains the procedure and stop decisions.

Command blocks use a POSIX shell for compactness. On Windows, use equivalent PowerShell commands and record the same identities and results.

## Non-negotiable authority boundary

| Repository | Authority | Normal merge method |
| --- | --- | --- |
| `EffortlessMetrics/shipper-swarm` | development, Changie fragments, changelog preparation, candidate proof, source freeze | squash-merged PRs |
| `EffortlessMetrics/shipper` | release rehearsal, tags, crates.io publication, GitHub Release, signing, and credentials | merge commits |

The shared `.github/workflows/release.yml` file is intentionally inert in `shipper-swarm`. Jobs that can rehearse, publish, resume a train, build release artifacts, or create a GitHub Release are guarded by:

```text
github.repository == 'EffortlessMetrics/shipper'
```

Do not add publication credentials to swarm and do not treat a workflow dispatch in swarm as release proof.

## Release sequence

```text
fragment intake and changelog preparation in shipper-swarm
→ exact merged-main candidate proof
→ freeze swarm SHA and tree
→ non-fast-forward promotion into shipper
→ prove promotion tree identity
→ exact release-authority rehearsal and binary matrix
→ authorize and push tag from shipper
→ monitor/resume the topological publish train
→ verify crates.io and GitHub Release
→ backfill final shipper history into shipper-swarm
```

Any commit after the frozen candidate reopens the candidate. Rerun the proof and update the checklist rather than appending a new SHA to old evidence.

## 1. Open the release record

Copy the template:

```bash
VERSION=<version>
cp docs/release/release-preparation-checklist.md \
  "docs/release/${VERSION}-preparation.md"
```

Fill the release version, owner, and current status. Do not populate a SHA until it is merged and immutable in the relevant repository.

Before release preparation begins:

- drain or explicitly disposition the swarm PR queue;
- identify blocking issues and carry-over;
- confirm the selected version matches the compatibility change;
- pause unrelated release-document edits;
- confirm no tag or publication has already occurred.

For the 0.5 line, [the readiness record](release/0.5.0-readiness.md) retains earlier candidate, promotion, semver, and binary evidence. Those records remain useful historical evidence, but a later swarm commit requires a fresh exact candidate and promotion before publication.

## 2. Prepare the changelog with Changie

Changie is local authoring support, not a CI gate. Fragment creation and dry-render validation occur at commit time; release batching and merge are deliberate maintainer actions.

Install and verify the pinned tool:

```bash
changie --version
# must report v1.25.1
cargo precommit install
cargo precommit status
```

### Historical boundary

`CHANGELOG.md` through 0.5.0 predates Changie. The complete historical body is retained verbatim in `.changes/0.5.0.md`, while `.changes/header.tpl.md` owns the title and `[Unreleased]` boundary.

Always prove the baseline before changing release files:

```bash
cargo changelog-roundtrip
```

Never run `changie batch 0.5.0`. The 0.5.0 release section is the opaque pre-Changie baseline and must not be regenerated or reinterpreted.

A synthetic configuration proof may use a deliberately impossible version without writing files:

```bash
changie batch 9999.0.0-baseline-proof \
  --dry-run \
  --allow-no-changes=true
```

### Releases after 0.5.0

Review every unreleased fragment before batching. Then:

```bash
VERSION=<next-version>
cargo changelog-roundtrip
changie batch "$VERSION"
# Deliberately curate .changes/$VERSION.md.
changie merge
cargo changelog-roundtrip
```

The generated version file is an editorial starting point. Reorder and edit it to communicate the release accurately, but do not rewrite retained historical files. Confirm that remaining unreleased fragments are intentional next-release work.

## 3. Prove the swarm candidate

Run from a clean checkout of `shipper-swarm/main` after every intended release change is merged:

```bash
git status --short
git rev-parse HEAD
git rev-parse HEAD^{tree}

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-targets --all-features --locked --profile ci
cargo test --workspace --doc --locked
cargo test -p xtask --all-targets --locked

cargo xtask package-surface
cargo xtask check-lint-policy
cargo xtask no-panic check --mode blocking
cargo xtask check-file-policy --mode blocking-allowlist
cargo xtask check-process-policy --mode blocking-allowlist
cargo xtask check-network-policy --mode blocking-allowlist
cargo xtask check-workflow-surfaces --mode blocking-allowlist
cargo xtask check-doc-contracts --mode advisory
cargo xtask policy-report

PACKAGE_TARGET_DIR=$(mktemp -d)
CARGO_TARGET_DIR="$PACKAGE_TARGET_DIR" \
  cargo package --workspace --locked --exclude xtask
rm -rf "$PACKAGE_TARGET_DIR"

cargo changelog-roundtrip
git diff --check
```

The isolated package gate is the complete 13-crate archive proof. It makes Cargo verify each generated `.crate` archive with intra-workspace path dependencies resolved through the archives produced by the same command, while avoiding stale temporary-registry state from a prior commit.

A raw `cargo publish --dry-run --workspace` is not the pre-release all-crate gate. Cargo's publish verification resolves registry dependencies; dependent 0.5.0 crates correctly stop before publication because their 0.5.0 workspace dependencies are not yet present in crates.io.

Exercise Cargo's publish checks directly for the seven dependency-free crates:

```bash
for crate in \
  shipper-cargo-failure \
  shipper-duration \
  shipper-encrypt \
  shipper-output-sanitizer \
  shipper-retry \
  shipper-sparse-index \
  shipper-webhook
do
  cargo publish --dry-run --locked -p "$crate"
done
```

Do not misclassify a dependent-crate dry-run failure caused solely by an unpublished workspace dependency as a package defect. The dependent train is covered by the isolated package gate, release rehearsal, and the resumable live publish train.

Also require the normalized GitHub branch-protection result on the exact merged candidate:

```text
Shipper Rust Small Result
```

Do not substitute a PR merge ref, an earlier branch head, a queued job, or a route-specific implementation job. Review output must be complete enough to show that no substantive thread remains unresolved.

### Install-facade smoke

```bash
rm -rf target/install-smoke
cargo install --path crates/shipper --locked --root target/install-smoke

target/install-smoke/bin/shipper --version
target/install-smoke/bin/shipper doctor --help
target/install-smoke/bin/shipper plan --help
target/install-smoke/bin/shipper publish --help
target/install-smoke/bin/shipper resume --help
```

Use `shipper.exe` on Windows.

### Compatibility and product evidence

The release record must identify current evidence for:

- loading and resuming 0.4 state, event, receipt, and encrypted artifacts;
- rebuilding current state from events for every claimed field;
- sequential/parallel scheduler conformance;
- interruption and artifact handoff;
- ambiguous-outcome reconciliation;
- registry destination, redirect, resolved-address, and credential-authority policy;
- webhook and authorization non-disclosure;
- package surface and semver compatibility across all 13 publishable crates;
- release binary targets on matching operating systems.

An earlier proof may be reused only when the checklist records both SHAs, the complete diff boundary, and why that diff cannot affect the claim. The exact-head Rust gate and release-authority rehearsal are always rerun.

## 4. Freeze the candidate

After the gate passes on merged swarm main:

```bash
SWARM_SHA=$(git rev-parse HEAD)
SWARM_TREE=$(git rev-parse HEAD^{tree})
printf 'swarm_sha=%s\nswarm_tree=%s\n' "$SWARM_SHA" "$SWARM_TREE"
```

Record both values and all workflow run IDs in the release checklist and readiness record. Freeze normal swarm merges. If any later commit is required, explicitly supersede this identity and return to section 3.

The candidate must include:

- final changelog and release notes;
- migration and compatibility notes;
- support-tier claim updates;
- release carry-over;
- the completed local Changie baseline proof;
- exact-SHA test, policy, package, and install evidence.

## 5. Promote the candidate into `shipper`

Run from an `EffortlessMetrics/shipper` checkout. The release-authority main branch must already be an ancestor of the frozen swarm candidate. If it is not, stop and use the source-backfill procedure in [SWARM_OPERATION.md](status/SWARM_OPERATION.md) before creating another candidate.

```bash
git remote get-url origin
git remote add swarm git@github.com:EffortlessMetrics/shipper-swarm.git 2>/dev/null || true
git fetch origin --prune --tags
git fetch swarm --prune

git merge-base --is-ancestor origin/main swarm/main

git switch -c sync/shipper-swarm-YYYY-MM-DD origin/main
git merge --no-ff swarm/main -m "merge: sync shipper-swarm development"

test "$(git rev-parse HEAD^{tree})" = "$(git rev-parse swarm/main^{tree})"
git diff --check
git push -u origin sync/shipper-swarm-YYYY-MM-DD
```

Open the PR in `EffortlessMetrics/shipper`. Merge with a merge commit. Never squash or rebase a swarm promotion.

The tree-equality check is blocking. A normal promotion merge should preserve the frozen swarm tree exactly. A conflict or content edit is not routine release preparation; return the source-authority change to swarm through a reviewed backfill, rebuild the candidate, and promote again.

Record:

- promotion PR;
- merge SHA;
- merge tree;
- proof that the merge tree equals `SWARM_TREE`.

Normal swarm development remains paused until the resulting `shipper/main` merge commit is backfilled.

## 6. Prove the release authority

After promotion is merged, operate only in `EffortlessMetrics/shipper`:

```bash
test "$(gh repo view --json nameWithOwner -q .nameWithOwner)" = \
  "EffortlessMetrics/shipper"

git fetch origin --prune --tags
git switch main
git reset --hard origin/main

RELEASE_SHA=$(git rev-parse HEAD)
RELEASE_TREE=$(git rev-parse HEAD^{tree})
```

Any release-authority-only change after promotion must be a separate reviewed PR. Update the approved SHA and rerun the full candidate gate after it lands.

Run the section 3 commands again in the release-authority checkout, including `cargo changelog-roundtrip`.

### Non-publishing rehearsal

Dispatch against the exact approved SHA:

```bash
gh workflow run release.yml \
  --repo EffortlessMetrics/shipper \
  --ref main \
  -f mode=rehearse \
  -f ref="$RELEASE_SHA"
```

The rehearsal must produce and retain plan, preflight, state, event, receipt, auth, and policy evidence without publishing to crates.io.

### Binary matrix

```bash
gh workflow run release.yml \
  --repo EffortlessMetrics/shipper \
  --ref main \
  -f mode=binaries \
  -f ref="$RELEASE_SHA"
```

Require four matching-platform artifacts:

- Linux x86_64;
- macOS x86_64;
- macOS arm64;
- Windows x86_64.

Record the workflow runs, artifact names, checksums, and expiry window. Do not tag if artifacts will expire before publication or if a target used the wrong operating-system runner.

### Authentication posture

Review the release auth evidence. It must identify observed OIDC context, token minting, fallback configuration/use, and selected auth source without retaining token values. Trusted Publishing may remain advisory when the fallback-token path is the proved release posture; the checklist must state which path is actually authorized.

## 7. Authorize the tag

Tag only when every blocking checklist item passes and the approved release-authority SHA is still current.

```bash
VERSION=<version>
RELEASE_SHA=<approved-shipper-main-sha>

WORKSPACE_VERSION=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "shipper") | .version')

test "$WORKSPACE_VERSION" = "$VERSION"
test "$(git rev-parse origin/main)" = "$RELEASE_SHA"
! git rev-parse "v$VERSION" >/dev/null 2>&1

git tag -a "v$VERSION" "$RELEASE_SHA" -m "shipper $VERSION"
test "$(git rev-list -n 1 "v$VERSION")" = "$RELEASE_SHA"
git push origin "v$VERSION"
```

The tag push starts the irreversible release workflow only in `EffortlessMetrics/shipper`.

Record the tag, tag SHA, and workflow run immediately. If any identity differs, stop before publication begins.

## 8. Monitor the publish train

The generated Shipper plan is authoritative for package order. The current public surface is:

- `shipper-cargo-failure`
- `shipper-duration`
- `shipper-encrypt`
- `shipper-output-sanitizer`
- `shipper-retry`
- `shipper-sparse-index`
- `shipper-webhook`
- `shipper-types`
- `shipper-config`
- `shipper-registry`
- `shipper-core`
- `shipper-cli`
- `shipper`

Before the first upload, confirm the plan contains exactly the intended versions and a topological order. During the train, treat `events.jsonl` as truth, `state.json` as its projection, and `receipt.json` as a summary.

After each package:

- confirm the durable event exists;
- confirm state projection agrees;
- confirm registry visibility before a dependent package runs;
- confirm the latest `.shipper` artifact can be used for resume;
- confirm logs and artifacts remain sanitized.

### Stop conditions

Stop rather than retry when any of these appears:

- `StillUnknown` reconciliation;
- event/state/receipt drift;
- missing or malformed state artifact;
- tag, version, repository, branch, or SHA mismatch;
- unexpected crate, version, owner, or package count;
- registry trust or credential-authority violation;
- cross-origin authorization forwarding;
- binary or checksum mismatch;
- an approved SHA superseded by a later commit.

Cargo output is a hint. Registry truth is the safety boundary. Never blind-retry an ambiguous publish.

## 9. Resume an interrupted train

Use the release workflow's retained artifact path. Identify the run that uploaded the last valid `.shipper` state and dispatch resume in `EffortlessMetrics/shipper`:

```bash
gh workflow run release.yml \
  --repo EffortlessMetrics/shipper \
  --ref main \
  -f mode=resume \
  -f ref="$RELEASE_SHA" \
  -f artifact_run_id=<source-run-id>
```

Record both the source artifact run and the resume run. Before resuming, inspect events and state; after resuming, verify already-published packages were skipped and no duplicate upload occurred.

Do not create a new tag or start a fresh publish workflow to recover an interrupted train.

## 10. Verify and close the release

After publication:

```bash
cargo install shipper --version "$VERSION" --locked
shipper --version
shipper doctor --help
shipper plan --help
shipper publish --help
shipper resume --help
```

Verify:

- all 13 crate versions are visible on crates.io;
- the facade installs from crates.io;
- the installed version and help surfaces are correct;
- GitHub Release contains the four expected binaries and checksums;
- final `.shipper` plan, preflight, events, state, receipt, auth evidence, and policy reports are retained;
- no credential value appears in logs or committed evidence;
- the readiness record includes the final tag, workflow runs, artifacts, and carry-over.

## 11. Backfill release authority into swarm

Normal swarm development may resume only after the release-authority merge and any final release-only commits are present in `shipper-swarm` history.

When swarm has not advanced:

```bash
# From the shipper checkout
git fetch origin --prune --tags
git fetch swarm --prune

git merge-base --is-ancestor swarm/main origin/main
git push swarm origin/main:main
```

Do not force push. If swarm advanced after freeze, use the source-backfill merge procedure in [SWARM_OPERATION.md](status/SWARM_OPERATION.md) instead of overwriting development commits.

Finally verify the ancestry count described in the operation policy and reopen normal swarm merges.

## Release no-go summary

Do not tag or publish when:

- the candidate is not merged swarm main;
- exact-head required proof is incomplete or belongs to another SHA;
- `cargo changelog-roundtrip` fails;
- 0.5.0 was rebatch-generated or history changed unintentionally;
- a substantive review or policy finding remains unresolved;
- promotion tree differs from the frozen swarm tree;
- rehearsal, binaries, interruption/resume, auth, or package evidence is failed or unavailable;
- release workflow is being invoked from swarm;
- credentials have moved into swarm or are exposed to untrusted code;
- workspace version, tag, approved SHA, and tag SHA disagree;
- publication state is ambiguous and registry truth has not reconciled it.

The correct result of an incomplete checklist is **no release**, not an inferred pass.
