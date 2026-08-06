//! Validate the exact, owned workflow-authority exception ledger.
//!
//! This binary is the schema/expiry ratchet for #267. The follow-up wiring
//! makes `check-workflow-surfaces --mode blocking-allowlist` reconcile these
//! exact identities against the detector's emitted findings.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use serde::Deserialize;

const LEDGER: &str = "policy/workflow-authority-exceptions.toml";
const EXPECTED_SCHEMA: &str = "1.0";
const EXPECTED_POLICY: &str = "workflow-authority-exceptions";

#[derive(Debug, Deserialize)]
struct AuthorityExceptionDoc {
    schema_version: String,
    policy: String,
    owner: String,
    status: String,
    #[serde(default)]
    authority_exception: Vec<AuthorityException>,
}

#[derive(Debug, Clone, Deserialize)]
struct AuthorityException {
    workflow: String,
    job: String,
    step: String,
    capability: String,
    trigger: String,
    repository: String,
    finding_repository_boundary: String,
    owner: String,
    reason: String,
    covered_by: String,
    created: String,
    review_after: String,
}

impl AuthorityException {
    fn identity(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.workflow,
            self.job,
            self.step,
            self.capability,
            self.trigger,
            self.finding_repository_boundary
        )
    }
}

fn main() -> Result<()> {
    let root = workspace_root()?;
    let path = root.join(LEDGER);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("reading workflow authority ledger {}", path.display()))?;
    let doc: AuthorityExceptionDoc = toml::from_str(&text)
        .with_context(|| format!("parsing workflow authority ledger {}", path.display()))?;
    let today = chrono::Utc::now().date_naive();
    let count = validate_doc(&doc, today, |workflow| root.join(workflow).is_file())?;
    println!(
        "workflow authority exceptions: schema={} entries={} invalid=0 expired=0 duplicates=0",
        doc.schema_version, count
    );
    Ok(())
}

fn workspace_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir().context("reading current directory")?;
    loop {
        if current.join("Cargo.toml").is_file() && current.join("xtask").is_dir() {
            return Ok(current);
        }
        if !current.pop() {
            bail!("could not locate the shipper workspace root");
        }
    }
}

fn validate_doc<F>(
    doc: &AuthorityExceptionDoc,
    today: NaiveDate,
    workflow_exists: F,
) -> Result<usize>
where
    F: Fn(&str) -> bool,
{
    if doc.schema_version != EXPECTED_SCHEMA {
        bail!(
            "unsupported authority-exception schema {}; expected {EXPECTED_SCHEMA}",
            doc.schema_version
        );
    }
    if doc.policy != EXPECTED_POLICY {
        bail!(
            "unexpected authority-exception policy {}; expected {EXPECTED_POLICY}",
            doc.policy
        );
    }
    require_text("ledger owner", &doc.owner, 3)?;
    if doc.status != "active" {
        bail!("authority-exception ledger status must be active");
    }

    let mut identities = BTreeSet::new();
    for entry in &doc.authority_exception {
        validate_exception(entry, today, &workflow_exists)?;
        let identity = entry.identity();
        if !identities.insert(identity.clone()) {
            bail!("duplicate workflow authority exception: {identity}");
        }
    }
    Ok(doc.authority_exception.len())
}

fn validate_exception<F>(
    entry: &AuthorityException,
    today: NaiveDate,
    workflow_exists: &F,
) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    for (label, value, minimum) in [
        ("workflow", entry.workflow.as_str(), 1),
        ("job", entry.job.as_str(), 1),
        ("step", entry.step.as_str(), 1),
        ("capability", entry.capability.as_str(), 3),
        ("trigger", entry.trigger.as_str(), 1),
        ("repository", entry.repository.as_str(), 3),
        (
            "finding_repository_boundary",
            entry.finding_repository_boundary.as_str(),
            3,
        ),
        ("owner", entry.owner.as_str(), 3),
        ("reason", entry.reason.as_str(), 40),
        ("covered_by", entry.covered_by.as_str(), 40),
    ] {
        require_text(label, value, minimum)?;
    }

    if !entry.workflow.starts_with(".github/workflows/")
        || !entry.workflow.ends_with(".yml")
    {
        bail!(
            "authority exception workflow must name one exact .github/workflows/*.yml path: {}",
            entry.workflow
        );
    }
    if entry.workflow.contains(['*', '?', '[', ']']) {
        bail!(
            "authority exception workflow may not contain a glob: {}",
            entry.workflow
        );
    }
    if !workflow_exists(&entry.workflow) {
        bail!(
            "authority exception references a missing workflow: {}",
            entry.workflow
        );
    }
    if entry.capability.contains('*') {
        bail!(
            "authority exception capability must be exact, not wildcarded: {}",
            entry.capability
        );
    }
    if !entry.repository.starts_with("EffortlessMetrics/") {
        bail!(
            "authority exception repository must name the exact EffortlessMetrics repository: {}",
            entry.repository
        );
    }

    let trigger_parts = entry.trigger.split(',').collect::<Vec<_>>();
    if trigger_parts.iter().any(|part| part.trim().is_empty()) {
        bail!("authority exception trigger contains an empty token");
    }
    let mut sorted_triggers = trigger_parts.clone();
    sorted_triggers.sort_unstable();
    sorted_triggers.dedup();
    if trigger_parts != sorted_triggers {
        bail!(
            "authority exception trigger must use sorted unique detector tokens: {}",
            entry.trigger
        );
    }

    let created = parse_date("created", &entry.created)?;
    let review_after = parse_date("review_after", &entry.review_after)?;
    if review_after <= created {
        bail!(
            "authority exception review_after {} must be after created {}",
            entry.review_after,
            entry.created
        );
    }
    if review_after < today {
        bail!(
            "authority exception expired on {}; today is {today}",
            entry.review_after
        );
    }

    for value in [
        &entry.reason,
        &entry.covered_by,
        &entry.finding_repository_boundary,
    ] {
        if contains_secret_material(value) {
            bail!("authority exception contains secret-like material");
        }
    }

    Ok(())
}

fn require_text(label: &str, value: &str, minimum: usize) -> Result<()> {
    if value.trim().len() < minimum {
        bail!("authority exception {label} must contain at least {minimum} characters");
    }
    Ok(())
}

fn parse_date(label: &str, value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .with_context(|| format!("authority exception {label} must be YYYY-MM-DD: {value}"))
}

fn contains_secret_material(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "github_pat_",
        "ghp_",
        "authorization: bearer",
        "cookie:",
        "token=",
        "password=",
        "passphrase=",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: &str = "2026-08-06";

    fn valid_doc() -> AuthorityExceptionDoc {
        AuthorityExceptionDoc {
            schema_version: EXPECTED_SCHEMA.to_string(),
            policy: EXPECTED_POLICY.to_string(),
            owner: "release/ci".to_string(),
            status: "active".to_string(),
            authority_exception: vec![AuthorityException {
                workflow: ".github/workflows/droid-security-scan.yml".to_string(),
                job: "droid-security-scan".to_string(),
                step: "<permissions>".to_string(),
                capability: "contents:write".to_string(),
                trigger: "schedule,workflow_dispatch".to_string(),
                repository: "EffortlessMetrics/shipper-swarm".to_string(),
                finding_repository_boundary: "not declared".to_string(),
                owner: "release/ci".to_string(),
                reason: "A durable security-report branch requires exact repository write authority."
                    .to_string(),
                covered_by: "Scheduled/manual triggers, fixed action SHA, trusted runner, and review."
                    .to_string(),
                created: "2026-08-06".to_string(),
                review_after: "2026-11-06".to_string(),
            }],
        }
    }

    fn today() -> NaiveDate {
        NaiveDate::parse_from_str(TODAY, "%Y-%m-%d").expect("valid date")
    }

    #[test]
    fn accepts_one_exact_current_exception() {
        assert_eq!(validate_doc(&valid_doc(), today(), |_| true).expect("valid"), 1);
    }

    #[test]
    fn rejects_duplicate_exception_identity() {
        let mut doc = valid_doc();
        doc.authority_exception.push(doc.authority_exception[0].clone());
        let error = validate_doc(&doc, today(), |_| true).expect_err("duplicate must fail");
        assert!(error.to_string().contains("duplicate"), "{error:#}");
    }

    #[test]
    fn rejects_globbed_workflow_authority() {
        let mut doc = valid_doc();
        doc.authority_exception[0].workflow = ".github/workflows/*.yml".to_string();
        let error = validate_doc(&doc, today(), |_| true).expect_err("glob must fail");
        assert!(error.to_string().contains("glob"), "{error:#}");
    }

    #[test]
    fn rejects_expired_exception() {
        let mut doc = valid_doc();
        doc.authority_exception[0].review_after = "2026-08-05".to_string();
        let error = validate_doc(&doc, today(), |_| true).expect_err("expiry must fail");
        assert!(error.to_string().contains("expired"), "{error:#}");
    }

    #[test]
    fn rejects_secret_like_exception_text() {
        let mut doc = valid_doc();
        doc.authority_exception[0].reason =
            "A long enough explanation that accidentally contains token=secret".to_string();
        let error = validate_doc(&doc, today(), |_| true).expect_err("secret must fail");
        assert!(error.to_string().contains("secret-like"), "{error:#}");
    }
}
