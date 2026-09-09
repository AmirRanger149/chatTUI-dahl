//! The chat API layer: protocol-neutral types and error handling, a family of
//! per-vendor backends behind the [`ChatBackend`](providers::ChatBackend)
//! trait, and a protocol-agnostic orchestration client that adds model
//! fallback on top.

pub mod client;
pub mod error;
pub mod providers;
pub mod sse;
pub mod types;
