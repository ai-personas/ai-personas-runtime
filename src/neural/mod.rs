//! Indexed persona runtime. Reuses the application's direct model transport and
//! host executors; history is immutable and never part of a mutable state blob.
pub mod cli;
mod connection;
pub mod commands;
pub mod accounting;
pub mod live;
pub mod footprint;
pub mod geometry;
pub mod html_print;
mod archive;
mod assessment;
pub mod asset_feedback;
pub mod asset_history;
pub mod delivery_feedback;
pub mod delivery_export;
pub mod issued_files;
pub mod issued_zip;
pub(crate) mod pdf_evidence;
pub mod activity_requests;
pub mod activity_tree;
pub mod activity_sources;
mod accepted_choice;
mod peer_timing;
pub mod package_review;
pub mod relationships;
mod character;
pub mod character_paths;
pub mod revision_uses;
pub mod provider_evidence;
mod platform_key;
mod experience;
mod handoff;
pub mod human_requests;
pub mod input_files;
pub mod briefs;
pub mod intake;
pub mod membership;
pub mod task_roles;
pub mod agreements;
pub mod development;
pub mod question_history;
pub mod discovery;
pub mod context;
pub mod controls;
pub mod workspace;
pub mod web;
pub mod operations;
pub mod protocol;
pub mod recovery;
pub mod runner;
pub mod store;
pub mod tools;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
pub fn failure(message: impl Into<String>) -> Error {
    std::io::Error::other(message.into()).into()
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn id(prefix: &str) -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| failure(e.to_string()))?;
    Ok(format!("{prefix}-{}", hex::encode(bytes)))
}
