//! Dependency-level scheduling for the canonical package executor.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};

use crate::engine::execute_package::{PackagePublishResult, publish_package};
use crate::plan::PlannedWorkspace;
use crate::plan::chunking::chunk_by_max_concurrent;
use crate::registry::RegistryClient;
use crate::state::events;
use shipper_types::{ExecutionState, PackageReceipt, PublishLevel, RuntimeOptions};

use super::{Reporter, SendReporter, drain_retry_waits};

/// Publish packages in one dependency level using the canonical executor.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_publish_level(
    level: &PublishLevel,
    ws: &PlannedWorkspace,
    opts: &RuntimeOptions,
    reg: &RegistryClient,
    st: &Arc<Mutex<ExecutionState>>,
    state_dir: &Path,
    event_log: &Arc<Mutex<events::EventLog>>,
    events_path: &Path,
    reporter: &mut dyn Reporter,
    send_reporter: &Arc<SendReporter>,
) -> Result<Vec<PackageReceipt>> {
    let num_packages = level.packages.len();
    let max_concurrent = opts.parallel.max_concurrent.min(num_packages);

    reporter.info(&format!(
        "Level {}: publishing {} packages (max concurrent: {})",
        level.level, num_packages, max_concurrent
    ));

    let mut all_receipts: Vec<PackageReceipt> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for chunk in chunk_by_max_concurrent(&level.packages, max_concurrent) {
        let mut handles: Vec<std::thread::JoinHandle<PackagePublishResult>> = Vec::new();

        for p in chunk {
            let p = p.clone();
            let ws_clone = ws.clone();
            let opts_clone = opts.clone();
            let reg_clone = reg.clone();
            let st_clone = Arc::clone(st);
            let state_dir = state_dir.to_path_buf();
            let event_log_clone = Arc::clone(event_log);
            let events_path = events_path.to_path_buf();
            let reporter_clone = Arc::clone(send_reporter);

            let handle = thread::spawn(move || {
                publish_package(
                    &p,
                    &ws_clone,
                    &opts_clone,
                    &reg_clone,
                    &st_clone,
                    &state_dir,
                    &event_log_clone,
                    &events_path,
                    &reporter_clone,
                )
            });

            handles.push(handle);
        }

        while handles.iter().any(|handle| !handle.is_finished()) {
            drain_retry_waits(reporter, send_reporter.as_ref());
            thread::sleep(Duration::from_millis(25));
        }
        drain_retry_waits(reporter, send_reporter.as_ref());

        for handle in handles {
            let result = handle
                .join()
                .map_err(|_| anyhow::anyhow!("publish thread panicked"))?;
            match result.result {
                Ok(receipt) => all_receipts.push(receipt),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
    }

    if !errors.is_empty() {
        bail!(
            "parallel publish failed for {} package(s): {}",
            errors.len(),
            errors.join("; ")
        );
    }

    Ok(all_receipts)
}
