# Shipper change fragments

Shipper uses [Changie](https://changie.dev/) to capture release-note material while the implementation context is still fresh. Fragments are authoring inputs; `CHANGELOG.md`, release notes, migration notes, and readiness records remain deliberately edited and reviewed release artifacts.

## Local setup

Install **Changie v1.25.1**, then install the repository-owned hook:

```bash
changie --version
cargo precommit install
cargo precommit status
```

Common installation paths:

```bash
# macOS
brew install changie

# Windows
winget install miniscruff.changie
```

These package-manager commands install the current packaged release. The hook checks for v1.25.1 exactly; when Changie advances, update the repository pin and local tool together rather than silently accepting a different formatter. A manual v1.25.1 binary can also be downloaded from Changie's GitHub Releases page and placed on `PATH`.

The hook is local-only. No GitHub Actions workflow installs or runs Changie. It validates the staged Git index so unstaged working-tree edits cannot satisfy or break the check.

## Add a fragment

For a user-visible or compatibility-relevant change:

```bash
changie new
```

Choose the changelog kind, primary audience, and release-note importance. The optional PR number may be added after the pull request exists. Stage the resulting file under `.changes/unreleased/` with the implementation.

Use the editorial fields deliberately:

- **Headline** — a major operator, compatibility, recovery, security, or usability reason to care about the release.
- **Detailed** — meaningful release detail that belongs in the changelog but not the release-note opening.
- **Maintenance** — repository, test, CI, or internal work worth retaining below user-facing changes.

The pre-commit gate requires a branch-local fragment when staged changes touch product Rust, public manifests and READMEs, user-facing guides, release workflow behavior, templates, or the toolchain floor. Test-only, snapshot-only, policy, ordinary CI, agent-control, and internal status-document changes are exempt by path.

For an unusual behavior-preserving change that matches a release-note path but genuinely needs no fragment, keep the rest of the gate active and provide a substantive reason of at least 12 characters:

```bash
SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT="test-only inline module" git commit
```

PowerShell:

```powershell
$env:SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT = "test-only inline module"
git commit
Remove-Item Env:SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT
```

The reason is retained in `target/hooks/pre-commit.json` and should also be stated in the PR. `git commit --no-verify` remains Git's emergency bypass, but it skips the entire local gate rather than only the fragment requirement.

## Validate manually

```bash
cargo precommit
```

When Changie surfaces are relevant, the command requires v1.25.1 and runs a dry-run batch against the staged snapshot. It writes an advisory receipt to:

```text
target/hooks/pre-commit.json
```

Use `SHIPPER_PRECOMMIT_BASE=<ref>` when the branch should be compared with something other than `origin/main`.

## Historical baseline

The tracked changelog through **0.5.0** predates Changie. Its complete 0.5.0-and-earlier body is retained verbatim in `.changes/0.5.0.md`; `.changes/header.tpl.md` owns the changelog title and `[Unreleased]` boundary.

The single baseline file is intentional. It preserves the pre-Changie document as an opaque historical record instead of splitting and reinterpreting old release prose. Later Changie version files sort above `0.5.0.md` while the historical body remains unchanged.

Before changing Changie configuration, retained version files, or `CHANGELOG.md`, run:

```bash
cargo changelog-roundtrip
```

That local command requires Changie v1.25.1, renders `changie merge --dry-run`, and compares it with the tracked changelog. It permits only a final-newline difference and reports the first mismatching line. Do not run a writing `changie merge` when this proof fails.

Never batch 0.5.0 again. A historical correction must deliberately update both the tracked changelog and retained baseline, pass the round-trip proof, and remain separately reviewable.

## Prepare a later release

```bash
changie batch <next-version>
# Curate and review .changes/<next-version>.md.
cargo changelog-roundtrip   # expected to fail until CHANGELOG.md is intentionally regenerated
changie merge
cargo changelog-roundtrip   # must pass on the resulting tracked state
```

The batch output is an editorial starting point. Release notes, migration notes, support-tier claims, and readiness evidence still receive focused review. Version selection remains a deliberate release decision; Changie does not choose it.
