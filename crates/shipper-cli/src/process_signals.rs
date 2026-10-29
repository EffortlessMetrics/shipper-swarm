//! Signal policy for the executable entry point, never the embedding adapter.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};

static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();

pub(super) fn install() -> Result<()> {
    let result = INSTALLED.get_or_init(|| {
        let interruption = shipper_core::engine::PublishInterruption::process_wide();
        for signal in [SIGINT, SIGTERM, SIGHUP] {
            // Order matters: outside supervision the first action terminates
            // normally. During supervision it returns and the second requests
            // bounded cleanup. Both remain installed for process lifetime.
            signal_hook::flag::register_conditional_default(
                signal,
                interruption.default_action_flag(),
            )
            .and_then(|_| signal_hook::flag::register(signal, interruption.request_flag()))
            .map_err(|error| format!("cannot install publish termination handling: {error}"))?;
        }
        Ok(())
    });
    result
        .as_ref()
        .map_err(|message| anyhow::anyhow!(message.clone()))
        .context("no publish command was started")?;
    Ok(())
}
