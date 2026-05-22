//! Basic command runners without timeout.

use anyhow::{Context, Result};

use super::types::CommandResult;

mod command_builder;
mod execution;

/// Run a command and capture its output.
#[allow(dead_code)]
pub(crate) fn run_command(program: &str, args: &[&str]) -> Result<CommandResult> {
    let start = std::time::Instant::now();

    execution::run_and_capture(
        command_builder::base_command(program, args),
        program,
        args,
        None,
        start,
    )
}

/// Run a command in a specific directory.
#[allow(dead_code)]
pub(crate) fn run_command_in_dir(
    program: &str,
    args: &[&str],
    dir: &std::path::Path,
) -> Result<CommandResult> {
    let start = std::time::Instant::now();

    let mut command = command_builder::base_command(program, args);
    command.current_dir(dir);

    execution::run_and_capture(
        command,
        program,
        args,
        Some(&format!(" in {}", dir.display())),
        start,
    )
}

/// Run a command with environment variables.
#[allow(dead_code)]
pub(crate) fn run_command_with_env(
    program: &str,
    args: &[&str],
    env: &[(String, String)],
) -> Result<CommandResult> {
    let start = std::time::Instant::now();

    let mut command = command_builder::base_command(program, args);

    for (key, value) in env {
        command.env(key, value);
    }

    execution::run_and_capture(command, program, args, None, start)
}

/// Run a command and stream output to stdout/stderr.
#[allow(dead_code)]
pub(crate) fn run_command_streaming(program: &str, args: &[&str]) -> Result<CommandResult> {
    let start = std::time::Instant::now();

    execution::run_and_capture(
        command_builder::streaming_command(program, args),
        program,
        args,
        None,
        start,
    )
}

/// Run a command and return success/failure without capturing output.
#[allow(dead_code)]
pub(crate) fn run_command_simple(program: &str, args: &[&str]) -> Result<bool> {
    let status = command_builder::base_command(program, args)
        .status()
        .with_context(|| format!("failed to run command: {} {:?}", program, args))?;
    Ok(status.success())
}
