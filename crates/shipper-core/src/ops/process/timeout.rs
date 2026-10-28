//! Command execution with optional timeout.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::run::run_command_in_dir;
use super::types::CommandOutput;

/// Run a command with optional timeout and captured output.
#[allow(dead_code)]
pub(crate) fn run_command_with_timeout(
    program: &str,
    args: &[&str],
    working_dir: &std::path::Path,
    timeout: Option<Duration>,
) -> Result<CommandOutput> {
    let start = Instant::now();

    let Some(timeout_dur) = timeout else {
        let output = run_command_in_dir(program, args, working_dir)?;
        return Ok(CommandOutput {
            exit_code: output.exit_code.unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
            duration: Duration::from_millis(output.duration_ms),
        });
    };

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn command: {}", program))?;

    let deadline = Instant::now() + timeout_dur;
    thread::scope(|scope| {
        // Keep this guard inside the scope so setup/polling errors terminate and
        // reap the direct child before the scope joins its output readers.
        let mut child = ChildGuard {
            child,
            reaped: false,
        };
        let stdout = child.child.stdout.take();
        let stderr = child.child.stderr.take();
        // Both pipes must be drained while the child runs. Waiting for an exit
        // first can deadlock a verbose child on a full stdout or stderr pipe.
        let stdout_reader = thread::Builder::new()
            .name("shipper-stdout".into())
            .spawn_scoped(scope, move || read_pipe(stdout))
            .context("failed to start command stdout reader")?;
        let stderr_reader = thread::Builder::new()
            .name("shipper-stderr".into())
            .spawn_scoped(scope, move || read_pipe(stderr))
            .context("failed to start command stderr reader")?;

        let (exit_code, timed_out) = loop {
            match child
                .child
                .try_wait()
                .with_context(|| format!("failed to poll command: {}", program))?
            {
                Some(status) => {
                    child.reaped = true;
                    break (status.code().unwrap_or(-1), false);
                }
                None => {
                    if Instant::now() >= deadline {
                        child.kill_and_reap();
                        break (-1, true);
                    }

                    thread::sleep(Duration::from_millis(100));
                }
            }
        };

        // These joins still require EOF, including from inherited descendant
        // handles. Bounded capture/cleanup are separate obligations in #412.
        let stdout = stdout_reader
            .join()
            .map_err(|_| anyhow::anyhow!("command stdout reader panicked"))?;
        let mut stderr = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("command stderr reader panicked"))?;
        if timed_out {
            stderr.push_str(&format!(
                "\n{} timed out after {}",
                program,
                humantime::format_duration(timeout_dur)
            ));
        }

        Ok(CommandOutput {
            exit_code,
            stdout,
            stderr,
            timed_out,
            duration: start.elapsed(),
        })
    })
}

struct ChildGuard {
    child: Child,
    reaped: bool,
}

impl ChildGuard {
    fn kill_and_reap(&mut self) {
        let _ = self.child.kill();
        self.reaped = self.child.wait().is_ok();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill_and_reap();
        }
    }
}

fn read_pipe<R: Read>(stream: Option<R>) -> String {
    let mut buffer = Vec::new();
    if let Some(mut s) = stream {
        let _ = s.read_to_end(&mut buffer);
    }
    String::from_utf8_lossy(&buffer).to_string()
}
