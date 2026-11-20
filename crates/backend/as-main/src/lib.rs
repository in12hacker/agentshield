//! Three-thread AgentShield dispatcher; functional modules plug into shared contracts.
#![forbid(unsafe_code)]
pub mod assembly;
pub mod dispatch;
mod flow_runtime;
pub mod messages;
mod native_events;
pub mod runtime;
pub use runtime::{MainDispatch, MainThreads, ProcessingThread};
#[cfg(test)]
mod test_driver;
