use std::process::Command;

use anyhow::{Context, Result};

use super::super::types::CommandResult;

pub(super) fn run_and_capture(
    mut command: Command,
    program: &str,
    args: &[&str],
    context_suffix: Option<&str>,
    start: std::time::Instant,
) -> Result<CommandResult> {
    let output = command.output().with_context(|| {
        let suffix = context_suffix.unwrap_or_default();
        format!("failed to run command: {} {:?}{}", program, args, suffix)
    })?;

    Ok(CommandResult::from_output(&output, start.elapsed()))
}
