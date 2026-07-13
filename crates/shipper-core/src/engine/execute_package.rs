//! Canonical per-package execution and scheduler support.
//!
//! `publish_package` owns Cargo invocation, retries, readiness,
//! reconciliation, persistence, receipts, and notifications for one crate.
//! The parallel scheduler remains in `engine::parallel`; the module boundary
//! deliberately keeps package execution independent from scheduling.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use chrono::{DateTime, Utc};

use crate::ops::cargo;
use crate::plan::PlannedWorkspace;
use crate::runtime::execution::{
    backoff_delay, classify_cargo_failure, pkg_key, registry_aware_backoff, retry_after_delay,
    retry_next_attempt_at,
};
use crate::state::events;
use crate::state::execution_state as state;
use shipper_registry::HttpRegistryClient as RegistryClient;
use shipper_types::{
    AttemptDetail, AttemptEvidence, ErrorClass, EventType, ExecutionState, PackageEvidence,
    PackageReceipt, PackageState, PlannedPackage, PublishEvent, PublishRegime, ReadinessConfig,
    ReadinessEvidence, ReconciliationOutcome, RuntimeOptions,
};

use crate::engine::parallel::SendReporter;
use crate::engine::parallel::policy::policy_effects;
use crate::engine::parallel::readiness::is_version_visible_with_backoff_and_events;
use crate::engine::parallel::reconcile::reconcile_ambiguous_upload;
use crate::engine::parallel::webhook::{WebhookEvent, maybe_send_event};

/// Result of publishing a single package (for parallel execution)
#[derive(Debug)]
pub(crate) struct PackagePublishResult {
    pub(crate) result: anyhow::Result<PackageReceipt>,
}

/// Execute packages in plan order through the canonical package executor.
///
/// This is the sequential scheduler counterpart to
/// `parallel::scheduler::run_publish_level`: it owns ordering and resume
/// gating, while `publish_package_with_timeout` owns package behavior.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_sequential_scheduler(
    ws: &PlannedWorkspace,
    opts: &RuntimeOptions,
    st: &mut ExecutionState,
    state_dir: &Path,
    reg: &crate::registry::RegistryClient,
    event_log: &mut events::EventLog,
    events_path: &Path,
    reporter: &mut dyn crate::engine::Reporter,
) -> Result<Vec<PackageReceipt>> {
    if let Some(resume_from) = opts.resume_from.as_deref()
        && !ws.plan.packages.iter().any(|p| p.name == resume_from)
    {
        bail!("resume package not found in plan: {resume_from}");
    }

    let api_base = reg.registry().api_base.trim_end_matches('/');
    let reg_inner = shipper_registry::HttpRegistryClient::new(api_base);
    let st_arc = Arc::new(Mutex::new(st.clone()));
    let event_log_arc = Arc::new(Mutex::new(std::mem::replace(
        event_log,
        events::EventLog::new(),
    )));
    let send_reporter = Arc::new(crate::engine::parallel::SendReporter::default());
    let mut reached_resume_point = opts.resume_from.is_none();
    let mut receipts = Vec::new();

    for package in &ws.plan.packages {
        let key = pkg_key(&package.name, &package.version);
        let progress = st_arc
            .lock()
            .map_err(|_| anyhow::anyhow!("execution state lock poisoned before package execution"))?
            .packages
            .get(&key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing package progress in state"))?;

        if !reached_resume_point
            && matches!(
                crate::engine::publish::resume::apply_resume_from_gate(
                    package,
                    &progress,
                    opts,
                    &mut reached_resume_point,
                    reporter,
                ),
                crate::engine::publish::resume::ResumeGate::Skip
            )
        {
            if matches!(
                progress.state,
                PackageState::Published | PackageState::Skipped { .. }
            ) {
                let mut log = event_log_arc
                    .lock()
                    .map_err(|_| anyhow::anyhow!("event log lock poisoned before resume skip"))?;
                crate::engine::publish::resume::record_terminal_resume_skip_event(
                    &progress,
                    &format!("{}@{}", package.name, package.version),
                    events_path,
                    &mut log,
                )?;
            }
            continue;
        }

        if matches!(
            progress.state,
            PackageState::Published | PackageState::Skipped { .. }
        ) {
            let mut log = event_log_arc
                .lock()
                .map_err(|_| anyhow::anyhow!("event log lock poisoned before terminal skip"))?;
            crate::engine::publish::resume::record_terminal_resume_skip(
                package,
                &progress,
                &format!("{}@{}", package.name, package.version),
                events_path,
                &mut log,
                reporter,
            )?;
            continue;
        }

        if matches!(progress.state, PackageState::Uploaded) {
            reporter.info(&format!(
                "{}@{}: resuming from uploaded (skipping cargo publish)",
                package.name, package.version
            ));
        }

        let result = publish_package_with_timeout(
            package,
            ws,
            opts,
            &reg_inner,
            &st_arc,
            state_dir,
            &event_log_arc,
            events_path,
            &send_reporter,
            None,
        );
        crate::engine::parallel::drain_retry_waits_to_host(reporter, &send_reporter);
        crate::engine::parallel::replay_buffered_messages_to_host(reporter, &send_reporter);

        match result.result {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => {
                synchronize_sequential_state(st, event_log, &st_arc, &event_log_arc)?;
                return Err(error);
            }
        }
    }

    crate::engine::parallel::drain_retry_waits_to_host(reporter, &send_reporter);
    crate::engine::parallel::replay_buffered_messages_to_host(reporter, &send_reporter);
    synchronize_sequential_state(st, event_log, &st_arc, &event_log_arc)?;
    Ok(receipts)
}

fn synchronize_sequential_state(
    state: &mut ExecutionState,
    event_log: &mut events::EventLog,
    state_arc: &Arc<Mutex<ExecutionState>>,
    event_log_arc: &Arc<Mutex<events::EventLog>>,
) -> Result<()> {
    *state = state_arc
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned after package execution"))?
        .clone();
    *event_log = std::mem::replace(
        &mut *event_log_arc
            .lock()
            .map_err(|_| anyhow::anyhow!("event log lock poisoned after package execution"))?,
        events::EventLog::new(),
    );
    Ok(())
}

/// Build a poison failure for `publish_package`. The `PackagePublishResult`
/// return shape (not `Result`) means the `let-else` arms at each lock site
/// can't use `?`; this helper keeps them to one line and guarantees every
/// poison site reports the same message shape.
fn poisoned_lock(resource: &str) -> PackagePublishResult {
    PackagePublishResult {
        result: Err(anyhow::anyhow!(
            "{resource} lock poisoned during parallel publish"
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    new_state: PackageState,
    event: PublishEvent,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during package transition"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during package transition"))?;
    crate::engine::transition::commit(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        new_state,
        event,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_pending_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    new_state: PackageState,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during package transition"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during package transition"))?;
    crate::engine::transition::commit_pending(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        new_state,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_attempt_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    attempt: u32,
    event: PublishEvent,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during package attempt"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during package attempt"))?;
    crate::engine::transition::commit_attempt(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        attempt,
        event,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_with_attempt_detail_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    new_state: PackageState,
    event: PublishEvent,
    detail: AttemptDetail,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during package transition"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during package transition"))?;
    crate::engine::transition::commit_with_attempt_detail(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        new_state,
        event,
        detail,
    )
}

#[allow(clippy::too_many_arguments)]
fn commit_pending_with_attempt_detail_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    new_state: PackageState,
    detail: AttemptDetail,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during package transition"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during package transition"))?;
    crate::engine::transition::commit_pending_with_attempt_detail(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        new_state,
        detail,
    )
}

fn commit_attempt_detail_transition(
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    key: &str,
    detail: AttemptDetail,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during attempt detail"))?;
    let mut state = st
        .lock()
        .map_err(|_| anyhow::anyhow!("execution state lock poisoned during attempt detail"))?;
    crate::engine::transition::commit_attempt_detail_pending(
        &mut state,
        state_dir,
        &mut log,
        events_path,
        key,
        detail,
    )
}

/// Emit a [`EventType::RetryBackoffStarted`] event + a human-readable warn
/// line through the Reporter, then `thread::sleep(delay)`. Used at every
/// retry-backoff site in the publish loop so operators never stare at a
/// silent CI log during the wait window. See #91.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_retry_backoff(
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    reporter: &Arc<SendReporter>,
    pkg_label: &str,
    pkg_name: &str,
    pkg_version: &str,
    attempt: u32,
    max_attempts: u32,
    delay: std::time::Duration,
    next_attempt_at: DateTime<Utc>,
    reason: ErrorClass,
    message: &str,
) {
    let _ = record_retry_backoff(
        event_log,
        events_path,
        pkg_label,
        attempt,
        max_attempts,
        delay,
        next_attempt_at,
        &reason,
        message,
    );
    let _ = flush_event_log(event_log, events_path);
    wait_after_retry(
        reporter,
        pkg_name,
        pkg_version,
        attempt,
        max_attempts,
        delay,
        reason,
        message,
    );
}

#[allow(clippy::too_many_arguments)]
fn record_retry_backoff(
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    pkg_label: &str,
    attempt: u32,
    max_attempts: u32,
    delay: std::time::Duration,
    next_attempt_at: DateTime<Utc>,
    reason: &ErrorClass,
    message: &str,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during retry scheduling"))?;
    log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::RetryScheduled {
            attempt,
            max_attempts,
            delay_ms: delay.as_millis() as u64,
            next_attempt_at,
            reason: reason.clone(),
            message: message.to_string(),
        },
        package: pkg_label.to_string(),
    });
    log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::PublishWaiting {
            reason: "retry backoff".to_string(),
            delay_ms: delay.as_millis() as u64,
            until: next_attempt_at,
        },
        package: pkg_label.to_string(),
    });
    log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::RetryBackoffStarted {
            attempt,
            max_attempts,
            delay_ms: delay.as_millis() as u64,
            next_attempt_at,
            reason: reason.clone(),
            message: message.to_string(),
        },
        package: pkg_label.to_string(),
    });
    let _ = events_path;
    Ok(())
}

fn record_rate_limit_observed(
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    pkg_label: &str,
    is_new_crate: bool,
    retry_after: Option<std::time::Duration>,
    message: &str,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned during rate-limit observation"))?;
    log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::RateLimitObserved {
            is_new_crate,
            retry_after_ms: retry_after.map(|delay| delay.as_millis() as u64),
            message: message.to_string(),
        },
        package: pkg_label.to_string(),
    });
    let _ = events_path;
    Ok(())
}

fn flush_event_log(event_log: &Arc<Mutex<events::EventLog>>, events_path: &Path) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned while flushing events"))?;
    log.write_to_file(events_path)?;
    log.clear();
    Ok(())
}

fn record_readiness_event(
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    event: PublishEvent,
) -> Result<()> {
    let mut log = event_log
        .lock()
        .map_err(|_| anyhow::anyhow!("event log lock poisoned while recording readiness event"))?;
    log.record(event);
    log.write_to_file(events_path)?;
    log.clear();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wait_after_retry(
    reporter: &Arc<SendReporter>,
    pkg_name: &str,
    pkg_version: &str,
    attempt: u32,
    max_attempts: u32,
    delay: std::time::Duration,
    reason: ErrorClass,
    message: &str,
) {
    reporter.retry_wait(
        pkg_name,
        pkg_version,
        attempt,
        max_attempts,
        delay,
        reason,
        message,
    );
}

fn write_reconciliation_report_best_effort(
    state_dir: &Path,
    ws: &PlannedWorkspace,
    events_path: &Path,
    reporter: &Arc<SendReporter>,
) {
    if let Err(err) = crate::state::reconciliation::write_report_from_events(
        state_dir,
        &ws.plan.plan_id,
        &ws.plan.registry,
        events_path,
    ) {
        reporter.warn(&format!("failed to write reconciliation report: {err}"));
    }
}

/// Publish a single package with retries (parallel-safe version)
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_package(
    p: &PlannedPackage,
    ws: &PlannedWorkspace,
    opts: &RuntimeOptions,
    reg: &RegistryClient,
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    reporter: &Arc<SendReporter>,
) -> PackagePublishResult {
    publish_package_with_timeout(
        p,
        ws,
        opts,
        reg,
        st,
        state_dir,
        event_log,
        events_path,
        reporter,
        Some(opts.parallel.per_package_timeout),
    )
}

/// Execute a package with an explicit Cargo timeout policy. Sequential
/// scheduling passes `None`; parallel scheduling passes its per-package
/// timeout through the compatibility wrapper above.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_package_with_timeout(
    p: &PlannedPackage,
    ws: &PlannedWorkspace,
    opts: &RuntimeOptions,
    reg: &RegistryClient,
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    reporter: &Arc<SendReporter>,
    cargo_timeout: Option<Duration>,
) -> PackagePublishResult {
    let key = pkg_key(&p.name, &p.version);
    let pkg_label = format!("{}@{}", p.name, p.version);
    let started_at = Utc::now();
    let start_instant = Instant::now();

    // Record package started event
    {
        let Ok(mut log) = event_log.lock() else {
            return poisoned_lock("event log");
        };
        log.record(PublishEvent {
            timestamp: started_at,
            event_type: EventType::PackageStarted {
                name: p.name.clone(),
                version: p.version.clone(),
            },
            package: pkg_label.clone(),
        });
        let _ = log.write_to_file(events_path);
        log.clear();
    }

    // Check if already published
    if let Ok(true) = reg.version_exists(&p.name, &p.version) {
        reporter.info(&format!(
            "{}@{}: already published (skipping)",
            p.name, p.version
        ));

        let skipped = PackageState::Skipped {
            reason: "already published".to_string(),
        };
        if let Err(err) = commit_transition(
            st,
            state_dir,
            event_log,
            events_path,
            &key,
            skipped.clone(),
            PublishEvent {
                timestamp: Utc::now(),
                event_type: EventType::PackageSkipped {
                    reason: "already published".to_string(),
                },
                package: pkg_label.clone(),
            },
        ) {
            return PackagePublishResult { result: Err(err) };
        }

        return PackagePublishResult {
            result: Ok(PackageReceipt {
                name: p.name.clone(),
                version: p.version.clone(),
                attempts: 0,
                state: skipped,
                started_at,
                finished_at: Utc::now(),
                duration_ms: start_instant.elapsed().as_millis(),
                evidence: PackageEvidence {
                    attempts: vec![],
                    readiness_checks: vec![],
                },
                compromised_at: None,
                compromised_by: None,
                superseded_by: None,
            }),
        };
    }

    reporter.info(&format!("{}@{}: publishing...", p.name, p.version));

    let mut attempt = 0u32;
    let mut last_err: Option<(ErrorClass, String)> = None;
    let mut attempt_evidence: Vec<AttemptEvidence> = Vec::new();
    let mut readiness_evidence: Vec<ReadinessEvidence> = Vec::new();
    let mut cargo_succeeded = false;

    // Check if resuming from Uploaded state (cargo publish succeeded previously)
    {
        let Ok(state) = st.lock() else {
            return poisoned_lock("execution state");
        };
        if let Some(pr) = state.packages.get(&key)
            && matches!(pr.state, PackageState::Uploaded)
        {
            cargo_succeeded = true;
        }
    }

    // Apply policy effects for readiness (Fix 7: parallel mode must respect PublishPolicy::Fast)
    let effects = policy_effects(opts);
    let readiness_config = ReadinessConfig {
        enabled: effects.readiness_enabled,
        ..opts.readiness.clone()
    };

    // Resume-path reconciliation (#99 follow-on): if a prior run left this
    // package in PackageState::Ambiguous, reconcile against registry truth
    // BEFORE entering the retry loop so we never re-upload a crate whose
    // prior upload may have actually succeeded.
    let ambiguous_prior: Option<String> = {
        let Ok(state) = st.lock() else {
            return poisoned_lock("execution state");
        };
        state.packages.get(&key).and_then(|pr| {
            if let PackageState::Ambiguous { message } = &pr.state {
                Some(message.clone())
            } else {
                None
            }
        })
    };

    if let Some(prior_reason) = ambiguous_prior {
        reporter.warn(&format!(
            "{}@{}: resume found ambiguous state ({}); reconciling against registry",
            p.name, p.version, prior_reason
        ));
        {
            let Ok(mut log) = event_log.lock() else {
                return poisoned_lock("event log");
            };
            log.record(PublishEvent {
                timestamp: Utc::now(),
                event_type: EventType::PublishReconciling {
                    method: readiness_config.method,
                },
                package: pkg_label.clone(),
            });
        }

        let (outcome, _evidence) =
            reconcile_ambiguous_upload(reg, &p.name, &p.version, &readiness_config);

        {
            let Ok(mut log) = event_log.lock() else {
                return poisoned_lock("event log");
            };
            log.record(PublishEvent {
                timestamp: Utc::now(),
                event_type: EventType::PublishReconciled {
                    outcome: outcome.clone(),
                },
                package: pkg_label.clone(),
            });
        }
        let reconciliation_report_path = state::reconciliation_path(state_dir);

        match outcome {
            ReconciliationOutcome::Published { .. } => {
                if let Err(e) = commit_transition(
                    st,
                    state_dir,
                    event_log,
                    events_path,
                    &key,
                    PackageState::Published,
                    PublishEvent {
                        timestamp: Utc::now(),
                        event_type: EventType::PackagePublished {
                            duration_ms: start_instant.elapsed().as_millis() as u64,
                        },
                        package: pkg_label.clone(),
                    },
                ) {
                    return PackagePublishResult { result: Err(e) };
                }
                write_reconciliation_report_best_effort(state_dir, ws, events_path, reporter);
                reporter.info(&format!(
                    "{}@{}: reconciliation outcome: Published; action: mark published and continue without republish (evidence: {})",
                    p.name,
                    p.version,
                    reconciliation_report_path.display()
                ));
                return PackagePublishResult {
                    result: Ok(PackageReceipt {
                        name: p.name.clone(),
                        version: p.version.clone(),
                        attempts: 0,
                        state: PackageState::Published,
                        started_at,
                        finished_at: Utc::now(),
                        duration_ms: start_instant.elapsed().as_millis(),
                        evidence: PackageEvidence {
                            attempts: vec![],
                            readiness_checks: vec![],
                        },
                        compromised_at: None,
                        compromised_by: None,
                        superseded_by: None,
                    }),
                };
            }
            ReconciliationOutcome::NotPublished { .. } => {
                if let Err(e) = commit_pending_transition(
                    st,
                    state_dir,
                    event_log,
                    events_path,
                    &key,
                    PackageState::Pending,
                ) {
                    return PackagePublishResult { result: Err(e) };
                }
                write_reconciliation_report_best_effort(state_dir, ws, events_path, reporter);
                reporter.info(&format!(
                    "{}@{}: reconciliation outcome: NotPublished; action: retry under publish policy (evidence: {})",
                    p.name,
                    p.version,
                    reconciliation_report_path.display()
                ));
                // Fall through to the normal retry loop below.
            }
            ReconciliationOutcome::StillUnknown { reason, .. } => {
                if let Err(e) = commit_pending_transition(
                    st,
                    state_dir,
                    event_log,
                    events_path,
                    &key,
                    PackageState::Ambiguous {
                        message: reason.clone(),
                    },
                ) {
                    return PackagePublishResult { result: Err(e) };
                }
                write_reconciliation_report_best_effort(state_dir, ws, events_path, reporter);
                reporter.error(&format!(
                    "{}@{}: reconciliation outcome: StillUnknown; action: stop before blind retry; operator action required (evidence: {}): {}",
                    p.name,
                    p.version,
                    reconciliation_report_path.display(),
                    reason
                ));
                maybe_send_event(
                    &opts.webhook,
                    WebhookEvent::PublishFailed {
                        plan_id: ws.plan.plan_id.clone(),
                        package_name: p.name.clone(),
                        package_version: p.version.clone(),
                        error_class: format!("{:?}", ErrorClass::Ambiguous),
                        message: format!("resume reconciliation still inconclusive: {reason}"),
                    },
                );
                return PackagePublishResult {
                    result: Err(anyhow::anyhow!(
                        "{}@{}: resume reconciliation still inconclusive; operator action required. Prior reason: {}",
                        p.name,
                        p.version,
                        reason
                    )),
                };
            }
        }
    }

    // Registry-aware backoff (#94 / #106 PR 1): prefer the `PublishRegime`
    // that preflight stamped onto the `PlannedPackage`. That answer is
    // authoritative; when it is present we never re-query the registry
    // mid-retry for "is this a brand-new crate?"
    //
    // `None` here means an older plan / state.json predating the regime
    // field, or a test harness that constructed a `PlannedPackage`
    // directly without populating it. In that case we fall back to the
    // legacy lazy-cached behavior so we remain backward compatible.
    let mut is_new_crate_cached: Option<bool> = p.regime.map(PublishRegime::is_new_crate);

    while attempt < opts.max_attempts {
        attempt += 1;

        let command = format!(
            "cargo publish -p {} --registry {}",
            p.name, ws.plan.registry.name
        );

        reporter.info(&format!(
            "{}@{}: attempt {}/{}",
            p.name, p.version, attempt, opts.max_attempts
        ));

        if !cargo_succeeded {
            let attempt_started_at = Utc::now();
            if let Err(e) = commit_attempt_transition(
                st,
                state_dir,
                event_log,
                events_path,
                &key,
                attempt,
                PublishEvent {
                    timestamp: attempt_started_at,
                    event_type: EventType::PackageAttempted {
                        attempt,
                        command: command.clone(),
                    },
                    package: pkg_label.clone(),
                },
            ) {
                return PackagePublishResult { result: Err(e) };
            }

            let out = match cargo::cargo_publish(
                &ws.workspace_root,
                &p.name,
                &ws.plan.registry.name,
                opts.allow_dirty,
                opts.no_verify,
                opts.output_lines,
                cargo_timeout,
            ) {
                Ok(o) => o,
                Err(e) => {
                    reporter.error(&format!(
                        "{}@{}: cargo publish failed to execute: {}",
                        p.name, p.version, e
                    ));
                    return PackagePublishResult { result: Err(e) };
                }
            };
            let attempt_ended_at = Utc::now();

            // Collect attempt evidence
            attempt_evidence.push(AttemptEvidence {
                attempt_number: attempt,
                command: command.clone(),
                exit_code: out.exit_code,
                stdout_tail: out.stdout_tail.clone(),
                stderr_tail: out.stderr_tail.clone(),
                timestamp: attempt_ended_at,
                duration: out.duration,
            });

            // Event: PackageOutput
            {
                let Ok(mut log) = event_log.lock() else {
                    return poisoned_lock("event log");
                };
                log.record(PublishEvent {
                    timestamp: Utc::now(),
                    event_type: EventType::PackageOutput {
                        stdout_tail: out.stdout_tail.clone(),
                        stderr_tail: out.stderr_tail.clone(),
                    },
                    package: pkg_label.clone(),
                });
                let _ = log.write_to_file(events_path);
                log.clear();
            }

            if out.exit_code == 0 && !out.timed_out {
                let attempt_detail = AttemptDetail {
                    package: p.name.clone(),
                    version: p.version.clone(),
                    attempt,
                    max_attempts: opts.max_attempts,
                    started_at: attempt_started_at,
                    ended_at: attempt_ended_at,
                    error_class: None,
                    next_attempt_at: None,
                    redacted_message: None,
                };
                cargo_succeeded = true;
                // ReadinessStarted is the durable checkpoint that proves
                // cargo accepted the upload and projects Uploaded on rebuild.
                if let Err(e) = commit_with_attempt_detail_transition(
                    st,
                    state_dir,
                    event_log,
                    events_path,
                    &key,
                    PackageState::Uploaded,
                    PublishEvent {
                        timestamp: Utc::now(),
                        event_type: EventType::ReadinessStarted {
                            method: readiness_config.method,
                        },
                        package: pkg_label.clone(),
                    },
                    attempt_detail,
                ) {
                    return PackagePublishResult { result: Err(e) };
                }
            } else {
                // Cargo failed, check registry
                reporter.warn(&format!(
                    "{}@{}: cargo publish failed (exit={}); checking registry...",
                    p.name, p.version, out.exit_code
                ));

                let failure_output = format!("{}\n{}", out.stderr_tail, out.stdout_tail);
                let (class, msg) = classify_cargo_failure(&out.stderr_tail, &out.stdout_tail);
                last_err = Some((class.clone(), msg.clone()));
                let mut attempt_detail = AttemptDetail {
                    package: p.name.clone(),
                    version: p.version.clone(),
                    attempt,
                    max_attempts: opts.max_attempts,
                    started_at: attempt_started_at,
                    ended_at: attempt_ended_at,
                    error_class: Some(class.clone()),
                    next_attempt_at: None,
                    redacted_message: Some(msg.clone()),
                };

                if reg.version_exists(&p.name, &p.version).unwrap_or(false) {
                    reporter.info(&format!(
                        "{}@{}: version is present on registry; treating as published",
                        p.name, p.version
                    ));

                    if let Err(e) = commit_with_attempt_detail_transition(
                        st,
                        state_dir,
                        event_log,
                        events_path,
                        &key,
                        PackageState::Published,
                        PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::PackagePublished {
                                duration_ms: start_instant.elapsed().as_millis() as u64,
                            },
                            package: pkg_label.clone(),
                        },
                        attempt_detail,
                    ) {
                        return PackagePublishResult { result: Err(e) };
                    }
                    last_err = None;
                    break;
                }

                // Event: PackageFailed
                {
                    let Ok(mut log) = event_log.lock() else {
                        return poisoned_lock("event log");
                    };
                    log.record(PublishEvent {
                        timestamp: Utc::now(),
                        event_type: EventType::PackageFailed {
                            class: class.clone(),
                            message: msg.clone(),
                        },
                        package: pkg_label.clone(),
                    });
                }

                // On Ambiguous: never blind-retry. Reconcile against registry
                // truth first so we don't risk a duplicate upload. See #99.
                if class == ErrorClass::Ambiguous {
                    {
                        let Ok(mut log) = event_log.lock() else {
                            return poisoned_lock("event log");
                        };
                        log.record(PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::PublishReconciling {
                                method: readiness_config.method,
                            },
                            package: pkg_label.clone(),
                        });
                    }
                    reporter.warn(&format!(
                        "{}@{}: cargo exit ambiguous; reconciling against registry",
                        p.name, p.version
                    ));

                    let (outcome, reconcile_evidence) =
                        reconcile_ambiguous_upload(reg, &p.name, &p.version, &readiness_config);

                    {
                        let Ok(mut log) = event_log.lock() else {
                            return poisoned_lock("event log");
                        };
                        log.record(PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::PublishReconciled {
                                outcome: outcome.clone(),
                            },
                            package: pkg_label.clone(),
                        });
                    }
                    let reconciliation_report_path = state::reconciliation_path(state_dir);

                    match outcome {
                        ReconciliationOutcome::Published { .. } => {
                            reporter.info(&format!(
                                "{}@{}: reconciliation outcome: Published; registry shows version present; action: mark published and continue without retry (evidence: {})",
                                p.name,
                                p.version,
                                reconciliation_report_path.display()
                            ));
                            if let Err(e) = commit_with_attempt_detail_transition(
                                st,
                                state_dir,
                                event_log,
                                events_path,
                                &key,
                                PackageState::Published,
                                PublishEvent {
                                    timestamp: Utc::now(),
                                    event_type: EventType::PackagePublished {
                                        duration_ms: start_instant.elapsed().as_millis() as u64,
                                    },
                                    package: pkg_label.clone(),
                                },
                                attempt_detail.clone(),
                            ) {
                                return PackagePublishResult { result: Err(e) };
                            }
                            write_reconciliation_report_best_effort(
                                state_dir,
                                ws,
                                events_path,
                                reporter,
                            );

                            // Preserve reconciliation evidence in the receipt.
                            // Do NOT emit PublishSucceeded webhook here ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â the
                            // end-of-function success path (below) handles that.
                            readiness_evidence = reconcile_evidence;
                            last_err = None;
                            break;
                        }
                        ReconciliationOutcome::NotPublished { .. } => {
                            {
                                let Ok(mut log) = event_log.lock() else {
                                    return poisoned_lock("event log");
                                };
                                if let Err(e) = log.write_to_file(events_path) {
                                    return PackagePublishResult { result: Err(e) };
                                }
                                log.clear();
                            }
                            write_reconciliation_report_best_effort(
                                state_dir,
                                ws,
                                events_path,
                                reporter,
                            );
                            reporter.info(&format!(
                                "{}@{}: reconciliation outcome: NotPublished; registry still absent; action: retry under publish policy (evidence: {})",
                                p.name,
                                p.version,
                                reconciliation_report_path.display()
                            ));
                            // Safe to enter the normal Retryable path below;
                            // registry confirms no duplicate-upload risk.
                            // Preserve negative-polling evidence for the receipt.
                            readiness_evidence = reconcile_evidence;
                        }
                        ReconciliationOutcome::StillUnknown { reason, .. } => {
                            let ambiguous_state = PackageState::Ambiguous {
                                message: reason.clone(),
                            };
                            if let Err(e) = commit_pending_with_attempt_detail_transition(
                                st,
                                state_dir,
                                event_log,
                                events_path,
                                &key,
                                ambiguous_state,
                                attempt_detail.clone(),
                            ) {
                                return PackagePublishResult { result: Err(e) };
                            }
                            write_reconciliation_report_best_effort(
                                state_dir,
                                ws,
                                events_path,
                                reporter,
                            );
                            reporter.error(&format!(
                                "{}@{}: reconciliation outcome: StillUnknown; action: stop before blind retry; operator action required (evidence: {}): {}",
                                p.name,
                                p.version,
                                reconciliation_report_path.display(),
                                reason
                            ));

                            // Notify operators: reconciliation was inconclusive
                            // and human judgment is required.
                            maybe_send_event(
                                &opts.webhook,
                                WebhookEvent::PublishFailed {
                                    plan_id: ws.plan.plan_id.clone(),
                                    package_name: p.name.clone(),
                                    package_version: p.version.clone(),
                                    error_class: format!("{:?}", ErrorClass::Ambiguous),
                                    message: format!("reconciliation inconclusive: {reason}"),
                                },
                            );

                            return PackagePublishResult {
                                result: Err(anyhow::anyhow!(
                                    "{}@{}: reconciliation inconclusive: {}",
                                    p.name,
                                    p.version,
                                    reason
                                )),
                            };
                        }
                    }
                }

                match class {
                    ErrorClass::Permanent => {
                        let failed = PackageState::Failed {
                            class: class.clone(),
                            message: msg.clone(),
                        };
                        if let Err(e) = commit_pending_with_attempt_detail_transition(
                            st,
                            state_dir,
                            event_log,
                            events_path,
                            &key,
                            failed,
                            attempt_detail,
                        ) {
                            return PackagePublishResult { result: Err(e) };
                        }

                        // Send webhook notification: package failed
                        maybe_send_event(
                            &opts.webhook,
                            WebhookEvent::PublishFailed {
                                plan_id: ws.plan.plan_id.clone(),
                                package_name: p.name.clone(),
                                package_version: p.version.clone(),
                                error_class: format!("{:?}", class),
                                message: msg.clone(),
                            },
                        );

                        return PackagePublishResult {
                            result: Err(anyhow::anyhow!(
                                "{}@{}: permanent failure: {}",
                                p.name,
                                p.version,
                                msg
                            )),
                        };
                    }
                    ErrorClass::Retryable | ErrorClass::Ambiguous => {
                        // Ambiguous can only reach here if reconciliation
                        // returned NotPublished ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â registry confirms no
                        // duplicate-upload risk, so cargo retry is safe.
                        // Only query crate_exists when the error looks like
                        // a rate limit (saves a registry round-trip for
                        // generic network/transient failures).
                        let is_new_crate =
                            if crate::runtime::execution::looks_like_rate_limit(&failure_output) {
                                *is_new_crate_cached.get_or_insert_with(|| {
                                    !reg.crate_exists(&p.name).unwrap_or(true)
                                })
                            } else {
                                false
                            };
                        if attempt < opts.max_attempts {
                            if crate::runtime::execution::looks_like_rate_limit(&failure_output)
                                && let Err(e) = record_rate_limit_observed(
                                    event_log,
                                    events_path,
                                    &pkg_label,
                                    is_new_crate,
                                    retry_after_delay(&failure_output),
                                    &msg,
                                )
                            {
                                return PackagePublishResult { result: Err(e) };
                            }
                            let delay = registry_aware_backoff(
                                opts.base_delay,
                                opts.max_delay,
                                attempt,
                                opts.retry_strategy,
                                opts.retry_jitter,
                                is_new_crate,
                                &failure_output,
                            );
                            let next_attempt_at = retry_next_attempt_at(delay);
                            attempt_detail.next_attempt_at = Some(next_attempt_at);
                            if let Err(e) = record_retry_backoff(
                                event_log,
                                events_path,
                                &pkg_label,
                                attempt,
                                opts.max_attempts,
                                delay,
                                next_attempt_at,
                                &class,
                                &msg,
                            ) {
                                return PackagePublishResult { result: Err(e) };
                            }
                            if let Err(e) = commit_attempt_detail_transition(
                                st,
                                state_dir,
                                event_log,
                                events_path,
                                &key,
                                attempt_detail,
                            ) {
                                return PackagePublishResult { result: Err(e) };
                            }
                            wait_after_retry(
                                reporter,
                                &p.name,
                                &p.version,
                                attempt,
                                opts.max_attempts,
                                delay,
                                class.clone(),
                                &msg,
                            );
                        } else if let Err(e) = commit_attempt_detail_transition(
                            st,
                            state_dir,
                            event_log,
                            events_path,
                            &key,
                            attempt_detail,
                        ) {
                            return PackagePublishResult { result: Err(e) };
                        }
                    }
                }
                continue;
            }
        }

        // Readiness verification (runs after first cargo success + all retries)
        reporter.info(&format!(
            "{}@{}: cargo publish exited successfully; verifying...",
            p.name, p.version
        ));

        let readiness_started_at = Instant::now();
        if let Err(e) = record_readiness_event(
            event_log,
            events_path,
            PublishEvent {
                timestamp: Utc::now(),
                event_type: EventType::ReadinessStarted {
                    method: readiness_config.method,
                },
                package: pkg_label.clone(),
            },
        ) {
            return PackagePublishResult { result: Err(e) };
        }
        let mut emit_readiness_event =
            |event| record_readiness_event(event_log, events_path, event);
        let verify_result = is_version_visible_with_backoff_and_events(
            reg,
            &p.name,
            &p.version,
            &readiness_config,
            &mut emit_readiness_event,
        );

        match verify_result {
            Ok((visible, checks)) => {
                readiness_evidence = checks;
                if visible {
                    if let Err(e) = record_readiness_event(
                        event_log,
                        events_path,
                        PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::ReadinessComplete {
                                duration_ms: readiness_started_at.elapsed().as_millis() as u64,
                                attempts: readiness_evidence.len() as u32,
                            },
                            package: pkg_label.clone(),
                        },
                    ) {
                        return PackagePublishResult { result: Err(e) };
                    }
                    if let Err(e) = commit_transition(
                        st,
                        state_dir,
                        event_log,
                        events_path,
                        &key,
                        PackageState::Published,
                        PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::PackagePublished {
                                duration_ms: start_instant.elapsed().as_millis() as u64,
                            },
                            package: pkg_label.clone(),
                        },
                    ) {
                        return PackagePublishResult { result: Err(e) };
                    }
                    last_err = None;

                    // Send webhook notification: package succeeded
                    maybe_send_event(
                        &opts.webhook,
                        WebhookEvent::PublishSucceeded {
                            plan_id: ws.plan.plan_id.clone(),
                            package_name: p.name.clone(),
                            package_version: p.version.clone(),
                            duration_ms: start_instant.elapsed().as_millis() as u64,
                        },
                    );

                    break;
                } else {
                    if let Err(e) = record_readiness_event(
                        event_log,
                        events_path,
                        PublishEvent {
                            timestamp: Utc::now(),
                            event_type: EventType::ReadinessTimeout {
                                max_wait_ms: readiness_config.max_total_wait.as_millis() as u64,
                            },
                            package: pkg_label.clone(),
                        },
                    ) {
                        return PackagePublishResult { result: Err(e) };
                    }
                    let message =
                        "published locally, but version not observed on registry within timeout";
                    last_err = Some((ErrorClass::Ambiguous, message.to_string()));
                    let delay = backoff_delay(
                        opts.base_delay,
                        opts.max_delay,
                        attempt,
                        opts.retry_strategy,
                        opts.retry_jitter,
                    );
                    let next_attempt_at = retry_next_attempt_at(delay);
                    emit_retry_backoff(
                        event_log,
                        events_path,
                        reporter,
                        &pkg_label,
                        &p.name,
                        &p.version,
                        attempt,
                        opts.max_attempts,
                        delay,
                        next_attempt_at,
                        ErrorClass::Ambiguous,
                        message,
                    );
                }
            }
            Err(_) => {
                let message = "readiness check failed";
                last_err = Some((ErrorClass::Ambiguous, message.to_string()));
                let delay = backoff_delay(
                    opts.base_delay,
                    opts.max_delay,
                    attempt,
                    opts.retry_strategy,
                    opts.retry_jitter,
                );
                let next_attempt_at = retry_next_attempt_at(delay);
                emit_retry_backoff(
                    event_log,
                    events_path,
                    reporter,
                    &pkg_label,
                    &p.name,
                    &p.version,
                    attempt,
                    opts.max_attempts,
                    delay,
                    next_attempt_at,
                    ErrorClass::Ambiguous,
                    message,
                );
            }
        }
    }

    // If package is still Uploaded (loop didn't run or readiness never checked), force a final check
    if last_err.is_none() {
        let Ok(state) = st.lock() else {
            return poisoned_lock("execution state");
        };
        let current_state = state.packages.get(&key).map(|p| p.state.clone());
        if matches!(current_state, Some(PackageState::Uploaded)) {
            if reg.version_exists(&p.name, &p.version).unwrap_or(false) {
                if let Err(e) = commit_transition(
                    st,
                    state_dir,
                    event_log,
                    events_path,
                    &key,
                    PackageState::Published,
                    PublishEvent {
                        timestamp: Utc::now(),
                        event_type: EventType::PackagePublished {
                            duration_ms: start_instant.elapsed().as_millis() as u64,
                        },
                        package: pkg_label.clone(),
                    },
                ) {
                    return PackagePublishResult { result: Err(e) };
                }

                // Send webhook notification: package succeeded
                maybe_send_event(
                    &opts.webhook,
                    WebhookEvent::PublishSucceeded {
                        plan_id: ws.plan.plan_id.clone(),
                        package_name: p.name.clone(),
                        package_version: p.version.clone(),
                        duration_ms: start_instant.elapsed().as_millis() as u64,
                    },
                );
            } else {
                last_err = Some((
                    ErrorClass::Ambiguous,
                    "package was uploaded but not confirmed visible on registry".into(),
                ));
            }
        }
    }

    let finished_at = Utc::now();
    let duration_ms = start_instant.elapsed().as_millis();

    if let Some((class, msg)) = last_err {
        // Final chance: maybe it eventually showed up.
        if reg.version_exists(&p.name, &p.version).unwrap_or(false) {
            if let Err(e) = commit_transition(
                st,
                state_dir,
                event_log,
                events_path,
                &key,
                PackageState::Published,
                PublishEvent {
                    timestamp: Utc::now(),
                    event_type: EventType::PackagePublished {
                        duration_ms: duration_ms as u64,
                    },
                    package: pkg_label.clone(),
                },
            ) {
                return PackagePublishResult { result: Err(e) };
            }

            // Send webhook notification: package succeeded
            maybe_send_event(
                &opts.webhook,
                WebhookEvent::PublishSucceeded {
                    plan_id: ws.plan.plan_id.clone(),
                    package_name: p.name.clone(),
                    package_version: p.version.clone(),
                    duration_ms: duration_ms as u64,
                },
            );

            return PackagePublishResult {
                result: Ok(PackageReceipt {
                    name: p.name.clone(),
                    version: p.version.clone(),
                    attempts: {
                        let Ok(st) = st.lock() else {
                            return poisoned_lock("execution state");
                        };
                        st.packages.get(&key).map_or(0, |p| p.attempts)
                    },
                    state: PackageState::Published,
                    started_at,
                    finished_at,
                    duration_ms,
                    evidence: PackageEvidence {
                        attempts: attempt_evidence,
                        readiness_checks: readiness_evidence,
                    },
                    compromised_at: None,
                    compromised_by: None,
                    superseded_by: None,
                }),
            };
        } else {
            let error_class_str = format!("{:?}", class);
            let failed = PackageState::Failed {
                class: class.clone(),
                message: msg.clone(),
            };
            // Event: PackageFailed (final)
            {
                let Ok(mut log) = event_log.lock() else {
                    return poisoned_lock("event log");
                };
                log.record(PublishEvent {
                    timestamp: Utc::now(),
                    event_type: EventType::PackageFailed {
                        class: ErrorClass::Ambiguous,
                        message: msg.clone(),
                    },
                    package: pkg_label.clone(),
                });
            }
            if let Err(e) =
                commit_pending_transition(st, state_dir, event_log, events_path, &key, failed)
            {
                return PackagePublishResult { result: Err(e) };
            }

            // Send webhook notification: package failed
            maybe_send_event(
                &opts.webhook,
                WebhookEvent::PublishFailed {
                    plan_id: ws.plan.plan_id.clone(),
                    package_name: p.name.clone(),
                    package_version: p.version.clone(),
                    error_class: error_class_str,
                    message: msg.clone(),
                },
            );

            return PackagePublishResult {
                result: Err(anyhow::anyhow!("{}@{}: failed: {}", p.name, p.version, msg)),
            };
        }
    }

    // Send webhook notification: package succeeded
    maybe_send_event(
        &opts.webhook,
        WebhookEvent::PublishSucceeded {
            plan_id: ws.plan.plan_id.clone(),
            package_name: p.name.clone(),
            package_version: p.version.clone(),
            duration_ms: duration_ms as u64,
        },
    );

    PackagePublishResult {
        result: Ok(PackageReceipt {
            name: p.name.clone(),
            version: p.version.clone(),
            attempts: {
                let Ok(st) = st.lock() else {
                    return poisoned_lock("execution state");
                };
                st.packages.get(&key).map_or(0, |p| p.attempts)
            },
            state: PackageState::Published,
            started_at,
            finished_at,
            duration_ms,
            evidence: PackageEvidence {
                attempts: attempt_evidence,
                readiness_checks: readiness_evidence,
            },
            compromised_at: None,
            compromised_by: None,
            superseded_by: None,
        }),
    }
}
