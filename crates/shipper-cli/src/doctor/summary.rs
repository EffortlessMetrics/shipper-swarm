//! Readiness summary shared by human and JSON `shipper doctor` output.

use serde::Serialize;

use super::findings::{Finding, FindingLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DoctorReadiness {
    Ready,
    ReadyWithWarnings,
    Blocked,
    Incomplete,
}

impl DoctorReadiness {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ReadyWithWarnings => "ready_with_warnings",
            Self::Blocked => "blocked",
            Self::Incomplete => "incomplete",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DoctorCheckStatus {
    Passed,
    Warning,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DoctorActionKind {
    Plan,
    ResolveBlockers,
    InvestigateUnknowns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DoctorNextAction {
    pub kind: DoctorActionKind,
    pub command: Vec<&'static str>,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct DoctorSummary {
    pub checks_evaluated: usize,
    pub checks_passed: usize,
    pub warning_count: usize,
    pub blocker_count: usize,
    pub skipped_or_unknown_count: usize,
    pub readiness: DoctorReadiness,
    pub next_action: DoctorNextAction,
}

impl DoctorSummary {
    pub(super) fn from_statuses(statuses: impl IntoIterator<Item = DoctorCheckStatus>) -> Self {
        let mut checks_evaluated = 0;
        let mut checks_passed = 0;
        let mut warning_count = 0;
        let mut blocker_count = 0;
        let mut skipped_or_unknown_count = 0;

        for status in statuses {
            checks_evaluated += 1;
            match status {
                DoctorCheckStatus::Passed => checks_passed += 1,
                DoctorCheckStatus::Warning => warning_count += 1,
                DoctorCheckStatus::Blocked => blocker_count += 1,
                DoctorCheckStatus::Unknown => skipped_or_unknown_count += 1,
            }
        }

        Self::from_counts(
            checks_evaluated,
            checks_passed,
            warning_count,
            blocker_count,
            skipped_or_unknown_count,
        )
    }

    pub(super) fn combine(
        workspace: DoctorCheckStatus,
        reports: impl IntoIterator<Item = DoctorSummary>,
    ) -> Self {
        let mut combined = Self::from_statuses([workspace]);
        for report in reports {
            combined.checks_evaluated += report.checks_evaluated;
            combined.checks_passed += report.checks_passed;
            combined.warning_count += report.warning_count;
            combined.blocker_count += report.blocker_count;
            combined.skipped_or_unknown_count += report.skipped_or_unknown_count;
        }
        Self::from_counts(
            combined.checks_evaluated,
            combined.checks_passed,
            combined.warning_count,
            combined.blocker_count,
            combined.skipped_or_unknown_count,
        )
    }

    fn from_counts(
        checks_evaluated: usize,
        checks_passed: usize,
        warning_count: usize,
        blocker_count: usize,
        skipped_or_unknown_count: usize,
    ) -> Self {
        let readiness = if blocker_count > 0 {
            DoctorReadiness::Blocked
        } else if skipped_or_unknown_count > 0 {
            DoctorReadiness::Incomplete
        } else if warning_count > 0 {
            DoctorReadiness::ReadyWithWarnings
        } else {
            DoctorReadiness::Ready
        };
        let next_action = match readiness {
            DoctorReadiness::Ready => DoctorNextAction {
                kind: DoctorActionKind::Plan,
                command: vec!["shipper", "plan"],
                reason: "the required environment checks passed",
            },
            DoctorReadiness::ReadyWithWarnings => DoctorNextAction {
                kind: DoctorActionKind::Plan,
                command: vec!["shipper", "plan"],
                reason: "no blocker remains; review warnings before preflight",
            },
            DoctorReadiness::Blocked => DoctorNextAction {
                kind: DoctorActionKind::ResolveBlockers,
                command: vec!["shipper", "doctor"],
                reason: "apply the blocking finding remediation, then rerun diagnostics",
            },
            DoctorReadiness::Incomplete => DoctorNextAction {
                kind: DoctorActionKind::InvestigateUnknowns,
                command: vec!["shipper", "doctor", "--format", "json"],
                reason: "one or more required checks could not be evaluated",
            },
        };

        Self {
            checks_evaluated,
            checks_passed,
            warning_count,
            blocker_count,
            skipped_or_unknown_count,
            readiness,
            next_action,
        }
    }

    pub(super) fn print_human(&self) {
        self.print_human_with_label("Doctor");
    }

    pub(super) fn print_registry_human(&self) {
        self.print_human_with_label("Registry doctor");
    }

    fn print_human_with_label(&self, label: &str) {
        println!();
        println!("{label}: {}", self.readiness.as_str());
        println!(
            "Checks: {} evaluated, {} passed, {} warnings, {} blockers, {} unknown",
            self.checks_evaluated,
            self.checks_passed,
            self.warning_count,
            self.blocker_count,
            self.skipped_or_unknown_count
        );
        println!("Next: {}", self.next_action.command.join(" "));
    }
}

pub(super) fn status_from_findings(findings: &[Finding]) -> DoctorCheckStatus {
    if findings
        .iter()
        .any(|finding| finding.status == FindingLevel::Blocked)
    {
        DoctorCheckStatus::Blocked
    } else if findings
        .iter()
        .any(|finding| finding.status == FindingLevel::Warning)
    {
        DoctorCheckStatus::Warning
    } else {
        DoctorCheckStatus::Passed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_is_ready_when_every_check_passes() {
        let summary =
            DoctorSummary::from_statuses([DoctorCheckStatus::Passed, DoctorCheckStatus::Passed]);
        assert_eq!(summary.readiness, DoctorReadiness::Ready);
        assert_eq!(summary.checks_evaluated, 2);
        assert_eq!(summary.checks_passed, 2);
        assert_eq!(summary.next_action.kind, DoctorActionKind::Plan);
    }

    #[test]
    fn blockers_take_precedence_over_warnings_and_unknowns() {
        let summary = DoctorSummary::from_statuses([
            DoctorCheckStatus::Warning,
            DoctorCheckStatus::Unknown,
            DoctorCheckStatus::Blocked,
        ]);
        assert_eq!(summary.readiness, DoctorReadiness::Blocked);
        assert_eq!(summary.warning_count, 1);
        assert_eq!(summary.blocker_count, 1);
        assert_eq!(summary.skipped_or_unknown_count, 1);
    }

    #[test]
    fn unknown_required_checks_make_the_result_incomplete() {
        let summary =
            DoctorSummary::from_statuses([DoctorCheckStatus::Passed, DoctorCheckStatus::Unknown]);
        assert_eq!(summary.readiness, DoctorReadiness::Incomplete);
        assert_eq!(
            summary.next_action.kind,
            DoctorActionKind::InvestigateUnknowns
        );
    }

    #[test]
    fn warnings_without_blockers_remain_actionable() {
        let summary =
            DoctorSummary::from_statuses([DoctorCheckStatus::Passed, DoctorCheckStatus::Warning]);
        assert_eq!(summary.readiness, DoctorReadiness::ReadyWithWarnings);
        assert_eq!(summary.next_action.command, vec!["shipper", "plan"]);
    }
}
