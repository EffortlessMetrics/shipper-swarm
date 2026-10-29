//! Finite cleanup of cooperative descendants in an invocation-owned group.
//!
//! Nonblocking reads avoid both pipe backpressure and abandoned reader threads.
//! The unreaped direct child pins the group ID until its single shutdown signal;
//! no destructive operation uses that numeric ID after the child is reaped.
//! Deliberate process-group/session escape is outside this containment boundary.

use std::io::{ErrorKind, Read};
use std::os::fd::AsFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::process::{
    Pid, Signal, WaitId, WaitIdOptions, kill_process_group, test_kill_process_group, waitid,
};

use super::super::capture::{CaptureBuffer, CaptureMode};
use super::super::{
    CaptureLoss, CaptureStatus, CapturedCommandOutput, CleanupFailure, CommandOutput,
};

/// This allowance starts at the first observed exit or execution deadline.
/// It bounds pipe drain and direct-child reaping independently of run time.
pub(super) const CLEANUP_ALLOWANCE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const READ_BYTES: usize = 8 * 1024;
// Limit each turn so a continuously writable stream cannot starve the other
// stream, status observation, or either deadline.
const READS_PER_TURN: usize = 8;

pub(super) fn run_supervised(
    program: &str,
    args: &[&str],
    working_dir: &Path,
    timeout: Option<Duration>,
    capture: CaptureMode,
) -> Result<CapturedCommandOutput> {
    let started = Instant::now();
    let execution_deadline = timeout
        .map(|duration| {
            started
                .checked_add(duration)
                .context("command timeout exceeds the monotonic clock range")
        })
        .transpose()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn command: {program}"))?;
    let mut child = OwnedGroup::new(child);
    let mut stdout = PipeCapture::new(child.child.stdout.take(), capture);
    let mut stderr = PipeCapture::new(child.child.stderr.take(), capture);
    let mut cleanup_deadline = None;
    let mut cleanup_failure = None;
    let mut group_signal_failed = false;
    let mut group_gone = false;
    let mut exit_code = -1;
    let mut timed_out = false;

    loop {
        let made_progress = stdout.drain_turn() | stderr.drain_turn();

        if cleanup_deadline.is_none() {
            // Unlike Child::try_wait, NOWAIT observes an exit without releasing
            // the leader's PID. Keep that identity through group termination.
            match child.observe_exit() {
                Ok(Some(status)) => {
                    exit_code = status;
                    cleanup_deadline = Some(Instant::now() + CLEANUP_ALLOWANCE);
                }
                Ok(None) => {
                    if execution_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                        timed_out = true;
                        cleanup_deadline = Some(Instant::now() + CLEANUP_ALLOWANCE);
                    }
                }
                Err(_) => {
                    // With no valid identity observation, do not signal a
                    // potentially recycled PGID. Return a retained safety hold.
                    child.identity_pinned = false;
                    cleanup_failure = Some(CleanupFailure::ChildStatus);
                    cleanup_deadline = Some(Instant::now() + CLEANUP_ALLOWANCE);
                }
            }

            if cleanup_deadline.is_some() && child.identity_pinned && child.signal_group().is_err()
            {
                // A group containing only an exited, unreaped leader may
                // reject a signal on some kernels. Final cleanup facts, not
                // this provisional signal result, decide whether work is held.
                group_signal_failed = true;
            }
        }

        if let Some(deadline) = cleanup_deadline {
            // Never use blocking wait(): a killed child can still be waiting
            // for an OS operation. A failed reap is an explicit refusal result.
            match child.try_reap() {
                Ok(_) => {}
                Err(_) => {
                    cleanup_failure.get_or_insert(CleanupFailure::Reap);
                }
            }
            if child.reaped && !group_gone {
                // Only observe after reaping: a cached PGID is no longer a safe
                // destructive target. Reuse can conservatively cause a hold,
                // but must never cause us to signal a different invocation.
                group_gone = matches!(
                    test_kill_process_group(child.pid),
                    Err(rustix::io::Errno::SRCH)
                );
            }
            if child.reaped && group_gone && stdout.finished() && stderr.finished() {
                break;
            }
            if Instant::now() >= deadline {
                if !child.reaped {
                    cleanup_failure.get_or_insert(CleanupFailure::Reap);
                }
                if !group_gone {
                    cleanup_failure.get_or_insert(if group_signal_failed {
                        CleanupFailure::GroupSignal
                    } else {
                        CleanupFailure::GroupStillPresent
                    });
                }
                if !stdout.finished() || !stderr.finished() {
                    cleanup_failure.get_or_insert(CleanupFailure::InheritedPipe);
                    stdout.abandon_capture();
                    stderr.abandon_capture();
                }
                break;
            }
        }

        // Busy output yields between bounded reads without sleeping; quiet
        // output uses a short poll delay, also bounded by the cleanup deadline.
        if !made_progress {
            let remaining = cleanup_deadline
                .or(execution_deadline)
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(POLL_INTERVAL);
            thread::sleep(POLL_INTERVAL.min(remaining));
        }
    }

    if timed_out {
        stderr.buffer.push(
            format!(
                "\n{} timed out after {}",
                program,
                humantime::format_duration(timeout.expect("execution deadline requires timeout"))
            )
            .as_bytes(),
        );
    }
    let (stdout, stdout_loss) = stdout.buffer.finish();
    let (stderr, stderr_loss) = stderr.buffer.finish();
    Ok(CapturedCommandOutput {
        output: CommandOutput {
            exit_code,
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
    })
}

struct PipeCapture<R> {
    stream: Option<R>,
    buffer: CaptureBuffer,
}

impl<R: Read + AsFd> PipeCapture<R> {
    fn new(stream: Option<R>, capture: CaptureMode) -> Self {
        let mut result = Self {
            stream,
            buffer: CaptureBuffer::new(capture),
        };
        let configured = result.stream.as_ref().is_some_and(|stream| {
            fcntl_getfl(stream)
                .and_then(|flags| fcntl_setfl(stream, flags | OFlags::NONBLOCK))
                .is_ok()
        });
        if !configured {
            result.buffer.lose(CaptureLoss::Read);
            result.stream = None;
        }
        result
    }

    fn drain_turn(&mut self) -> bool {
        let Some(stream) = self.stream.as_mut() else {
            return false;
        };
        let mut chunk = [0; READ_BYTES];
        let mut made_progress = false;
        for _ in 0..READS_PER_TURN {
            match stream.read(&mut chunk) {
                Ok(0) => {
                    self.stream = None;
                    break;
                }
                Ok(len) => {
                    self.buffer.push(&chunk[..len]);
                    made_progress = true;
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    self.buffer.lose(CaptureLoss::Read);
                    self.stream = None;
                    break;
                }
            }
        }
        made_progress
    }

    fn finished(&self) -> bool {
        self.stream.is_none()
    }

    fn abandon_capture(&mut self) {
        if self.stream.take().is_some() {
            self.buffer.lose(CaptureLoss::Cleanup);
        }
    }
}

struct OwnedGroup {
    child: Child,
    pid: Pid,
    identity_pinned: bool,
    signalled: bool,
    reaped: bool,
}

impl OwnedGroup {
    fn new(child: Child) -> Self {
        // A spawned child cannot be PID 0/1 or exceed the signed native PID
        // range. In particular, never pass 1 to kill_process_group: -1 targets
        // every permitted process rather than an invocation-owned group.
        let raw = i32::try_from(child.id()).expect("child PID fits native pid_t");
        assert!(raw > 1, "spawned child must have an ordinary PID");
        Self {
            child,
            pid: Pid::from_raw(raw).expect("positive child PID"),
            identity_pinned: true,
            signalled: false,
            reaped: false,
        }
    }

    fn observe_exit(&self) -> rustix::io::Result<Option<i32>> {
        waitid(
            WaitId::Pid(self.pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .map(|status| status.map(|status| status.exit_status().unwrap_or(-1)))
    }

    fn signal_group(&mut self) -> rustix::io::Result<()> {
        self.signalled = true;
        #[cfg(test)]
        if INJECT_GROUP_SIGNAL_ERROR.swap(false, std::sync::atomic::Ordering::SeqCst) {
            return Err(rustix::io::Errno::PERM);
        }
        kill_process_group(self.pid, Signal::KILL)
    }

    fn try_reap(&mut self) -> std::io::Result<()> {
        if !self.reaped && self.child.try_wait()?.is_some() {
            self.reaped = true;
            self.identity_pinned = false;
        }
        Ok(())
    }
}

#[cfg(test)]
static INJECT_GROUP_SIGNAL_ERROR: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
pub(super) struct GroupSignalErrorGuard;

#[cfg(test)]
pub(super) fn inject_group_signal_error() -> GroupSignalErrorGuard {
    assert!(!INJECT_GROUP_SIGNAL_ERROR.swap(true, std::sync::atomic::Ordering::SeqCst));
    GroupSignalErrorGuard
}

#[cfg(test)]
impl Drop for GroupSignalErrorGuard {
    fn drop(&mut self) {
        INJECT_GROUP_SIGNAL_ERROR.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if self.identity_pinned && !self.signalled {
            let _ = self.signal_group();
        }
        // Unwind/setup cleanup must also remain finite. No group signal can
        // occur after this reap. Normal execution already polls through its
        // cleanup allowance and exposes an unsuccessful reap to the caller.
        let _ = self.try_reap();
    }
}
