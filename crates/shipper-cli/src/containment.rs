//! Resolve the existing plan and path owners into one containment context.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use shipper_core::plan::PlannedWorkspace;
use shipper_core::runtime::execution::resolve_state_dir;
use shipper_core::types::RuntimeOptions;

pub(super) struct ContainmentContext<'a> {
    pub workspace_root: &'a Path,
    pub registry_name: &'a str,
    pub state_dir: PathBuf,
}

impl<'a> ContainmentContext<'a> {
    pub fn resolve(planned: &'a PlannedWorkspace, opts: &RuntimeOptions) -> Result<Self> {
        // The CLI rejects multi-registry selectors before planning. Keep this
        // boundary explicit too: containment never chooses the first target.
        if !opts.registries.is_empty() {
            bail!("containment supports one registry; select it with --registry");
        }
        let registry_name = planned.plan.registry.name.as_str();
        if registry_name.trim().is_empty() {
            bail!("containment requires a non-empty configured registry name");
        }
        Ok(Self {
            workspace_root: &planned.workspace_root,
            registry_name,
            state_dir: resolve_state_dir(&planned.workspace_root, &opts.state_dir),
        })
    }

    pub fn validate_plan_registry(&self, kind: &str, registry: &str) -> Result<()> {
        if registry != self.registry_name {
            bail!(
                "{kind} registry '{registry}' does not match configured registry '{}'",
                self.registry_name
            );
        }
        Ok(())
    }
}
