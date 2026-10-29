//! Layer 3: persistence. State, events, receipts, and the StateStore trait.
//!
//! May import from `runtime` and `ops`. Must not import from `engine` or `plan`.
//! See `CLAUDE.md` in this folder for the architectural rules.

pub mod consistency;
pub mod events;
pub mod execution_state;
mod file_sync;
#[cfg(test)]
pub(crate) use file_sync::test_support as file_sync_test_support;
pub mod rebuild;
pub mod reconciliation;
pub mod rehearsal;
pub(crate) mod run_observation;
