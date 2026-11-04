//! The small swarm guard must run without allocating the pool it validates.
//! Recognize the deliberately bounded workflow shape, not arbitrary Actions YAML.

const WORKFLOW: &str = include_str!("../../.github/workflows/runner-routing-guard.yml");
const SWARM: &str = "EffortlessMetrics/shipper-swarm";
const TRUST: &str = "github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository";

fn has_line(text: &str, expected: &str) -> bool {
    text.lines().any(|line| line == expected)
}

fn guard_contract(source: &str) -> bool {
    let Some((global, jobs)) = source.split_once("\njobs:\n") else {
        return false;
    };
    let Some((_, permission)) = global.split_once("\npermissions:\n") else {
        return false;
    };
    let Some((_, events)) = global.split_once("\n  pull_request:\n") else {
        return false;
    };
    let Some((pull_request, remaining_events)) = events.split_once("\n  push:\n") else {
        return false;
    };
    let Some((push, _)) = remaining_events.split_once("\n  workflow_dispatch:") else {
        return false;
    };
    let Some((nano, hosted)) = jobs.split_once("\n  no-bare-self-hosted-hosted:\n") else {
        return false;
    };
    let nano_if = format!("    if: github.repository != '{SWARM}' && ({TRUST})");
    let hosted_if = format!("    if: github.repository == '{SWARM}' && ({TRUST})");
    if permission.trim() != "contents: read"
        || jobs.contains("\n    permissions:")
        || !has_line(nano, &nano_if)
        || !has_line(hosted, "    runs-on: ubuntu-24.04")
        || !has_line(hosted, &hosted_if)
        || !has_line(nano, "      group: em-ci-nano")
        || !has_line(nano, "    timeout-minutes: 5")
        || !has_line(hosted, "    timeout-minutes: 5")
        || !has_line(nano, "    steps: &routing_guard_steps")
        || !has_line(hosted, "    steps: *routing_guard_steps")
        || hosted.matches("steps:").count() != 1
        // All shared steps are unconditional. A run line alone is not proof
        // when a step-local condition can prevent its execution.
        || nano.lines().any(|line| line.starts_with("        if:"))
    {
        return false;
    }
    let labels = "      labels: [self-hosted, linux, x64, em-ci, cx23, ci-nano, trusted-pr]";
    if !has_line(nano, labels) {
        return false;
    }
    let checks = [
        "      - uses: actions/checkout@v7.0.1",
        "          ACTIONLINT_VERSION: 1.7.12",
        "          ACTIONLINT_SHA256: 8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
        "          echo \"$ACTIONLINT_SHA256  $install_dir/$archive\" | sha256sum --check --status",
        "        run: actionlint -shellcheck=\"\"",
        "        run: scripts/ci/no-bare-self-hosted.sh",
        "        run: python3 scripts/ci/normalize-routed-rust-result.py --test",
    ];
    let paths = [
        ".github/workflows/**",
        "scripts/ci/no-bare-self-hosted.sh",
        "scripts/ci/normalize-routed-rust-result.py",
        "xtask/tests/runner_guard_routing.rs",
    ];
    checks.iter().all(|line| has_line(nano, line))
        && [pull_request, push]
            .iter()
            .all(|event| has_line(event, "    branches: [main]") && has_line(event, "    paths:"))
        && paths.iter().all(|path| {
            let entry = format!("      - \"{path}\"");
            [pull_request, push]
                .iter()
                .all(|event| event.lines().filter(|line| *line == entry.as_str()).count() == 1)
        })
}

#[test]
fn swarm_guard_shares_the_governed_checks_without_waiting_for_nano() {
    assert!(guard_contract(WORKFLOW));
}

#[test]
fn guard_contract_rejects_lost_checks_or_weakened_routes() {
    let mutations = [
        ("    runs-on: ubuntu-24.04", "    runs-on: self-hosted"),
        (
            "github.repository == 'EffortlessMetrics/shipper-swarm'",
            "true",
        ),
        (
            "github.repository != 'EffortlessMetrics/shipper-swarm'",
            "true",
        ),
        (TRUST, "true"),
        ("contents: read", "contents: write"),
        ("    steps: *routing_guard_steps", "    steps: []"),
        ("    timeout-minutes: 5", "    timeout-minutes: 360"),
        ("ACTIONLINT_VERSION: 1.7.12", "ACTIONLINT_VERSION: latest"),
        ("sha256sum --check --status", "true"),
        ("run: actionlint -shellcheck=\"\"", "run: true"),
        ("run: scripts/ci/no-bare-self-hosted.sh", "run: true"),
        (
            "normalize-routed-rust-result.py --test",
            "normalize-routed-rust-result.py",
        ),
        ("em-ci, cx23, ci-nano, trusted-pr", "em-ci, cx23, ci-nano"),
    ];
    for (from, to) in mutations {
        assert!(WORKFLOW.contains(from), "mutation must change actual input");
        let changed = WORKFLOW.replacen(from, to, 1);
        assert!(!guard_contract(&changed), "missed mutation: {from}");
    }
}

#[test]
fn guard_contract_rejects_disabled_proof_steps() {
    for run in [
        "        run: actionlint -shellcheck=\"\"",
        "        run: scripts/ci/no-bare-self-hosted.sh",
        "        run: python3 scripts/ci/normalize-routed-rust-result.py --test",
    ] {
        let from = format!("{run}\n");
        assert!(WORKFLOW.contains(from.as_str()));
        let to = format!("{run}\n        if: false\n");
        let changed = WORKFLOW.replacen(from.as_str(), to.as_str(), 1);
        assert!(has_line(&changed, run), "keep the original proof command");
        assert!(!guard_contract(&changed), "disabled proof step: {run}");
    }
}

#[test]
fn guard_contract_rejects_paths_moved_between_events() {
    let (before_push, after_push) = WORKFLOW.split_once("\n  push:\n").expect("push event");
    for path in [
        "scripts/ci/normalize-routed-rust-result.py",
        "xtask/tests/runner_guard_routing.rs",
    ] {
        let entry = format!("      - \"{path}\"");
        let doubled = format!("{entry}\n{entry}");
        for (remove_from, add_to, removing_from_push) in [
            (after_push, before_push, true),
            (before_push, after_push, false),
        ] {
            assert!(remove_from.contains(entry.as_str()));
            assert!(add_to.contains(entry.as_str()));
            let removed = remove_from.replacen(entry.as_str(), "", 1);
            let added = add_to.replacen(entry.as_str(), doubled.as_str(), 1);
            let changed = if removing_from_push {
                format!("{added}\n  push:\n{removed}")
            } else {
                format!("{removed}\n  push:\n{added}")
            };
            assert_eq!(changed.matches(entry.as_str()).count(), 2);
            assert!(!guard_contract(&changed), "misplaced path: {path}");
        }
    }
}

#[test]
fn guard_contract_rejects_changed_event_filters() {
    for event in ["pull_request", "push"] {
        let header = format!("  {event}:\n    branches: [main]\n    paths:\n");
        assert!(WORKFLOW.contains(header.as_str()));
        for replacement in [
            format!("  {event}:\n    branches: [never-guard-target]\n    paths:\n"),
            format!("  {event}:\n    branches: [main]\n    paths-ignore:\n"),
        ] {
            let changed = WORKFLOW.replacen(header.as_str(), replacement.as_str(), 1);
            assert!(!guard_contract(&changed), "changed event filter: {event}");
        }
    }
}
