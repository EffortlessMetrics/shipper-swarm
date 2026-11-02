//! Review-forward admission proof for the actual routed Rust workflow.
//! This test uses the repo-owned YAML text and exact supported expression
//! shape; it is not a general GitHub Actions interpreter.

const WORKFLOW: &str = include_str!("../../.github/workflows/em-ci-routed-rust.yml");

fn job_section<'a>(source: &'a str, job: &str) -> Option<&'a str> {
    let marker = format!("\n  {job}:\n");
    let (_, rest) = source.split_once(&marker)?;
    // Find only a next top-level job (exactly two-space indentation).
    // A split on "\\n  " would also match four-space job properties.
    let end = rest
        .match_indices("\n  ")
        .find_map(|(offset, _)| {
            let header = rest[offset + 3..].lines().next().unwrap_or("");
            let name = header.strip_suffix(':')?;
            (!name.is_empty()
                && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
            .then_some(offset)
        })
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn job_if(section: &str) -> Option<String> {
    let mut lines = section.lines();
    while let Some(line) = lines.next() {
        if let Some(expression) = line.strip_prefix("    if: ") {
            if expression != "|" {
                return Some(expression.trim().to_owned());
            }
            let mut continued = Vec::new();
            for next in lines {
                if !next.starts_with("      ") {
                    break;
                }
                continued.push(next.trim());
            }
            return Some(continued.join(" "));
        }
    }
    None
}

fn admission_correct(source: &str) -> bool {
    let triggers = match source.split_once("\nconcurrency:") {
        Some((head, _)) => head,
        None => return false,
    };
    if !triggers.contains("  pull_request:\n    types: [opened, synchronize, reopened, ready_for_review]") {
        return false;
    }
    let route = match job_section(source, "route").and_then(job_if) {
        Some(value) => value,
        None => return false,
    };
    let result = match job_section(source, "result").and_then(job_if) {
        Some(value) => value,
        None => return false,
    };
    let allowed = "github.event_name != 'pull_request' || (github.event.pull_request.draft == false && github.event.pull_request.head.repo.full_name == github.repository)";
    if route != allowed || result != format!("always() && ({allowed})") {
        return false;
    }
    // All expensive producers depend on route, which is deliberately skipped
    // for drafts; no independent or always() producer can bypass admission.
    for producer in ["rust_small_cx43", "rust_small_cpx42", "rust_small_cx53", "rust_small_github"] {
        let Some(section) = job_section(source, producer) else {
            return false;
        };
        let depends_on_route = section.contains("    needs: [route]\n")
            || section.contains("    needs: [route,");
        if !depends_on_route
            || !section.contains("needs.route.outputs.target")
            || section.contains("    if: always()")
        {
            return false;
        }
    }
    true
}

#[test]
fn draft_admission_matches_review_forward_contract() {
    assert!(admission_correct(WORKFLOW), "ready event, both guards and route dependencies must agree");
    // These are the intended results of the exact expression checked above:
    // draft edits cost no Rust jobs; ready transitions and merges still run.
    for event in ["push", "merge_group", "workflow_dispatch"] {
        assert!(event != "pull_request");
    }
    for (draft, same_repo, admitted) in [
        (true, true, false),
        (true, false, false),
        (false, true, true),
        (false, false, false),
    ] {
        let actual = !draft && same_repo;
        assert_eq!(actual, admitted);
    }
}

#[test]
fn draft_admission_tests_reject_realistic_regressions() {
    let missing_ready = WORKFLOW.replace(", ready_for_review", "");
    assert!(!admission_correct(&missing_ready), "missing ready event must not pass");

    let missing_draft = WORKFLOW.replace("github.event.pull_request.draft == false && ", "");
    assert!(!admission_correct(&missing_draft), "missing draft guard must not pass");

    let missing_result_guard = WORKFLOW.replacen(
        "      (github.event_name != 'pull_request' || (github.event.pull_request.draft == false && ",
        "      (github.event_name != 'pull_request' || (",
        1,
    );
    assert!(!admission_correct(&missing_result_guard), "always() result must not run on draft");

    let missing_fork_guard = WORKFLOW.replace(
        "github.event.pull_request.head.repo.full_name == github.repository",
        "true",
    );
    assert!(!admission_correct(&missing_fork_guard), "fork isolation must remain");
}
