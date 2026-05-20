use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::engine::Reporter;
use crate::plan::PlannedWorkspace;
use crate::registry::RegistryClient;
use crate::types::{Registry, RuntimeOptions};

pub(super) fn write_reconciliation_report_best_effort(
    state_dir: &Path,
    ws: &PlannedWorkspace,
    events_path: &Path,
    reporter: &mut dyn Reporter,
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

pub(super) fn init_registry_client(registry: Registry, state_dir: &Path) -> Result<RegistryClient> {
    let cache_dir = state_dir.join("cache");
    RegistryClient::new(registry).map(|c| c.with_cache_dir(cache_dir))
}

pub(super) fn enforce_rehearsal_gate(
    ws: &PlannedWorkspace,
    opts: &RuntimeOptions,
    state_dir: &Path,
    reporter: &mut dyn Reporter,
) -> Result<()> {
    let Some(rehearsal_name) = opts.rehearsal_registry.as_deref() else {
        return Ok(());
    };

    if opts.rehearsal_skip {
        reporter.warn(&format!(
            "--skip-rehearsal was set; publish is proceeding without a rehearsal against '{rehearsal_name}'. \
             This is an operator-authorized bypass; auditors reading events.jsonl will see no RehearsalComplete event for this plan_id."
        ));
        return Ok(());
    }

    let receipt = crate::state::rehearsal::load_rehearsal(state_dir)
        .context("failed to read rehearsal receipt while enforcing hard gate")?;

    let rehearsal_path = crate::state::rehearsal::rehearsal_path(state_dir);

    let receipt = match receipt {
        Some(r) => r,
        None => bail!(
            "rehearsal is required (rehearsal registry '{rehearsal_name}' is configured) but no rehearsal receipt was found at {}. \
             Run `shipper rehearse --rehearsal-registry {rehearsal_name}` first, \
             or pass --skip-rehearsal to override (not recommended).",
            rehearsal_path.display()
        ),
    };

    if receipt.plan_id != ws.plan.plan_id {
        bail!(
            "rehearsal receipt is stale: rehearsal ran for plan_id {} but the current plan_id is {}. \
             The workspace changed between rehearse and publish; re-run `shipper rehearse` against the current plan.",
            receipt.plan_id,
            ws.plan.plan_id
        );
    }

    if !receipt.passed {
        bail!(
            "rehearsal against '{}' did NOT pass for plan_id {}: {}. \
             Fix the cause and re-run `shipper rehearse` before publishing.",
            receipt.registry,
            receipt.plan_id,
            receipt.summary
        );
    }

    reporter.info(&format!(
        "rehearsal gate: passing receipt found ({} packages against '{}', plan_id {})",
        receipt.packages_published, receipt.registry, receipt.plan_id
    ));
    Ok(())
}
