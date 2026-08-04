# Manage changelog fragments locally

Shipper uses Changie to capture release-note material at commit time while the implementation context is still fresh. This is a **local pre-commit workflow**, not a GitHub Actions gate.

The fragment ledger helps release preparation answer three different editorial questions without treating every merged PR as equally important:

- What should lead the release notes?
- What belongs in the detailed changelog?
- What should be retained but demoted to maintenance?

## Install the tools

Install Changie v1.25.1, then install the repository-owned hook:

```bash
changie --version
cargo precommit install
cargo precommit status
```

Use a packaged or release binary that reports v1.25.1. Homebrew and Winget install the current packaged release; a manual v1.25.1 asset is available from Changie's GitHub Releases page. When Changie advances, update the repository pin and local binary together rather than silently accepting changed rendering behavior.

The hook installer is idempotent and refuses to overwrite a foreign hook. Remove only the Shipper-owned hook with:

```bash
cargo precommit uninstall
```

## Add a fragment

For a user-visible or compatibility-relevant change:

```bash
changie new
```

Select:

1. the Keep-a-Changelog kind;
2. the primary audience;
3. the release-note importance;
4. the optional PR number, when known;
5. a concise operator- or consumer-facing body.

Stage the generated `.changes/unreleased/*.yaml` file with the implementation.

## What the hook checks

`cargo precommit` materializes a temporary checkout of the staged Git index and performs only local, inexpensive checks:

- staged whitespace and conflict-marker hygiene;
- whether release-note-relevant staged paths have a fragment already staged or committed on the branch;
- Changie v1.25.1 availability when Changie validation is required;
- a dry-run batch over the staged configuration and fragments.

The default comparison base is `origin/main`. Override it for a stacked or unusual branch:

```bash
SHIPPER_PRECOMMIT_BASE=<ref> cargo precommit
```

A run writes an advisory receipt to `target/hooks/pre-commit.json`. The receipt records staged paths, relevant paths, discovered branch fragments, Changie version, any exemption reason, and the local result.

## Relevant and exempt paths

The hook expects a fragment for changes to:

- production Rust under `crates/*/src/`;
- public crate manifests and READMEs;
- root release/product surfaces;
- user-facing tutorials, how-to, reference, and explanation docs;
- release workflow behavior and generated CI templates;
- the Rust toolchain floor.

It automatically exempts test directories and files, snapshots, ordinary CI workflows, policy ledgers, agent-control files, xtask internals, and internal status documents.

Inline test-only edits inside a production source file cannot be inferred safely from the path. For a genuine exception, keep the rest of the hook active and provide a substantive reason of at least 12 characters:

```bash
SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT="test-only inline module" git commit
```

PowerShell:

```powershell
$env:SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT = "test-only inline module"
git commit
Remove-Item Env:SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT
```

State the same reason in the PR. `git commit --no-verify` remains Git's emergency bypass, but it skips all local checks rather than only the fragment requirement.

## Why this is not CI

The hook is shift-left authoring support. It is intentionally bypassable, depends on a developer-local Changie binary, and is not merge authority. GitHub continues to prove candidate-head Rust behavior, policy, security, and review independently.

No workflow should install Changie merely to repeat this gate. The lasting evidence is the reviewed fragment and the release documents produced from the fragment ledger, not a green hosted hook simulation.

## Historical baseline and round-trip proof

The tracked changelog through **0.5.0** predates Changie. `.changes/0.5.0.md` retains the complete historical body from 0.5.0 through 0.1.0, while `.changes/header.tpl.md` retains the title, format statement, and `[Unreleased]` heading.

This intentionally treats pre-Changie history as one opaque baseline. Do not split, reorder, or rewrite the old sections merely to make them look like newly batched Changie output.

Validate the baseline locally with:

```bash
cargo changelog-roundtrip
```

The command:

1. requires the exact pinned Changie v1.25.1 binary;
2. runs `changie merge --dry-run` from the repository root;
3. compares the rendered output with `CHANGELOG.md`;
4. permits only a final-newline difference;
5. reports the first mismatching line or missing-history line count.

Do not run a writing merge while that command fails. Never batch 0.5.0 again.

## Prepare a later release

```bash
changie batch <next-version>
# Curate and review .changes/<next-version>.md.
changie merge
cargo changelog-roundtrip
```

The final round-trip proof must pass before the generated changelog is accepted. The batch output is still an editorial starting point: release notes, migration notes, support-tier claims, and readiness evidence receive focused review, and Changie does not select the release version.
