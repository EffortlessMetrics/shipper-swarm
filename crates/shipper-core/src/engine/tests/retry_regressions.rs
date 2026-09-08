use super::*;
use anyhow::ensure;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct RetryRegistry {
    url: String,
    stop: Arc<AtomicBool>,
    hits: Arc<AtomicUsize>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RetryRegistry {
    fn start(status: u16) -> Result<Self> {
        Self::start_after_missing(status, 1)
    }

    fn start_after_missing(status: u16, missing_requests: usize) -> Result<Self> {
        let server = Server::http("127.0.0.1:0").map_err(|error| anyhow::anyhow!("{error}"))?;
        let url = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(AtomicUsize::new(0));
        let thread_stop = Arc::clone(&stop);
        let thread_hits = Arc::clone(&hits);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                if let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(5)) {
                    let previous = thread_hits.fetch_add(1, Ordering::Relaxed);
                    let response_status = if previous < missing_requests {
                        404
                    } else {
                        status
                    };
                    let _ = request
                        .respond(Response::from_string("{}").with_status_code(response_status));
                }
            }
        });
        Ok(Self {
            url,
            stop,
            hits,
            thread: Some(thread),
        })
    }
}

impl Drop for RetryRegistry {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn class_policy(max_attempts: u32) -> crate::retry::RetryStrategyConfig {
    crate::retry::RetryStrategyConfig {
        strategy: crate::retry::RetryStrategyType::Constant,
        max_attempts,
        base_delay: Duration::from_millis(3),
        max_delay: Duration::from_millis(3),
        jitter: 0.0,
    }
}

fn write_permanent_then_success_cargo(bin: &Path) -> Result<()> {
    #[cfg(windows)]
    fs::write(
        bin.join("cargo.cmd"),
        "@echo off\r\nif not \"%SHIPPER_CARGO_ARGS_LOG%\"==\"\" echo %*>>\"%SHIPPER_CARGO_ARGS_LOG%\"\r\nif exist \"%SHIPPER_RETRY_MARKER%\" exit /b 0\r\necho attempted>\"%SHIPPER_RETRY_MARKER%\"\r\necho permission denied 1>&2\r\nexit /b 1\r\n",
    )?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = bin.join("cargo");
        fs::write(
            &path,
            "#!/usr/bin/env sh\nset -eu\nprintf '%s\\n' \"$*\" >>\"$SHIPPER_CARGO_ARGS_LOG\"\nif [ -f \"$SHIPPER_RETRY_MARKER\" ]; then exit 0; fi\nprintf attempted >\"$SHIPPER_RETRY_MARKER\"\nprintf 'permission denied\\n' >&2\nexit 1\n",
        )?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn check_rebuild(state_dir: &Path, ws: &PlannedWorkspace) -> Result<ExecutionState> {
    let live = state::load_state(state_dir)?.context("missing live state")?;
    let rebuilt = crate::state::rebuild::rebuild_state_from_events(
        &events::events_path(state_dir),
        crate::state::rebuild::StateRebuildOptions::new(ws.plan.registry.clone())
            .with_fallback_plan_id(&ws.plan.plan_id),
    )?;
    ensure!(
        live.attempt_history == rebuilt.attempt_history,
        "attempt history differs: live={:?}; rebuilt={:?}",
        live.attempt_history,
        rebuilt.attempt_history
    );
    for (key, progress) in &live.packages {
        let projected = rebuilt
            .packages
            .get(key)
            .context("missing rebuilt package")?;
        ensure!(
            progress.state == projected.state && progress.attempts == projected.attempts,
            "package projection differs for {key}"
        );
    }
    Ok(live)
}

#[test]
#[serial]
fn per_error_retryable_and_permanent_limits_reach_cargo_and_event_projection() -> Result<()> {
    for (class, configured, attempts) in [
        (ErrorClass::Retryable, true, 2_u32),
        (ErrorClass::Permanent, false, 1),
        (ErrorClass::Permanent, true, 2),
    ] {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start(404)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 6;
        opts.readiness.enabled = false;
        if configured {
            match class {
                ErrorClass::Retryable => opts.retry_per_error.retryable = Some(class_policy(2)),
                ErrorClass::Permanent => opts.retry_per_error.permanent = Some(class_policy(2)),
                ErrorClass::Ambiguous => bail!("unexpected fixture class"),
            }
        }
        let output = if class == ErrorClass::Retryable {
            "timeout talking to server"
        } else {
            "permission denied"
        };
        with_test_env(
            &bin,
            vec![
                ("SHIPPER_CARGO_EXIT", Some("1".into())),
                ("SHIPPER_CARGO_STDERR", Some(output.into())),
                (
                    "SHIPPER_CARGO_ARGS_LOG",
                    Some(cargo_log.to_string_lossy().into_owned()),
                ),
            ],
            || -> Result<()> {
                ensure!(
                    run_publish(&workspace, &opts, &mut CollectingReporter::default()).is_err()
                );
                let invocations = fs::read_to_string(&cargo_log)?.lines().count();
                ensure!(
                    invocations == attempts as usize,
                    "class {class:?} invoked Cargo {invocations} times"
                );
                let live = check_rebuild(&state_dir, &workspace)?;
                ensure!(live.attempt_history.len() == attempts as usize);
                for detail in &live.attempt_history {
                    ensure!(detail.error_class.as_ref() == Some(&class));
                    ensure!(detail.max_attempts == if configured { 2 } else { 6 });
                    ensure!(detail.next_attempt_at.is_some() == (detail.attempt < attempts));
                }
                let log = events::EventLog::read_from_file(&events::events_path(&state_dir))?;
                let waits = log.all_events().iter().filter(|event| matches!(&event.event_type,
                EventType::RetryScheduled { max_attempts: 2, delay_ms: 3, reason, .. } if reason == &class)).count();
                ensure!(
                    waits == attempts.saturating_sub(1) as usize,
                    "class backoff not represented"
                );
                let state_before = fs::read(state::state_path(&state_dir))?;
                let events_before = fs::read(events::events_path(&state_dir))?;
                let calls_before = registry.hits.load(Ordering::Relaxed);
                if configured {
                    let error = run_resume(&workspace, &opts, &mut CollectingReporter::default())
                        .err()
                        .context("exhausted resume was admitted")?;
                    let budget = classify_resume_retry_budget(&error)
                        .context("missing budget classification")?;
                    ensure!(
                        budget.requested_max_attempts == 6 && budget.effective_max_attempts == 2
                    );
                    ensure!(fs::read(state::state_path(&state_dir))? == state_before);
                    ensure!(fs::read(events::events_path(&state_dir))? == events_before);
                    ensure!(registry.hits.load(Ordering::Relaxed) == calls_before);
                    ensure!(fs::read_to_string(&cargo_log)?.lines().count() == invocations);
                }
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[test]
#[serial]
fn per_error_permanent_retry_can_succeed_with_exact_rebuilt_history() -> Result<()> {
    let directory = tempdir()?;
    let bin = directory.path().join("bin");
    write_fake_tools(&bin);
    write_permanent_then_success_cargo(&bin)?;
    let cargo_log = directory.path().join("cargo.log");
    let marker = directory.path().join("attempted");
    // The initial probe and reconciliation after the first failed invocation
    // must both observe absence before the second invocation succeeds.
    let registry = RetryRegistry::start_after_missing(200, 2)?;
    let workspace = planned_workspace(directory.path(), registry.url.clone());
    let state_dir = directory.path().join(".shipper");
    let mut opts = default_opts(state_dir.clone());
    opts.max_attempts = 6;
    opts.retry_per_error.permanent = Some(class_policy(2));
    opts.readiness.enabled = false;
    with_test_env(
        &bin,
        vec![
            (
                "SHIPPER_CARGO_ARGS_LOG",
                Some(cargo_log.to_string_lossy().into_owned()),
            ),
            (
                "SHIPPER_RETRY_MARKER",
                Some(marker.to_string_lossy().into_owned()),
            ),
        ],
        || -> Result<()> {
            let receipt = run_publish(&workspace, &opts, &mut CollectingReporter::default())?;
            ensure!(receipt.packages.len() == 1);
            ensure!(matches!(
                receipt.packages.first().context("receipt package")?.state,
                PackageState::Published
            ));
            ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 2);
            let live = check_rebuild(&state_dir, &workspace)?;
            ensure!(live.attempt_history.len() == 2);
            let failed = live.attempt_history.first().context("failed attempt")?;
            ensure!(failed.attempt == 1 && failed.max_attempts == 2);
            ensure!(failed.error_class == Some(ErrorClass::Permanent));
            ensure!(failed.next_attempt_at.is_some());
            let succeeded = live.attempt_history.last().context("successful attempt")?;
            ensure!(succeeded.attempt == 2 && succeeded.error_class.is_none());
            ensure!(succeeded.next_attempt_at.is_none());
            Ok(())
        },
    )
}

#[test]
#[serial]
fn per_error_ambiguous_registry_truth_controls_retries_and_completion() -> Result<()> {
    for status in [404, 500, 200] {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start(status)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 6;
        opts.retry_per_error.ambiguous = Some(class_policy(2));
        opts.readiness.enabled = false;
        with_test_env(
            &bin,
            vec![
                ("SHIPPER_CARGO_EXIT", Some("1".into())),
                ("SHIPPER_CARGO_STDERR", None),
                (
                    "SHIPPER_CARGO_ARGS_LOG",
                    Some(cargo_log.to_string_lossy().into_owned()),
                ),
            ],
            || -> Result<()> {
                let result = run_publish(&workspace, &opts, &mut CollectingReporter::default());
                if status == 200 {
                    let receipt = result?;
                    ensure!(matches!(
                        receipt
                            .packages
                            .first()
                            .context("reconciled package")?
                            .state,
                        PackageState::Published
                    ));
                    ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 1);
                    let live = check_rebuild(&state_dir, &workspace)?;
                    ensure!(live.attempt_history.len() == 1);
                    let detail = live.attempt_history.first().context("reconciled attempt")?;
                    ensure!(
                        detail.max_attempts == 2
                            && detail.error_class == Some(ErrorClass::Ambiguous)
                    );
                    ensure!(detail.next_attempt_at.is_none());
                    let log = events::EventLog::read_from_file(&events::events_path(&state_dir))?;
                    ensure!(
                        !log.all_events().iter().any(|event| matches!(
                            event.event_type,
                            EventType::RetryScheduled { .. }
                        ))
                    );
                    return Ok(());
                }
                let error = result.err().context("ambiguous run succeeded")?;
                let calls = fs::read_to_string(&cargo_log)?.lines().count();
                ensure!(calls == if status == 404 { 2 } else { 1 });
                let live = check_rebuild(&state_dir, &workspace)?;
                ensure!(
                    live.attempt_history
                        .iter()
                        .all(|detail| detail.max_attempts == 2
                            && detail.error_class == Some(ErrorClass::Ambiguous))
                );
                let log = events::EventLog::read_from_file(&events::events_path(&state_dir))?;
                if status == 404 {
                    ensure!(matches!(
                        classify_publish_stop(&error),
                        Some(PublishStopClassification::RecoverableNotPublished {
                            evidence_consistent: true
                        })
                    ));
                    let reconciled = log
                        .all_events()
                        .iter()
                        .position(|event| {
                            matches!(
                                event.event_type,
                                EventType::PublishReconciled {
                                    outcome: ReconciliationOutcome::NotPublished { .. }
                                }
                            )
                        })
                        .context("no negative reconciliation")?;
                    let retried = log
                        .all_events()
                        .iter()
                        .position(|event| {
                            matches!(
                                event.event_type,
                                EventType::PackageAttempted { attempt: 2, .. }
                            )
                        })
                        .context("no second attempt")?;
                    ensure!(reconciled < retried);
                    let before = fs::read(events::events_path(&state_dir))?;
                    let hits = registry.hits.load(Ordering::Relaxed);
                    let rejection =
                        run_resume(&workspace, &opts, &mut CollectingReporter::default())
                            .err()
                            .context("class ceiling bypassed")?;
                    let budget =
                        classify_resume_retry_budget(&rejection).context("missing class budget")?;
                    ensure!(
                        budget.current_attempts == 2
                            && budget.requested_max_attempts == 6
                            && budget.effective_max_attempts == 2
                    );
                    ensure!(fs::read(events::events_path(&state_dir))? == before);
                    ensure!(registry.hits.load(Ordering::Relaxed) == hits);
                    let mut raised = opts.clone();
                    raised.max_attempts = 3;
                    raised.retry_per_error.ambiguous = Some(class_policy(3));
                    reject_exhausted_resume_retry_ceiling(&workspace, &raised, &live, true)?;
                    raised.retry_per_error.ambiguous = Some(class_policy(9));
                    let mut exhausted = live.clone();
                    exhausted
                        .packages
                        .get_mut("demo@0.1.0")
                        .context("package")?
                        .attempts = 3;
                    ensure!(
                        reject_exhausted_resume_retry_ceiling(
                            &workspace, &raised, &exhausted, true
                        )
                        .is_err()
                    );
                } else {
                    ensure!(matches!(
                        classify_publish_stop(&error),
                        Some(PublishStopClassification::StillUnknown { .. })
                    ));
                    ensure!(
                        !log.all_events().iter().any(|event| matches!(
                            event.event_type,
                            EventType::RetryScheduled { .. }
                        ))
                    );
                }
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[test]
fn invalid_effective_retry_options_reject_publish_and_resume_before_effects() -> Result<()> {
    let directory = tempdir()?;
    let workspace = planned_workspace(directory.path(), "http://127.0.0.1:9".into());
    for case in 0..6 {
        let state_dir = directory.path().join(format!("state-{case}"));
        let mut opts = default_opts(state_dir.clone());
        match case {
            0 => opts.max_attempts = 0,
            1 => opts.retry_jitter = f64::NAN,
            2 => opts.base_delay = opts.max_delay + Duration::from_secs(1),
            3 => opts.retry_per_error.retryable = Some(class_policy(0)),
            4 => {
                let mut policy = class_policy(2);
                policy.jitter = f64::INFINITY;
                opts.retry_per_error.permanent = Some(policy);
            }
            _ => {
                let mut policy = class_policy(2);
                policy.max_delay = Duration::ZERO;
                opts.retry_per_error.ambiguous = Some(policy);
            }
        }
        for resume in [false, true] {
            let mut reporter = CollectingReporter::default();
            let result = if resume {
                run_resume(&workspace, &opts, &mut reporter)
            } else {
                run_publish(&workspace, &opts, &mut reporter)
            };
            let error = result
                .err()
                .context("invalid runtime policy was accepted")?;
            ensure!(
                error.to_string().starts_with("retry"),
                "wrong rejection: {error:#}"
            );
            ensure!(!state_dir.exists());
            ensure!(
                reporter.infos.is_empty()
                    && reporter.warns.is_empty()
                    && reporter.errors.is_empty()
            );
        }
    }
    let mut immediate = default_opts(directory.path().join("valid"));
    immediate.base_delay = Duration::ZERO;
    immediate.max_delay = Duration::ZERO;
    immediate.retry_per_error.ambiguous = Some(crate::retry::RetryStrategyConfig {
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
        ..class_policy(2)
    });
    execute_package::retry_policy::validate_runtime_retry_options(&immediate)?;
    Ok(())
}
