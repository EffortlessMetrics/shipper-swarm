use super::*;
use anyhow::ensure;
use chrono::TimeDelta;

fn fixture() -> (Vec<PublishEvent>, PublishEvent) {
    let start = Utc::now();
    let end = start + TimeDelta::milliseconds(1);
    let events = vec![
        PublishEvent {
            timestamp: start,
            package: "demo@1.0.0".into(),
            event_type: EventType::PackageAttempted {
                attempt: 1,
                command: "cargo publish".into(),
                max_attempts: 6,
            },
        },
        PublishEvent {
            timestamp: end,
            package: "demo@1.0.0".into(),
            event_type: EventType::PackageFailed {
                class: ErrorClass::Retryable,
                message: "timeout".into(),
            },
        },
    ];
    let completed = PublishEvent {
        timestamp: end,
        package: "demo@1.0.0".into(),
        event_type: EventType::PackageAttemptCompleted {
            detail: AttemptDetail {
                package: "demo".into(),
                version: "1.0.0".into(),
                attempt: 1,
                max_attempts: 2,
                started_at: start,
                ended_at: end,
                error_class: Some(ErrorClass::Retryable),
                next_attempt_at: None,
                redacted_message: Some("timeout".into()),
            },
        },
    };
    (events, completed)
}

fn rebuild(input: &[PublishEvent]) -> Result<ExecutionState> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("events.jsonl");
    let mut log = events::EventLog::new();
    for event in input {
        log.record(event.clone());
    }
    log.write_to_file(&path)?;
    rebuild_state_from_events(
        &path,
        StateRebuildOptions::new(Registry::crates_io()).with_fallback_plan_id("completion-test"),
    )
}

#[test]
fn completion_projects_exact_facts_after_failure_prefix_without_publication_claim() -> Result<()> {
    let (mut input, completed) = fixture();
    let prefix = rebuild(&input)?;
    ensure!(prefix.attempt_history.len() == 1);
    ensure!(
        prefix
            .attempt_history
            .first()
            .context("prefix detail")?
            .max_attempts
            == 6
    );
    input.push(completed.clone());
    let final_state = rebuild(&input)?;
    let EventType::PackageAttemptCompleted { detail } = completed.event_type else {
        bail!("fixture")
    };
    ensure!(final_state.attempt_history == vec![detail]);
    ensure!(matches!(
        final_state
            .packages
            .get("demo@1.0.0")
            .context("package")?
            .state,
        PackageState::Failed {
            class: ErrorClass::Retryable,
            ..
        }
    ));
    input.push(input.last().context("completion")?.clone());
    ensure!(
        rebuild(&input)?.attempt_history == final_state.attempt_history,
        "identical duplicate added a history row"
    );
    Ok(())
}

#[test]
fn completion_rejects_missing_foreign_conflicting_and_malformed_records() -> Result<()> {
    let (input, completed) = fixture();
    ensure!(
        rebuild(std::slice::from_ref(&completed)).is_err(),
        "missing attempt accepted"
    );
    for case in 0..9 {
        let mut changed = completed.clone();
        let EventType::PackageAttemptCompleted { detail } = &mut changed.event_type else {
            bail!("fixture")
        };
        match case {
            0 => changed.package = "other@1.0.0".into(),
            1 => detail.version = "2.0.0".into(),
            2 => detail.attempt = 2,
            3 => detail.started_at -= TimeDelta::milliseconds(1),
            4 => detail.ended_at = detail.started_at - TimeDelta::milliseconds(1),
            5 => detail.error_class = None,
            6 => detail.next_attempt_at = Some(changed.timestamp),
            7 => detail.max_attempts = 0,
            _ => detail.redacted_message = Some("different failure".into()),
        }
        let mut malformed = input.clone();
        malformed.push(changed);
        ensure!(
            rebuild(&malformed).is_err(),
            "malformed completion {case} accepted"
        );
    }
    let mut duplicate = completed.clone();
    let EventType::PackageAttemptCompleted { detail } = &mut duplicate.event_type else {
        bail!("fixture")
    };
    detail.max_attempts = 3;
    let mut conflict = input;
    conflict.extend([completed, duplicate]);
    ensure!(
        rebuild(&conflict).is_err(),
        "contradictory duplicate accepted"
    );
    Ok(())
}

#[test]
fn mixed_legacy_completion_records_preserve_permanent_history_order() -> Result<()> {
    let (mut input, completed) = fixture();
    let mut legacy = input.clone();
    for event in &mut legacy {
        event.package = "legacy@1.0.0".into();
    }
    if let Some(PublishEvent {
        event_type: EventType::PackageFailed { class, .. },
        ..
    }) = legacy.last_mut()
    {
        *class = ErrorClass::Permanent;
    }
    // The legacy terminal permanent attempt finishes while demo remains active.
    let attempted = input.remove(0);
    let mut interleaved = vec![attempted];
    interleaved.extend(legacy);
    interleaved.extend(input);
    let prefix = rebuild(&interleaved)?;
    ensure!(
        prefix
            .attempt_history
            .first()
            .context("legacy detail")?
            .package
            == "legacy"
    );
    interleaved.push(completed);
    let state = rebuild(&interleaved)?;
    ensure!(state.attempt_history.len() == 2);
    ensure!(
        state.attempt_history.first().context("legacy detail")?
            == prefix.attempt_history.first().context("legacy prefix")?
    );
    ensure!(
        state
            .attempt_history
            .last()
            .context("new detail")?
            .max_attempts
            == 2
    );
    Ok(())
}

#[test]
fn delayed_completion_cannot_discard_a_newer_active_attempt() -> Result<()> {
    let (mut input, completed) = fixture();
    input.push(PublishEvent {
        timestamp: completed.timestamp + TimeDelta::milliseconds(1),
        package: completed.package.clone(),
        event_type: EventType::PackageAttempted {
            attempt: 2,
            command: "cargo publish".into(),
            max_attempts: 6,
        },
    });
    let prefix = rebuild(&input)?;
    ensure!(prefix.attempt_history.len() == 2);
    input.push(completed);
    let error = rebuild(&input)
        .err()
        .context("late completion discarded active attempt")?;
    ensure!(error.to_string().contains("newer PackageAttempted"));
    Ok(())
}

#[test]
fn readiness_retry_preserves_completed_cargo_facts_for_new_and_legacy_logs() -> Result<()> {
    let (mut input, mut completed) = fixture();
    input.last_mut().context("upload event")?.event_type = EventType::PackageUploaded;
    let EventType::PackageAttemptCompleted { detail } = &mut completed.event_type else {
        bail!("fixture")
    };
    detail.max_attempts = 6;
    detail.error_class = None;
    detail.redacted_message = None;
    let expected = detail.clone();
    let wait = PublishEvent {
        timestamp: completed.timestamp + TimeDelta::milliseconds(1),
        package: completed.package.clone(),
        event_type: EventType::RetryScheduled {
            attempt: 1,
            max_attempts: 2,
            delay_ms: 3,
            next_attempt_at: completed.timestamp + TimeDelta::milliseconds(4),
            reason: ErrorClass::Ambiguous,
            message: "waiting for registry visibility".into(),
        },
    };
    for with_completion in [false, true] {
        let mut events = input.clone();
        if with_completion {
            events.push(completed.clone());
        }
        events.push(wait.clone());
        let rebuilt = rebuild(&events)?;
        ensure!(rebuilt.attempt_history == vec![expected.clone()]);
        ensure!(matches!(
            rebuilt.packages.get("demo@1.0.0").context("package")?.state,
            PackageState::Uploaded
        ));
    }
    let (mut failed, failure_completion) = fixture();
    failed.extend([failure_completion, wait]);
    ensure!(
        rebuild(&failed).is_err(),
        "failed completed Cargo attempt was rescheduled"
    );
    Ok(())
}
