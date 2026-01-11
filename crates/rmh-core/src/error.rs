//! Core error types for RustMediaHub

use thiserror::Error;

/// Core error type for the application
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Metadata fetch error: {0}")]
    Metadata(String),

    #[error("Player error: {0}")]
    Player(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// Result type alias using our Error
pub type Result<T> = std::result::Result<T, CoreError>;
