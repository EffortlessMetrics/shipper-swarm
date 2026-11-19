//! Bounded Windows supervision of an invocation-owned Job Object.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read};
use std::os::windows::io::AsHandle;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use interprocess::os::windows::named_pipe::{PipeListenerOptions, PipeMode, PipeStream, pipe_mode};
use processkit::ProcessGroup;
use tokio::process::{Child, Command};

use super::super::capture::{CaptureBuffer, CaptureMode};
use super::super::interruption::{self, InterruptionLease};
use super::super::{
    CaptureLoss, CaptureStatus, CapturedCommandOutput, CleanupFailure, CommandOutput,
};

pub(super) const CLEANUP_ALLOWANCE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(8);
const READ_BYTES: usize = 8 * 1024;
const READS_PER_TURN: usize = 8;

pub(super) fn run_supervised(
    program: &str,
    args: &[&str],
    working_dir: &Path,
    timeout: Option<Duration>,
    capture: CaptureMode,
) -> Result<CapturedCommandOutput> {
    // The Windows implementations of spawn and try_wait use synchronous
    // process-handle calls; none of Tokio's future/stdio adapters are used.
    let lease = InterruptionLease::acquire()?;
    interruption::check()?;
    let started = Instant::now();
    let execution_deadline = timeout
        .map(|duration| {
            started
                .checked_add(duration)
                .context("command timeout exceeds the monotonic clock range")
        })
        .transpose()?;

    // A fresh, unnamed, kill-on-close job. The wrapper spawns suspended,
    // assigns through the owned process handle, then resumes. We retain both
    // the job and the direct child; descendant state cannot change its status.
    let job = create_job()?;
    let (stdout, stdout_writer) = capture_pipe().context("failed to prepare Windows stdout")?;
    let (stderr, stderr_writer) = capture_pipe().context("failed to prepare Windows stderr")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::from(stdout_writer))
        .stderr(Stdio::from(stderr_writer));

    interruption::check()?;
    let mut child = match spawn_in_job(&job, command) {
        Ok(child) => child,
        Err(error) => {
            let reason = std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<io::Error>());
            if reason.is_some_and(|source| source.kind() == ErrorKind::NotFound) {
                return Err(error).with_context(|| format!("failed to spawn command: {program}"));
            }
            // The wrapper may already have created a suspended child. Its
            // rollback attempts termination but discards the cleanup result.
            return Ok(CapturedCommandOutput {
                output: CommandOutput {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out: false,
                    duration: started.elapsed(),
                },
                capture: CaptureStatus {
                    stdout: Some(CaptureLoss::Cleanup),
                    stderr: Some(CaptureLoss::Cleanup),
                },
                cleanup_failure: Some(CleanupFailure::JobSpawn),
                interrupted: interruption::requested(),
                interruption_lease: lease,
            });
        }
    };
    let mut stdout = PipeCapture::new(stdout, capture);
    let mut stderr = PipeCapture::new(stderr, capture);
    let mut cleanup_failure = None;
    let mut exit_status = None;
    let mut timed_out = false;
    let mut interrupted = false;
    let mut cleanup = None;

    loop {
        let made_progress = stdout.drain_turn() | stderr.drain_turn();
        interrupted |= interruption::requested();
        let now = Instant::now();

        if cleanup.is_none() {
            match child.try_wait() {
                Ok(status) => exit_status = status,
                Err(_) => cleanup_failure = Some(CleanupFailure::ChildStatus),
            }
            if exit_status.is_none() && cleanup_failure.is_none() && !interrupted {
                timed_out = execution_deadline.is_some_and(|deadline| now >= deadline);
            }
            if exit_status.is_some() || cleanup_failure.is_some() || interrupted || timed_out {
                // Termination initiation and completion are separate. Even a
                // failed termination can be followed by confirmed empty state.
                cleanup = Some((now + CLEANUP_ALLOWANCE, !terminate(&job)));
            }
        }

        if let Some((deadline, termination_failed)) = cleanup {
            let reaped = poll_reap(&mut child, exit_status.is_some(), &mut cleanup_failure);
            let empty = job_empty(&job);
            if reaped && empty && stdout.finished() && stderr.finished() {
                break;
            }
            if Instant::now() >= deadline {
                if !reaped {
                    cleanup_failure.get_or_insert(CleanupFailure::Reap);
                }
                if !empty {
                    cleanup_failure.get_or_insert(if termination_failed {
                        CleanupFailure::JobTermination
                    } else {
                        CleanupFailure::JobStillActive
                    });
                }
                if !stdout.finished() || !stderr.finished() {
                    cleanup_failure.get_or_insert(CleanupFailure::InheritedPipe);
                    stdout.cancel();
                    stderr.cancel();
                }
                break;
            }
        }

        if !made_progress {
            let remaining = cleanup
                .map(|(deadline, _)| deadline)
                .or(execution_deadline)
                .map_or(POLL_INTERVAL, |deadline| {
                    deadline.saturating_duration_since(Instant::now())
                });
            thread::sleep(POLL_INTERVAL.min(remaining));
        }
    }

    // Close the last job handle before handing the lease to the caller. This
    // also issues kill-on-close if explicit termination could not be confirmed.
    drop(child);
    drop(job);
    if let Some(duration) = timeout.filter(|_| timed_out) {
        stderr.buffer.push(
            format!(
                "\n{program} timed out after {}",
                humantime::format_duration(duration)
            )
            .as_bytes(),
        );
    }
    let (stdout, stdout_loss) = stdout.buffer.finish();
    let (stderr, stderr_loss) = stderr.buffer.finish();
    Ok(CapturedCommandOutput {
        output: CommandOutput {
            exit_code: exit_status
                .and_then(|status: ExitStatus| status.code())
                .unwrap_or(-1),
            stdout,
            stderr,
            timed_out,
            duration: started.elapsed(),
        },
        capture: CaptureStatus {
            stdout: stdout_loss,
            stderr: stderr_loss,
        },
        cleanup_failure,
        interrupted: interrupted || interruption::requested(),
        interruption_lease: lease,
    })
}

struct PipeReader {
    probe: PipeStream<pipe_mode::Messages, pipe_mode::Messages>,
    bytes: PipeStream<pipe_mode::Bytes, pipe_mode::Bytes>,
}

impl Read for PipeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // Read in byte mode so zero-length messages cannot mask later bytes.
        // interprocess normalizes both ERROR_NO_DATA and ERROR_BROKEN_PIPE
        // to zero here. PeekNamedPipe distinguishes open-empty from EOF.
        let count = self.bytes.read(buffer)?;
        if count != 0 {
            return Ok(count);
        }
        match self.probe.peek_msg_len() {
            Ok(_) => Err(ErrorKind::WouldBlock.into()),
            Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(0),
            Err(error) => Err(error),
        }
    }
}

fn capture_pipe() -> io::Result<(PipeReader, File)> {
    let name = format!(r"\\.\pipe\shipper-{:032x}", rand::random::<u128>());
    let listener = PipeListenerOptions::new()
        .path(name.as_str())
        .mode(PipeMode::Messages)
        .nonblocking(true)
        .input_buffer_size_hint(64 * 1024)
        .create_duplex::<pipe_mode::Messages>()?;
    let writer = OpenOptions::new().write(true).open(&name)?;
    let probe = listener.accept()?;
    // accept installs another listening instance. Close it before conversion
    // tries ReOpenFile, so the byte view cannot select the unconnected instance.
    drop(listener);
    let bytes = PipeStream::try_from(probe.as_handle().try_clone_to_owned()?)
        .map_err(|error| io::Error::other(error.to_string()))?;
    // The second view reads bytes and never writes. Prevent the conversion's
    // default flush-on-drop, and keep the OS read mode nonblocking.
    bytes.assume_flushed();
    bytes.set_nonblocking(true)?;
    Ok((PipeReader { probe, bytes }, writer))
}

fn poll_reap(
    child: &mut Child,
    already_reaped: bool,
    failure: &mut Option<CleanupFailure>,
) -> bool {
    #[cfg(test)]
    if has_fault(Fault::Reap) {
        return false;
    }
    if already_reaped {
        return true;
    }
    match child.try_wait() {
        Ok(status) => status.is_some(),
        Err(_) => {
            failure.get_or_insert(CleanupFailure::Reap);
            false
        }
    }
}

fn create_job() -> Result<ProcessGroup> {
    #[cfg(test)]
    if has_fault(Fault::JobSetup) {
        anyhow::bail!("failed to create the invocation's Windows Job Object");
    }
    ProcessGroup::new().context("failed to create the invocation's Windows Job Object")
}

fn spawn_in_job(job: &ProcessGroup, command: Command) -> processkit::Result<Child> {
    #[cfg(test)]
    if has_fault(Fault::Spawn) {
        return Err(processkit::Error::spawn(
            "fixture",
            io::Error::other("injected job launch failure"),
        ));
    }
    job.spawn(command)
}

fn terminate(job: &ProcessGroup) -> bool {
    #[cfg(test)]
    if has_fault(Fault::Terminate) {
        return false;
    }
    job.kill_all().is_ok()
}

fn job_empty(job: &ProcessGroup) -> bool {
    #[cfg(test)]
    if has_fault(Fault::Accounting) {
        return false;
    }
    job.stats()
        .is_ok_and(|stats| stats.active_process_count == 0)
}

struct PipeCapture<R> {
    stream: Option<R>,
    buffer: CaptureBuffer,
}

impl<R: Read> PipeCapture<R> {
    fn new(stream: R, capture: CaptureMode) -> Self {
        Self {
            stream: Some(stream),
            buffer: CaptureBuffer::new(capture),
        }
    }

    fn drain_turn(&mut self) -> bool {
        let Some(stream) = self.stream.as_mut() else {
            return false;
        };
        #[cfg(test)]
        if has_fault(Fault::Read) {
            self.buffer.lose(CaptureLoss::Read);
            self.stream = None;
            return false;
        }
        let mut chunk = [0; READ_BYTES];
        let mut made_progress = false;
        for _ in 0..READS_PER_TURN {
            let bytes = match stream.read(&mut chunk) {
                Ok(0) => {
                    self.stream = None;
                    break;
                }
                Ok(len) => chunk.get(..len),
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => None,
            };
            let Some(bytes) = bytes else {
                self.buffer.lose(CaptureLoss::Read);
                self.stream = None;
                break;
            };
            self.buffer.push(bytes);
            made_progress = true;
        }
        made_progress
    }

    fn finished(&self) -> bool {
        self.stream.is_none()
    }

    fn cancel(&mut self) {
        if self.stream.take().is_some() {
            // The polling reader has no operation outstanding. Dropping it
            // closes the owned handle without waiting for a writer or EOF.
            self.buffer.lose(CaptureLoss::Cleanup);
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Fault {
    JobSetup,
    Spawn,
    Terminate,
    Accounting,
    Reap,
    Read,
}

#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<Option<Fault>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn has_fault(fault: Fault) -> bool {
    FAULT.get() == Some(fault)
}

#[cfg(test)]
pub(super) fn with_fault<T>(fault: Fault, run: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAULT.set(None);
        }
    }
    assert!(FAULT.replace(Some(fault)).is_none());
    let _reset = Reset;
    run()
}
