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
    let Some((nano, hosted)) = jobs.split_once("\n  no-bare-self-hosted-hosted:\n") else {
        return false;
    };
    let nano_if = format!("    if: github.repository != '{SWARM}' && ({TRUST})");
    let hosted_if = format!("    if: github.repository == '{SWARM}' && ({TRUST})");
    if permission.trim() != "contents: read"
        || jobs.contains("\n    permissions:")
        || !has_line(nano, &nano_if)
        || !has_line(hosted, &hosted_if)
        || !has_line(nano, "      group: em-ci-nano")
        || !has_line(hosted, "    runs-on: ubuntu-24.04")
        || !has_line(nano, "    timeout-minutes: 5")
        || !has_line(hosted, "    timeout-minutes: 5")
        || !has_line(nano, "    steps: &routing_guard_steps")
        || !has_line(hosted, "    steps: *routing_guard_steps")
        || hosted.matches("steps:").count() != 1
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
    checks.iter().all(|line| has_line(nano, line))
        && global.matches("\"scripts/ci/normalize-routed-rust-result.py\"").count() == 2
        && global.matches("\"xtask/tests/runner_guard_routing.rs\"").count() == 2
}

#[test]
fn swarm_guard_shares_the_governed_checks_without_waiting_for_nano() {
    assert!(guard_contract(WORKFLOW));
}

#[test]
fn guard_contract_rejects_lost_checks_or_weakened_routes() {
    let mutations = [
        ("    runs-on: ubuntu-24.04", "    runs-on: self-hosted"),
        ("github.repository == 'EffortlessMetrics/shipper-swarm'", "true"),
        ("github.repository != 'EffortlessMetrics/shipper-swarm'", "true"),
        (TRUST, "true"),
        ("contents: read", "contents: write"),
        ("    steps: *routing_guard_steps", "    steps: []"),
        ("    timeout-minutes: 5", "    timeout-minutes: 360"),
        ("ACTIONLINT_VERSION: 1.7.12", "ACTIONLINT_VERSION: latest"),
        ("sha256sum --check --status", "true"),
        ("run: actionlint -shellcheck=\"\"", "run: true"),
        ("run: scripts/ci/no-bare-self-hosted.sh", "run: true"),
        ("normalize-routed-rust-result.py --test", "normalize-routed-rust-result.py"),
        ("em-ci, cx23, ci-nano, trusted-pr", "em-ci, cx23, ci-nano"),
    ];
    for (from, to) in mutations {
        assert!(WORKFLOW.contains(from), "mutation must change actual input");
        let changed = WORKFLOW.replacen(from, to, 1);
        assert!(!guard_contract(&changed), "missed mutation: {from}");
    }
}
