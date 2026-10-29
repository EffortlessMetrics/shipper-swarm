use super::*;
use crate::lock;
use anyhow::ensure;

mod fixtures;
mod recovery;
use fixtures::Fixture;

#[derive(Clone, Copy, Debug)]
enum Entry {
    Publish,
    Resume,
    DirectParallel,
}

fn invoke(
    fixture: &Fixture,
    entry: Entry,
    opts: &RuntimeOptions,
    supplied: &mut ExecutionState,
) -> Result<()> {
    // A broken admission or negative-control mutation must still reach only
    // the refusing fake publisher, never the host's real Cargo executable.
    fixture.with_cargo("71", || {
        let mut reporter = CollectingReporter::default();
        match entry {
            Entry::Publish => run_publish(&fixture.ws, opts, &mut reporter).map(drop),
            Entry::Resume => run_resume(&fixture.ws, opts, &mut reporter).map(drop),
            Entry::DirectParallel => {
                let registry = test_registry_client(fixture.ws.plan.registry.clone())?;
                parallel::run_publish_parallel(
                    &fixture.ws,
                    opts,
                    supplied,
                    &fixture.state_dir,
                    &registry,
                    &mut reporter,
                )
                .map(drop)
            }
        }
    })
}

#[test]
#[serial]
fn every_entry_refuses_the_same_corrupted_controlled_stop_without_effects() -> Result<()> {
    for parallel in [false, true] {
        let fixture = Fixture::new(parallel, false);
        let supplied = fixture.seed_stop()?;
        let stopped = fixture.snapshot()?;
        for defect in [
            "missing-marker",
            "reordered-marker",
            "corrupt-reconciliation",
            "mismatched-reconciliation",
            "missing-reconciliation",
            "unexpected-receipt",
            "live-record",
            "unknown-record",
            "exhausted-ceiling",
            "missing-state",
        ] {
            fixture.restore(&stopped)?;
            let mut opts = fixture.opts.clone();
            opts.max_attempts = 2;
            opts.webhook.url = fixture.registry.url.clone();
            match defect {
                "missing-marker" | "reordered-marker" => {
                    let path = events::events_path(&fixture.state_dir);
                    let raw = fs::read_to_string(&path)?;
                    let mut lines: Vec<_> = raw.lines().collect();
                    let marker = lines.pop().context("terminal marker")?;
                    ensure!(marker.contains("execution_stopped"));
                    if defect == "reordered-marker" {
                        lines.insert(1, marker);
                    }
                    fs::write(path, format!("{}\n", lines.join("\n")))?;
                }
                "corrupt-reconciliation" => {
                    fs::write(state::reconciliation_path(&fixture.state_dir), b"{broken")?;
                }
                "mismatched-reconciliation" => {
                    let path = state::reconciliation_path(&fixture.state_dir);
                    let mut report: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
                    report["plan_id"] = "unrelated-plan".into();
                    fs::write(path, serde_json::to_vec(&report)?)?;
                }
                "missing-reconciliation" => {
                    fs::remove_file(state::reconciliation_path(&fixture.state_dir))?;
                }
                "unexpected-receipt" => {
                    fs::write(
                        state::receipt_path(&fixture.state_dir),
                        b"retained receipt sentinel",
                    )?;
                }
                "live-record" | "unknown-record" => {
                    let mut record = fixture.released_owner_record()?;
                    record["acquired_at"] =
                        serde_json::to_value(Utc::now() - chrono::Duration::days(30))?;
                    if defect == "unknown-record" {
                        record["hostname"] = "different-host".into();
                    }
                    fs::write(fixture.lock_path(), serde_json::to_vec(&record)?)?;
                    if defect == "live-record" && cfg!(target_os = "linux") {
                        ensure!(matches!(
                            crate::state::run_observation::observe_run(
                                &fixture.state_dir,
                                Some(&fixture.ws.workspace_root)
                            )?,
                            crate::cli_bridge::RunObservation::Stopped {
                                liveness: Some(crate::cli_bridge::RunLiveness::Live),
                                ..
                            }
                        ));
                    }
                }
                "exhausted-ceiling" => opts.max_attempts = 1,
                "missing-state" => fs::remove_file(state::state_path(&fixture.state_dir))?,
                _ => unreachable!(),
            }
            let retained = fixture.snapshot()?;
            for force in [false, true] {
                opts.force = force;
                for entry in [Entry::Publish, Entry::Resume, Entry::DirectParallel] {
                    let mut caller_state = supplied.clone();
                    let caller_before = serde_json::to_value(&caller_state)?;
                    let hits = fixture.registry.hits();
                    let cargo = fixture.cargo_calls();
                    let error = invoke(&fixture, entry, &opts, &mut caller_state)
                        .err()
                        .with_context(|| {
                            format!(
                                "{entry:?} admitted {defect}, parallel={parallel}, force={force}"
                            )
                        })?;
                    ensure!(is_publish_admission_error(&error), "{error:#}");
                    if defect == "exhausted-ceiling" {
                        let budget =
                            classify_resume_retry_budget(&error).context("typed ceiling")?;
                        ensure!(budget.current_attempts == 1 && budget.requested_max_attempts == 1);
                    }
                    ensure!(
                        fixture.snapshot()? == retained,
                        "{entry:?} mutated {defect}"
                    );
                    ensure!(
                        fixture.cargo_calls() == cargo,
                        "{entry:?} dispatched Cargo for {defect}"
                    );
                    ensure!(
                        fixture.registry.hits() == hits,
                        "{entry:?} contacted registry/webhook for {defect}"
                    );
                    ensure!(crate::webhook::active_test_deliveries() == 0);
                    ensure!(serde_json::to_value(&caller_state)? == caller_before);
                }
            }
        }
    }
    Ok(())
}

#[test]
#[serial]
fn final_admission_rechecks_the_ceiling_after_a_competing_run() -> Result<()> {
    for parallel in [false, true] {
        let fixture = Fixture::new(parallel, false);
        fixture.seed_stop()?;
        let mut opts = fixture.opts.clone();
        opts.max_attempts = 2;
        let required = publish::admission::precheck(&fixture.ws, &opts, &fixture.state_dir, false)?;
        ensure!(required);
        // Advance the retained tuple through the real executor while the first
        // caller has only a stale read-only precheck, not invocation ownership.
        fixture.with_cargo("1", || {
            run_publish(&fixture.ws, &opts, &mut CollectingReporter::default())
                .expect_err("second genuine controlled stop");
        });
        ensure!(fixture.cargo_calls() == 2);
        ensure!(fixture.retained()?.packages["demo@0.1.0"].attempts == 2);
        let retained = fixture.snapshot()?;
        let hits = fixture.registry.hits();
        let error = fixture.with_cargo("71", || {
            publish::bootstrap::prepare_publish_run(
                &fixture.ws,
                &opts,
                &mut CollectingReporter::default(),
                required,
            )
            .err()
            .context("final admission must reject the current ceiling")
        })?;
        let budget = classify_resume_retry_budget(&error).context("current typed budget")?;
        ensure!(budget.current_attempts == 2 && budget.requested_max_attempts == 2);
        ensure!(fixture.snapshot()? == retained);
        ensure!(fixture.cargo_calls() == 2 && fixture.registry.hits() == hits);
    }
    Ok(())
}

#[test]
#[serial]
fn final_admission_does_not_initialize_state_that_disappeared_after_precheck() -> Result<()> {
    for parallel in [false, true] {
        for remove_all_artifacts in [false, true] {
            let fixture = Fixture::new(parallel, false);
            fixture.seed_stop()?;
            let mut opts = fixture.opts.clone();
            opts.max_attempts = 2;
            let required =
                publish::admission::precheck(&fixture.ws, &opts, &fixture.state_dir, false)?;
            ensure!(required);
            let paths = fixture.snapshot()?;
            for (path, _) in paths {
                if (remove_all_artifacts || path == state::state_path(&fixture.state_dir))
                    && path.exists()
                {
                    fs::remove_file(path)?;
                }
            }
            let retained = fixture.snapshot()?;
            let hits = fixture.registry.hits();
            let error = fixture.with_cargo("71", || {
                publish::bootstrap::prepare_publish_run(
                    &fixture.ws,
                    &opts,
                    &mut CollectingReporter::default(),
                    required,
                )
                .err()
                .context("missing required state must refuse under ownership")
            })?;
            ensure!(is_publish_admission_error(&error));
            ensure!(format!("{error:#}").contains("retained state is required"));
            ensure!(fixture.snapshot()? == retained);
            ensure!(fixture.cargo_calls() == 1 && fixture.registry.hits() == hits);
        }
    }
    Ok(())
}

#[test]
#[serial]
fn final_admission_uses_the_reserved_predecessor_before_replacing_metadata() -> Result<()> {
    for parallel in [false, true] {
        for unknown in [false, true] {
            let fixture = Fixture::new(parallel, false);
            fixture.seed_stop()?;
            let mut opts = fixture.opts.clone();
            opts.max_attempts = 2;
            opts.force = true;
            let required =
                publish::admission::precheck(&fixture.ws, &opts, &fixture.state_dir, false)?;
            let mut prior = fixture.released_owner_record()?;
            if unknown {
                prior.as_object_mut().unwrap().remove("process_identity");
            }
            // The sidecar is unowned and a recognized protocol record is
            // recoverable at the ops layer. Semantic controlled-stop liveness
            // must still be checked using this retained record, not our own.
            fs::write(fixture.lock_path(), serde_json::to_vec(&prior)?)?;
            let retained = fixture.snapshot()?;
            let hits = fixture.registry.hits();
            let error = fixture.with_cargo("71", || {
                publish::bootstrap::prepare_publish_run(
                    &fixture.ws,
                    &opts,
                    &mut CollectingReporter::default(),
                    required,
                )
                .err()
                .context("changed predecessor must refuse before claim")
            })?;
            ensure!(is_publish_admission_error(&error));
            ensure!(format!("{error:#}").contains("controlled-stop"));
            ensure!(
                fixture.snapshot()? == retained,
                "predecessor JSON must remain exact"
            );
            ensure!(fixture.cargo_calls() == 1 && fixture.registry.hits() == hits);
        }
    }
    Ok(())
}

#[test]
#[serial]
fn publish_and_resume_raise_the_same_cumulative_ceiling_without_resetting_history() -> Result<()> {
    for parallel in [false, true] {
        for resume in [false, true] {
            let fixture = Fixture::new(parallel, false);
            let prior = fixture.seed_stop()?;
            let events_path = events::events_path(&fixture.state_dir);
            let prior_events = fs::read(&events_path)?;
            let mut opts = fixture.opts.clone();
            opts.max_attempts = 2;
            fixture.registry.respond_with(&[404, 200]);
            let receipt = fixture.with_cargo("0", || {
                if resume {
                    run_resume(&fixture.ws, &opts, &mut CollectingReporter::default())
                } else {
                    run_publish(&fixture.ws, &opts, &mut CollectingReporter::default())
                }
            })?;
            let after = fixture.retained()?;
            ensure!(receipt.execution_result == ExecutionResult::Success);
            ensure!(matches!(
                after.packages["demo@0.1.0"].state,
                PackageState::Published
            ));
            ensure!(after.packages["demo@0.1.0"].attempts == 2 && fixture.cargo_calls() == 2);
            ensure!(after.attempt_history.len() == 2);
            ensure!(
                serde_json::to_value(&after.attempt_history[..prior.attempt_history.len()])?
                    == serde_json::to_value(&prior.attempt_history)?
            );
            ensure!(fs::read(&events_path)?.starts_with(&prior_events));
            ensure!(fixture.registry.hits() == 4);
            let hits = fixture.registry.hits();
            fixture.with_cargo("71", || {
                run_publish(&fixture.ws, &opts, &mut CollectingReporter::default())
            })?;
            ensure!(fixture.cargo_calls() == 2 && fixture.registry.hits() == hits);
        }
    }
    Ok(())
}

#[test]
#[serial]
fn direct_parallel_uses_level_scope_even_when_the_parallel_flag_is_false() -> Result<()> {
    let fixture = Fixture::new(false, true);
    let mut supplied = fixture.seed_stop()?;
    let retained = fixture.snapshot()?;
    let mut opts = fixture.opts.clone();
    opts.parallel.enabled = false;
    opts.resume_from = Some("later-sibling".into());
    let error = invoke(&fixture, Entry::DirectParallel, &opts, &mut supplied)
        .err()
        .context("same-level exhausted package must be admitted together")?;
    let budget = classify_resume_retry_budget(&error).context("parallel level ceiling")?;
    ensure!(budget.package == "demo@0.1.0" && budget.current_attempts == 1);
    ensure!(
        fixture.snapshot()? == retained
            && fixture.cargo_calls() == 1
            && fixture.registry.hits() == 2
    );
    Ok(())
}

#[test]
#[serial]
fn direct_parallel_reloads_durable_state_and_refuses_a_separately_held_lock() -> Result<()> {
    let fixture = Fixture::new(false, false);
    let mut supplied = init_state(&fixture.ws, &fixture.state_dir)?;
    let owner = lock::LockFile::acquire(&fixture.state_dir, Some(&fixture.ws.workspace_root))?;
    owner.set_plan_id(&fixture.ws.plan.plan_id)?;
    let retained = fixture.snapshot()?;
    for force in [false, true] {
        let mut opts = fixture.opts.clone();
        opts.force = force;
        let error = invoke(&fixture, Entry::DirectParallel, &opts, &mut supplied)
            .err()
            .context("the wrapper cannot borrow an unprovided OS handle")?;
        ensure!(is_publish_admission_error(&error));
        ensure!(
            fixture.snapshot()? == retained
                && fixture.cargo_calls() == 0
                && fixture.registry.hits() == 0
        );
    }
    owner.release()?;
    let mut durable = supplied.clone();
    durable.packages.get_mut("demo@0.1.0").unwrap().state = PackageState::Skipped {
        reason: "already published".into(),
    };
    state::save_state(&fixture.state_dir, &durable)?;
    invoke(
        &fixture,
        Entry::DirectParallel,
        &fixture.opts,
        &mut supplied,
    )?;
    ensure!(serde_json::to_value(&supplied.packages)? == serde_json::to_value(&durable.packages)?);
    ensure!(fixture.cargo_calls() == 0 && fixture.registry.hits() == 0);
    Ok(())
}

#[test]
#[serial]
fn fresh_direct_parallel_requires_unattempted_memory_when_no_state_exists() -> Result<()> {
    let fixture = Fixture::new(false, false);
    let fresh = init_state(&fixture.ws, &fixture.state_dir)?;
    fs::remove_file(state::state_path(&fixture.state_dir))?;
    let mut stale = fresh.clone();
    stale.packages.get_mut("demo@0.1.0").unwrap().attempts = 1;
    let before = fixture.snapshot()?;
    ensure!(invoke(&fixture, Entry::DirectParallel, &fixture.opts, &mut stale).is_err());
    ensure!(
        fixture.snapshot()? == before && fixture.registry.hits() == 0 && fixture.cargo_calls() == 0
    );
    let mut supplied = fresh;
    // Older direct-library runs can have package events without run-level
    // lifecycle markers. Losing their state must not turn them into a new run.
    let mut orphaned = events::EventLog::new();
    orphaned.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::PackageStarted {
            name: "demo".into(),
            version: "0.1.0".into(),
        },
        package: "demo@0.1.0".into(),
    });
    let event_path = events::events_path(&fixture.state_dir);
    orphaned.write_to_file(&event_path)?;
    let orphaned_bytes = fixture.snapshot()?;
    ensure!(
        invoke(
            &fixture,
            Entry::DirectParallel,
            &fixture.opts,
            &mut supplied
        )
        .is_err()
    );
    ensure!(
        fixture.snapshot()? == orphaned_bytes
            && fixture.registry.hits() == 0
            && fixture.cargo_calls() == 0
    );
    fs::remove_file(event_path)?;
    fixture.registry.respond_with(&[200]);
    invoke(
        &fixture,
        Entry::DirectParallel,
        &fixture.opts,
        &mut supplied,
    )?;
    ensure!(matches!(
        supplied.packages["demo@0.1.0"].state,
        PackageState::Skipped { .. }
    ));
    ensure!(fixture.registry.hits() == 1 && fixture.cargo_calls() == 0);
    Ok(())
}

#[test]
#[serial]
fn preflight_and_rehearsal_history_do_not_prevent_a_fresh_publish() -> Result<()> {
    for parallel in [false, true] {
        let fixture = Fixture::new(parallel, false);
        fs::create_dir_all(&fixture.state_dir)?;
        let events_path = events::events_path(&fixture.state_dir);
        let mut log = events::EventLog::new();
        for event_type in [
            EventType::PlanCreated {
                plan_id: fixture.ws.plan.plan_id.clone(),
                package_count: 1,
            },
            EventType::PreflightStarted,
            EventType::PreflightComplete {
                finishability: crate::types::Finishability::Proven,
            },
            EventType::RehearsalComplete {
                passed: true,
                registry: "fixture".into(),
                plan_id: fixture.ws.plan.plan_id.clone(),
                summary: "separate rehearsal proof".into(),
            },
        ] {
            log.record(PublishEvent {
                timestamp: Utc::now(),
                event_type,
                package: "all".into(),
            });
        }
        log.write_to_file(&events_path)?;
        let prefix = fs::read(&events_path)?;
        fixture.registry.respond_with(&[404, 200]);
        let receipt = fixture.with_cargo("0", || {
            run_publish(
                &fixture.ws,
                &fixture.opts,
                &mut CollectingReporter::default(),
            )
        })?;
        ensure!(receipt.execution_result == ExecutionResult::Success);
        ensure!(fixture.cargo_calls() == 1 && fixture.registry.hits() == 2);
        ensure!(fs::read(&events_path)?.starts_with(&prefix));
    }
    Ok(())
}
