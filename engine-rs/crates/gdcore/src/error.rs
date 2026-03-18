//! Common error types for the engine.
//!
//! Provides a unified error hierarchy that other crates build upon.

use thiserror::Error;

/// Top-level engine error type.
#[derive(Debug, Error)]
pub enum EngineError {
    /// An invalid operation was attempted on an object or resource.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// A requested resource was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// A type conversion failed.
    #[error("type error: expected {expected}, got {actual}")]
    TypeError {
        /// The type that was expected.
        expected: String,
        /// The type that was actually encountered.
        actual: String,
    },

    /// An I/O error occurred.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A parse error occurred.
    #[error("parse error: {0}")]
    Parse(String),
}

/// Convenience alias used throughout the engine.
pub type EngineResult<T> = Result<T, EngineError>;
