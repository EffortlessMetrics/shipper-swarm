# How to run a Shipper release in GitHub Actions

Goal: a tag push triggers a workspace release driven by Shipper. Interruption-safe, evidence-preserved.

> **Destructive live-registry fence:** the workflow below can publish permanent
> versions. Protect the `release` environment with maintainer approval and use
> it only from the repository that owns release authority. Pull requests and
> ordinary CI should use the fake-Cargo/mock-registry recovery rehearsal, not a
> live publish. `EffortlessMetrics/shipper` remains the release-authority
> repository. Its
> `.github/workflows/release.yml` is the production example; the active
> development repository does not own publish credentials.

## Minimal workflow

```yaml
name: Release

on:
  push:
    tags: ['v*.*.*']

permissions:
  contents: write

jobs:
  publish:
    runs-on: ubuntu-latest
    environment: release
    timeout-minutes: 180
    steps:
      - uses: actions/checkout@v6

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Shipper
        run: cargo install shipper --locked

      - name: Diagnose release prerequisites
        run: shipper doctor

      - name: Plan
        run: |
          mkdir -p .shipper
          shipper plan --format json | tee .shipper/plan.txt

      - name: Upload plan artifact (before anything destructive)
        if: always()
        uses: actions/upload-artifact@v7
        with:
          name: shipper-state-plan
          path: .shipper/
          include-hidden-files: true
          retention-days: 30

      - name: Preflight
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
        run: shipper preflight --policy safe

      - name: Upload preflight artifact
        if: always()
        uses: actions/upload-artifact@v7
        with:
          name: shipper-state-preflight
          path: .shipper/
          include-hidden-files: true
          retention-days: 30

      - name: Publish
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
        run: |
          shipper publish \
            --policy safe \
            --readiness-method both \
            --max-attempts 12 \
            --max-delay 15m

      - name: Upload final state (always)
        if: always()
        uses: actions/upload-artifact@v7
        with:
          name: shipper-state-final
          path: .shipper/
          include-hidden-files: true
          retention-days: 90
```

## Key considerations

### `include-hidden-files: true`

`.shipper/` is a hidden directory. Without this flag, the artifact upload silently skips it. This bit us in rc.1 (issue #89).

### Upload state at every stage

Upload the `.shipper/` directory after plan, after preflight, and after publish (or on failure). If the publish job times out or dies, the most recent artifact is what you need to resume.

### Timeout budget

Registry rate limits and visibility delays can change. Budget from a rehearsal
of the actual package graph, retain Shipper's wait/backoff events, and leave
enough workflow time for evidence upload even after a failure. The example uses
180 minutes; that number is an operator budget, not a registry guarantee.

### Token vs Trusted Publishing

The example above uses `CARGO_REGISTRY_TOKEN` — a long-lived personal
access token stored as a repo secret. The retained 0.4.0 Shipper release
evidence proves that fallback path: it recorded Trusted Publishing token
mint failure, a configured fallback secret, and `selected_token_source =
"fallback_secret"` without storing token values.

Trusted Publishing (OIDC) is the target path once crates.io registration and a
release rehearsal prove it for every crate in the workspace. It uses
short-lived tokens scoped to a specific repo, workflow, ref pattern, and GitHub
Actions environment, with no PATs to rotate or leak. Keep the fallback secret
configured until the Trusted Publishing path is proven end to end.

**One-time setup on crates.io** (per crate):

1. Log in to <https://crates.io>, open the crate's **Settings →
   Trusted Publishing** panel.
2. Add a new trusted publisher:
   - Repository: `<owner>/<repo>`
   - Workflow filename: `release.yml` (or whatever yours is called)
   - Environment: `release` (match the `environment:` name in the job
     below — this is the scope guard)
3. Repeat for **every** crate the workspace publishes. Do NOT enable
   OIDC until the list is complete.

> **Why "every crate"**: if only some crates are registered, the
> OIDC action still succeeds and mints a token — but that token 401s
> on the unregistered crates mid-train, after some publishes have
> already succeeded. Shipper's preflight catches scope mismatches
> for *existing* crates via ownership checks, but new crates have no
> owner record yet so the first-publish case depends on operator
> discipline. Complete registration first; rehearse second; tag third.
>
> **Rehearsal validates the mechanism.** `release.yml`'s
> `release-rehearse` job binds to `environment: release` so the OIDC
> scope it mints matches production. A rehearsal that mints
> successfully proves the scope wiring. A mid-train 401 on a
> different crate proves you missed a registration step — fix the
> missing registration, don't retry the tag.

**Workflow**:

```yaml
permissions:
  contents: write
  id-token: write           # required to mint the OIDC token

jobs:
  publish:
    runs-on: ubuntu-latest
    environment: release    # must match the crates.io trusted-publisher config
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install shipper --locked

      # Exchange the workflow's OIDC token for a short-lived
      # crates.io publish token. Output: steps.auth.outputs.token.
      - id: auth
        uses: rust-lang/crates-io-auth-action@v1

      - name: shipper publish
        env:
          # Falls back to the long-lived secret if OIDC is unavailable
          # (e.g. during incident response or the first bootstrap run).
          CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token || secrets.CARGO_REGISTRY_TOKEN }}
        run: shipper publish --policy safe
```

`shipper doctor` validates the local workflow prerequisites it can see:
`id-token: write`, `environment: release`,
`rust-lang/crates-io-auth-action@v1`, and an explicit
`secrets.CARGO_REGISTRY_TOKEN` fallback. It does not validate crates.io's
per-crate Trusted Publishing registration; that remains a crates.io-side
setup step and is proven by the token exchange plus preflight ownership
checks for existing crates.

When the workflow keeps `secrets.CARGO_REGISTRY_TOKEN` as a fallback,
`shipper doctor` and `shipper preflight` keep that path visible with
advisory warnings. Treat the fallback as the retained proven posture and the
incident-recovery path until newer release evidence proves otherwise. Promote the short-lived token from
`rust-lang/crates-io-auth-action@v1` to the normal release path only after
release evidence proves the token mint path succeeds and fallback use is
explicitly unnecessary for the published crate set.

**Troubleshooting**:

- `id-token: write` missing → GitHub refuses the OIDC exchange → the
  action fails loudly; add the permission.
- Crate not registered as a trusted publisher → `cargo publish` returns
  401 despite a valid-looking token. Check crates.io's Trusted
  Publishing panel for the crate.
- Tag/branch mismatch → token minted for the wrong ref pattern →
  crates.io refuses. The `environment:` name is the tightest scope —
  make sure the workflow's environment matches what you registered.

See the release authority's
[`release.yml`](https://github.com/EffortlessMetrics/shipper/blob/main/.github/workflows/release.yml)
for the production example, and
[#96](https://github.com/EffortlessMetrics/shipper/issues/96) for the migration
history.

### Resume mode

If a release is interrupted, manually trigger the release authority's resume
workflow (a `workflow_dispatch` with `mode: resume` and
`artifact_run_id: <failed run id>`) — or adapt the resume job from the linked
production workflow while preserving its artifact and identity checks.

### Exit codes

`shipper publish` and `shipper resume` use a structured exit-code vocabulary so CI can distinguish outcomes:

| Code | Meaning | CI action |
|-----:|---------|-----------|
| 0 | All packages published/skipped | Proceed |
| 1 | General failure (config error, preflight failure, complete publish failure) | Alert / investigate |
| 2 | Finalized partial publish result — some packages published, some failed | Inspect the outcome and retained evidence before deciding whether resume is safe |

Clap/usage errors also exit 2, but print usage and create no `.shipper/`
execution evidence. They are not partial publish results. Do not branch on the
number alone; for a finalized JSON result inspect `execution_result`,
`safe_to_rerun` plus its reason, `next_action`, and evidence references.

Capture the code without hiding it from the job, then upload evidence for an
operator-controlled follow-up:

```yaml
- name: Publish
  id: publish
  shell: bash
  run: |
    set +e
    shipper publish --format json \
      > .shipper/publish.json \
      2> .shipper/publish.stderr
    code=$?
    echo "shipper_exit=$code" >> "$GITHUB_OUTPUT"
    exit "$code"

- name: Upload retained evidence
  if: always()
  uses: actions/upload-artifact@v7
  with:
    name: shipper-state-final
    path: .shipper/
    include-hidden-files: true
```

For a wrapped plan-build or publish-engine failure before a receipt exists,
inspect `.shipper/publish.stderr` for the redacted
`shipper.publish.error.v1` envelope; stdout is intentionally empty in that
case. Parser/usage and config/option-validation errors outside that typed
boundary may instead be prose or usage output. The separate stderr file keeps
either form available; retain both files with the rest of `.shipper/`.

The completed-result JSON envelope carries `execution_result` (`"success"`,
`"partial_failure"`, `"complete_failure"`) for programmatic gating. A later
approved job may restore the exact artifact as `.shipper/` inside the exact
source checkout and run `shipper status --durable` with the matching candidate
binary. It should invoke `shipper resume` only when the evidence-backed rerun
posture allows it.

## Generate a template

```bash
shipper ci github-actions > .github/workflows/release.yml
```

This prints a recent-defaults template you can customize.

## See also

- [Publish missing workspace crates](publish-missing-workspace-crates.md) — minimal idempotent publish recipe for CI
- [Tutorial: First publish](../tutorials/first-publish.md)
- [Tutorial: Recover from an interrupted release](../tutorials/recover-from-interruption.md)
- [Release runbook](../release-runbook.md) — operator reference for production releases
