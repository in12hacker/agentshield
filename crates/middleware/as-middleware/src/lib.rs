//! Structured business logging, source-backed activity reads and conditional execution feedback.
//!
//! The SQLite owner stores collected values; queries allocate temporary views rather than history
//! caches. The authenticated JSONL adapter shares real Main grants. No Agent/tool bridge is inferred.
#![forbid(unsafe_code)]

pub mod activity;
pub mod agent_graph;
pub mod attribution;
pub mod audit;
pub mod business;
pub mod business_log;
pub mod context;
mod database;
pub mod execution_feedback;
mod gateway;
pub mod media;

pub mod codec;
pub mod transport;
