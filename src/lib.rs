#![recursion_limit = "256"]
//! AI Personas Rust implementation. See deployment limitations before use.
mod phase_timing;
pub mod auth;
pub mod computation;
pub mod http_fetch;
pub mod host_command;
pub mod files;
pub mod mock_effects;
pub mod protocol;
pub mod provider;
pub mod store;
pub mod work;

pub mod runtime;
pub use runtime::launch;
