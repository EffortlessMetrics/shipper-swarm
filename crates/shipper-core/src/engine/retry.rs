use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;

use super::Reporter;
use crate::runtime::execution::retry_next_attempt_at;
use crate::state::events;
use crate::types::{ErrorClass, EventType, PublishEvent};

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_retry_backoff_event(
    event_log: &mut events::EventLog,
    events_path: &Path,
    reporter: &mut dyn Reporter,
    pkg_label: &str,
    pkg_name: &str,
    pkg_version: &str,
    attempt: u32,
    max_attempts: u32,
    delay: Duration,
    next_attempt_at: chrono::DateTime<Utc>,
    reason: ErrorClass,
    message: &str,
) -> Result<()> {
    record_retry_backoff_event(
        event_log,
        events_path,
        pkg_label,
        attempt,
        max_attempts,
        delay,
        next_attempt_at,
        &reason,
        message,
    )?;
    flush_event_log(event_log, events_path)?;
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
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_retry_backoff_event(
    event_log: &mut events::EventLog,
    _events_path: &Path,
    pkg_label: &str,
    attempt: u32,
    max_attempts: u32,
    delay: Duration,
    next_attempt_at: chrono::DateTime<Utc>,
    reason: &ErrorClass,
    message: &str,
) -> Result<()> {
    event_log.record(PublishEvent {
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
    record_publish_wait_event(
        event_log,
        pkg_label,
        delay,
        "retry backoff",
        Some(next_attempt_at),
    )?;
    event_log.record(PublishEvent {
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
    Ok(())
}

pub(crate) fn record_rate_limit_observed_event(
    event_log: &mut events::EventLog,
    _events_path: &Path,
    pkg_label: &str,
    is_new_crate: bool,
    retry_after: Option<Duration>,
    message: &str,
) -> Result<()> {
    event_log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::RateLimitObserved {
            is_new_crate,
            retry_after_ms: retry_after.map(|delay| delay.as_millis() as u64),
            message: message.to_string(),
        },
        package: pkg_label.to_string(),
    });
    Ok(())
}

fn record_publish_wait_event(
    event_log: &mut events::EventLog,
    pkg_label: &str,
    delay: Duration,
    reason: &str,
    until: Option<chrono::DateTime<Utc>>,
) -> Result<()> {
    event_log.record(PublishEvent {
        timestamp: Utc::now(),
        event_type: EventType::PublishWaiting {
            reason: reason.to_string(),
            delay_ms: delay.as_millis() as u64,
            until: until.unwrap_or_else(|| retry_next_attempt_at(delay)),
        },
        package: pkg_label.to_string(),
    });
    Ok(())
}

fn flush_event_log(event_log: &mut events::EventLog, events_path: &Path) -> Result<()> {
    event_log.write_to_file(events_path)?;
    event_log.clear();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_after_retry(
    reporter: &mut dyn Reporter,
    pkg_name: &str,
    pkg_version: &str,
    attempt: u32,
    max_attempts: u32,
    delay: Duration,
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
