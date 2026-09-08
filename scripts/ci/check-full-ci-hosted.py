"""Check ci.yml's bounded manual hosted route; actionlint owns YAML syntax.

This dependency-free checker deliberately accepts the repository's explicit job
and anchor layout. It checks routing, dependency and shared-command ownership,
not arbitrary YAML. Changes to that layout require updating these predicates.
"""

import argparse
from pathlib import Path
import re
import sys


HOSTED = (
    "github.repository == 'EffortlessMetrics/shipper-swarm' && "
    "github.event_name == 'workflow_dispatch' && "
    "inputs.runner_route == 'github-hosted'"
)
DEFAULT = "${{ !(" + HOSTED + ") }}"
MANUAL = "${{ " + HOSTED + " }}"
JOBS = {
    "lint": ("cx43", 30),
    "policy": ("cx43", 20),
    "test": ("cx43", 75),
    "install-smoke": ("cx43", 30),
    "crypto-proptests-heavy": ("cx53", 90),
    "msrv": ("cx43", 30),
    "security": ("cx43", 10),
    "docs": ("cx43", 30),
    "bdd": ("cx43", 45),
    "fuzz-smoke": ("cx53", 30),
    "cross-platform": ("cx43", 30),
    "release-build": ("cx53", 60),
}
DEPENDENCIES = {
    "bdd": ["lint"],
    "fuzz-smoke": ["lint"],
    "release-build": ["lint", "test"],
}
PREDICATES = {
    "crypto-proptests-heavy": "github.event_name == 'schedule' || github.event_name == 'push' || github.event_name == 'workflow_dispatch'",
    "fuzz-smoke": "github.event_name != 'schedule'",
    "release-build": "github.event_name == 'push' || github.event_name == 'workflow_dispatch'",
}
CACHE_PREFIX = "${{ runner.environment == 'github-hosted' && 'manual-hosted-' || '' }}"


def sections(text, indentation):
    """Read explicit mapping fields at one indentation, retaining nested text."""
    starts = list(re.finditer(r"^" + " " * indentation + r"([\w-]+):([^\n]*)", text, re.M))
    result = {}
    for index, match in enumerate(starts):
        end = starts[index + 1].start() if index + 1 < len(starts) else len(text)
        if match[1] in result:
            raise ValueError(f"duplicate field: {match[1]}")
        value = text[match.start():end].split(":", 1)[1]
        value = "\n".join(
            line for line in value.splitlines() if not line.lstrip().startswith("#")
        ).rstrip()
        result[match[1]] = value[1:] if value.startswith("\n") else value.lstrip()
    return result


def scalar(value, aliases):
    if value.startswith("*"):
        return aliases.get(value[1:], "unknown alias")
    value = re.sub(r"^&[\w-]+\s+", "", value)
    return " ".join(value.removeprefix(">-").split())


def check(text):
    failures = []

    def require(condition, message):
        if not condition:
            failures.append(message)

    document = sections(text, 0)
    jobs = sections(document.get("jobs", ""), 2)
    expected = set(JOBS) | {job + "-hosted" for job in JOBS}
    require(set(jobs) == expected, "full CI must retain exactly twelve job pairs")
    trigger = document.get("on", "")
    require("  pull_request:" not in trigger, "full CI must not acquire a PR trigger")
    require("default: self-hosted" in trigger, "manual input must default to self-hosted")
    require("type: choice" in trigger and "- github-hosted" in trigger, "manual route must be an explicit choice")
    require(document.get("permissions", "").strip() == "contents: read", "CI must retain read-only permissions")
    expected_group = "ci-${{ github.ref }}${{ " + HOSTED + " && '-manual-hosted' || '' }}"
    concurrency = sections(document.get("concurrency", ""), 2)
    require(concurrency.get("group") == expected_group, "manual hosted concurrency must be isolated from default runs")
    require(concurrency.get("cancel-in-progress") == "true", "repeated runs must remain cancellable")
    require("continue-on-error:" not in text, "full CI must not waive failed checks")
    aliases = {"default_runners": DEFAULT, "manual_hosted": MANUAL}

    for job, (capacity, timeout) in JOBS.items():
        original = sections(jobs.get(job, ""), 4)
        hosted = sections(jobs.get(job + "-hosted", ""), 4)
        gate = "${{ !(" + HOSTED + ") && (" + PREDICATES[job] + ") }}" if job in PREDICATES else DEFAULT
        require(scalar(original.get("if", ""), aliases) == gate, f"{job}: default route/predicate changed")
        require(scalar(hosted.get("if", ""), aliases) == MANUAL, f"{job}: hosted route must require explicit swarm dispatch")
        if job == "lint":
            require(original.get("if", "").startswith("&default_runners "), "default route anchor must exist")
            require(hosted.get("if", "").startswith("&manual_hosted "), "manual route anchor must exist")
        tier = "rust-large" if capacity == "cx53" else "rust-medium"
        expected_runner = f"group: em-ci-small labels: [self-hosted, linux, x64, em-ci, {capacity}, {tier}, trusted-pr]"
        require(" ".join(original.get("runs-on", "").split()) == expected_runner, f"{job}: original scoped runner changed")
        require(hosted.get("runs-on") == "ubuntu-24.04", f"{job}: hosted route must allocate an ephemeral Ubuntu runner")
        require(hosted.get("timeout-minutes") == str(timeout), f"{job}: hosted runtime budget changed")
        anchor = job.replace("-", "_") + "_steps"
        require(original.get("steps", "").startswith("&" + anchor + "\n"), f"{job}: shared steps must be defined once")
        require(hosted.get("steps") == "*" + anchor, f"{job}: hosted steps must alias the complete original proof")
        deps = DEPENDENCIES.get(job)
        require(original.get("needs") == ("[" + ", ".join(deps) + "]" if deps else None), f"{job}: original dependencies changed")
        require(hosted.get("needs") == ("[" + ", ".join(d + "-hosted" for d in deps) + "]" if deps else None), f"{job}: hosted dependencies must use selected hosted jobs")
        allowed = {"name", "if", "runs-on", "timeout-minutes", "steps"}
        if deps:
            allowed.add("needs")
        if job == "cross-platform":
            allowed.add("strategy")
            require(original.get("strategy", "").startswith("&target_strategy\n"), "target strategy anchor missing")
            require(hosted.get("strategy") == "*target_strategy", "hosted target matrix must alias the original")
        require(set(hosted) == allowed, f"{job}: hosted job cannot override shared proof environment or permissions")
        for cache in re.findall(r"^\s+(?:key|restore-keys): (.+)$", original.get("steps", ""), re.M):
            require(cache.startswith(CACHE_PREFIX), f"{job}: hosted cache must have its own namespace")

    # Pin the high-cost/deep proof that a nominally equivalent fallback might omit.
    for witness in [
        "cargo nextest run --workspace --all-features --profile ci",
        'PROPTEST_CASES: "256"',
        "cargo test -p shipper-cli --test e2e_rehearse -- --nocapture",
        "cargo build --release",
        "cargo test --workspace --doc",
        "cargo check --workspace --target ${{ matrix.target }}",
    ]:
        require(witness in text, f"required full-CI proof missing: {witness}")
    return failures


def test_mutations(text):
    mutants = [
        ("default route", "default: self-hosted", "default: github-hosted"),
        ("automatic hosted", MANUAL, MANUAL.replace("'workflow_dispatch'", "'push'")),
        ("unscoped hosted", "runs-on: ubuntu-24.04", "runs-on: self-hosted"),
        ("partial commands", "steps: *test_steps", "steps: *lint_steps"),
        ("skipped dependency", "needs: [lint-hosted, test-hosted]", "needs: [lint, test]"),
        ("matrix drift", "strategy: *target_strategy", "strategy: *other_matrix"),
        ("environment override", "steps: *lint_steps", 'env:\n      PROPTEST_CASES: "1"\n    steps: *lint_steps'),
        ("permission expansion", "contents: read", "contents: write"),
        ("weakened deep proof", 'PROPTEST_CASES: "256"', 'PROPTEST_CASES: "16"'),
        ("cache mixing", CACHE_PREFIX, ""),
        ("missing job", "  release-build-hosted:", "  omitted-release-proof:"),
        ("unbounded runtime", "timeout-minutes: 75", "timeout-minutes: 360"),
        ("concurrency collision", " && '-manual-hosted' || ''", " && '' || ''"),
        ("waived failure", "steps: *lint_steps", "continue-on-error: true\n    steps: *lint_steps"),
    ]
    for name, old, new in mutants:
        if old not in text:
            raise ValueError(f"mutation fixture no longer reaches {name}")
        if not check(text.replace(old, new, 1)):
            raise ValueError(f"checker accepted invalid fixture: {name}")
    print(f"Passed {len(mutants)} rejecting workflow fixtures.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test", action="store_true", help="also exercise rejecting workflow fixtures")
    parser.add_argument("--workflow", type=Path, default=Path(".github/workflows/ci.yml"))
    args = parser.parse_args()
    text = args.workflow.read_text(encoding="utf-8")
    failures = check(text)
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        return 1
    print("Manual full-CI route, twelve job pairs, shared proof and hosted isolation verified.")
    if args.test:
        test_mutations(text)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
