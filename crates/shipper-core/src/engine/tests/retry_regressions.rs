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
    for ((class, configured, attempts), parallel) in [
        (ErrorClass::Retryable, true, 2_u32),
        (ErrorClass::Permanent, false, 1),
        (ErrorClass::Permanent, true, 2),
    ]
    .into_iter()
    .flat_map(|case| [false, true].map(|parallel| (case.clone(), parallel)))
    {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start(404)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 6;
        opts.parallel.enabled = parallel;
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
                    let mut direct_opts = opts.clone();
                    direct_opts.webhook.url = registry.url.clone();
                    let mut direct_state = live.clone();
                    let client = test_registry_client(workspace.plan.registry.clone())?;
                    let direct_error = parallel::run_publish_parallel(
                        &workspace,
                        &direct_opts,
                        &mut direct_state,
                        &state_dir,
                        &client,
                        &mut CollectingReporter::default(),
                    )
                    .err()
                    .context("direct parallel wrapper bypassed class ceiling")?;
                    ensure!(classify_resume_retry_budget(&direct_error).is_some());
                    ensure!(crate::webhook::active_test_deliveries() == 0);
                    ensure!(registry.hits.load(Ordering::Relaxed) == calls_before);
                    ensure!(fs::read(state::state_path(&state_dir))? == state_before);
                    ensure!(fs::read(events::events_path(&state_dir))? == events_before);
                    ensure!(fs::read_to_string(&cargo_log)?.lines().count() == invocations);
                    for resume in [false, true] {
                        let result = if resume {
                            run_resume(&workspace, &opts, &mut CollectingReporter::default())
                        } else {
                            run_publish(&workspace, &opts, &mut CollectingReporter::default())
                        };
                        let error = result
                            .err()
                            .context("exhausted retained state was admitted")?;
                        let budget = classify_resume_retry_budget(&error)
                            .context("missing budget classification")?;
                        ensure!(
                            budget.requested_max_attempts == 6
                                && budget.effective_max_attempts == 2
                        );
                        ensure!(fs::read(state::state_path(&state_dir))? == state_before);
                        ensure!(fs::read(events::events_path(&state_dir))? == events_before);
                        ensure!(registry.hits.load(Ordering::Relaxed) == calls_before);
                        ensure!(fs::read_to_string(&cargo_log)?.lines().count() == invocations);
                    }
                    let mut raised = opts.clone();
                    raised.max_attempts = 3;
                    match class {
                        ErrorClass::Retryable => {
                            raised.retry_per_error.retryable = Some(class_policy(3))
                        }
                        ErrorClass::Permanent => {
                            raised.retry_per_error.permanent = Some(class_policy(3))
                        }
                        ErrorClass::Ambiguous => bail!("fixture"),
                    }
                    ensure!(
                        run_publish(&workspace, &raised, &mut CollectingReporter::default())
                            .is_err()
                    );
                    ensure!(
                        fs::read_to_string(&cargo_log)?.lines().count() == invocations + 1,
                        "raised ceiling did not admit exactly one new attempt"
                    );
                    check_rebuild(&state_dir, &workspace)?;
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
    for (status, parallel) in [404, 500, 200]
        .into_iter()
        .flat_map(|status| [false, true].map(move |parallel| (status, parallel)))
    {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start(status)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 6;
        opts.parallel.enabled = parallel;
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
                    let mut direct_opts = opts.clone();
                    direct_opts.webhook.url = registry.url.clone();
                    let mut direct_state = live.clone();
                    let client = test_registry_client(workspace.plan.registry.clone())?;
                    let direct_error = parallel::run_publish_parallel(
                        &workspace,
                        &direct_opts,
                        &mut direct_state,
                        &state_dir,
                        &client,
                        &mut CollectingReporter::default(),
                    )
                    .err()
                    .context("direct parallel wrapper bypassed controlled class ceiling")?;
                    ensure!(classify_resume_retry_budget(&direct_error).is_some());
                    ensure!(crate::webhook::active_test_deliveries() == 0);
                    ensure!(fs::read(events::events_path(&state_dir))? == before);
                    ensure!(registry.hits.load(Ordering::Relaxed) == hits);
                    for resume in [false, true] {
                        let result = if resume {
                            run_resume(&workspace, &opts, &mut CollectingReporter::default())
                        } else {
                            run_publish(&workspace, &opts, &mut CollectingReporter::default())
                        };
                        let rejection = result.err().context("class ceiling bypassed")?;
                        let budget = classify_resume_retry_budget(&rejection)
                            .context("missing class budget")?;
                        ensure!(
                            budget.current_attempts == 2
                                && budget.requested_max_attempts == 6
                                && budget.effective_max_attempts == 2
                        );
                        ensure!(fs::read(events::events_path(&state_dir))? == before);
                        ensure!(registry.hits.load(Ordering::Relaxed) == hits);
                    }
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
                    // A lower-level caller may already hold its own advisory
                    // lock while resuming a coherent controlled stop.
                    let owned_lock =
                        lock::LockFile::acquire(&state_dir, Some(&workspace.workspace_root))?;
                    owned_lock.set_plan_id(&workspace.plan.plan_id)?;
                    let client = test_registry_client(workspace.plan.registry.clone())?;
                    let mut direct_state = live.clone();
                    let direct_error = parallel::run_publish_parallel(
                        &workspace,
                        &raised,
                        &mut direct_state,
                        &state_dir,
                        &client,
                        &mut CollectingReporter::default(),
                    )
                    .err()
                    .context("absent registry fixture unexpectedly published")?;
                    ensure!(classify_resume_retry_budget(&direct_error).is_none());
                    ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 3);
                    check_rebuild(&state_dir, &workspace)?;
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
#[serial]
fn invalid_effective_retry_options_reject_publish_and_resume_before_effects() -> Result<()> {
    let directory = tempdir()?;
    let registry = RetryRegistry::start(200)?;
    let workspace = planned_workspace(directory.path(), registry.url.clone());
    let client = test_registry_client(workspace.plan.registry.clone())?;
    let initial = init_state(&workspace, &directory.path().join("seed"))?;
    for case in 0..6 {
        let state_dir = directory.path().join(format!("state-{case}"));
        let mut opts = default_opts(state_dir.clone());
        opts.webhook.url = registry.url.clone();
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
        let mut direct_state = initial.clone();
        let mut reporter = CollectingReporter::default();
        let error = parallel::run_publish_parallel(
            &workspace,
            &opts,
            &mut direct_state,
            &state_dir,
            &client,
            &mut reporter,
        )
        .err()
        .context("direct parallel wrapper accepted invalid runtime policy")?;
        ensure!(error.to_string().starts_with("retry"));
        ensure!(!state_dir.exists());
        ensure!(crate::webhook::active_test_deliveries() == 0);
        ensure!(registry.hits.load(Ordering::Relaxed) == 0);
        ensure!(
            reporter.infos.is_empty() && reporter.warns.is_empty() && reporter.errors.is_empty()
        );
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

#[test]
#[serial]
fn repaired_permanent_failure_allows_manual_retry_but_not_automatic_retry() -> Result<()> {
    for (parallel, resume) in [(false, false), (false, true), (true, false), (true, true)] {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        write_permanent_then_success_cargo(&bin)?;
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start_after_missing(200, 2)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 3;
        opts.parallel.enabled = parallel;
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
                    Some(
                        directory
                            .path()
                            .join("repaired")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ),
            ],
            || -> Result<()> {
                ensure!(
                    run_publish(&workspace, &opts, &mut CollectingReporter::default()).is_err()
                );
                ensure!(
                    fs::read_to_string(&cargo_log)?.lines().count() == 1,
                    "permanent failure automatically retried"
                );
                let before = fs::read(events::events_path(&state_dir))?;
                let hits = registry.hits.load(Ordering::Relaxed);
                let mut exhausted = opts.clone();
                exhausted.max_attempts = 1;
                let rejected = if resume {
                    run_resume(&workspace, &exhausted, &mut CollectingReporter::default())
                } else {
                    run_publish(&workspace, &exhausted, &mut CollectingReporter::default())
                };
                ensure!(
                    classify_resume_retry_budget(
                        &rejected.err().context("exhausted manual retry admitted")?
                    )
                    .is_some()
                );
                ensure!(fs::read(events::events_path(&state_dir))? == before);
                ensure!(registry.hits.load(Ordering::Relaxed) == hits);
                let receipt = if resume {
                    run_resume(&workspace, &opts, &mut CollectingReporter::default())?
                } else {
                    run_publish(&workspace, &opts, &mut CollectingReporter::default())?
                };
                ensure!(matches!(
                    receipt.packages.first().context("package")?.state,
                    PackageState::Published
                ));
                ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 2);
                check_rebuild(&state_dir, &workspace)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[test]
#[serial]
fn retained_budget_is_rechecked_under_lock_after_a_stale_precheck() -> Result<()> {
    for (parallel, ambiguous) in [(false, false), (false, true), (true, false), (true, true)] {
        let directory = tempdir()?;
        let bin = directory.path().join("bin");
        write_fake_tools(&bin);
        let cargo_log = directory.path().join("cargo.log");
        let registry = RetryRegistry::start(404)?;
        let workspace = planned_workspace(directory.path(), registry.url.clone());
        let state_dir = directory.path().join(".shipper");
        let mut opts = default_opts(state_dir.clone());
        opts.max_attempts = 6;
        opts.parallel.enabled = parallel;
        opts.readiness.enabled = false;
        if ambiguous {
            opts.retry_per_error.ambiguous = Some(class_policy(1));
        } else {
            opts.retry_per_error.retryable = Some(class_policy(1));
        }
        with_test_env(
            &bin,
            vec![
                ("SHIPPER_CARGO_EXIT", Some("1".into())),
                (
                    "SHIPPER_CARGO_STDERR",
                    (!ambiguous).then(|| "timeout talking to server".into()),
                ),
                (
                    "SHIPPER_CARGO_ARGS_LOG",
                    Some(cargo_log.to_string_lossy().into_owned()),
                ),
            ],
            || -> Result<()> {
                ensure!(
                    run_publish(&workspace, &opts, &mut CollectingReporter::default()).is_err()
                );
                ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 1);
                if ambiguous {
                    opts.retry_per_error.ambiguous = Some(class_policy(2));
                } else {
                    opts.retry_per_error.retryable = Some(class_policy(2));
                }
                let stale = state::load_state(&state_dir)?.context("first retained state")?;
                validate_retained_publish_state(
                    &workspace,
                    &opts,
                    &state_dir,
                    &stale,
                    RetainedStateAdmission::BeforeLock,
                )?;

                // A competing invocation completes before the prechecked caller
                // obtains the lock. Use the real executor to advance all facts.
                ensure!(
                    run_publish(&workspace, &opts, &mut CollectingReporter::default()).is_err()
                );
                ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 2);
                check_rebuild(&state_dir, &workspace)?;
                let state_before = fs::read(state::state_path(&state_dir))?;
                let events_before = fs::read(events::events_path(&state_dir))?;
                let hits_before = registry.hits.load(Ordering::Relaxed);

                // Enter exactly where the stale caller continues. Bootstrap
                // acquires the lock and must validate its freshly loaded state.
                let error = publish::bootstrap::prepare_publish_run(
                    &workspace,
                    &opts,
                    &mut CollectingReporter::default(),
                )
                .err()
                .context("under-lock bootstrap admitted exhausted current state")?;
                let budget = classify_resume_retry_budget(&error).context("budget error")?;
                ensure!(
                    budget.current_attempts == 2
                        && budget.requested_max_attempts == 6
                        && budget.effective_max_attempts == 2
                );
                ensure!(fs::read(state::state_path(&state_dir))? == state_before);
                ensure!(fs::read(events::events_path(&state_dir))? == events_before);
                ensure!(registry.hits.load(Ordering::Relaxed) == hits_before);
                ensure!(fs::read_to_string(&cargo_log)?.lines().count() == 2);
                Ok(())
            },
        )?;
    }
    Ok(())
}

#[test]
fn observed_publish_lock_requires_the_same_owned_record() -> Result<()> {
    let directory = tempdir()?;
    let workspace = planned_workspace(directory.path(), "http://127.0.0.1:9".into());
    let state_dir = directory.path().join(".shipper");
    let lock = lock::LockFile::acquire(&state_dir, Some(&workspace.workspace_root))?;
    lock.set_plan_id(&workspace.plan.plan_id)?;
    let owned = publish::bootstrap::validate_observed_publish_lock(&workspace, &state_dir, None)?;
    let original = serde_json::to_vec(&owned)?;
    let path = lock::lock_path(&state_dir, Some(&workspace.workspace_root));
    publish::bootstrap::validate_observed_publish_lock(&workspace, &state_dir, Some(&owned))?;
    if !cfg!(target_os = "linux") {
        ensure!(owned.process_identity.is_none());
    }
    for case in 0..8 {
        let mut changed: lock::LockRecord = serde_json::from_slice(&original)?;
        match case {
            0 => fs::remove_file(&path)?,
            1 => fs::write(&path, b"corrupt")?,
            _ => {
                match case {
                    2 => changed.info.pid = owned.info.pid.wrapping_add(1),
                    3 => changed.info.hostname.push_str("-foreign"),
                    4 => changed.info.plan_id = Some("other-plan".into()),
                    5 => changed.info.acquired_at += chrono::Duration::seconds(1),
                    6 => {
                        changed.process_identity = Some(lock::ProcessIdentity {
                            boot_id: "other-boot".into(),
                            pid_namespace: "other-namespace".into(),
                            process_start_ticks: 0,
                        });
                    }
                    7 => changed.process_identity = None,
                    _ => bail!("unexpected fixture case"),
                }
                fs::write(&path, serde_json::to_vec(&changed)?)?;
            }
        }
        let result = publish::bootstrap::validate_observed_publish_lock(
            &workspace,
            &state_dir,
            Some(&owned),
        );
        if case == 7 && owned.process_identity.is_none() {
            result?;
        } else {
            ensure!(result.is_err(), "changed record case {case} was admitted");
        }
        fs::write(&path, &original)?;
    }
    // Optional identity is also a supported fallback on restricted Linux /proc.
    let mut without_identity: lock::LockRecord = serde_json::from_slice(&original)?;
    without_identity.process_identity = None;
    fs::write(&path, serde_json::to_vec(&without_identity)?)?;
    let captured =
        publish::bootstrap::validate_observed_publish_lock(&workspace, &state_dir, None)?;
    publish::bootstrap::validate_observed_publish_lock(&workspace, &state_dir, Some(&captured))?;
    Ok(())
}

#[test]
#[serial]
fn direct_parallel_admission_uses_level_scope_when_parallel_flag_is_false() -> Result<()> {
    let directory = tempdir()?;
    let registry = RetryRegistry::start(200)?;
    let mut workspace = planned_workspace(directory.path(), registry.url.clone());
    let mut sibling = workspace
        .plan
        .packages
        .first()
        .context("first package")?
        .clone();
    sibling.name = "later-sibling".into();
    workspace.plan.packages.push(sibling);
    let state_dir = directory.path().join(".shipper");
    let mut supplied = init_state(&workspace, &state_dir)?;
    let exhausted = supplied.packages.get_mut("demo@0.1.0").context("demo")?;
    exhausted.attempts = 2;
    exhausted.state = PackageState::Failed {
        class: ErrorClass::Retryable,
        message: "timeout".into(),
    };
    state::save_state(&state_dir, &supplied)?;
    let before = fs::read(state::state_path(&state_dir))?;
    let mut opts = default_opts(state_dir.clone());
    opts.max_attempts = 6;
    opts.retry_per_error.retryable = Some(class_policy(2));
    opts.parallel.enabled = false;
    opts.resume_from = Some("later-sibling".into());
    opts.webhook.url = registry.url.clone();
    let client = test_registry_client(workspace.plan.registry.clone())?;
    let error = parallel::run_publish_parallel(
        &workspace,
        &opts,
        &mut supplied,
        &state_dir,
        &client,
        &mut CollectingReporter::default(),
    )
    .err()
    .context("direct parallel resume skipped same-level exhausted package")?;
    let budget = classify_resume_retry_budget(&error).context("budget rejection")?;
    ensure!(budget.package == "demo@0.1.0" && budget.effective_max_attempts == 2);
    ensure!(crate::webhook::active_test_deliveries() == 0);
    ensure!(registry.hits.load(Ordering::Relaxed) == 0);
    ensure!(fs::read(state::state_path(&state_dir))? == before);
    ensure!(!events::events_path(&state_dir).exists());
    Ok(())
}
