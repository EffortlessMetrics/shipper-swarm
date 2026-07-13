//! Controlled durable package transitions.
//!
//! `events.jsonl` is authoritative, so a transition records the domain event
//! before persisting the new state projection. Callers receive the first
//! persistence error instead of silently continuing with a partially recorded
//! transition.

use std::path::Path;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::state::events::EventLog;
use crate::state::execution_state;
use crate::types::{ExecutionState, PackageState, PublishEvent};

/// Apply one package transition through the shared event/state boundary.
///
/// The state is cloned before mutation. This keeps the caller's in-memory
/// projection unchanged when the event cannot be appended. If state
/// persistence fails after the event is written, the event remains the
/// authoritative record and finalization/rebuild can surface the drift.
pub(crate) fn commit(
    state: &mut ExecutionState,
    state_dir: &Path,
    event_log: &mut EventLog,
    events_path: &Path,
    key: &str,
    new_state: PackageState,
    event: PublishEvent,
) -> Result<()> {
    if event.package != key {
        bail!(
            "package transition key '{}' does not match event package '{}'",
            key,
            event.package
        );
    }

    let mut next_state = state.clone();
    let progress = next_state
        .packages
        .get_mut(key)
        .with_context(|| format!("missing package progress for transition: {key}"))?;
    progress.state = new_state;
    progress.last_updated_at = Utc::now();
    next_state.updated_at = Utc::now();

    event_log.record(event);
    event_log
        .write_to_file(events_path)
        .with_context(|| format!("failed to persist package transition event for {key}"))?;
    event_log.clear();

    execution_state::save_state(state_dir, &next_state)
        .with_context(|| format!("failed to persist package transition state for {key}"))?;
    *state = next_state;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::File;

    use anyhow::Context;
    use chrono::Utc;
    use tempfile::tempdir;

    use super::commit;
    use crate::state::events::{EventLog, events_path};
    use crate::types::{
        EventType, ExecutionState, PackageProgress, PackageState, PublishEvent, Registry,
    };

    fn state() -> ExecutionState {
        let now = Utc::now();
        ExecutionState {
            state_version: "shipper.state.v1".to_string(),
            plan_id: "plan-transition-test".to_string(),
            registry: Registry::crates_io(),
            created_at: now,
            updated_at: now,
            attempt_history: Vec::new(),
            packages: BTreeMap::from([(
                "demo@1.0.0".to_string(),
                PackageProgress {
                    name: "demo".to_string(),
                    version: "1.0.0".to_string(),
                    attempts: 0,
                    state: PackageState::Pending,
                    last_updated_at: now,
                },
            )]),
        }
    }

    fn published_event() -> PublishEvent {
        PublishEvent {
            timestamp: Utc::now(),
            event_type: EventType::PackagePublished { duration_ms: 7 },
            package: "demo@1.0.0".to_string(),
        }
    }

    #[test]
    fn commit_persists_event_before_state_projection() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let events = events_path(dir.path());
        let mut state = state();
        let original = state.clone();
        let mut log = EventLog::new();

        commit(
            &mut state,
            dir.path(),
            &mut log,
            &events,
            "demo@1.0.0",
            PackageState::Published,
            published_event(),
        )?;

        assert_eq!(state.packages["demo@1.0.0"].state, PackageState::Published);
        assert!(log.all_events().is_empty());
        let persisted = EventLog::read_from_file(&events)?;
        assert!(matches!(
            persisted
                .all_events()
                .first()
                .map(|event| &event.event_type),
            Some(EventType::PackagePublished { duration_ms: 7 })
        ));
        let saved = crate::state::execution_state::load_state(dir.path())?
            .context("transition test state was not persisted")?;
        assert_eq!(saved.packages["demo@1.0.0"].state, PackageState::Published);
        assert_eq!(original.packages["demo@1.0.0"].state, PackageState::Pending);
        Ok(())
    }

    #[test]
    fn commit_rejects_an_event_for_a_different_package() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let events = events_path(dir.path());
        let mut state = state();
        let mut log = EventLog::new();
        let err = commit(
            &mut state,
            dir.path(),
            &mut log,
            &events,
            "demo@1.0.0",
            PackageState::Published,
            PublishEvent {
                package: "other@1.0.0".to_string(),
                ..published_event()
            },
        )
        .expect_err("mismatched transition must fail");

        assert!(err.to_string().contains("does not match event package"));
        assert!(log.all_events().is_empty());
        assert!(!events.exists());
        assert_eq!(state.packages["demo@1.0.0"].state, PackageState::Pending);
        Ok(())
    }

    #[test]
    fn commit_does_not_persist_state_when_event_append_fails() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let events = events_path(dir.path());
        std::fs::create_dir(&events)?;
        let mut state = state();
        let mut log = EventLog::new();

        let err = commit(
            &mut state,
            dir.path(),
            &mut log,
            &events,
            "demo@1.0.0",
            PackageState::Published,
            published_event(),
        )
        .expect_err("an events directory cannot be opened as a JSONL file");

        assert!(
            err.to_string()
                .contains("failed to persist package transition event")
        );
        assert_eq!(state.packages["demo@1.0.0"].state, PackageState::Pending);
        assert_eq!(log.all_events().len(), 1);
        Ok(())
    }

    #[test]
    fn commit_leaves_event_truth_when_state_projection_fails() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let state_dir = dir.path().join("state-file");
        File::create(&state_dir)?;
        let events = events_path(dir.path());
        let mut state = state();
        let mut log = EventLog::new();

        let err = commit(
            &mut state,
            &state_dir,
            &mut log,
            &events,
            "demo@1.0.0",
            PackageState::Published,
            published_event(),
        )
        .expect_err("a file cannot be used as the state directory");

        assert!(
            err.to_string()
                .contains("failed to persist package transition state")
        );
        assert_eq!(state.packages["demo@1.0.0"].state, PackageState::Pending);
        assert!(log.all_events().is_empty());
        let persisted = EventLog::read_from_file(&events)?;
        assert!(matches!(
            persisted
                .all_events()
                .first()
                .map(|event| &event.event_type),
            Some(EventType::PackagePublished { duration_ms: 7 })
        ));
        Ok(())
    }
}
