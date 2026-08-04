# Shipper change fragments

Shipper uses [Changie](https://changie.dev/) to capture release-note material while the implementation context is still fresh. Fragments are authoring inputs; `CHANGELOG.md`, the 0.5.0 release notes, migration notes, and readiness record remain deliberately edited and reviewed release artifacts.

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

# From source
go install github.com/miniscruff/changie@v1.25.1
```

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

For an unusual behavior-preserving change that matches a release-note path but genuinely needs no fragment, keep the rest of the gate active and provide a substantive local reason:

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

## Release preparation

The existing hand-maintained changelog predates Changie. Do **not** run `changie merge` until the focused changelog catch-up campaign has:

1. reconciled the historical entries;
2. moved the cleaned pre-Changie history into the Changie baseline version file;
3. curated the accumulated 0.5.0 fragments;
4. reviewed the generated 0.5.0 version section.

The intended release commands after that migration are:

```bash
changie batch 0.5.0
# edit/review .changes/0.5.0.md
changie merge
```

Version selection remains a deliberate release decision; Changie does not choose it.
