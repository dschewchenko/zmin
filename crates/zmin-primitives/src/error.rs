use std::fmt;

use thiserror::Error;

/// Result type used across the core crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Unified error enumeration for the core services.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {details}")]
    Config { details: String },

    #[error("unsupported runtime: {runtime}")]
    UnsupportedRuntime { runtime: String },

    #[error("storage error: {details}")]
    Storage { details: String },

    #[error("crypto error: {details}")]
    Crypto { details: String },

    #[error("transport error: {details}")]
    Transport { details: String },

    #[error("authorization error: {details}")]
    Authorization { details: String },

    #[error("validation error: {details}")]
    Validation { details: String },

    #[error("git operation failed: {details}")]
    Git { details: String },

    #[error("command not implemented: {0}")]
    NotImplemented(&'static str),

    #[error("command exited with status {code}")]
    ExitStatus { code: i32 },

    #[error("{message}")]
    ExitMessage { code: i32, message: String },

    #[error("fatal: {message}")]
    Fatal { code: i32, message: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    /// Helper to create error from displayable details without allocating in multiple places.
    pub fn from_display<E>(error: E, variant: ErrorKind) -> Self
    where
        E: fmt::Display,
    {
        let details = error.to_string();
        match variant {
            ErrorKind::Config => Self::Config { details },
            ErrorKind::Storage => Self::Storage { details },
            ErrorKind::Crypto => Self::Crypto { details },
            ErrorKind::Transport => Self::Transport { details },
            ErrorKind::Authorization => Self::Authorization { details },
            ErrorKind::Validation => Self::Validation { details },
            ErrorKind::Git => Self::Git { details },
        }
    }
}

/// Narrow set of error categories to assist with conversions.
#[derive(Debug, Clone, Copy)]
pub enum ErrorKind {
    Config,
    Storage,
    Crypto,
    Transport,
    Authorization,
    Validation,
    Git,
}
