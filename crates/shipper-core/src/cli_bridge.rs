//! Unsupported read-only bridge for the separately packaged CLI adapter.

use std::path::Path;

use anyhow::Result;

use crate::types::ExecutionResult;

/// Why unfinished evidence could not prove whether its publisher is alive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownLivenessReason {
    MissingLock,
    CorruptLock,
    CrossHost,
    LegacyLock,
    MissingPlanIdentity,
    PlanMismatch,
    ProcessIdentityMismatch,
    ProcessProbeUnavailable,
}

/// Fail-closed liveness classification for an unfinished run segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLiveness {
    Live,
    NotLive,
    Unknown(UnknownLivenessReason),
}

/// Current durable run segment. Finished segments cannot carry liveness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunObservation {
    NoEvidence,
    Unfinished {
        plan_id: Option<String>,
        liveness: RunLiveness,
    },
    Finished {
        plan_id: Option<String>,
        result: ExecutionResult,
    },
}

/// Read authoritative events and advisory lock evidence without modifying it.
pub fn observe_run(state_dir: &Path, workspace_root: Option<&Path>) -> Result<RunObservation> {
    crate::state::run_observation::observe_run(state_dir, workspace_root)
}
