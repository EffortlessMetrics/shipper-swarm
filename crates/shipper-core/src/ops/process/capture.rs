//! Opt-in bounded diagnostic capture. Complete-output callers keep full capture.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read};

use super::types::CommandOutput;

pub(crate) const PUBLISH_TAIL_BYTES: usize = 256 * 1024;
const LINE_BYTES: usize = 64 * 1024;
const TAIL_ENTRIES: usize = 4096;
const READ_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaptureLoss {
    TailBytes,
    LineBytes,
    TailEntries,
    Read,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CaptureStatus {
    pub(crate) stdout: Option<CaptureLoss>,
    pub(crate) stderr: Option<CaptureLoss>,
}

impl CaptureStatus {
    pub(crate) fn is_complete(self) -> bool {
        self.stdout.is_none() && self.stderr.is_none()
    }
}

pub(crate) struct CapturedCommandOutput {
    pub(crate) output: CommandOutput,
    pub(crate) capture: CaptureStatus,
}

#[derive(Clone, Copy)]
pub(super) enum CaptureMode {
    Full,
    PublishTail(usize),
}

pub(super) enum CaptureBuffer {
    Full(Vec<u8>),
    Tail(TailCapture),
}

impl CaptureBuffer {
    pub(super) fn read<R: Read>(stream: Option<R>, mode: CaptureMode) -> Self {
        match mode {
            CaptureMode::Full => {
                let mut bytes = Vec::new();
                if let Some(mut stream) = stream {
                    // Preserve the existing complete-output runner's behavior.
                    let _ = stream.read_to_end(&mut bytes);
                }
                Self::Full(bytes)
            }
            CaptureMode::PublishTail(lines) => {
                let mut tail = TailCapture::new(lines);
                if let Some(mut stream) = stream {
                    let mut chunk = [0; READ_BYTES];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(len) => tail.push(&chunk[..len]),
                            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                            Err(_) => {
                                tail.lose(CaptureLoss::Read);
                                break;
                            }
                        }
                    }
                } else {
                    tail.lose(CaptureLoss::Read);
                }
                Self::Tail(tail)
            }
        }
    }

    pub(super) fn push(&mut self, bytes: &[u8]) {
        match self {
            Self::Full(buffer) => buffer.extend_from_slice(bytes),
            Self::Tail(tail) => tail.push(bytes),
        }
    }

    pub(super) fn finish(self) -> (String, Option<CaptureLoss>) {
        match self {
            Self::Full(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), None),
            Self::Tail(tail) => tail.finish(),
        }
    }
}

/// Resource/read loss is sticky. After loss, continue draining but retain no
/// fragments: a suffix or later continuation can depend on discarded redaction
/// context. The Cargo adapter suppresses BOTH streams whenever either is lost.
pub(super) struct TailCapture {
    requested: usize,
    lines: VecDeque<Box<[u8]>>,
    retained_bytes: usize,
    partial: Vec<u8>,
    skip_lf: bool,
    ended_with_break: bool,
    evicted_for_tail: bool,
    loss: Option<CaptureLoss>,
}

impl TailCapture {
    fn new(requested: usize) -> Self {
        Self {
            requested,
            lines: VecDeque::new(),
            retained_bytes: 0,
            partial: Vec::new(),
            skip_lf: false,
            ended_with_break: false,
            evicted_for_tail: false,
            loss: None,
        }
    }

    fn lose(&mut self, reason: CaptureLoss) {
        self.loss.get_or_insert(reason);
        self.lines.clear();
        self.partial.clear();
        self.retained_bytes = 0;
    }

    fn push(&mut self, bytes: &[u8]) {
        // Zero is intentional empty evidence, not capacity loss. Read errors
        // still have their own typed status in CaptureBuffer::read.
        if self.requested == 0 || self.loss.is_some() {
            return;
        }
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            if byte == b'\r' || byte == b'\n' {
                self.complete_line();
                self.ended_with_break = true;
                self.skip_lf = byte == b'\r';
            } else {
                if self.partial.len() == LINE_BYTES {
                    self.lose(CaptureLoss::LineBytes);
                    return;
                }
                self.partial.push(byte);
                self.ended_with_break = false;
            }
            if self.loss.is_some() {
                return;
            }
        }
    }

    fn complete_line(&mut self) {
        if self.lines.len() == self.requested {
            if let Some(old) = self.lines.pop_front() {
                self.retained_bytes -= old.len() + 1;
            }
            self.evicted_for_tail = true;
        }
        if self.lines.len() == TAIL_ENTRIES {
            self.lose(CaptureLoss::TailEntries);
            return;
        }
        // Charge a separator even for an empty or final unterminated line.
        let cost = self.partial.len() + 1;
        if self.retained_bytes + cost > PUBLISH_TAIL_BYTES {
            self.lose(CaptureLoss::TailBytes);
            return;
        }
        self.retained_bytes += cost;
        // Boxed slices avoid retaining a large spare capacity per short line.
        self.lines
            .push_back(std::mem::take(&mut self.partial).into_boxed_slice());
    }

    fn finish(mut self) -> (String, Option<CaptureLoss>) {
        if self.loss.is_some() {
            return (String::new(), self.loss);
        }
        if !self.partial.is_empty() {
            self.complete_line();
        }
        if self.loss.is_some() {
            return (String::new(), self.loss);
        }
        let mut text = String::with_capacity(self.retained_bytes);
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            text.push_str(&String::from_utf8_lossy(line));
        }
        // Match tail_lines: full short input retains its terminal newline;
        // selecting a suffix joins lines without a final newline.
        if self.ended_with_break && !self.evicted_for_tail {
            text.push('\n');
        }
        if text.len() > PUBLISH_TAIL_BYTES {
            // Lossy UTF-8 decoding can expand bytes. Its input was bounded,
            // and the rendered result must obey the same payload budget.
            return (String::new(), Some(CaptureLoss::TailBytes));
        }
        (text, None)
    }
}

#[cfg(test)]
mod tests;
