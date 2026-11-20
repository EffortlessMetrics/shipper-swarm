# Contributing to Shipper

Thank you for your interest in contributing to Shipper! This document provides guidelines and instructions for contributing.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Making Changes](#making-changes)
- [Testing](#testing)
- [Pull Request Process](#pull-request-process)
- [Code Style](#code-style)

---

## Code of Conduct

We follow the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Please be respectful and constructive in all interactions.

---

## Getting Started

Active development happens in
[`EffortlessMetrics/shipper-swarm`](https://github.com/EffortlessMetrics/shipper-swarm).
The original [`EffortlessMetrics/shipper`](https://github.com/EffortlessMetrics/shipper)
repository remains the release authority for crates.io publishing and release
evidence until that authority is explicitly moved.

Do not add crates.io publish tokens, release signing secrets, or release
workflow credentials to `shipper-swarm`.

The operating policy is documented in
[docs/status/SWARM_OPERATION.md](docs/status/SWARM_OPERATION.md):

- PRs into `shipper-swarm/main` are squash-merged.
- Promotion from `shipper-swarm/main` back to `shipper/main` uses a merge
  commit, never squash or rebase.
- `shipper-swarm/main` must remain a continuation of `shipper/main` history.
- Source-repo sync and credential rules are mirrored in
  [docs/status/SWARM_SYNC.md](docs/status/SWARM_SYNC.md).

1. Fork the development repository.
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/shipper-swarm.git
   cd shipper-swarm
   ```
3. Install Changie v1.25.1 and the repository-owned local pre-commit hook:
   ```bash
   changie --version
   cargo precommit install
   cargo precommit status
   ```
4. Create a branch for your changes:
   ```bash
   git checkout -b my-feature
   ```

---

## Development Environment

### Prerequisites

- **Rust**: 1.95 or later (check with `rustc --version`)
- **Git**: For version control
- **Changie**: v1.25.1 for local change-fragment authoring and pre-commit validation
- **cargo-nextest** (optional): For better test output

### Native Windows setup (x64 MSVC)

Install the Visual Studio **Desktop development with C++** workload with the x64/x86
MSVC tools and a Windows SDK. These provide the native linker and import libraries.
See the [MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

If `rustup --version` is unavailable, download the x64 `rustup-init.exe` from
[Install Rust](https://rust-lang.org/tools/install/). In its download directory,
run `.\rustup-init.exe -y --default-toolchain none`, then open a new PowerShell.
Provision the moving stable channel for general work:

```powershell
rustup toolchain install stable --profile minimal --no-self-update
```

For general tooling, download the Windows x64 MSI for the latest final release
from the [official Node.js distribution](https://nodejs.org/dist/latest/). For uv,
use the latest Windows release from [Astral's installation guide](https://docs.astral.sh/uv/getting-started/installation/).
For an existing uv installation, use `uv self update` with the standalone
installer, or upgrade through the package manager that installed it.
Use any versions required by the checkout or CI for project qualification.
Open a new PowerShell after installation so it picks up PATH changes.

From the checkout root, in a shell without a toolchain override, install the
compiler and components selected by `rust-toolchain.toml`:

```powershell
rustup toolchain install --no-self-update
```

The workspace's `rust-version` in `Cargo.toml` declares the minimum supported
Rust version (MSRV). Use the repository's MSRV build and test checks to verify
compatibility. To inspect a rebuilt environment, open a fresh PowerShell at
the checkout root and compare the active toolchain and compiler to the pin:

```powershell
$env:RUSTUP_AUTO_INSTALL = '0'
rustup run stable rustc -V
rustup run stable cargo -V
node --version
uv --version
rustup show active-toolchain
rustc -Vv
cargo -V
```

### Codex Cloud (Linux)

In **Settings > Codex Cloud > Environments**, create an environment for
`EffortlessMetrics/shipper-swarm`, or select **Edit** for an existing one.
Paste the Bash block below into **Install script**. Run it from the repository
root on a GNU/Linux development image with Python 3.11+ and the build tools
checked below. The image also needs the `libatomic` runtime (`libatomic1` on
Debian/Ubuntu) to execute official Linux Node.js binaries starting with v25.
See [Node.js binary requirements](https://github.com/nodejs/node/blob/v26.x/BUILDING.md#official-binary-platforms-and-toolchains).

Rustup manages both the moving `stable` channel for general tools and the
checkout's pinned toolchain for Shipper. For 0.5, `rust-toolchain.toml` pins
1.95.0 and `workspace.package.rust-version` in `Cargo.toml` declares MSRV 1.95.
The install also prepares the latest final Node.js release (including npm) and
latest uv for general use, plus the repository versions of Changie and jactionlint.
Node's upstream `latest` channel can point to a release that has entered LTS.

The install phase needs HTTPS access to Rust (`sh.rustup.rs`, `static.rust-lang.org`),
crates.io (`index.crates.io`, `static.crates.io`), Node (`nodejs.org`),
Astral (`astral.sh` and its release redirects), and GitHub release assets
(`github.com`, `release-assets.githubusercontent.com`). The locked fetch
prepares the Cargo cache for an offline agent task.

```bash
set -euo pipefail
tools_dir="$HOME/.local/share/shipper-codex"
export PATH="$tools_dir/node/bin:$tools_dir/bin:${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    node_arch=x64; arch=amd64
    changie_sha=90b90b459b345335ef5715e4e03c354cde99070729939cbab004ac37e5966630
    jactionlint_sha=5ac307b0499bb0eab45ae99c23e8e0790874fbc667c75224410954e0e3c6e0f5 ;;
  Linux-aarch64)
    node_arch=arm64; arch=arm64
    changie_sha=115b255d2f5da1a1d50ffcd7853a2616d87a40487679d46f95a39c3956b9b618
    jactionlint_sha=ad7e33b6264cea16b7e83ad2bb004824d5b1c15aedb35ba0dc84917f791c5525 ;;
  *) echo 'Use an x86_64 or aarch64 GNU/Linux development image.' >&2; exit 1 ;;
esac
for tool in git python3 curl tar xz sha256sum cc c++ make cmake; do
  command -v "$tool" >/dev/null || { echo "Missing image prerequisite: $tool" >&2; exit 1; }
done
pin=$(python3 -c 'import tomllib; print(tomllib.load(open("rust-toolchain.toml","rb"))["toolchain"]["channel"])')
mkdir -p "$tools_dir/bin"
scratch_dir=$(mktemp -d "$tools_dir/.setup.XXXXXXXX")
trap 'rm -rf -- "$scratch_dir"' EXIT
fetch() { curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL "$1" -o "$2"; }
verify() { (cd "$scratch_dir" && printf '%s  %s\n' "$1" "$2" | sha256sum --check --status); }

if ! command -v rustup >/dev/null; then
  fetch https://sh.rustup.rs "$scratch_dir/rustup-init.sh"
  sh "$scratch_dir/rustup-init.sh" -y --no-modify-path --default-toolchain none
fi
rustup toolchain install stable --profile minimal --no-self-update
rustup toolchain install "$pin" --profile minimal --component rustfmt,clippy --no-self-update
rustup run "$pin" cargo fetch --locked

# Resolve the latest final Node.js release once, then verify its archive.
dist=https://nodejs.org/dist
fetch "$dist/latest/SHASUMS256.txt" "$scratch_dir/node-sums"
read -r digest asset < <(awk -v arch="$node_arch" \
  '$2 ~ ("^node-v[0-9]+\\.[0-9]+\\.[0-9]+-linux-" arch "\\.tar\\.xz$") {print $1, $2; exit}' "$scratch_dir/node-sums")
folder=${asset%.tar.xz}
version=${folder#node-}; version=${version%-linux-*}
if [[ ! -x "$tools_dir/$folder/bin/node" || ! -x "$tools_dir/$folder/bin/npm" ]]; then
  fetch "$dist/$version/$asset" "$scratch_dir/$asset"
  verify "$digest" "$asset"
  tar -xf "$scratch_dir/$asset" -C "$scratch_dir"
  test -x "$scratch_dir/$folder/bin/node"
  test -x "$scratch_dir/$folder/bin/npm"
  if [[ -d "$tools_dir/$folder" ]]; then
    mv -T "$tools_dir/$folder" "$scratch_dir/previous-node"
  fi
  mv -T "$scratch_dir/$folder" "$tools_dir/$folder"
fi
"$tools_dir/$folder/bin/node" --version
ln -sfnT "$folder" "$tools_dir/node"

fetch https://astral.sh/uv/install.sh "$scratch_dir/uv-install.sh"
UV_UNMANAGED_INSTALL="$tools_dir/bin" sh "$scratch_dir/uv-install.sh"
install_release() {
  local repo=$1 name=${1##*/} version=$2 sha=$3
  local asset="${name}_${version}_linux_${arch}.tar.gz"
  fetch "https://github.com/$repo/releases/download/v$version/$asset" "$scratch_dir/$asset"
  verify "$sha" "$asset"
  tar -xf "$scratch_dir/$asset" -C "$scratch_dir" "$name"
  mv -f "$scratch_dir/$name" "$tools_dir/bin/$name"
}
install_release miniscruff/changie 1.25.1 "$changie_sha"
install_release jdx/jactionlint 2.5.0 "$jactionlint_sha"
```

After rebuilding, start a new task at the repository root. Paste this block in
its shell to activate the prepared tools and check the locked graph offline.
If an installed-tool probe or metadata resolution fails, refresh the setup
for that checkout. Keep agent internet access off.

```bash
export PATH="$HOME/.local/share/shipper-codex/node/bin:$HOME/.local/share/shipper-codex/bin:${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
(
  set -eu
  export RUSTUP_AUTO_INSTALL=0
  pin=$(python3 -c 'import tomllib; print(tomllib.load(open("rust-toolchain.toml","rb"))["toolchain"]["channel"])')
  rustup which --toolchain stable rustc >/dev/null
  export RUSTC
  RUSTC=$(rustup which --toolchain "$pin" rustc)
  rustup which --toolchain "$pin" cargo >/dev/null
  rustup run stable rustc --version
  "$RUSTC" --version
  rustup run "$pin" cargo --version
  for rel in node/bin/node node/bin/npm bin/uv bin/changie bin/jactionlint; do
    expected="$HOME/.local/share/shipper-codex/$rel"
    if [[ ! -x "$expected" || "$(command -v "${rel##*/}")" != "$expected" ]]; then
      printf 'Expected executable on PATH: %s\n' "$expected" >&2
      exit 1
    fi
  done
  node --version
  npm --version
  uv --version
  changie_version=$(changie --version)
  jactionlint_version=$(jactionlint --version)
  printf '%s\n' "$changie_version" "$jactionlint_version"
  [[ "$changie_version" == 'changie version v1.25.1' ]]
  [[ "${jactionlint_version%%$'\n'*}" == '2.5.0' ]]
  rustup run "$pin" cargo metadata --format-version 1 --locked --offline >/dev/null
)
```

For later project commands in a shell with a Rust toolchain override, use
`cargo +1.95.0` on the 0.5 checkout (use the checkout pin on other branches).
For workflow edits, run the prepared linter offline from the repository root
with `jactionlint --profile correctness --no-online --shellcheck=""`.

### Building

```bash
# Build all crates
cargo build --workspace

# Build in release mode
cargo build --workspace --release
```

### Running

```bash
# Run the CLI locally
cargo run --package shipper -- <command>

# Example
cargo run --package shipper -- plan --help
```

---

## Making Changes

### Before You Start

- Check the [development issues](https://github.com/EffortlessMetrics/shipper-swarm/issues)
  and open PRs for related work before claiming a slice. Start with the
  [build-readiness handoff](plans/swarm/build-readiness.md) for the audited work
  order and the current release-freeze boundary.
- Open routine product, test, documentation, and CI work in `shipper-swarm`.
  Existing linked [source-repository issues](https://github.com/EffortlessMetrics/shipper/issues)
  retain their context; implement routine development through swarm PRs and link
  the original issue. Promotion, rehearsal, publication, and release evidence
  remain owned by the release-authority issues.
- For significant changes, open an issue first to discuss the approach.
- Keep changes focused and atomic.

### Changelog fragments

Shipper uses Changie as a **local pre-commit authoring contract**, not a CI gate.
For user-visible, compatibility-relevant, security, recovery, or operational
changes, create and stage a fragment with the implementation:

```bash
changie new
```

The resulting YAML file lives under `.changes/unreleased/`. Choose its kind,
primary audience, and release-note importance deliberately so the release-prep
campaign can promote headline changes, retain useful detail, and demote
maintenance work without reconstructing intent months later.

The repository-owned hook validates the staged Git index, checks that relevant
changes have a branch-local fragment, validates Changie v1.25.1 with a dry-run
batch, and writes `target/hooks/pre-commit.json`:

```bash
cargo precommit
```

Test-only, snapshot-only, ordinary CI, policy, agent-control, and internal status
changes are exempt by path. For an unusual behavior-preserving change inside a
normally user-facing source path, set
`SHIPPER_PRECOMMIT_CHANGELOG_EXEMPT` to a substantive reason of at least 12
characters for that commit; the reason is retained in the local receipt and
should be stated in the PR. See [.changes/README.md](.changes/README.md) for the
full workflow.

No GitHub Actions workflow installs or runs Changie. The hook is bypassable and
is not merge authority; normal hosted Rust, policy, and review gates remain
separate.

### Code Organization

| Directory | Purpose |
|-----------|---------|
| `crates/shipper/` | Install facade and curated library re-export |
| `crates/shipper-cli/` | CLI adapter: clap, subcommands, help, human/JSON output |
| `crates/shipper-core/` | Engine/library implementation |
| `docs/` | User documentation |
| `templates/` | CI/CD templates |
| `fuzz/` | Fuzzing targets |

### Key Modules

| Module | Responsibility |
|--------|----------------|
| `crates/shipper-core/src/plan/` | Publish planning and ordering |
| `crates/shipper-core/src/engine/` | Publish/preflight/resume execution engine |
| `crates/shipper-registry/src/` | Registry API interactions |
| `crates/shipper-core/src/ops/cargo/` | Cargo command wrappers |
| `crates/shipper-core/src/state/` | State persistence |
| `crates/shipper-core/src/state/events/` | Event logging |
| `crates/shipper-config/` | Configuration handling |

---

## Testing

### GitHub Actions validation

Run the pinned workflow linter from the repository root before handing off
workflow changes:

```bash
uvx --from jactionlint==2.5.0 jactionlint --profile correctness --no-online --shellcheck=""
```

If installed with `uv tool install jactionlint==2.5.0`, the equivalent command is
`jactionlint --profile correctness --no-online --shellcheck=""`; `jactionlint --version`
shows the selected version. The `Runner Routing Guard` uses the same correctness
profile and checks a SHA256-pinned release binary. It keeps the pre-existing
ShellCheck-disabled scope and reads `.github/actionlint.yaml` for runner labels.
The gate is static workflow proof, not evidence of runner availability, release
success, or cold/warm-cache timings. YAML parsing alone does not replace it.

### Running Tests

The [Testing Guide](docs/testing.md) explains package selection, focused tests,
and configured CI proof. At the workspace root, plain `cargo test` selects only
the `shipper` and `shipper-cli` default members; it does not select the engine,
support crates, or xtask for their own tests.

```bash
# Run workspace tests and doc tests with all features
cargo test --workspace --all-features --locked

# Quick check of the two default members
cargo test --locked

# Run a specific CLI integration test binary
cargo test --package shipper-cli --test cli_e2e --locked

# Run with verbose output
cargo test --workspace --all-features --locked -- --nocapture

# Run library unit tests only (skip integration tests and doc tests)
cargo test --workspace --lib --all-features --locked
```

### Test Categories

| Type | Location | Purpose |
|------|----------|---------|
| Unit tests | `src/**/tests` modules | Test individual functions |
| Integration tests | `tests/` directories | Test module interactions |
| E2E tests | `crates/shipper-cli/tests/cli_e2e.rs` | Test CLI behavior |
| BDD tests | `crates/shipper-cli/tests/bdd_*.rs` | Behavior-driven scenarios |
| Property tests | Throughout using proptest | Property-based testing |

### Writing Tests

- Place unit tests in `#[cfg(test)]` modules within source files.
- Place integration tests in the `tests/` directory.
- Use descriptive test names: `given_X_when_Y_then_Z`.
- Add property tests for complex logic using `proptest`.

---

## Pull Request Process

### Before Submitting

1. **Run the staged local gate:**
   ```bash
   cargo precommit
   ```

2. **Format your code:**
   ```bash
   cargo fmt
   ```

3. **Run clippy:**
   ```bash
   cargo clippy --workspace -- -D warnings
   ```
   All warnings must be resolved.

4. **Run all tests:**
   ```bash
   cargo test --workspace
   ```

5. **Update documentation** if your changes affect user-facing behavior.

6. **Check the Changie disposition:** include a fragment for a relevant change,
   or state the explicit exemption reason in the PR.

### PR Guidelines

- **Title**: Use conventional commit format
  - `feat: add shell completion support`
  - `fix: handle missing registry gracefully`
  - `docs: update configuration examples`
  - `refactor: simplify publish loop`

- **Description**: Explain what and why, not how.
- **Link issues**: Reference any related issues.
- **Small PRs**: Keep changes focused and reviewable.
- **Changelog disposition**: identify the fragment or the reason no fragment is needed.
- **Review identity**: record the intended base, exact head, base SHA,
  merge-base SHA, and any synthetic merge/check commit used for final proof.
- **Review map**: name semantic owners, callers/consumers, highest-risk
  invariants, required adversarial/platform proof, and schema/docs/package/
  support/release impact.
- **Required gate**: `shipper-swarm/main` requires `Shipper Rust Small Result`;
  do not require route-specific implementation jobs directly because only one
  route runs per attempt.
- **Merge method**: use squash merge for normal `shipper-swarm` PRs.
  Source-backfill PRs that merge release-authority commits from `shipper/main`
  are the exception and must use merge commits.

### Review Process

A PR needs both a substantive **candidate judgment** and a separate live
**integration posture**. An approval or green check list alone is not enough.

1. Reconstruct the cumulative claim, non-goals, controlling authority, exact
   head/base/merge-base identities, changed semantic surfaces, proof, prior
   findings, and limitations.
2. Apply proportionate correctness, architecture, integration, test-oracle,
   security/release/claim-boundary, and simplification passes. Inspect relevant
   callers, consumers, schemas, fixtures, packages, docs, and workflows beyond
   the changed lines.
3. Post precise inline findings where GitHub permits. Avoid duplicate threads.
   A clean review records inspected surfaces, challenge passes, residual risk,
   evidence provenance, and what remains unproved; a generic `LGTM` is not
   review evidence.
4. Return one substantive result:

   ```text
   REVIEW_CURRENT
   CHANGES_REQUIRED
   NOT_PROVEN
   BLOCKED_BY_PREREQUISITE
   SUPERSEDED_OR_CLOSE
   ```

5. After a repair, rerun affected proof and re-review affected findings,
   semantic dimensions, and repair-created edge cases. A fixing commit or
   evidence-backed rejection must be replied to the relevant thread before it
   is resolved; blanket automated resolution is forbidden.
6. Only a current `REVIEW_CURRENT` result proceeds to live integration
   evaluation. Classify checks and merge state separately as:

   ```text
   INTEGRATION_READY
   PR_IN_FLIGHT
   MERGE_BLOCKED
   NOT_PROVEN
   ```

7. Merge only when the reviewed effective subject remains current, required
   checks are terminal and successful or explicitly not applicable,
   substantive threads are resolved with evidence, and the PR is actually
   mergeable. Squash-merge normal development PRs; use the separate
   history-preserving contract for source-backfill PRs.
8. Verify merged `main`, reconcile the controlling issue and dependent PRs,
   and retain the review/proof record after merge.

Claude Code and Codex execute this lifecycle through their own complete native
skills under `.claude/skills/` and `.agents/skills/`. Shared semantics are in
[docs/agent-context/review-currentness.md](docs/agent-context/review-currentness.md),
but that document is not an executable review authority. Every PR in a stack
must receive its own review before a campaign-level synthesis.

---

## Code Style

### Formatting

- Use `cargo fmt` before committing.
- Maximum line length: 100 characters (rustfmt default).

### Naming Conventions

| Item | Convention | Example |
|------|------------|---------|
| Types | PascalCase | `PublishPlan` |
| Functions | snake_case | `build_plan()` |
| Constants | SCREAMING_SNAKE | `MAX_RETRIES` |
| Modules | snake_case | `engine_parallel` |

### Documentation

- Add rustdoc comments (`///`) for public items.
- Include examples in doc comments when helpful.
- Keep line comments (`//`) for implementation notes.

### Error Handling

- Use `Result<T, E>` for fallible operations.
- Use `thiserror` for custom error types.
- Provide actionable error messages.

### Commit Messages

Follow conventional commits:

```
<type>: <description>

[optional body]

[optional footer]
```

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`

---

## Questions?

- Open a [discussion](https://github.com/effortlessmetrics/shipper/discussions)
  for questions. Discussions remain in the release-authority repo until
  explicitly moved.
- Open a [development issue](https://github.com/EffortlessMetrics/shipper-swarm/issues)
  for routine bugs or features. Promotion, rehearsal, publication, and release
  evidence retain the source-repository issue owners described in
  [Before You Start](#before-you-start).

Thank you for contributing!
