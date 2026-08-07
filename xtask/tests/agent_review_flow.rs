//! Repository contract tests for provider-native PR convergence.
//!
//! Shared documents describe semantics, but Claude and Codex must each retain
//! an executable skill route that naturally invokes substantive review before
//! live-CI verification and merge reconciliation.

use std::fs;
use std::path::{Path, PathBuf};

const SKILLS: [&str; 5] = [
    "finish-pr",
    "final-challenge",
    "review-pr",
    "verify-live-ci",
    "merge-reconcile",
];
const CANDIDATE_RESULTS: [&str; 5] = [
    "REVIEW_CURRENT",
    "CHANGES_REQUIRED",
    "NOT_PROVEN",
    "BLOCKED_BY_PREREQUISITE",
    "SUPERSEDED_OR_CLOSE",
];
const INTEGRATION_RESULTS: [&str; 4] = [
    "INTEGRATION_READY",
    "PR_IN_FLIGHT",
    "MERGE_BLOCKED",
    "NOT_PROVEN",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must live directly under the repository root")
        .to_path_buf()
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
        .replace("\r\n", "\n")
}

fn assert_in_order(text: &str, markers: &[&str], subject: &str) {
    let mut offset = 0;
    for marker in markers {
        let position = text[offset..]
            .find(marker)
            .unwrap_or_else(|| panic!("{subject} must contain `{marker}` in route order"));
        offset += position + marker.len();
    }
}

#[test]
fn both_providers_have_complete_native_skill_routes() {
    let root = repository_root();

    for provider in [".agents", ".claude"] {
        for skill in SKILLS {
            let path = format!("{provider}/skills/{skill}/SKILL.md");
            let content = read(&root, &path);
            assert!(
                content.starts_with(&format!("---\nname: {skill}\n")),
                "{path} must have portable normalized front matter"
            );
            assert!(
                content.contains("description:"),
                "{path} must provide a discovery description"
            );
        }
    }
}

#[test]
fn entry_points_route_review_before_live_ci_and_merge() {
    let root = repository_root();
    let agents = read(&root, "AGENTS.md");
    let claude = read(&root, "CLAUDE.md");

    assert_in_order(
        &agents,
        &[
            ".agents/skills/finish-pr",
            ".agents/skills/final-challenge",
            ".agents/skills/review-pr",
            ".agents/skills/verify-live-ci",
            ".agents/skills/merge-reconcile",
        ],
        "AGENTS.md",
    );
    assert_in_order(
        &claude,
        &[
            ".claude/skills/finish-pr",
            ".claude/skills/final-challenge",
            ".claude/skills/review-pr",
            ".claude/skills/verify-live-ci",
            ".claude/skills/merge-reconcile",
        ],
        "CLAUDE.md",
    );
}

#[test]
fn native_reviews_enforce_currentness_inline_findings_and_independence() {
    let root = repository_root();
    let required = [
        "repository",
        "head SHA",
        "base ref and base SHA",
        "merge-base SHA",
        "synthetic merge/check commit",
        "inline",
        "Failure mode:",
        "Why here:",
        "Fix direction:",
        "Validation:",
        "Confidence:",
        "Observed",
        "Reported",
        "Not verified",
        "No actionable findings emitted.",
        "Reviewer identity alone does not create independence",
        "Review every candidate PR individually",
        "Blanket automated thread resolution is forbidden",
    ];

    for provider in [".agents", ".claude"] {
        let path = format!("{provider}/skills/review-pr/SKILL.md");
        let content = read(&root, &path);
        for marker in required {
            assert!(content.contains(marker), "{path} must contain `{marker}`");
        }
        for result in CANDIDATE_RESULTS {
            assert!(content.contains(result), "{path} must contain `{result}`");
        }
    }
}

#[test]
fn candidate_and_integration_vocabularies_remain_separate() {
    let root = repository_root();

    for provider in [".agents", ".claude"] {
        let review_path = format!("{provider}/skills/review-pr/SKILL.md");
        let verify_path = format!("{provider}/skills/verify-live-ci/SKILL.md");
        let review = read(&root, &review_path);
        let verify = read(&root, &verify_path);

        for result in CANDIDATE_RESULTS {
            assert!(review.contains(result), "{review_path} must contain `{result}`");
        }
        for result in INTEGRATION_RESULTS {
            assert!(verify.contains(result), "{verify_path} must contain `{result}`");
        }
        assert!(
            verify.contains("This skill evaluates **integration posture**, not candidate quality."),
            "{verify_path} must preserve the review/integration boundary"
        );
    }
}

#[test]
fn shared_currentness_document_is_explicitly_non_executable() {
    let root = repository_root();
    let shared = read(&root, "docs/agent-context/review-currentness.md");

    assert!(shared.contains("not an executable review authority"));
    assert!(!root.join("docs/agents/PR_REVIEW_STANDARD.md").exists());
}

#[test]
fn contract_checks_are_crlf_portable() {
    let sample = "---\r\nname: review-pr\r\ndescription: example\r\n---\r\n";
    let normalized = sample.replace("\r\n", "\n");
    assert!(normalized.starts_with("---\nname: review-pr\n"));
}
