//! Indexed persona runtime. Reuses the application's direct model transport and
//! host executors; history is immutable and never part of a mutable state blob.
pub mod cli;
pub mod context;
pub mod controls;
pub mod jobs;
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
