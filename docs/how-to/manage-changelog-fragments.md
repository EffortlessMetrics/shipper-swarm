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

The hook installer is idempotent and refuses to overwrite a foreign hook. A hook is current only when its complete script matches the repository-generated dispatcher. An older Shipper-owned version is stale and repairable; a truncated or customized script carrying the current marker is conflicting and is never overwritten automatically. Remove only the unmodified current or stale Shipper-owned hook with:

```bash
cargo precommit uninstall
```

The installed dispatcher creates an owner-only temporary checkout of the staged index and runs Cargo, `.cargo/config.toml`, and `xtask` from that checkout. Git queries target the original repository through a hook-only root handoff that must resolve to that repository's exact top level. This keeps unstaged tooling edits or an unrelated ambient environment variable from changing which staged commit passes. Cleanup preserves the Cargo process exit status even when temporary-directory removal fails.

Before materializing or executing the staged checkout, both the shell dispatcher and the Rust command reject every Git index entry with symbolic-link mode `120000`. The repository-owned gate deliberately does not follow staged symlinks, because a link can escape the private checkout and expose unstaged source or configuration to Cargo or Changie. Any future contained-symlink policy must be designed and reviewed separately.

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
- symbolic-link rejection before Cargo or Changie can follow staged paths;
- whether release-note-relevant staged paths have a fragment already staged or committed on the branch;
- Changie v1.25.1 availability when Changie validation is required;
- a dry-run batch over the staged configuration and fragments.

A dry-run may allow an empty unreleased ledger only when no release-note-relevant path and no branch-local fragment are present, as after a deliberate release batch. This does not bypass fragment enforcement: a staged product change without a fragment is recorded as a failure before Changie validation runs.

The default comparison base is `origin/main`. Override it for a stacked or unusual branch:

```bash
SHIPPER_PRECOMMIT_BASE=<ref> cargo precommit
```

A run writes an advisory receipt to `target/hooks/pre-commit.json`. The receipt records staged paths, relevant paths, discovered branch fragments, whether an empty Changie batch was allowed, the observed Changie version, any exemption reason, and the local result.

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

## Historical migration boundary

The tracked changelog through **0.5.0** predates Changie. This intake contract begins collecting forward-looking fragments, but it does not itself rewrite existing release history.

Until a focused baseline-migration PR imports the existing 0.5.0-and-earlier text and proves a lossless round trip, do not run `changie merge` and do not batch 0.5.0 again. The migration must demonstrate that a clean merge reproduces the tracked `CHANGELOG.md` exactly and must retain a regression check against accidental history truncation.

## Prepare a later release

After the baseline migration exists, release preparation is:

```bash
changie batch <next-version>
# Curate and review .changes/<next-version>.md.
changie merge
```

The batch output is an editorial starting point. Release notes, migration notes, support-tier claims, and readiness evidence still receive focused review. Changie does not select the release version.
