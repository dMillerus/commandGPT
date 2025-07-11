// Library crate for CommandGPT - enables testing and benchmarking

// Make all modules public for testing
pub mod config;
pub mod context; 
pub mod error;
pub mod executor;
pub mod history;
pub mod openai;
pub mod safety;
pub mod telemetry;

// Re-export commonly used types for convenience
pub use error::{CommandGPTError, Result};
pub use config::AppConfig;

// Include history_old for backwards compatibility testing
#[cfg(test)]
pub mod history_old;
