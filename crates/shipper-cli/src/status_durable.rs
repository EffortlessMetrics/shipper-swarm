//! Durable, read-only status adapter over authoritative local evidence.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::output::outcome::{ActionKind, OperatorAction};
use shipper_core::cli_bridge::{RunLiveness, RunObservation, observe_run};
use shipper_core::state::consistency::verify_finalization_consistency;
use shipper_core::state::events::{EVENTS_FILE, events_path};
use shipper_core::state::execution_state::{
    RECEIPT_FILE, RECONCILIATION_FILE, STATE_FILE, load_receipt, load_receipt_encrypted,
    load_state, load_state_encrypted, reconciliation_path,
};
use shipper_core::types::{
    ErrorClass, ExecutionResult, ExecutionState, PackageState, Receipt, ReconciliationOutcome,
    ReconciliationReport, Registry, ReleasePlan,
};

const SCHEMA_VERSION: &str = "shipper.status.durable.v1";

#[derive(Debug, Serialize)]
pub(crate) struct DurableStatusReport {
    schema_version: &'static str,
    workspace_root: String,
    state_dir: String,
    outcome: DurableOperatorOutcome,
}

#[derive(Debug, Clone, Serialize)]
struct DurableOperatorOutcome {
    status: DurableStatus,
    publication_performed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    execution_result: Option<ExecutionResult>,
    safe_to_resume: SafeResumePosture,
    next_action: OperatorAction,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DurableStatus {
    NoEvidence,
    Terminal,
    Interrupted,
    Ambiguous,
    IdentityMismatch,
    Live,
    Unknown,
    EvidenceDisagreement,
}

impl DurableStatus {
    fn human_label(self) -> &'static str {
        match self {
            Self::NoEvidence => "no durable evidence",
            Self::Terminal => "terminal",
            Self::Interrupted => "interrupted",
            Self::Ambiguous => "ambiguous",
            Self::IdentityMismatch => "identity mismatch",
            Self::Live => "live",
            Self::Unknown => "unknown or possibly interrupted",
            Self::EvidenceDisagreement => "evidence disagreement",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SafeResumePosture {
    value: Option<bool>,
    reason: String,
}

struct EvidencePacket {
    observation: RunObservation,
    state: Option<ExecutionState>,
    receipt: Option<Receipt>,
    reconciliation: Option<ReconciliationReport>,
    finalization_consistent: Option<bool>,
    evidence: Vec<String>,
}

pub(crate) fn run(
    plan: &ReleasePlan,
    workspace_root: &Path,
    configured_state_dir: &Path,
    resolved_state_dir: &Path,
    encryption: &shipper_core::encryption::EncryptionConfig,
    format: &str,
) -> Result<()> {
    let packet = load_evidence(
        resolved_state_dir,
        workspace_root,
        configured_state_dir,
        encryption,
    )?;
    let outcome = classify(plan, configured_state_dir, packet);
    let report = DurableStatusReport {
        schema_version: SCHEMA_VERSION,
        workspace_root: workspace_root.display().to_string(),
        state_dir: configured_state_dir.display().to_string(),
        outcome,
    };
    write_report(&report, format)
}

fn load_evidence(
    resolved_state_dir: &Path,
    workspace_root: &Path,
    configured_state_dir: &Path,
    encryption: &shipper_core::encryption::EncryptionConfig,
) -> Result<EvidencePacket> {
    let observation = observe_run(resolved_state_dir, Some(workspace_root)).with_context(|| {
        format!(
            "failed to observe durable run at {}",
            resolved_state_dir.display()
        )
    })?;
    let (state, receipt) = if encryption.enabled {
        (
            load_state_encrypted(resolved_state_dir, encryption)?,
            load_receipt_encrypted(resolved_state_dir, encryption)?,
        )
    } else {
        (
            load_state(resolved_state_dir)?,
            load_receipt(resolved_state_dir)?,
        )
    };
    let reconciliation = load_reconciliation(resolved_state_dir)?;
    let finalization_consistent = match (&state, &receipt) {
        (Some(state), Some(receipt)) => Some(
            verify_finalization_consistency(
                &events_path(resolved_state_dir),
                state,
                receipt,
                reconciliation.as_ref(),
            )
            .is_ok(),
        ),
        _ => None,
    };
    let evidence = [
        (events_path(resolved_state_dir), EVENTS_FILE),
        (resolved_state_dir.join(STATE_FILE), STATE_FILE),
        (resolved_state_dir.join(RECEIPT_FILE), RECEIPT_FILE),
        (reconciliation_path(resolved_state_dir), RECONCILIATION_FILE),
    ]
    .into_iter()
    .filter(|(resolved, _)| resolved.exists())
    .map(|(_, file)| configured_state_dir.join(file).display().to_string())
    .collect();

    Ok(EvidencePacket {
        observation,
        state,
        receipt,
        reconciliation,
        finalization_consistent,
        evidence,
    })
}

fn load_reconciliation(state_dir: &Path) -> Result<Option<ReconciliationReport>> {
    let path = reconciliation_path(state_dir);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read reconciliation report {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse reconciliation report {}", path.display()))
        .map(Some)
}

fn classify(
    plan: &ReleasePlan,
    configured_state_dir: &Path,
    packet: EvidencePacket,
) -> DurableOperatorOutcome {
    let plan_id = observation_plan_id(&packet.observation).map(str::to_owned);
    if is_no_evidence(&packet) {
        return outcome(
            DurableStatus::NoEvidence,
            None,
            None,
            unknown_safety("no durable run evidence exists"),
            OperatorAction::posture(
                ActionKind::Status,
                "no durable run exists to interpret; run plan or inspect the configured state directory",
            ),
            packet.evidence,
        );
    }

    if identities_disagree(plan, &packet) {
        return outcome(
            DurableStatus::IdentityMismatch,
            plan_id,
            observation_result(&packet.observation).cloned(),
            unsafe_safety("source, plan, registry, or durable evidence identity does not match"),
            OperatorAction::posture(
                ActionKind::StopAndInvestigate,
                "durable evidence identity differs from the current workspace plan or registry; do not resume",
            ),
            packet.evidence,
        );
    }

    let observation = packet.observation.clone();
    match observation {
        RunObservation::NoEvidence => outcome(
            DurableStatus::EvidenceDisagreement,
            None,
            None,
            unsafe_safety(
                "state, receipt, or reconciliation exists without authoritative run events",
            ),
            investigate_action(),
            packet.evidence,
        ),
        RunObservation::Finished { plan_id, result } => {
            classify_finished(plan_id, &result, configured_state_dir, packet)
        }
        RunObservation::Unfinished { plan_id, liveness } => {
            classify_unfinished(plan_id, liveness, configured_state_dir, packet)
        }
    }
}

fn classify_finished(
    plan_id: Option<String>,
    result: &ExecutionResult,
    configured_state_dir: &Path,
    packet: EvidencePacket,
) -> DurableOperatorOutcome {
    let (Some(state), Some(receipt)) = (packet.state.as_ref(), packet.receipt.as_ref()) else {
        return disagreement(plan_id, Some(result.clone()), packet.evidence);
    };
    if receipt.execution_result != *result || packet.finalization_consistent != Some(true) {
        return disagreement(plan_id, Some(result.clone()), packet.evidence);
    }
    if contains_ambiguity(&packet) {
        return ambiguous(plan_id, Some(result.clone()), packet.evidence);
    }

    let posture = package_posture(state.packages.values().map(|package| &package.state));
    let (safe_to_resume, next_action) = action_for_posture(posture, configured_state_dir, true);
    outcome(
        DurableStatus::Terminal,
        plan_id,
        Some(result.clone()),
        safe_to_resume,
        next_action,
        packet.evidence,
    )
}

fn classify_unfinished(
    plan_id: Option<String>,
    liveness: RunLiveness,
    configured_state_dir: &Path,
    packet: EvidencePacket,
) -> DurableOperatorOutcome {
    if packet.receipt.is_some() || packet.state.is_none() {
        return disagreement(plan_id, None, packet.evidence);
    }
    if contains_ambiguity(&packet) {
        return ambiguous(plan_id, None, packet.evidence);
    }
    match liveness {
        RunLiveness::Live => outcome(
            DurableStatus::Live,
            plan_id,
            None,
            unsafe_safety("the exact local publisher process identity is still live"),
            OperatorAction::posture(
                ActionKind::Status,
                "the exact publisher identity is live; continue read-only observation and do not resume",
            ),
            packet.evidence,
        ),
        RunLiveness::Unknown(_) => outcome(
            DurableStatus::Unknown,
            plan_id,
            None,
            unknown_safety(
                "liveness cannot be established from the available authoritative evidence",
            ),
            OperatorAction::posture(
                ActionKind::InspectEvents,
                "the run may still be active or interrupted; inspect events and do not manufacture a resume command",
            ),
            packet.evidence,
        ),
        RunLiveness::NotLive => {
            let Some(state) = packet.state.as_ref() else {
                return disagreement(plan_id, None, packet.evidence);
            };
            let posture = package_posture(state.packages.values().map(|package| &package.state));
            let (safe_to_resume, next_action) =
                action_for_posture(posture, configured_state_dir, false);
            outcome(
                DurableStatus::Interrupted,
                plan_id,
                None,
                safe_to_resume,
                next_action,
                packet.evidence,
            )
        }
    }
}

#[derive(Clone, Copy)]
enum PackagePosture {
    Ambiguous,
    PermanentBlocker,
    Resumable,
    Complete,
    Disagreement,
}

fn package_posture<'a>(states: impl Iterator<Item = &'a PackageState>) -> PackagePosture {
    let mut saw_state = false;
    let mut resumable = false;
    let mut permanent = false;
    for state in states {
        saw_state = true;
        match state {
            PackageState::Ambiguous { .. }
            | PackageState::Uploaded
            | PackageState::Failed {
                class: ErrorClass::Ambiguous,
                ..
            } => return PackagePosture::Ambiguous,
            PackageState::Failed {
                class: ErrorClass::Permanent,
                ..
            } => permanent = true,
            PackageState::Pending
            | PackageState::Failed {
                class: ErrorClass::Retryable,
                ..
            } => resumable = true,
            PackageState::Published | PackageState::Skipped { .. } => {}
        }
    }
    if permanent {
        PackagePosture::PermanentBlocker
    } else if resumable {
        PackagePosture::Resumable
    } else if saw_state {
        PackagePosture::Complete
    } else {
        PackagePosture::Disagreement
    }
}

fn action_for_posture(
    posture: PackagePosture,
    configured_state_dir: &Path,
    terminal: bool,
) -> (SafeResumePosture, OperatorAction) {
    match posture {
        PackagePosture::Ambiguous => (
            unsafe_safety("registry truth remains unknown for at least one package"),
            OperatorAction::posture(
                ActionKind::Reconcile,
                "inspect reconciliation and event evidence before resuming",
            ),
        ),
        PackagePosture::PermanentBlocker => (
            unsafe_safety("a permanent package failure must be resolved before resume"),
            OperatorAction::posture(
                ActionKind::ResolveBlockers,
                "resolve the permanent failure recorded in durable state before resuming",
            ),
        ),
        PackagePosture::Resumable => (
            SafeResumePosture {
                value: Some(true),
                reason:
                    "durable state identifies retryable or pending work and all identities match"
                        .to_string(),
            },
            OperatorAction::command(
                ActionKind::Resume,
                [
                    "shipper".to_string(),
                    "--state-dir".to_string(),
                    configured_state_dir.display().to_string(),
                    "resume".to_string(),
                ],
                "resume through the same configured state directory; resume will revalidate identity before publishing",
            ),
        ),
        PackagePosture::Complete if terminal => (
            unsafe_safety("the run is complete; there is no unfinished work to resume"),
            OperatorAction::posture(
                ActionKind::NoneComplete,
                "the authoritative terminal evidence is coherent; retain its receipt and events",
            ),
        ),
        PackagePosture::Complete | PackagePosture::Disagreement => (
            unsafe_safety("package state does not prove unfinished resumable work"),
            investigate_action(),
        ),
    }
}

fn contains_ambiguity(packet: &EvidencePacket) -> bool {
    let state_ambiguous = packet.state.as_ref().is_some_and(|state| {
        state.packages.values().any(|package| {
            matches!(
                package.state,
                PackageState::Ambiguous { .. }
                    | PackageState::Uploaded
                    | PackageState::Failed {
                        class: ErrorClass::Ambiguous,
                        ..
                    }
            )
        })
    });
    let receipt_ambiguous = packet.receipt.as_ref().is_some_and(|receipt| {
        receipt.packages.iter().any(|package| {
            matches!(
                package.state,
                PackageState::Ambiguous { .. }
                    | PackageState::Uploaded
                    | PackageState::Failed {
                        class: ErrorClass::Ambiguous,
                        ..
                    }
            )
        })
    });
    let reconciliation_unknown = packet.reconciliation.as_ref().is_some_and(|report| {
        report
            .records
            .iter()
            .any(|record| matches!(record.outcome, ReconciliationOutcome::StillUnknown { .. }))
    });
    state_ambiguous || receipt_ambiguous || reconciliation_unknown
}

fn identities_disagree(plan: &ReleasePlan, packet: &EvidencePacket) -> bool {
    let event_plan = observation_plan_id(&packet.observation);
    let plan_ids = [
        event_plan,
        packet.state.as_ref().map(|state| state.plan_id.as_str()),
        packet
            .receipt
            .as_ref()
            .map(|receipt| receipt.plan_id.as_str()),
        packet
            .reconciliation
            .as_ref()
            .map(|report| report.plan_id.as_str()),
    ];
    if plan_ids
        .into_iter()
        .flatten()
        .any(|identity| identity != plan.plan_id)
    {
        return true;
    }
    [
        packet.state.as_ref().map(|state| &state.registry),
        packet.receipt.as_ref().map(|receipt| &receipt.registry),
        packet
            .reconciliation
            .as_ref()
            .map(|report| &report.registry),
    ]
    .into_iter()
    .flatten()
    .any(|registry| !same_registry(registry, &plan.registry))
}

fn same_registry(left: &Registry, right: &Registry) -> bool {
    left.name == right.name
        && left.api_base == right.api_base
        && left.index_base == right.index_base
}

fn is_no_evidence(packet: &EvidencePacket) -> bool {
    matches!(packet.observation, RunObservation::NoEvidence)
        && packet.state.is_none()
        && packet.receipt.is_none()
        && packet.reconciliation.is_none()
}

fn observation_plan_id(observation: &RunObservation) -> Option<&str> {
    match observation {
        RunObservation::NoEvidence => None,
        RunObservation::Unfinished { plan_id, .. } | RunObservation::Finished { plan_id, .. } => {
            plan_id.as_deref()
        }
    }
}

fn observation_result(observation: &RunObservation) -> Option<&ExecutionResult> {
    match observation {
        RunObservation::Finished { result, .. } => Some(result),
        RunObservation::NoEvidence | RunObservation::Unfinished { .. } => None,
    }
}

fn outcome(
    status: DurableStatus,
    plan_id: Option<String>,
    execution_result: Option<ExecutionResult>,
    safe_to_resume: SafeResumePosture,
    next_action: OperatorAction,
    evidence: Vec<String>,
) -> DurableOperatorOutcome {
    DurableOperatorOutcome {
        status,
        publication_performed: false,
        plan_id,
        execution_result,
        safe_to_resume,
        next_action,
        evidence,
    }
}

fn disagreement(
    plan_id: Option<String>,
    result: Option<ExecutionResult>,
    evidence: Vec<String>,
) -> DurableOperatorOutcome {
    outcome(
        DurableStatus::EvidenceDisagreement,
        plan_id,
        result,
        unsafe_safety("authoritative events disagree with state or receipt evidence"),
        investigate_action(),
        evidence,
    )
}

fn ambiguous(
    plan_id: Option<String>,
    result: Option<ExecutionResult>,
    evidence: Vec<String>,
) -> DurableOperatorOutcome {
    outcome(
        DurableStatus::Ambiguous,
        plan_id,
        result,
        unsafe_safety("registry truth remains unknown for at least one package"),
        OperatorAction::posture(
            ActionKind::Reconcile,
            "inspect reconciliation and event evidence before any publish or resume attempt",
        ),
        evidence,
    )
}

fn investigate_action() -> OperatorAction {
    OperatorAction::posture(
        ActionKind::StopAndInvestigate,
        "events are authoritative; inspect retained events, state, and receipt evidence before acting",
    )
}

fn unsafe_safety(reason: impl Into<String>) -> SafeResumePosture {
    SafeResumePosture {
        value: Some(false),
        reason: reason.into(),
    }
}

fn unknown_safety(reason: impl Into<String>) -> SafeResumePosture {
    SafeResumePosture {
        value: None,
        reason: reason.into(),
    }
}

fn write_report(report: &DurableStatusReport, format: &str) -> Result<()> {
    if format == "json" {
        let json =
            serde_json::to_string_pretty(report).context("serialize durable status report")?;
        println!("{json}");
        return Ok(());
    }

    println!("Durable result: {}", report.outcome.status.human_label());
    println!("Publication performed: no");
    let safety = match report.outcome.safe_to_resume.value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    println!(
        "Safe to resume: {safety} — {}",
        report.outcome.safe_to_resume.reason
    );
    match report.outcome.next_action.command_line() {
        Some(command) => println!("Next: {command} — {}", report.outcome.next_action.reason),
        None => println!("Next: {}", report.outcome.next_action.reason),
    }
    if report.outcome.evidence.is_empty() {
        println!("Evidence: none");
    } else {
        println!("Evidence: {}", report.outcome.evidence.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use anyhow::{Result, ensure};
    use chrono::Utc;
    use shipper_core::cli_bridge::UnknownLivenessReason;
    use shipper_core::types::{
        EnvironmentFingerprint, PackageEvidence, PackageProgress, PackageReceipt,
    };

    use super::*;

    fn registry() -> Registry {
        Registry {
            name: "fixture".into(),
            api_base: "https://registry.invalid".into(),
            index_base: None,
        }
    }

    fn plan() -> ReleasePlan {
        ReleasePlan {
            plan_version: "shipper.plan.v1".into(),
            plan_id: "plan-a".into(),
            created_at: Utc::now(),
            registry: registry(),
            packages: Vec::new(),
            dependencies: BTreeMap::new(),
        }
    }

    fn state(package_state: PackageState) -> ExecutionState {
        let now = Utc::now();
        ExecutionState {
            state_version: "shipper.state.v1".into(),
            plan_id: "plan-a".into(),
            registry: registry(),
            created_at: now,
            updated_at: now,
            attempt_history: Vec::new(),
            packages: BTreeMap::from([(
                "demo@0.1.0".into(),
                PackageProgress {
                    name: "demo".into(),
                    version: "0.1.0".into(),
                    attempts: 1,
                    state: package_state,
                    last_updated_at: now,
                },
            )]),
        }
    }

    fn receipt(result: ExecutionResult, package_state: PackageState) -> Receipt {
        let now = Utc::now();
        Receipt {
            receipt_version: "shipper.receipt.v2".into(),
            plan_id: "plan-a".into(),
            registry: registry(),
            started_at: now,
            finished_at: now,
            packages: vec![PackageReceipt {
                name: "demo".into(),
                version: "0.1.0".into(),
                attempts: 1,
                state: package_state,
                started_at: now,
                finished_at: now,
                duration_ms: 0,
                evidence: PackageEvidence {
                    attempts: Vec::new(),
                    readiness_checks: Vec::new(),
                },
                compromised_at: None,
                compromised_by: None,
                superseded_by: None,
            }],
            event_log_path: PathBuf::from(".operator-state/events.jsonl"),
            git_context: None,
            environment: EnvironmentFingerprint {
                shipper_version: "test".into(),
                cargo_version: None,
                rust_version: None,
                os: "test".into(),
                arch: "test".into(),
            },
            auth_evidence: None,
            execution_result: result,
        }
    }

    fn packet(observation: RunObservation, package_state: Option<PackageState>) -> EvidencePacket {
        EvidencePacket {
            observation,
            state: package_state.map(state),
            receipt: None,
            reconciliation: None,
            finalization_consistent: None,
            evidence: vec![".operator-state/events.jsonl".into()],
        }
    }

    fn assert_posture(
        outcome: &DurableOperatorOutcome,
        status: DurableStatus,
        safety: Option<bool>,
        action: ActionKind,
        command_expected: bool,
    ) -> Result<()> {
        ensure!(outcome.status == status, "unexpected status");
        ensure!(outcome.safe_to_resume.value == safety, "unexpected safety");
        ensure!(outcome.next_action.kind == action, "unexpected action");
        ensure!(
            outcome.next_action.command.is_empty() != command_expected,
            "unexpected command presence"
        );
        ensure!(!outcome.publication_performed, "status must be read-only");
        Ok(())
    }

    #[test]
    fn durable_outcome_precedence_matrix_is_fail_closed() -> Result<()> {
        let plan = plan();
        let configured = Path::new(".operator-state");
        assert_posture(
            &classify(&plan, configured, packet(RunObservation::NoEvidence, None)),
            DurableStatus::NoEvidence,
            None,
            ActionKind::Status,
            false,
        )?;
        assert_posture(
            &classify(
                &plan,
                configured,
                packet(
                    RunObservation::Unfinished {
                        plan_id: Some("plan-a".into()),
                        liveness: RunLiveness::Unknown(UnknownLivenessReason::MissingLock),
                    },
                    Some(PackageState::Pending),
                ),
            ),
            DurableStatus::Unknown,
            None,
            ActionKind::InspectEvents,
            false,
        )?;
        assert_posture(
            &classify(
                &plan,
                configured,
                packet(
                    RunObservation::Unfinished {
                        plan_id: Some("plan-a".into()),
                        liveness: RunLiveness::Live,
                    },
                    Some(PackageState::Pending),
                ),
            ),
            DurableStatus::Live,
            Some(false),
            ActionKind::Status,
            false,
        )?;
        let interrupted = classify(
            &plan,
            configured,
            packet(
                RunObservation::Unfinished {
                    plan_id: Some("plan-a".into()),
                    liveness: RunLiveness::NotLive,
                },
                Some(PackageState::Pending),
            ),
        );
        assert_posture(
            &interrupted,
            DurableStatus::Interrupted,
            Some(true),
            ActionKind::Resume,
            true,
        )?;
        ensure!(
            interrupted.next_action.command
                == ["shipper", "--state-dir", ".operator-state", "resume"]
        );
        assert_posture(
            &classify(
                &plan,
                configured,
                packet(
                    RunObservation::Unfinished {
                        plan_id: Some("plan-a".into()),
                        liveness: RunLiveness::NotLive,
                    },
                    Some(PackageState::Uploaded),
                ),
            ),
            DurableStatus::Ambiguous,
            Some(false),
            ActionKind::Reconcile,
            false,
        )?;
        assert_posture(
            &classify(
                &plan,
                configured,
                packet(
                    RunObservation::Unfinished {
                        plan_id: Some("plan-a".into()),
                        liveness: RunLiveness::NotLive,
                    },
                    Some(PackageState::Failed {
                        class: ErrorClass::Permanent,
                        message: "blocked".into(),
                    }),
                ),
            ),
            DurableStatus::Interrupted,
            Some(false),
            ActionKind::ResolveBlockers,
            false,
        )?;
        assert_posture(
            &classify(
                &ReleasePlan {
                    plan_id: "other".into(),
                    ..plan.clone()
                },
                configured,
                packet(
                    RunObservation::Unfinished {
                        plan_id: Some("plan-a".into()),
                        liveness: RunLiveness::NotLive,
                    },
                    Some(PackageState::Pending),
                ),
            ),
            DurableStatus::IdentityMismatch,
            Some(false),
            ActionKind::StopAndInvestigate,
            false,
        )?;
        assert_posture(
            &classify(
                &plan,
                configured,
                EvidencePacket {
                    observation: RunObservation::NoEvidence,
                    state: Some(state(PackageState::Pending)),
                    receipt: None,
                    reconciliation: None,
                    finalization_consistent: None,
                    evidence: vec![".operator-state/state.json".into()],
                },
            ),
            DurableStatus::EvidenceDisagreement,
            Some(false),
            ActionKind::StopAndInvestigate,
            false,
        )?;
        assert_posture(
            &classify(
                &plan,
                configured,
                EvidencePacket {
                    observation: RunObservation::NoEvidence,
                    state: Some(state(PackageState::Uploaded)),
                    receipt: None,
                    reconciliation: None,
                    finalization_consistent: None,
                    evidence: vec![".operator-state/state.json".into()],
                },
            ),
            DurableStatus::EvidenceDisagreement,
            Some(false),
            ActionKind::StopAndInvestigate,
            false,
        )?;
        Ok(())
    }

    #[test]
    fn terminal_requires_matching_completed_receipt_and_consistency() -> Result<()> {
        let plan = plan();
        let configured = Path::new(".operator-state");
        let mut coherent = packet(
            RunObservation::Finished {
                plan_id: Some("plan-a".into()),
                result: ExecutionResult::Success,
            },
            Some(PackageState::Published),
        );
        coherent.receipt = Some(receipt(ExecutionResult::Success, PackageState::Published));
        coherent.finalization_consistent = Some(true);
        assert_posture(
            &classify(&plan, configured, coherent),
            DurableStatus::Terminal,
            Some(false),
            ActionKind::NoneComplete,
            false,
        )?;

        let mut inconsistent = packet(
            RunObservation::Finished {
                plan_id: Some("plan-a".into()),
                result: ExecutionResult::Success,
            },
            Some(PackageState::Published),
        );
        inconsistent.receipt = Some(receipt(
            ExecutionResult::CompleteFailure,
            PackageState::Published,
        ));
        inconsistent.finalization_consistent = Some(false);
        assert_posture(
            &classify(&plan, configured, inconsistent),
            DurableStatus::EvidenceDisagreement,
            Some(false),
            ActionKind::StopAndInvestigate,
            false,
        )?;

        let terminal_without_receipt = packet(
            RunObservation::Finished {
                plan_id: Some("plan-a".into()),
                result: ExecutionResult::PartialFailure,
            },
            Some(PackageState::Uploaded),
        );
        assert_posture(
            &classify(&plan, configured, terminal_without_receipt),
            DurableStatus::EvidenceDisagreement,
            Some(false),
            ActionKind::StopAndInvestigate,
            false,
        )?;
        Ok(())
    }
}
