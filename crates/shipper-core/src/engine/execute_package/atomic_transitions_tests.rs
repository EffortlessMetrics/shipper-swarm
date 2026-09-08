use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, TryLockError, mpsc};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use chrono::Utc;

use super::{
    commit_attempt_transition, commit_pending_transition,
    commit_pending_with_attempt_detail_transition, retry_backoff_events, with_package_event_batch,
};
use crate::state::events::{EventLog, events_path};
use crate::types::{
    AttemptDetail, ErrorClass, EventType, ExecutionState, PackageProgress, PackageState,
    PublishEvent, ReadinessMethod, ReconciliationOutcome, Registry,
};

fn failed_event(package: &str) -> PublishEvent {
    PublishEvent {
        timestamp: Utc::now(),
        package: package.to_owned(),
        event_type: EventType::PackageFailed {
            class: ErrorClass::Permanent,
            message: "rejected".to_owned(),
        },
    }
}

fn initial_state() -> ExecutionState {
    let now = Utc::now();
    ExecutionState {
        state_version: "shipper.state.v1".to_owned(),
        plan_id: "parallel-transition-witness".to_owned(),
        registry: Registry::crates_io(),
        created_at: now,
        updated_at: now,
        attempt_history: Vec::new(),
        packages: ["alpha", "beta"]
            .into_iter()
            .map(|name| {
                (
                    format!("{name}@1.0.0"),
                    PackageProgress {
                        name: name.to_owned(),
                        version: "1.0.0".to_owned(),
                        attempts: 0,
                        state: PackageState::Pending,
                        last_updated_at: now,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    }
}

fn competing_pending_event(flush: bool) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = events_path(directory.path());
    let log = Arc::new(Mutex::new(EventLog::new()));
    let state = Arc::new(Mutex::new(initial_state()));
    let beta_event = failed_event("beta@1.0.0");
    // Beta owns its prepared event while alpha appends or flushes the shared log.
    let worker_log = Arc::clone(&log);
    let worker_path = path.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<()> {
        let mut pending = worker_log
            .lock()
            .map_err(|_| anyhow::anyhow!("competing event log poisoned"))?;
        pending.record(failed_event("alpha@1.0.0"));
        if flush {
            pending.write_to_file(&worker_path)?;
            pending.clear();
        }
        done_tx.send(())?;
        Ok(())
    });
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .context("competing writer must finish before beta commits")?;
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("competing writer panicked"))??;
    commit_pending_transition(
        &state,
        directory.path(),
        &log,
        &path,
        "beta@1.0.0",
        PackageState::Failed {
            class: ErrorClass::Permanent,
            message: "rejected".to_owned(),
        },
        vec![beta_event],
    )?;
    commit_pending_transition(
        &state,
        directory.path(),
        &log,
        &path,
        "alpha@1.0.0",
        PackageState::Published,
        vec![PublishEvent {
            timestamp: Utc::now(),
            package: "alpha@1.0.0".to_owned(),
            event_type: EventType::PackagePublished { duration_ms: 1 },
        }],
    )?;
    let persisted = EventLog::read_from_file(&path)?;
    ensure!(persisted.events_for_package("beta@1.0.0").len() == 1);
    ensure!(persisted.events_for_package("alpha@1.0.0").len() == 2);
    let state = state
        .lock()
        .map_err(|_| anyhow::anyhow!("state poisoned"))?;
    verify_rebuild(&path, &state)?;
    Ok(())
}

#[test]
fn pending_transition_survives_foreign_tail() -> Result<()> {
    competing_pending_event(false)
}

#[test]
fn pending_transition_survives_competing_flush() -> Result<()> {
    competing_pending_event(true)
}

fn verify_rebuild(path: &std::path::Path, state: &ExecutionState) -> Result<()> {
    let rebuilt = crate::state::rebuild::rebuild_state_from_events(
        path,
        crate::state::rebuild::StateRebuildOptions::new(state.registry.clone())
            .with_fallback_plan_id(&state.plan_id),
    )?;
    ensure!(rebuilt.packages.len() == state.packages.len());
    for (key, expected) in &state.packages {
        let actual = rebuilt.packages.get(key).context("rebuilt package")?;
        ensure!(actual.state == expected.state, "state mismatch for {key}");
        ensure!(
            actual.attempts == expected.attempts,
            "attempt mismatch for {key}"
        );
    }
    let mut actual = rebuilt.attempt_history;
    let mut expected = state.attempt_history.clone();
    actual.sort_by(|a, b| a.package.cmp(&b.package));
    expected.sort_by(|a, b| a.package.cmp(&b.package));
    ensure!(
        actual == expected,
        "attempt history mismatch: {actual:?} != {expected:?}"
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum BatchKind {
    Failure,
    Reconciliation,
    Retry,
}

fn batch_holds_log_through_projection(kind: BatchKind) -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = events_path(directory.path());
    let log = Arc::new(Mutex::new(EventLog::new()));
    let state = Arc::new(Mutex::new(initial_state()));
    let started = Utc::now();
    let ended = started + chrono::TimeDelta::milliseconds(1);
    for package in ["alpha@1.0.0", "beta@1.0.0"] {
        commit_attempt_transition(
            &state,
            directory.path(),
            &log,
            &path,
            package,
            1,
            PublishEvent {
                timestamp: started,
                package: package.to_owned(),
                event_type: EventType::PackageAttempted {
                    attempt: 1,
                    max_attempts: 2,
                    command: "fake cargo publish".to_owned(),
                },
            },
        )?;
    }
    let class = match kind {
        BatchKind::Failure => ErrorClass::Permanent,
        BatchKind::Reconciliation => ErrorClass::Ambiguous,
        BatchKind::Retry => ErrorClass::Retryable,
    };
    let mut detail = AttemptDetail {
        package: "beta".to_owned(),
        version: "1.0.0".to_owned(),
        attempt: 1,
        max_attempts: 2,
        started_at: started,
        ended_at: ended,
        error_class: Some(class.clone()),
        next_attempt_at: None,
        redacted_message: Some("rejected".to_owned()),
    };
    let mut batch = vec![PublishEvent {
        timestamp: ended,
        package: "beta@1.0.0".to_owned(),
        event_type: EventType::PackageFailed {
            class: class.clone(),
            message: "rejected".to_owned(),
        },
    }];
    let final_state = match kind {
        BatchKind::Failure => PackageState::Failed {
            class: class.clone(),
            message: "rejected".to_owned(),
        },
        BatchKind::Reconciliation => {
            batch.push(PublishEvent {
                timestamp: ended,
                package: "beta@1.0.0".to_owned(),
                event_type: EventType::PublishReconciling {
                    method: ReadinessMethod::Api,
                },
            });
            batch.push(PublishEvent {
                timestamp: ended,
                package: "beta@1.0.0".to_owned(),
                event_type: EventType::PublishReconciled {
                    outcome: ReconciliationOutcome::StillUnknown {
                        attempts: 1,
                        elapsed_ms: 0,
                        reason: "unknown".to_owned(),
                    },
                },
            });
            PackageState::Ambiguous {
                message: "unknown".to_owned(),
            }
        }
        BatchKind::Retry => {
            let next = ended + chrono::TimeDelta::seconds(1);
            detail.next_attempt_at = Some(next);
            batch.extend(retry_backoff_events(
                "beta@1.0.0",
                1,
                2,
                Duration::from_secs(1),
                next,
                &class,
                "rejected",
            ));
            PackageState::Pending
        }
    };
    let expected_batch = serde_json::to_value(&batch)?;
    let worker_log = Arc::clone(&log);
    let worker_state = Arc::clone(&state);
    let worker_path = path.clone();
    let worker_dir = directory.path().to_path_buf();
    let (probe_tx, probe_rx) = mpsc::channel();
    let (blocked_tx, blocked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<()> {
        probe_rx.recv_timeout(Duration::from_secs(5))?;
        let blocked = matches!(worker_log.try_lock(), Err(TryLockError::WouldBlock));
        blocked_tx.send(blocked)?;
        release_rx.recv_timeout(Duration::from_secs(5))?;
        let mut event = failed_event("alpha@1.0.0");
        event.timestamp = ended;
        commit_pending_with_attempt_detail_transition(
            &worker_state,
            &worker_dir,
            &worker_log,
            &worker_path,
            "alpha@1.0.0",
            PackageState::Failed {
                class: ErrorClass::Permanent,
                message: "rejected".to_owned(),
            },
            AttemptDetail {
                package: "alpha".to_owned(),
                version: "1.0.0".to_owned(),
                attempt: 1,
                max_attempts: 2,
                started_at: started,
                ended_at: ended,
                error_class: Some(ErrorClass::Permanent),
                next_attempt_at: None,
                redacted_message: Some("rejected".to_owned()),
            },
            vec![event],
        )
    });
    let result = with_package_event_batch(&state, &log, "beta@1.0.0", batch, |state, log| {
        probe_tx.send(())?;
        ensure!(
            blocked_rx.recv_timeout(Duration::from_secs(5))?,
            "competing writer entered before projection"
        );
        match kind {
            BatchKind::Retry => crate::engine::transition::commit_attempt_detail_pending(
                state,
                directory.path(),
                log,
                &path,
                "beta@1.0.0",
                detail,
            ),
            _ => crate::engine::transition::commit_pending_with_attempt_detail(
                state,
                directory.path(),
                log,
                &path,
                "beta@1.0.0",
                final_state,
                detail,
            ),
        }
    });
    release_tx.send(())?;
    let joined = worker
        .join()
        .map_err(|_| anyhow::anyhow!("competing writer panicked"))?;
    result?;
    joined?;
    let persisted = EventLog::read_from_file(&path)?;
    let beta_events = persisted.events_for_package("beta@1.0.0");
    ensure!(
        serde_json::to_value(beta_events.into_iter().skip(1).collect::<Vec<_>>())?
            == expected_batch
    );
    ensure!(persisted.events_for_package("alpha@1.0.0").len() == 2);
    if matches!(kind, BatchKind::Retry) {
        // Finish the retained failed attempt after proving its waiting timeline.
        commit_pending_transition(
            &state,
            directory.path(),
            &log,
            &path,
            "beta@1.0.0",
            PackageState::Failed {
                class: class.clone(),
                message: "rejected".to_owned(),
            },
            vec![PublishEvent {
                timestamp: ended,
                package: "beta@1.0.0".to_owned(),
                event_type: EventType::PackageFailed {
                    class,
                    message: "rejected".to_owned(),
                },
            }],
        )?;
    }
    let state = state
        .lock()
        .map_err(|_| anyhow::anyhow!("state poisoned"))?;
    ensure!(state.attempt_history.len() == 2);
    verify_rebuild(&path, &state)
}

#[test]
fn failure_batch_excludes_competing_flush_until_projection() -> Result<()> {
    batch_holds_log_through_projection(BatchKind::Failure)
}

#[test]
fn reconciliation_batch_excludes_competing_flush_until_projection() -> Result<()> {
    batch_holds_log_through_projection(BatchKind::Reconciliation)
}

#[test]
fn retry_batch_excludes_competing_flush_until_projection() -> Result<()> {
    batch_holds_log_through_projection(BatchKind::Retry)
}

#[test]
fn invalid_batch_is_rejected_before_any_event_is_appended() -> Result<()> {
    for batch in [
        vec![],
        vec![failed_event("beta@1.0.0"), failed_event("alpha@1.0.0")],
    ] {
        let state = Arc::new(Mutex::new(initial_state()));
        let log = Arc::new(Mutex::new(EventLog::new()));
        let result = with_package_event_batch(&state, &log, "beta@1.0.0", batch, |_, _| {
            anyhow::bail!("invalid batch reached projection")
        });
        let error = result.err().context("invalid batch must fail")?;
        ensure!(!error.to_string().contains("reached projection"));
        ensure!(
            log.lock()
                .map_err(|_| anyhow::anyhow!("log poisoned"))?
                .is_empty()
        );
    }
    Ok(())
}

#[test]
fn failed_event_write_keeps_batch_and_original_projection() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = events_path(directory.path());
    std::fs::create_dir(&path)?;
    let state = Arc::new(Mutex::new(initial_state()));
    let log = Arc::new(Mutex::new(EventLog::new()));
    let before = serde_json::to_value(
        &*state
            .lock()
            .map_err(|_| anyhow::anyhow!("state poisoned"))?,
    )?;
    let event = failed_event("beta@1.0.0");
    let expected = serde_json::to_value(&event)?;
    let result = commit_pending_transition(
        &state,
        directory.path(),
        &log,
        &path,
        "beta@1.0.0",
        PackageState::Failed {
            class: ErrorClass::Permanent,
            message: "rejected".to_owned(),
        },
        vec![event],
    );
    let error = result.err().context("event write must fail")?;
    ensure!(
        error
            .to_string()
            .contains("failed to persist package transition event")
    );
    ensure!(
        serde_json::to_value(
            &*state
                .lock()
                .map_err(|_| anyhow::anyhow!("state poisoned"))?,
        )? == before
    );
    let log = log.lock().map_err(|_| anyhow::anyhow!("log poisoned"))?;
    ensure!(log.len() == 1);
    ensure!(serde_json::to_value(log.all_events().first().context("retained event")?)? == expected);
    ensure!(!directory.path().join("state.json").exists());
    Ok(())
}
