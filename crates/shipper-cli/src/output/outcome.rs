//! Shared semantic vocabulary for operator-facing command outcomes.
//!
//! Human prose and versioned JSON envelopes may render these values
//! differently, but they should not independently infer safety or the next
//! command. Engine/state/receipt truth remains authoritative; this module is
//! the CLI adaptation layer for that truth.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OutcomeStatus {
    Ready,
    ReadyWithWarnings,
    Planned,
    Success,
    PartialFailure,
    Failed,
    Interrupted,
    Ambiguous,
    Blocked,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionKind {
    Plan,
    Preflight,
    Publish,
    Resume,
    Status,
    InspectEvents,
    InspectReceipt,
    Reconcile,
    FixConfiguration,
    WaitForRegistry,
    ResolveBlockers,
    InvestigateUnknowns,
    StopAndInvestigate,
    NoneComplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OperatorAction {
    pub kind: ActionKind,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    pub reason: String,
    pub requires_confirmation: bool,
}

impl OperatorAction {
    pub(crate) fn command(
        kind: ActionKind,
        command: impl IntoIterator<Item = impl Into<String>>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            command: command.into_iter().map(Into::into).collect(),
            reason: reason.into(),
            requires_confirmation: false,
        }
    }

    pub(crate) fn posture(kind: ActionKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            command: Vec::new(),
            reason: reason.into(),
            requires_confirmation: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_confirmation(mut self) -> Self {
        self.requires_confirmation = true;
        self
    }

    pub(crate) fn command_line(&self) -> Option<String> {
        (!self.command.is_empty()).then(|| self.command.join(" "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RerunSafety {
    Safe,
    Unsafe,
    Unknown,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SafeRerun {
    pub value: RerunSafety,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceKind {
    Events,
    State,
    Receipt,
    Plan,
    Preflight,
    Reconciliation,
    WorkflowRun,
    Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct EvidenceReference {
    pub kind: EvidenceKind,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OutcomeIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OperatorOutcome {
    pub status: OutcomeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_to_rerun: Option<SafeRerun>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<OperatorAction>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<OutcomeIdentity>,
}

impl OperatorOutcome {
    pub(crate) fn new(status: OutcomeStatus) -> Self {
        Self {
            status,
            failure_class: None,
            safe_to_rerun: None,
            next_action: None,
            evidence: Vec::new(),
            identity: None,
        }
    }

    pub(crate) fn with_next_action(mut self, action: OperatorAction) -> Self {
        self.next_action = Some(action);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_action_has_stable_machine_and_human_identity() {
        let action = OperatorAction::command(
            ActionKind::Resume,
            ["shipper", "resume"],
            "durable evidence proves the interrupted run is resumable",
        );
        assert_eq!(action.command_line().as_deref(), Some("shipper resume"));
        assert!(!action.requires_confirmation);
        let json = serde_json::to_value(&action).expect("serialize action");
        assert_eq!(json["kind"], "resume");
        assert_eq!(json["command"], serde_json::json!(["shipper", "resume"]));
    }

    #[test]
    fn stop_posture_does_not_fabricate_a_command() {
        let action = OperatorAction::posture(
            ActionKind::StopAndInvestigate,
            "registry outcome remains unknown",
        );
        assert_eq!(action.command_line(), None);
        assert_eq!(
            serde_json::to_value(&action).expect("serialize action")["kind"],
            "stop_and_investigate"
        );
    }

    #[test]
    fn confirmation_is_explicit_not_inferred_from_action_kind() {
        let action = OperatorAction::command(
            ActionKind::Publish,
            ["shipper", "publish"],
            "reversible proof passed",
        )
        .with_confirmation();
        assert!(action.requires_confirmation);
    }

    #[test]
    fn outcome_omits_unavailable_semantics_instead_of_inventing_them() {
        let outcome = OperatorOutcome::new(OutcomeStatus::Planned).with_next_action(
            OperatorAction::command(
                ActionKind::Preflight,
                ["shipper", "preflight"],
                "planning is read-only and the graph is valid",
            ),
        );
        let json = serde_json::to_value(&outcome).expect("serialize outcome");
        assert!(json.get("failure_class").is_none());
        assert!(json.get("safe_to_rerun").is_none());
        assert!(json.get("evidence").is_none());
        assert_eq!(json["next_action"]["kind"], "preflight");
    }
}
