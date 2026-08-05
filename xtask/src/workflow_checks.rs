//! Workflow / process / network policy checks.
//!
//! Three subcommands:
//!
//! - `cargo xtask check-workflow-surfaces`  — every `.github/workflows/*.yml`
//!   (and `.github/dependabot.yml`) must be receipted in
//!   `policy/workflow-allowlist.toml`. Each entry must name a
//!   `process_policy` and `network_policy` that exist in their respective
//!   ledgers.
//! - `cargo xtask check-process-policy`     — for each receipted workflow,
//!   scan its file content for command names; flag commands present in any
//!   other process profile but NOT in this workflow's declared profile.
//! - `cargo xtask check-network-policy`     — for each receipted workflow,
//!   scan its file content for `https?://<host>` URLs; flag hostnames not in
//!   the declared network profile.
//!
//! All three accept `--mode advisory|blocking-allowlist|blocking-strict`.
//! The user's spec for PR 8 says explicitly "start simple": these checks are
//! grep-style heuristics, not full YAML/AST parsers. Advisory mode is the
//! default and what CI runs (PR 10).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use regex::Regex;
use serde::{Deserialize, Serialize};

const OUTPUT_DIR_REL: &str = "target/policy";

const WORKFLOW_ALLOWLIST: &str = "policy/workflow-allowlist.toml";
const PROCESS_ALLOWLIST: &str = "policy/process-allowlist.toml";
const NETWORK_ALLOWLIST: &str = "policy/network-allowlist.toml";

/// Shared CLI mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Mode {
    Advisory,
    BlockingAllowlist,
    BlockingStrict,
}

// ─── Allowlist deserialization ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct WorkflowAllowlistDoc {
    #[serde(default)]
    workflow: Vec<RawWorkflowEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawWorkflowEntry {
    path: Option<String>,
    kind: Option<String>,
    owner: Option<String>,
    reason: Option<String>,
    process_policy: Option<String>,
    network_policy: Option<String>,
    required_repository_guard: Option<String>,
    created: Option<String>,
    review_after: Option<String>,
    expires: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProfileDoc {
    #[serde(default)]
    profile: Vec<RawProfile>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawProfile {
    name: Option<String>,
    #[serde(default)]
    allowed_processes: Vec<String>,
    #[serde(default)]
    allowed_endpoints: Vec<String>,
}

// ─── check-workflow-surfaces ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct WorkflowReport {
    tool: &'static str,
    mode: &'static str,
    today: String,
    summary: WorkflowSummary,
    findings: WorkflowFindings,
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowSummary {
    tracked_workflow_files: usize,
    allowlist_entries: usize,
    unreceipted: usize,
    missing_fields: usize,
    expired: usize,
    stale: usize,
    unused: usize,
    invalid_policy_refs: usize,
    repository_guard_violations: usize,
    authority_violations: usize,
}

#[derive(Debug, Clone, Serialize)]
struct WorkflowFindings {
    unreceipted: Vec<String>,
    missing_fields: Vec<MissingFields>,
    expired: Vec<ExpiredEntry>,
    stale: Vec<StaleEntry>,
    unused: Vec<String>,
    invalid_policy_refs: Vec<InvalidPolicyRef>,
    repository_guard_violations: Vec<RepositoryGuardViolation>,
    authority_violations: Vec<WorkflowAuthorityFinding>,
}

/// A workflow capability that needs an explicit authority decision.
///
/// 257A deliberately reports these findings before 257B makes the exception
/// ledger blocking. Keeping the finding structured here lets the enforcement
/// PR consume the same evidence without replacing the detector with a second
/// parser.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct WorkflowAuthorityFinding {
    workflow: String,
    job: String,
    step: String,
    capability: String,
    trigger: String,
    repository_boundary: String,
    remediation: String,
}

#[derive(Debug, Clone, Serialize)]
struct MissingFields {
    entry: String,
    missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ExpiredEntry {
    entry: String,
    expires: String,
    today: String,
}

#[derive(Debug, Clone, Serialize)]
struct StaleEntry {
    entry: String,
    review_after: String,
    today: String,
}

#[derive(Debug, Clone, Serialize)]
struct InvalidPolicyRef {
    workflow: String,
    policy_kind: &'static str, // "process_policy" | "network_policy"
    named: String,
    available: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RepositoryGuardViolation {
    workflow: String,
    required_repository: String,
    job: String,
    reason: String,
}

pub fn check_workflow_surfaces(mode: Mode) -> Result<()> {
    let workspace_root = workspace_root()?;
    let workflows = tracked_workflow_files(&workspace_root)?;
    let all_entries = load_workflow_allowlist(&workspace_root)?;
    // `dependabot_config` entries live in workflow-allowlist for catalog
    // purposes but are not workflow files — skip them from the workflow-
    // surface reconciliation. They still get receipt validation (missing
    // fields, expired, stale) via their own loop below.
    let entries: Vec<RawWorkflowEntry> = all_entries
        .iter()
        .filter(|e| !is_dependabot_config(e))
        .cloned()
        .collect();
    let dependabot_entries: Vec<RawWorkflowEntry> = all_entries
        .iter()
        .filter(|e| is_dependabot_config(e))
        .cloned()
        .collect();
    let process_profiles = load_profile_names(&workspace_root, PROCESS_ALLOWLIST)?;
    let network_profiles = load_profile_names(&workspace_root, NETWORK_ALLOWLIST)?;
    let today = today_iso();

    // unreceipted / unused
    let entry_paths: BTreeSet<String> = entries.iter().filter_map(|e| e.path.clone()).collect();
    let workflow_set: BTreeSet<&str> = workflows.iter().map(String::as_str).collect();

    let unreceipted: Vec<String> = workflows
        .iter()
        .filter(|p| !entry_paths.contains(p.as_str()))
        .cloned()
        .collect();
    let unused: Vec<String> = entries
        .iter()
        .filter_map(|e| {
            e.path
                .as_ref()
                .filter(|p| !workflow_set.contains(p.as_str()))
                .cloned()
        })
        .collect();

    // missing_fields, expired, stale — across ALL entries (including
    // dependabot_config catalog entries) so their receipts get validated too.
    let missing_fields: Vec<MissingFields> = all_entries
        .iter()
        .filter_map(|e| {
            let missing = missing_workflow_fields(e);
            if missing.is_empty() {
                None
            } else {
                Some(MissingFields {
                    entry: format!("workflow: {}", e.path.clone().unwrap_or_default()),
                    missing,
                })
            }
        })
        .collect();

    let expired: Vec<ExpiredEntry> = all_entries
        .iter()
        .filter_map(|e| {
            e.expires.as_ref().and_then(|exp| {
                if date_is_past(exp, &today) {
                    Some(ExpiredEntry {
                        entry: format!("workflow: {}", e.path.clone().unwrap_or_default()),
                        expires: exp.clone(),
                        today: today.clone(),
                    })
                } else {
                    None
                }
            })
        })
        .collect();

    let stale: Vec<StaleEntry> = all_entries
        .iter()
        .filter_map(|e| {
            e.review_after.as_ref().and_then(|rev| {
                if date_is_past(rev, &today) {
                    Some(StaleEntry {
                        entry: format!("workflow: {}", e.path.clone().unwrap_or_default()),
                        review_after: rev.clone(),
                        today: today.clone(),
                    })
                } else {
                    None
                }
            })
        })
        .collect();

    // invalid policy refs — checked across ALL entries; even
    // dependabot_config entries name policies.
    let mut invalid_policy_refs: Vec<InvalidPolicyRef> = Vec::new();
    for e in &all_entries {
        let label = e.path.clone().unwrap_or_default();
        if let Some(named) = &e.process_policy
            && !process_profiles.contains(named)
        {
            invalid_policy_refs.push(InvalidPolicyRef {
                workflow: label.clone(),
                policy_kind: "process_policy",
                named: named.clone(),
                available: process_profiles.iter().cloned().collect(),
            });
        }
        if let Some(named) = &e.network_policy
            && !network_profiles.contains(named)
        {
            invalid_policy_refs.push(InvalidPolicyRef {
                workflow: label.clone(),
                policy_kind: "network_policy",
                named: named.clone(),
                available: network_profiles.iter().cloned().collect(),
            });
        }
    }

    let repository_guard_violations = repository_guard_violations(&workspace_root, &entries);
    let authority_violations = workflow_authority_violations(&workspace_root, &entries);

    let findings = WorkflowFindings {
        unreceipted,
        missing_fields,
        expired,
        stale,
        unused,
        invalid_policy_refs,
        repository_guard_violations,
        authority_violations,
    };

    let _ = dependabot_entries; // tracked-but-skipped; kept for future per-kind audits.

    let summary = WorkflowSummary {
        tracked_workflow_files: workflows.len(),
        allowlist_entries: all_entries.len(),
        unreceipted: findings.unreceipted.len(),
        missing_fields: findings.missing_fields.len(),
        expired: findings.expired.len(),
        stale: findings.stale.len(),
        unused: findings.unused.len(),
        invalid_policy_refs: findings.invalid_policy_refs.len(),
        repository_guard_violations: findings.repository_guard_violations.len(),
        authority_violations: findings.authority_violations.len(),
    };

    let report = WorkflowReport {
        tool: "cargo xtask check-workflow-surfaces",
        mode: mode_str(mode),
        today,
        summary,
        findings,
    };

    write_workflow_report(&workspace_root, &report)?;
    println!(
        "{} ({}): workflows={} entries={} unreceipted={} missing_fields={} expired={} stale={} unused={} invalid_refs={} repository_guard_violations={} authority_violations={}",
        report.tool,
        report.mode,
        report.summary.tracked_workflow_files,
        report.summary.allowlist_entries,
        report.summary.unreceipted,
        report.summary.missing_fields,
        report.summary.expired,
        report.summary.stale,
        report.summary.unused,
        report.summary.invalid_policy_refs,
        report.summary.repository_guard_violations,
        report.summary.authority_violations,
    );

    let blocking = workflow_blocking_count(mode, &report.findings);
    if blocking > 0 && !matches!(mode, Mode::Advisory) {
        bail!(
            "{}: {} mode found {} blocking issue(s); see {}/workflow-policy-report.md",
            report.tool,
            report.mode,
            blocking,
            OUTPUT_DIR_REL
        );
    }
    Ok(())
}

fn missing_workflow_fields(e: &RawWorkflowEntry) -> Vec<String> {
    let mut missing = Vec::new();
    if e.path.is_none() {
        missing.push("path".to_string());
    }
    for (name, present) in [
        ("kind", e.kind.is_some()),
        ("owner", e.owner.is_some()),
        ("reason", e.reason.is_some()),
        ("process_policy", e.process_policy.is_some()),
        ("network_policy", e.network_policy.is_some()),
        ("created", e.created.is_some()),
        ("review_after", e.review_after.is_some()),
    ] {
        if !present {
            missing.push(name.to_string());
        }
    }
    missing
}

fn workflow_blocking_count(mode: Mode, f: &WorkflowFindings) -> usize {
    let mut n = f.unreceipted.len()
        + f.missing_fields.len()
        + f.expired.len()
        + f.invalid_policy_refs.len()
        + f.repository_guard_violations.len();
    if matches!(mode, Mode::BlockingStrict) {
        n += f.unused.len() + f.stale.len();
    }
    n
}

fn write_workflow_report(workspace_root: &Path, r: &WorkflowReport) -> Result<()> {
    let out_dir = workspace_root.join(OUTPUT_DIR_REL);
    fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let json = serde_json::to_string_pretty(r).context("serializing workflow report")?;
    fs::write(out_dir.join("workflow-policy-report.json"), json)
        .context("writing workflow-policy-report.json")?;
    let md = render_workflow_md(r);
    fs::write(out_dir.join("workflow-policy-report.md"), md)
        .context("writing workflow-policy-report.md")?;
    Ok(())
}

fn render_workflow_md(r: &WorkflowReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} Report\n\n", r.tool));
    out.push_str(&format!(
        "Generated by `{} --mode {}` on {}.\n\n",
        r.tool, r.mode, r.today
    ));
    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "- Tracked workflow files: {}\n",
        r.summary.tracked_workflow_files
    ));
    out.push_str(&format!(
        "- Allowlist entries: {}\n",
        r.summary.allowlist_entries
    ));
    out.push_str(&format!("- Unreceipted: {}\n", r.summary.unreceipted));
    out.push_str(&format!("- Missing fields: {}\n", r.summary.missing_fields));
    out.push_str(&format!("- Expired: {}\n", r.summary.expired));
    out.push_str(&format!("- Stale review: {}\n", r.summary.stale));
    out.push_str(&format!("- Unused: {}\n", r.summary.unused));
    out.push_str(&format!(
        "- Invalid policy refs: {}\n\n",
        r.summary.invalid_policy_refs
    ));
    out.push_str(&format!(
        "- Repository guard violations: {}\n\n",
        r.summary.repository_guard_violations
    ));
    out.push_str(&format!(
        "- Authority violations: {}\n\n",
        r.summary.authority_violations
    ));
    list_strings(&mut out, "Unreceipted workflows", &r.findings.unreceipted);
    for m in &r.findings.missing_fields {
        out.push_str(&format!(
            "- `{}`: missing {}\n",
            m.entry,
            m.missing.join(", ")
        ));
    }
    for ipr in &r.findings.invalid_policy_refs {
        out.push_str(&format!(
            "- INVALID {}: `{}` references `{}` which is not in {{{}}}\n",
            ipr.policy_kind,
            ipr.workflow,
            ipr.named,
            ipr.available.join(", ")
        ));
    }
    for guard in &r.findings.repository_guard_violations {
        out.push_str(&format!(
            "- REPOSITORY GUARD: `{}` job `{}` must be guarded to `{}` ({})\n",
            guard.workflow, guard.job, guard.required_repository, guard.reason
        ));
    }
    for finding in &r.findings.authority_violations {
        out.push_str(&format!(
            "- AUTHORITY: `{}` job `{}` step `{}` capability `{}` trigger `{}` boundary `{}`; {}\n",
            finding.workflow,
            finding.job,
            finding.step,
            finding.capability,
            finding.trigger,
            finding.repository_boundary,
            finding.remediation,
        ));
    }
    out
}

fn repository_guard_violations(
    workspace_root: &Path,
    entries: &[RawWorkflowEntry],
) -> Vec<RepositoryGuardViolation> {
    let mut violations = Vec::new();

    for entry in entries {
        let path = entry.path.clone().unwrap_or_default();
        let required_repository = match entry.required_repository_guard.as_ref() {
            Some(repo) => repo.clone(),
            None if entry.kind.as_deref() == Some("release") => {
                violations.push(RepositoryGuardViolation {
                    workflow: path,
                    required_repository: "EffortlessMetrics/shipper".to_string(),
                    job: "<allowlist>".to_string(),
                    reason: "release workflow is missing required_repository_guard in workflow allowlist"
                        .to_string(),
                });
                continue;
            }
            None => continue,
        };

        let content = read_workflow_content(workspace_root, &path).unwrap_or_default();
        let unguarded_jobs = workflow_jobs_missing_repository_guard(&content, &required_repository);
        if unguarded_jobs.is_empty() {
            continue;
        }
        for job in unguarded_jobs {
            violations.push(RepositoryGuardViolation {
                workflow: path.clone(),
                required_repository: required_repository.clone(),
                job,
                reason:
                    "job-level if does not contain the required github.repository equality guard"
                        .to_string(),
            });
        }
    }

    violations
}

#[derive(Debug, Clone)]
struct WorkflowStepBlock {
    name: String,
    content: String,
}

#[derive(Debug, Clone)]
struct PermissionScope {
    job: String,
    values: BTreeMap<String, String>,
}

/// Detect workflow authority hazards without pretending to be a general YAML
/// parser. The workflow checker intentionally works on the same small,
/// indentation-aware surface as the existing repository-guard checker. YAML
/// syntax validation remains actionlint's responsibility; this function owns
/// only authority-bearing patterns that need a stable policy signal.
fn workflow_authority_violations(
    workspace_root: &Path,
    entries: &[RawWorkflowEntry],
) -> Vec<WorkflowAuthorityFinding> {
    let mut findings = Vec::new();

    for entry in entries {
        if is_dependabot_config(entry) {
            continue;
        }
        let Some(path) = entry.path.as_deref() else {
            continue;
        };
        let Ok(content) = read_workflow_content(workspace_root, path) else {
            continue;
        };
        findings.extend(analyze_workflow_authority(
            path,
            entry.kind.as_deref().unwrap_or("unknown"),
            entry.required_repository_guard.as_deref(),
            &content,
        ));
    }

    findings
}

fn analyze_workflow_authority(
    workflow: &str,
    kind: &str,
    required_repository: Option<&str>,
    yaml_text: &str,
) -> Vec<WorkflowAuthorityFinding> {
    let triggers = workflow_trigger_names(yaml_text);
    let trigger = if triggers.is_empty() {
        "<unknown>".to_string()
    } else {
        triggers.iter().cloned().collect::<Vec<_>>().join(",")
    };
    let workflow_name = workflow_name(yaml_text).unwrap_or_else(|| workflow.to_string());
    let mut findings = Vec::new();

    if temporary_workflow_identity(workflow, &workflow_name) {
        findings.push(authority_finding(
            workflow,
            "<workflow>",
            "<workflow>",
            "temporary-workflow-identity",
            &trigger,
            "repository workflow inventory",
            "rename the durable workflow or add a narrow, owned exception with a review date",
        ));
    }

    for branch in temporary_branch_filters(yaml_text) {
        findings.push(authority_finding(
            workflow,
            "<workflow>",
            "<trigger>",
            &format!("temporary-branch-filter:{branch}"),
            &trigger,
            "repository branch trigger",
            "use a durable documented branch or record an exact bounded maintenance exception",
        ));
    }

    if triggers.contains("labeled") && has_true_cancel_in_progress(yaml_text) {
        findings.push(authority_finding(
            workflow,
            "<workflow>",
            "<concurrency>",
            "label-triggered-cancellation",
            &trigger,
            "meaningful code-change run",
            "separate label-gated work from the meaningful code-change concurrency group or disable cancellation",
        ));
    }

    let permission_scopes = workflow_permission_scopes(yaml_text);
    for scope in &permission_scopes {
        for (capability, value) in &scope.values {
            if value != "write" && value != "*" {
                continue;
            }
            let high_risk = matches!(
                capability.as_str(),
                "contents" | "id-token" | "actions" | "workflows"
            );
            let workflow_level = scope.job == "<workflow>";
            if workflow_level
                || capability == "*"
                || (high_risk
                    && (kind != "release"
                        || required_repository
                            .is_none_or(|repo| !workflow_has_repository_guard(yaml_text, repo))))
            {
                findings.push(authority_finding(
                    workflow,
                    &scope.job,
                    "<permissions>",
                    &format!("{capability}:{value}"),
                    &trigger,
                    required_repository.unwrap_or("not declared"),
                    "move the grant to the narrow job that needs it and prove the repository boundary",
                ));
            }
        }
    }

    for (job, block) in workflow_job_blocks(yaml_text) {
        let guarded =
            required_repository.is_some_and(|repo| block_has_repository_guard(&block, repo));
        let scopes = workflow_permission_scopes_for_job(&block, &job);
        let steps = workflow_step_blocks(&block);
        let release_sensitive = kind == "release"
            && (contains_release_authority(&block)
                || scopes
                    .iter()
                    .any(|scope| scope.values.get("id-token").is_some_and(|v| v == "write")));

        for step in &steps {
            for capability in self_mutation_capabilities(&step.content) {
                findings.push(authority_finding(
                    workflow,
                    &job,
                    &step.name,
                    capability,
                    &trigger,
                    required_repository.unwrap_or("not declared"),
                    "edit through a normal reviewed branch/PR; do not self-commit, self-push, or self-delete workflow source",
                ));
            }
        }

        if release_sensitive && !guarded {
            findings.push(authority_finding(
                workflow,
                &job,
                "<job>",
                "release-authority-without-repository-guard",
                &trigger,
                required_repository.unwrap_or("EffortlessMetrics/shipper"),
                "guard every release-sensitive job with the release-authority repository equality",
            ));
        }

        if triggers.contains("pull_request_target")
            && block_has_untrusted_checkout(&block)
            && (block.contains("secrets.") || scope_has_write_permission(&scopes))
        {
            findings.push(authority_finding(
                workflow,
                &job,
                "<checkout>",
                "pull-request-target-untrusted-execution",
                &trigger,
                "untrusted pull request code",
                "do not execute PR-controlled checkout with secrets or write authority; use pull_request with read-only permissions",
            ));
        }
    }

    if kind == "release"
        && triggers.contains("workflow_dispatch")
        && has_dispatch_ref_input(yaml_text)
        && !has_exact_release_identity_language(yaml_text)
    {
        findings.push(authority_finding(
            workflow,
            "<workflow>",
            "<workflow_dispatch.ref>",
            "mutable-release-dispatch-ref",
            &trigger,
            required_repository.unwrap_or("EffortlessMetrics/shipper"),
            "dispatch release work with an approved immutable SHA/tree, never a mutable branch name",
        ));
    }

    if kind == "release"
        && triggers.contains("push")
        && workflow_has_tag_trigger(yaml_text)
        && contains_release_authority(yaml_text)
        && !has_exact_release_identity_language(yaml_text)
    {
        findings.push(authority_finding(
            workflow,
            "<workflow>",
            "<tag-trigger>",
            "tag-release-without-approved-source-gate",
            &trigger,
            required_repository.unwrap_or("EffortlessMetrics/shipper"),
            "validate approved source SHA/tree, version, package graph, notes, and gates before mutation",
        ));
    }

    findings
}

fn authority_finding(
    workflow: &str,
    job: &str,
    step: &str,
    capability: &str,
    trigger: &str,
    repository_boundary: &str,
    remediation: &str,
) -> WorkflowAuthorityFinding {
    WorkflowAuthorityFinding {
        workflow: workflow.to_string(),
        job: job.to_string(),
        step: step.to_string(),
        capability: capability.to_string(),
        trigger: trigger.to_string(),
        repository_boundary: repository_boundary.to_string(),
        remediation: remediation.to_string(),
    }
}

fn workflow_name(yaml_text: &str) -> Option<String> {
    yaml_text.lines().find_map(|line| {
        let without_comment = strip_yaml_inline_comment(line);
        if without_comment.len() != without_comment.trim_start().len() {
            return None;
        }
        let trimmed = without_comment.trim();
        trimmed
            .strip_prefix("name:")
            .map(|name| name.trim().trim_matches(['\'', '"']).to_string())
    })
}

fn workflow_trigger_names(yaml_text: &str) -> BTreeSet<String> {
    let mut triggers = BTreeSet::new();
    let mut in_on = false;
    for line in yaml_text.lines() {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        if indent == 0 && trimmed.starts_with("on:") {
            in_on = true;
            if let Some(value) = trimmed.strip_prefix("on:") {
                let inline = value.trim().trim_start_matches('[').trim_end_matches(']');
                for trigger in inline.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                    triggers.insert(trigger.trim_matches(['\'', '"']).to_string());
                }
            }
            if trimmed != "on:" {
                in_on = false;
            }
            continue;
        }
        if indent == 0 && in_on {
            break;
        }
        if !in_on || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.to_ascii_lowercase().contains("labeled") {
            triggers.insert("labeled".to_string());
        }
        if indent != 2 {
            continue;
        }
        if let Some(name) = trimmed.strip_suffix(':') {
            triggers.insert(name.trim().to_string());
        } else if let Some((name, _)) = trimmed.split_once(':') {
            triggers.insert(name.trim().to_string());
        }
    }
    triggers
}

fn workflow_has_tag_trigger(yaml_text: &str) -> bool {
    let triggers = workflow_trigger_names(yaml_text);
    if !triggers.contains("push") {
        return false;
    }
    yaml_text.lines().any(|line| {
        let trimmed = strip_yaml_inline_comment(line).trim();
        trimmed == "tags:" || trimmed.starts_with("tags:")
    })
}

fn has_true_cancel_in_progress(yaml_text: &str) -> bool {
    yaml_text.lines().any(|line| {
        let trimmed = strip_yaml_inline_comment(line).trim();
        trimmed
            .strip_prefix("cancel-in-progress:")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("true"))
    })
}

fn temporary_workflow_identity(path: &str, name: &str) -> bool {
    let path_lower = path.to_ascii_lowercase();
    let stem = path_lower
        .rsplit('/')
        .next()
        .unwrap_or(&path_lower)
        .trim_end_matches(".yml");
    let path_marker = stem.starts_with("_temp")
        || stem.starts_with("temp-")
        || stem.starts_with("temporary")
        || stem.contains("one-off")
        || stem.contains("proof-pulse")
        || stem.contains("repair");
    let words: Vec<String> = name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    let named_marker = words.iter().any(|word| {
        matches!(word.as_str(), "temp" | "temporary" | "repair" | "proof-pulse")
    }) || words.windows(2).any(|pair| {
        matches!(pair, [first, second] if (first == "one" && second == "off") || (first == "proof" && second == "pulse"))
    });
    path_marker || named_marker
}

fn temporary_branch_filters(yaml_text: &str) -> Vec<String> {
    let markers = ["temp", "temporary", "repair", "proof-pulse", "one-off"];
    let mut found = BTreeSet::new();
    let lines: Vec<&str> = yaml_text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        if !trimmed.starts_with("branches:") {
            continue;
        }
        let indent = without_comment.len() - without_comment.trim_start().len();
        let inline = trimmed.strip_prefix("branches:").unwrap_or_default();
        if let Some(marker) = markers.iter().find(|marker| inline.contains(**marker)) {
            found.insert((*marker).to_string());
        }
        for branch_line in lines.iter().skip(index + 1) {
            let branch_without_comment = strip_yaml_inline_comment(branch_line);
            let branch_trimmed = branch_without_comment.trim();
            if branch_trimmed.is_empty() {
                continue;
            }
            let branch_indent =
                branch_without_comment.len() - branch_without_comment.trim_start().len();
            if branch_indent <= indent {
                break;
            }
            if let Some(marker) = markers
                .iter()
                .find(|marker| branch_trimmed.contains(**marker))
            {
                found.insert((*marker).to_string());
            }
        }
    }
    found.into_iter().collect()
}

fn workflow_permission_scopes(yaml_text: &str) -> Vec<PermissionScope> {
    let mut scopes = Vec::new();
    let lines: Vec<&str> = yaml_text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let without_comment = strip_yaml_inline_comment(lines[index]);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        if indent == 0 && trimmed.starts_with("permissions:") {
            scopes.push(PermissionScope {
                job: "<workflow>".to_string(),
                values: parse_permission_mapping(&lines, index, indent),
            });
            break;
        }
        index += 1;
    }
    for (job, block) in workflow_job_blocks(yaml_text) {
        scopes.extend(workflow_permission_scopes_for_job(&block, &job));
    }
    scopes
}

fn workflow_permission_scopes_for_job(block: &str, job: &str) -> Vec<PermissionScope> {
    let lines: Vec<&str> = block.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        if indent > 0 && trimmed.starts_with("permissions:") {
            return vec![PermissionScope {
                job: job.to_string(),
                values: parse_permission_mapping(&lines, index, indent),
            }];
        }
    }
    Vec::new()
}

fn parse_nested_mapping(
    lines: &[&str],
    start: usize,
    parent_indent: usize,
) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for line in lines.iter().skip(start + 1) {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = without_comment.len() - without_comment.trim_start().len();
        if indent <= parent_indent {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            values.insert(key.trim().to_string(), value.trim().to_ascii_lowercase());
        }
    }
    values
}

fn parse_permission_mapping(
    lines: &[&str],
    start: usize,
    parent_indent: usize,
) -> BTreeMap<String, String> {
    let declaration = strip_yaml_inline_comment(lines[start]);
    let value = declaration
        .trim()
        .strip_prefix("permissions:")
        .map(str::trim)
        .unwrap_or_default();
    if value.is_empty() {
        return parse_nested_mapping(lines, start, parent_indent);
    }

    let mut values = BTreeMap::new();
    let normalized = value.trim_matches(['{', '}']).trim();
    if normalized.eq_ignore_ascii_case("write-all") {
        values.insert("*".to_string(), "write".to_string());
    } else if normalized.eq_ignore_ascii_case("read-all") {
        values.insert("*".to_string(), "read".to_string());
    } else {
        for pair in normalized.split(',') {
            if let Some((key, permission)) = pair.split_once(':') {
                values.insert(
                    key.trim().trim_matches(['\'', '"']).to_string(),
                    permission
                        .trim()
                        .trim_matches(['\'', '"'])
                        .to_ascii_lowercase(),
                );
            }
        }
        if values.is_empty() {
            values.insert("*".to_string(), normalized.to_ascii_lowercase());
        }
    }
    values
}

fn scope_has_write_permission(scopes: &[PermissionScope]) -> bool {
    scopes.iter().any(|scope| {
        scope
            .values
            .values()
            .any(|value| value == "write" || value == "*")
    })
}

fn workflow_has_repository_guard(yaml_text: &str, repository: &str) -> bool {
    let jobs = workflow_job_blocks(yaml_text);
    !jobs.is_empty()
        && jobs
            .iter()
            .all(|(_, block)| block_has_repository_guard(block, repository))
}

fn contains_release_authority(text: &str) -> bool {
    let lower = uncommented_workflow_text(text).to_ascii_lowercase();
    lower.contains("cargo publish")
        || lower.contains("gh release")
        || lower.contains("git tag")
        || lower.contains("cosign")
        || lower.contains("cargo_registry_token")
        || lower.contains("id-token: write")
}

fn has_dispatch_ref_input(yaml_text: &str) -> bool {
    let lines: Vec<&str> = yaml_text.lines().collect();
    let mut in_on = false;
    let mut in_dispatch = false;
    let mut inputs_indent = None;
    for line in lines {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        if indent == 0 && trimmed.starts_with("on:") {
            in_on = true;
            continue;
        }
        if !in_on {
            continue;
        }
        if indent == 0 {
            break;
        }
        if indent == 2 {
            in_dispatch = trimmed.starts_with("workflow_dispatch:");
            inputs_indent = None;
            continue;
        }
        if !in_dispatch {
            continue;
        }
        if indent == 4 && trimmed.starts_with("inputs:") {
            inputs_indent = Some(indent);
            continue;
        }
        if let Some(parent_indent) = inputs_indent {
            if indent <= parent_indent {
                inputs_indent = None;
                continue;
            }
            if indent == parent_indent + 2 && trimmed.starts_with("ref:") {
                return true;
            }
        }
    }
    false
}

fn has_exact_release_identity_language(yaml_text: &str) -> bool {
    let lower = uncommented_workflow_text(yaml_text).to_ascii_lowercase();
    lower.contains("release-identity")
        || lower.contains("approved_sha")
        || lower.contains("exact approved sha")
        || lower.contains("exact-source")
}

fn workflow_step_blocks(job_block: &str) -> Vec<WorkflowStepBlock> {
    let lines: Vec<&str> = job_block.lines().collect();
    let Some((steps_index, steps_indent)) = lines.iter().enumerate().find_map(|(index, line)| {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        (trimmed == "steps:").then_some((index, indent))
    }) else {
        return Vec::new();
    };

    let mut steps = Vec::new();
    let mut current: Option<WorkflowStepBlock> = None;
    for line in lines.into_iter().skip(steps_index + 1) {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim();
        let indent = without_comment.len() - without_comment.trim_start().len();
        if !trimmed.is_empty() && indent <= steps_indent {
            break;
        }
        if indent == steps_indent + 2 && trimmed.starts_with("- ") {
            if let Some(step) = current.take() {
                steps.push(step);
            }
            let label = trimmed
                .strip_prefix("- name:")
                .map(|value| value.trim().trim_matches(['\'', '"']).to_string())
                .or_else(|| {
                    trimmed
                        .strip_prefix("- uses:")
                        .map(|value| value.trim().to_string())
                })
                .unwrap_or_else(|| "<unnamed>".to_string());
            current = Some(WorkflowStepBlock {
                name: label,
                content: String::new(),
            });
        }
        if let Some(step) = current.as_mut() {
            step.content.push_str(line);
            step.content.push('\n');
        }
    }
    if let Some(step) = current {
        steps.push(step);
    }
    steps
}

fn self_mutation_capabilities(content: &str) -> Vec<&'static str> {
    let code: String = content
        .lines()
        .map(strip_yaml_inline_comment)
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let lower = code.to_ascii_lowercase();
    let segments = shell_segments(&lower);
    let mut capabilities = Vec::new();
    for (command, capability) in [
        ("git add", "git-add"),
        ("git commit", "git-commit"),
        ("git push", "git-push"),
        ("git update-ref", "git-ref-mutation"),
        ("git tag", "git-tag-mutation"),
        ("gh api", "github-api-mutation"),
    ] {
        let mut matches = segments
            .iter()
            .filter(|segment| shell_contains_command(segment, command));
        let mut detected = matches.next().is_some();
        if command == "gh api" {
            detected = segments
                .iter()
                .filter(|segment| shell_contains_command(segment, command))
                .any(|segment| gh_api_mutates(segment));
        }
        if detected {
            capabilities.push(capability);
        }
    }
    let deletes_workflow = shell_segments(&lower).iter().any(|segment| {
        let delete_command = shell_starts_with_command(segment, "rm")
            || shell_starts_with_command(segment, "git rm")
            || shell_starts_with_command(segment, "remove-item")
            || shell_starts_with_command(segment, "del");
        delete_command && segment.contains(".github/workflows/")
    });
    if deletes_workflow {
        capabilities.push("workflow-self-deletion");
    }
    capabilities
}

fn shell_segments(text: &str) -> Vec<&str> {
    text.split(['\n', ';', '|', '&'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn shell_contains_command(text: &str, command: &str) -> bool {
    shell_segments(text)
        .iter()
        .any(|segment| shell_starts_with_command(segment, command))
}

fn shell_starts_with_command(segment: &str, command: &str) -> bool {
    let mut candidate = segment.trim();
    loop {
        let next = candidate
            .strip_prefix("if ")
            .or_else(|| candidate.strip_prefix("then "))
            .or_else(|| candidate.strip_prefix("do "))
            .or_else(|| candidate.strip_prefix("sudo "))
            .or_else(|| candidate.strip_prefix("run:"))
            .or_else(|| candidate.strip_prefix("!"))
            .or_else(|| candidate.strip_prefix("("))
            .or_else(|| candidate.strip_prefix("{"));
        let Some(next) = next else {
            break;
        };
        candidate = next.trim_start();
    }
    while let Some(first) = candidate.split_whitespace().next() {
        if !first.contains('=') || first.starts_with('=') {
            break;
        }
        let Some(offset) = candidate.find(first) else {
            break;
        };
        candidate = candidate[offset + first.len()..].trim_start();
    }
    candidate == command
        || candidate
            .strip_prefix(command)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

fn gh_api_mutates(lowercase_content: &str) -> bool {
    let Some(api_index) = lowercase_content.find("gh api") else {
        return false;
    };
    let invocation = &lowercase_content[api_index..];
    [
        "--method post",
        "--method put",
        "--method patch",
        "--method delete",
        "-x post",
        "-x put",
        "-x patch",
        "-x delete",
    ]
    .iter()
    .any(|method| invocation.contains(method))
}

fn block_has_untrusted_checkout(block: &str) -> bool {
    let lower = uncommented_workflow_text(block).to_ascii_lowercase();
    lower.contains("actions/checkout")
        && (lower.contains("github.event.pull_request")
            || lower.contains("refs/pull/")
            || lower.contains("head.sha"))
}

fn uncommented_workflow_text(text: &str) -> String {
    text.lines()
        .map(strip_yaml_inline_comment)
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn workflow_jobs_missing_repository_guard(
    yaml_text: &str,
    required_repository: &str,
) -> Vec<String> {
    workflow_job_blocks(yaml_text)
        .into_iter()
        .filter_map(|(job, block)| {
            if block_has_repository_guard(&block, required_repository) {
                None
            } else {
                Some(job)
            }
        })
        .collect()
}

fn workflow_job_blocks(yaml_text: &str) -> Vec<(String, String)> {
    let mut jobs = Vec::new();
    let mut in_jobs = false;
    let mut jobs_indent = 0usize;
    let mut current_job: Option<String> = None;
    let mut current_block = String::new();

    for line in yaml_text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            if current_job.is_some() {
                current_block.push_str(line);
                current_block.push('\n');
            }
            continue;
        }

        let indent = line.len() - trimmed.len();
        if !in_jobs {
            if trimmed == "jobs:" {
                in_jobs = true;
                jobs_indent = indent;
            }
            continue;
        }

        if indent <= jobs_indent {
            break;
        }

        let is_job_key = indent == jobs_indent + 2
            && trimmed.ends_with(':')
            && !trimmed.starts_with('-')
            && !trimmed.contains(' ');
        if is_job_key {
            if let Some(job) = current_job.take() {
                jobs.push((job, std::mem::take(&mut current_block)));
            }
            current_job = Some(trimmed.trim_end_matches(':').to_string());
            current_block.push_str(line);
            current_block.push('\n');
            continue;
        }

        if current_job.is_some() {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    if let Some(job) = current_job {
        jobs.push((job, current_block));
    }

    jobs
}

fn block_has_repository_guard(block: &str, required_repository: &str) -> bool {
    let Some(expression) = job_level_if_expression(block) else {
        return false;
    };
    let single_quoted = format!("github.repository == '{required_repository}'");
    let double_quoted = format!("github.repository == \"{required_repository}\"");
    expression.contains(&single_quoted) || expression.contains(&double_quoted)
}

fn job_level_if_expression(block: &str) -> Option<String> {
    let lines: Vec<&str> = block.lines().collect();
    let job_indent = lines.iter().find_map(|line| {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            None
        } else {
            Some(without_comment.len() - trimmed.len())
        }
    })?;
    let field_indent = job_indent + 2;

    let mut index = 0usize;
    while index < lines.len() {
        let without_comment = strip_yaml_inline_comment(lines[index]);
        let trimmed = without_comment.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }

        let indent = without_comment.len() - trimmed.len();
        if indent == field_indent && trimmed.starts_with("if:") {
            let value = trimmed.trim_start_matches("if:").trim();
            if is_yaml_block_scalar(value) || value.is_empty() {
                return Some(collect_yaml_continuation_expression(
                    &lines,
                    index + 1,
                    field_indent,
                ));
            }
            let continuation =
                collect_yaml_continuation_expression(&lines, index + 1, field_indent);
            if continuation.is_empty() {
                return Some(value.to_string());
            }
            return Some(format!("{value} {continuation}"));
        }

        index += 1;
    }

    None
}

fn is_yaml_block_scalar(value: &str) -> bool {
    matches!(value.chars().next(), Some('|' | '>'))
}

fn collect_yaml_continuation_expression(
    lines: &[&str],
    start: usize,
    parent_indent: usize,
) -> String {
    let mut expression = String::new();

    for line in &lines[start..] {
        let without_comment = strip_yaml_inline_comment(line);
        let trimmed = without_comment.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = without_comment.len() - trimmed.len();
        if indent <= parent_indent {
            break;
        }

        expression.push_str(trimmed);
        expression.push(' ');
    }

    expression
}

fn strip_yaml_inline_comment(line: &str) -> &str {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut previous_was_whitespace = true;

    for (index, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                previous_was_whitespace = false;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                previous_was_whitespace = false;
            }
            '#' if !in_single_quote && !in_double_quote && previous_was_whitespace => {
                return &line[..index];
            }
            _ => {
                previous_was_whitespace = ch.is_whitespace();
            }
        }
    }

    line
}

// ─── check-process-policy ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct PerWorkflowReport {
    workflow: String,
    declared_profile: String,
    detected: Vec<String>,
    unknown: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ScanReport {
    tool: &'static str,
    mode: &'static str,
    today: String,
    summary: ScanSummary,
    workflows: Vec<PerWorkflowReport>,
}

#[derive(Debug, Clone, Serialize)]
struct ScanSummary {
    workflows: usize,
    unknown_total: usize,
}

/// Well-known shell-command tokens we look for inside workflow contents.
/// This list is the recognition surface; commands that appear here but are
/// not in the workflow's declared process profile are flagged as "unknown".
/// Tokens outside this list are silently ignored (start-simple posture).
const KNOWN_COMMANDS: &[&str] = &[
    "cargo",
    "rustup",
    "rustc",
    "cargo-fuzz",
    "cargo-mutants",
    "cargo-llvm-cov",
    "cargo-nextest",
    "shipper",
    "gh",
    "tar",
    "sha256sum",
    "install",
    "sudo",
    "bash",
    "curl",
    "wget",
    "sh",
    "bun",
    "node",
    "npm",
    "python",
    "python3",
    "pip",
    "docker",
    "kubectl",
    "make",
    "mkdir",
    "cat",
    "jq",
];

pub fn check_process_policy(mode: Mode) -> Result<()> {
    let workspace_root = workspace_root()?;
    let entries = load_workflow_allowlist(&workspace_root)?;
    let profiles_by_name = load_profiles(&workspace_root, PROCESS_ALLOWLIST)?;
    let today = today_iso();

    let mut per_workflow = Vec::new();
    let mut unknown_total = 0usize;
    for e in &entries {
        if is_dependabot_config(e) {
            // dependabot.yml is a config file, not a script — there are no
            // shell commands to scan for.
            continue;
        }
        let path = match &e.path {
            Some(p) => p,
            None => continue,
        };
        let profile = e.process_policy.clone().unwrap_or_default();
        let allowed: BTreeSet<String> = profiles_by_name
            .get(&profile)
            .map(|p| p.allowed_processes.iter().cloned().collect())
            .unwrap_or_default();

        let content = read_workflow_content(&workspace_root, path).unwrap_or_default();
        let detected = detect_commands_in_runs(&content, KNOWN_COMMANDS);
        let unknown: Vec<String> = detected
            .iter()
            .filter(|c| !allowed.contains(c.as_str()))
            .cloned()
            .collect();
        unknown_total += unknown.len();

        per_workflow.push(PerWorkflowReport {
            workflow: path.clone(),
            declared_profile: profile,
            detected,
            unknown,
        });
    }

    let report = ScanReport {
        tool: "cargo xtask check-process-policy",
        mode: mode_str(mode),
        today,
        summary: ScanSummary {
            workflows: per_workflow.len(),
            unknown_total,
        },
        workflows: per_workflow,
    };
    write_scan_report(&workspace_root, "process-policy-report", &report)?;
    println!(
        "{} ({}): workflows={} unknown_total={}",
        report.tool, report.mode, report.summary.workflows, report.summary.unknown_total
    );

    if !matches!(mode, Mode::Advisory) && unknown_total > 0 {
        bail!(
            "{}: {} mode found {} unknown command(s) across {} workflow(s)",
            report.tool,
            report.mode,
            unknown_total,
            report.summary.workflows
        );
    }
    Ok(())
}

// ─── check-network-policy ───────────────────────────────────────────────────

pub fn check_network_policy(mode: Mode) -> Result<()> {
    let workspace_root = workspace_root()?;
    let entries = load_workflow_allowlist(&workspace_root)?;
    let profiles_by_name = load_profiles(&workspace_root, NETWORK_ALLOWLIST)?;
    let today = today_iso();
    let host_re =
        Regex::new(r"https?://([A-Za-z0-9.\-]+)").context("compiling network hostname regex")?;

    let mut per_workflow = Vec::new();
    let mut unknown_total = 0usize;
    for e in &entries {
        if is_dependabot_config(e) {
            // dependabot.yml is configuration, not a script — no URLs to scan.
            continue;
        }
        let path = match &e.path {
            Some(p) => p,
            None => continue,
        };
        let profile = e.network_policy.clone().unwrap_or_default();
        let allowed: BTreeSet<String> = profiles_by_name
            .get(&profile)
            .map(|p| p.allowed_endpoints.iter().cloned().collect())
            .unwrap_or_default();

        let content = read_workflow_content(&workspace_root, path).unwrap_or_default();
        let mut detected: BTreeSet<String> = BTreeSet::new();
        for caps in host_re.captures_iter(&content) {
            if let Some(host) = caps.get(1) {
                detected.insert(host.as_str().to_string());
            }
        }
        let detected_vec: Vec<String> = detected.into_iter().collect();
        let unknown: Vec<String> = detected_vec
            .iter()
            .filter(|h| !endpoint_covered(h, &allowed))
            .cloned()
            .collect();
        unknown_total += unknown.len();

        per_workflow.push(PerWorkflowReport {
            workflow: path.clone(),
            declared_profile: profile,
            detected: detected_vec,
            unknown,
        });
    }

    let report = ScanReport {
        tool: "cargo xtask check-network-policy",
        mode: mode_str(mode),
        today,
        summary: ScanSummary {
            workflows: per_workflow.len(),
            unknown_total,
        },
        workflows: per_workflow,
    };
    write_scan_report(&workspace_root, "network-policy-report", &report)?;
    println!(
        "{} ({}): workflows={} unknown_total={}",
        report.tool, report.mode, report.summary.workflows, report.summary.unknown_total
    );

    if !matches!(mode, Mode::Advisory) && unknown_total > 0 {
        bail!(
            "{}: {} mode found {} unknown endpoint(s) across {} workflow(s)",
            report.tool,
            report.mode,
            unknown_total,
            report.summary.workflows
        );
    }
    Ok(())
}

fn endpoint_covered(host: &str, allowed: &BTreeSet<String>) -> bool {
    // Exact match, or `host` is a subdomain of an allowed entry.
    if allowed.contains(host) {
        return true;
    }
    allowed.iter().any(|a| {
        host == a || host.ends_with(&format!(".{}", a)) || a.ends_with(&format!(".{}", host))
    })
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn tracked_workflow_files(workspace_root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .arg("ls-files")
        .arg("-z")
        .output()
        .context("running `git ls-files -z`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "`git ls-files -z` exited {}: {}",
            output.status,
            stderr.trim()
        );
    }
    let mut paths: Vec<String> = output
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .filter(|p| p.starts_with(".github/workflows/") && p.ends_with(".yml"))
        .collect();
    paths.sort();
    Ok(paths)
}

fn load_workflow_allowlist(workspace_root: &Path) -> Result<Vec<RawWorkflowEntry>> {
    let path = workspace_root.join(WORKFLOW_ALLOWLIST);
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let doc: WorkflowAllowlistDoc =
        toml::from_str(&raw).with_context(|| format!("parsing TOML in {}", path.display()))?;
    Ok(doc.workflow)
}

fn load_profile_names(workspace_root: &Path, rel: &str) -> Result<BTreeSet<String>> {
    let profiles = load_profiles(workspace_root, rel)?;
    Ok(profiles.keys().cloned().collect())
}

fn load_profiles(workspace_root: &Path, rel: &str) -> Result<BTreeMap<String, RawProfile>> {
    let path = workspace_root.join(rel);
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let doc: ProfileDoc =
        toml::from_str(&raw).with_context(|| format!("parsing TOML in {}", path.display()))?;
    let mut by_name = BTreeMap::new();
    for p in doc.profile {
        if let Some(name) = p.name.clone() {
            by_name.insert(name, p);
        }
    }
    Ok(by_name)
}

fn read_workflow_content(workspace_root: &Path, rel: &str) -> Result<String> {
    let path = workspace_root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

/// Position-aware command detection inside `run:` blocks only.
///
/// The previous implementation grep-matched the whole YAML for known
/// command tokens, which produced false positives where a command name
/// appears as a cargo build target (`-p shipper`), an action ref
/// (`taiki-e/install-action`), or in a step `name:` line. This refined
/// scanner:
///
/// 1. Walks the YAML by indentation and picks out content under
///    `run:` keys — both inline (`run: cargo build`) and block scalars
///    (`run: |` followed by indented lines).
/// 2. Splits each run-block's content by shell statement separators
///    (newline, `;`, `&&`, `||`, `|`).
/// 3. Looks at the **first word** of each segment. Only that first
///    word can be a command in shell semantics; subsequent tokens are
///    arguments.
/// 4. Strips leading redirections and environment-variable assignments
///    (`FOO=bar cmd ...` ⇒ `cmd`).
///
/// `cargo build -p shipper` now flags `cargo` and nothing else.
/// `sudo apt-get install -y gcc` flags `sudo`.
/// `mkdir -p /tmp/x` flags `mkdir`.
fn detect_commands_in_runs(yaml_text: &str, vocabulary: &[&str]) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    let vocab: BTreeSet<&str> = vocabulary.iter().copied().collect();

    let mut buffer = String::new();
    let mut in_run_block = false;
    let mut run_indent: usize = 0;

    for line in yaml_text.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();

        // If we're inside a run block and the next line's indentation
        // returns to or below the run's `run:` key indent, the block ends.
        if in_run_block && !trimmed.is_empty() && indent <= run_indent {
            scan_run_content(&buffer, &vocab, &mut found);
            buffer.clear();
            in_run_block = false;
        }

        if let Some(rest) = trimmed.strip_prefix("run:") {
            // Flush any prior unterminated block (defensive).
            if in_run_block {
                scan_run_content(&buffer, &vocab, &mut found);
                buffer.clear();
            }
            in_run_block = true;
            run_indent = indent;
            let value = rest.trim();
            // Block-scalar markers: `|`, `>`, `|-`, `>-`, `|+`, `>+`.
            if !value.is_empty() && !matches!(value, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
                buffer.push_str(value);
                buffer.push('\n');
            }
            continue;
        }

        if in_run_block {
            // Skip blank lines (don't end the block; YAML allows them inside).
            if !trimmed.is_empty() {
                buffer.push_str(trimmed);
                buffer.push('\n');
            }
        }
    }
    if in_run_block && !buffer.is_empty() {
        scan_run_content(&buffer, &vocab, &mut found);
    }

    found.into_iter().collect()
}

fn scan_run_content(content: &str, vocab: &BTreeSet<&str>, found: &mut BTreeSet<String>) {
    // Split by shell separators. We don't try to honor quoted strings;
    // false-negatives there are acceptable (an attacker hiding a command
    // inside quoted strings would also need to break out of them, and the
    // policy stack assumes review).
    let separators: &[char] = &['\n', ';', '|', '&'];
    for raw_segment in content.split(separators) {
        let segment = raw_segment.trim();
        if segment.is_empty() {
            continue;
        }
        // Drop leading shell glue and env-var assignments.
        let mut tokens = segment.split_whitespace();
        let mut first = loop {
            match tokens.next() {
                Some(t) if t.contains('=') && !t.starts_with('=') => continue, // FOO=bar
                Some(t) if t == "\\" || t == "&&" || t == "||" => continue,
                Some(t) => break Some(t),
                None => break None,
            }
        };
        // Strip leading `(`, `!`, etc.
        while let Some(t) = first {
            let stripped = t.trim_start_matches(['(', '{', '!', ' ']);
            if stripped != t {
                first = Some(stripped);
                continue;
            }
            break;
        }
        if let Some(t) = first
            && vocab.contains(t)
        {
            found.insert(t.to_string());
        }
    }
}

fn is_dependabot_config(e: &RawWorkflowEntry) -> bool {
    e.kind.as_deref() == Some("dependabot_config")
}

fn write_scan_report(workspace_root: &Path, basename: &str, r: &ScanReport) -> Result<()> {
    let out_dir = workspace_root.join(OUTPUT_DIR_REL);
    fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let json = serde_json::to_string_pretty(r).context("serializing scan report")?;
    fs::write(out_dir.join(format!("{basename}.json")), json).context("writing scan JSON")?;
    fs::write(out_dir.join(format!("{basename}.md")), render_scan_md(r))
        .context("writing scan MD")?;
    Ok(())
}

fn render_scan_md(r: &ScanReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {} Report\n\n", r.tool));
    out.push_str(&format!(
        "Generated by `{} --mode {}` on {}.\n\n",
        r.tool, r.mode, r.today
    ));
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Workflows scanned: {}\n", r.summary.workflows));
    out.push_str(&format!(
        "- Unknown commands/endpoints total: {}\n\n",
        r.summary.unknown_total
    ));
    out.push_str("## Per-workflow\n\n");
    for w in &r.workflows {
        out.push_str(&format!(
            "### `{}` (profile: `{}`)\n\n",
            w.workflow, w.declared_profile
        ));
        out.push_str(&format!("- Detected: {}\n", join_or_none(&w.detected)));
        if w.unknown.is_empty() {
            out.push_str("- Unknown: _(none)_\n\n");
        } else {
            out.push_str(&format!("- **Unknown**: {}\n\n", w.unknown.join(", ")));
        }
    }
    out
}

fn join_or_none(v: &[String]) -> String {
    if v.is_empty() {
        "_(none)_".to_string()
    } else {
        v.join(", ")
    }
}

fn list_strings(out: &mut String, title: &str, items: &[String]) {
    out.push_str(&format!("## {} ({})\n\n", title, items.len()));
    if items.is_empty() {
        out.push_str("_(none)_\n\n");
    } else {
        for s in items {
            out.push_str(&format!("- `{s}`\n"));
        }
        out.push('\n');
    }
}

fn mode_str(mode: Mode) -> &'static str {
    match mode {
        Mode::Advisory => "advisory",
        Mode::BlockingAllowlist => "blocking-allowlist",
        Mode::BlockingStrict => "blocking-strict",
    }
}

fn date_is_past(date: &str, today: &str) -> bool {
    let parsed = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok();
    let today_parsed = NaiveDate::parse_from_str(today, "%Y-%m-%d").ok();
    match (parsed, today_parsed) {
        (Some(d), Some(t)) => d < t,
        _ => date.trim() < today,
    }
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .context("CARGO_MANIFEST_DIR not set; run via `cargo xtask`")?;
    let xtask_dir = PathBuf::from(manifest_dir);
    let root = xtask_dir
        .parent()
        .with_context(|| format!("xtask manifest dir has no parent: {}", xtask_dir.display()))?
        .to_path_buf();
    Ok(root)
}

fn today_iso() -> String {
    chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_guard_scanner_accepts_guarded_jobs() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: github.repository == 'EffortlessMetrics/shipper' && github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
  rehearse:
    if: github.repository == "EffortlessMetrics/shipper" && github.event_name == 'workflow_dispatch'
    runs-on: ubuntu-latest
    steps:
      - run: cargo xtask policy-report
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert!(missing.is_empty());
    }

    #[test]
    fn repository_guard_scanner_reports_unguarded_jobs() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
  create-release:
    runs-on: ubuntu-latest
    steps:
      - run: gh release create
  rehearse:
    if: github.repository == 'EffortlessMetrics/shipper' && github.event_name == 'workflow_dispatch'
    runs-on: ubuntu-latest
    steps:
      - run: cargo xtask policy-report
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert_eq!(missing, vec!["publish", "create-release"]);
    }

    #[test]
    fn repository_guard_scanner_ignores_inline_comment_bypass() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: github.repository == 'EffortlessMetrics/shipper-swarm' # github.repository == 'EffortlessMetrics/shipper'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
  rehearse:
    if: github.repository == 'EffortlessMetrics/shipper'
    runs-on: ubuntu-latest
    steps:
      - run: cargo xtask policy-report
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert_eq!(missing, vec!["publish"]);
    }

    #[test]
    fn repository_guard_scanner_accepts_multiline_job_if() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: >
      github.repository == 'EffortlessMetrics/shipper'
      && github.event_name == 'push'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert!(missing.is_empty());
    }

    #[test]
    fn repository_guard_scanner_ignores_multiline_comment_bypass() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: >
      github.repository == 'EffortlessMetrics/shipper-swarm'
      # github.repository == 'EffortlessMetrics/shipper'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert_eq!(missing, vec!["publish"]);
    }

    #[test]
    fn repository_guard_scanner_accepts_plain_multiline_job_if() {
        let yaml = r#"
name: Release

jobs:
  publish:
    if: github.event_name == 'push'
      && github.repository == 'EffortlessMetrics/shipper'
    runs-on: ubuntu-latest
    steps:
      - run: cargo publish
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert!(missing.is_empty());
    }

    #[test]
    fn repository_guard_scanner_ignores_step_level_if() {
        let yaml = r#"
name: Release

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - if: github.repository == 'EffortlessMetrics/shipper'
        run: cargo publish
"#;

        let missing = workflow_jobs_missing_repository_guard(yaml, "EffortlessMetrics/shipper");

        assert_eq!(missing, vec!["publish"]);
    }

    #[test]
    fn authority_detector_rejects_temporary_self_mutating_workflow() {
        let yaml = r#"
name: Proof Pulse Repair

on:
  push:
    branches: ["repair/one-off"]

jobs:
  repair:
    permissions:
      contents: write
    steps:
      - name: Commit repair
        run: |
          git add .github/workflows/_temp-fix.yml
          git commit -m repair
          git push origin repair/one-off
      - name: Delete itself
        run: rm .github/workflows/_temp-fix.yml
"#;

        let findings = analyze_workflow_authority(
            ".github/workflows/_temp-fix.yml",
            "maintenance",
            None,
            yaml,
        );

        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "temporary-workflow-identity")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "temporary-branch-filter:repair")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "git-add")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "git-commit")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "git-push")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "workflow-self-deletion")
        );
    }

    #[test]
    fn authority_detector_rejects_workflow_level_write_permissions() {
        let yaml = r#"
name: Mixed CI

on:
  push:
    branches: [main]

permissions:
  contents: write
  id-token: write
  pull-requests: write

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: cargo test
"#;

        let findings = analyze_workflow_authority(".github/workflows/ci.yml", "ci", None, yaml);

        assert!(findings.iter().any(|finding| {
            finding.job == "<workflow>" && finding.capability == "contents:write"
        }));
        assert!(findings.iter().any(|finding| {
            finding.job == "<workflow>" && finding.capability == "id-token:write"
        }));
        assert!(findings.iter().any(|finding| {
            finding.job == "<workflow>" && finding.capability == "pull-requests:write"
        }));
    }

    #[test]
    fn authority_detector_accepts_guarded_release_job_local_permissions() {
        let yaml = r#"
name: Release

on:
  push:
    tags: ["v*.*.*"]
  workflow_dispatch:
    inputs:
      ref:
        description: Exact approved SHA to check out.

permissions:
  contents: read

jobs:
  identity:
    if: github.repository == 'EffortlessMetrics/shipper'
    permissions:
      contents: read
    steps:
      - name: Validate release identity
        run: cargo xtask release-identity --approved_sha "$SHA"
  publish:
    if: github.repository == 'EffortlessMetrics/shipper'
    permissions:
      contents: read
      id-token: write
    steps:
      - name: Publish
        run: cargo publish
"#;

        let findings = analyze_workflow_authority(
            ".github/workflows/release.yml",
            "release",
            Some("EffortlessMetrics/shipper"),
            yaml,
        );

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn authority_detector_rejects_untrusted_pull_request_target_execution() {
        let yaml = r#"
name: Unsafe target workflow

on:
  pull_request_target:
    types: [opened]

jobs:
  test:
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v7
        with:
          ref: ${{ github.event.pull_request.head.sha }}
      - name: Run PR code
        env:
          SECRET: ${{ secrets.SECRET }}
        run: cargo test
"#;

        let findings = analyze_workflow_authority(".github/workflows/unsafe.yml", "ci", None, yaml);

        assert!(
            findings
                .iter()
                .any(|finding| { finding.capability == "pull-request-target-untrusted-execution" })
        );
    }

    #[test]
    fn authority_detector_rejects_label_cancellation_and_mutable_release_ref() {
        let labeled = r#"
name: Label gate

on:
  pull_request:
    types: [opened, labeled]

concurrency:
  group: code-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    steps:
      - run: cargo test
"#;
        let labeled_findings =
            analyze_workflow_authority(".github/workflows/label.yml", "ci", None, labeled);
        assert!(
            labeled_findings
                .iter()
                .any(|finding| { finding.capability == "label-triggered-cancellation" })
        );

        let release = r#"
name: Release

on:
  push:
    tags: ["v*.*.*"]
  workflow_dispatch:
    inputs:
      ref:
        default: main

jobs:
  publish:
    if: github.repository == 'EffortlessMetrics/shipper'
    permissions:
      id-token: write
    steps:
      - run: cargo publish
"#;
        let release_findings = analyze_workflow_authority(
            ".github/workflows/release.yml",
            "release",
            Some("EffortlessMetrics/shipper"),
            release,
        );
        assert!(
            release_findings
                .iter()
                .any(|finding| { finding.capability == "mutable-release-dispatch-ref" })
        );
        assert!(
            release_findings.iter().any(|finding| {
                finding.capability == "tag-release-without-approved-source-gate"
            })
        );
    }

    #[test]
    fn authority_detector_handles_block_branch_filters_and_dispatch_scope() {
        let yaml = r#"
name: Repair gate

on:
  push:
    branches:
      - repair/one-off
  workflow_dispatch:
    inputs:
      target:
        description: "A ref-like target, not the release ref input"
      ref:
        description: Exact approved SHA.
"#;

        let findings =
            analyze_workflow_authority(".github/workflows/repair.yml", "maintenance", None, yaml);

        assert!(
            findings
                .iter()
                .any(|finding| { finding.capability == "temporary-branch-filter:repair" })
        );
        assert!(has_dispatch_ref_input(yaml));
        assert!(!has_dispatch_ref_input(
            "on:\n  workflow_dispatch:\n    inputs:\n      target:\n        default: main\nenv:\n  ref: unrelated\n"
        ));
    }

    #[test]
    fn authority_detector_bounds_each_gh_api_invocation() {
        let yaml = r#"
name: Release inspection

on:
  workflow_dispatch:

jobs:
  inspect:
    steps:
      - name: Read and mutate separate refs
        run: |
          gh api repos/EffortlessMetrics/shipper/git/ref/heads/main --method get
          gh api repos/EffortlessMetrics/shipper/git/ref/heads/main --method POST
"#;

        let findings =
            analyze_workflow_authority(".github/workflows/inspect.yml", "ci", None, yaml);

        assert!(
            findings
                .iter()
                .any(|finding| finding.capability == "github-api-mutation")
        );
    }

    #[test]
    fn authority_detector_models_scalar_permissions() {
        let write_all = r#"
name: Broad permissions

on: workflow_dispatch

permissions: write-all

jobs:
  check:
    steps:
      - run: cargo test
"#;
        let read_all = write_all.replace("write-all", "read-all");

        let write_findings =
            analyze_workflow_authority(".github/workflows/broad.yml", "ci", None, write_all);
        assert!(
            write_findings
                .iter()
                .any(|finding| { finding.job == "<workflow>" && finding.capability == "*:write" })
        );

        let read_findings =
            analyze_workflow_authority(".github/workflows/read.yml", "ci", None, &read_all);
        assert!(
            !read_findings
                .iter()
                .any(|finding| finding.capability == "*:read")
        );
    }

    #[test]
    fn repository_guard_check_rejects_empty_workflow() {
        assert!(!workflow_has_repository_guard(
            "name: Empty\n\npermissions: read-all\n",
            "EffortlessMetrics/shipper"
        ));
    }

    #[test]
    fn authority_detector_ignores_read_only_reconciliation_wording() {
        let yaml = r#"
name: Registry remediation report

on:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  report:
    steps:
      - name: Explain remediation
        run: echo "repair guidance"
"#;

        let findings = analyze_workflow_authority(".github/workflows/report.yml", "ci", None, yaml);

        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn authority_detector_does_not_misclassify_read_only_gh_api() {
        let yaml = r#"
name: Release

on:
  workflow_dispatch:

jobs:
  identity:
    if: github.repository == 'EffortlessMetrics/shipper'
    steps:
      - name: Read ref
        run: gh api repos/EffortlessMetrics/shipper/git/ref/heads/main
      - name: Explain the command
        run: echo "git push origin main"
"#;

        let findings = analyze_workflow_authority(
            ".github/workflows/release.yml",
            "release",
            Some("EffortlessMetrics/shipper"),
            yaml,
        );

        assert!(
            !findings
                .iter()
                .any(|finding| finding.capability == "github-api-mutation")
        );
        assert!(
            !findings
                .iter()
                .any(|finding| finding.capability == "git-push")
        );
    }
}
