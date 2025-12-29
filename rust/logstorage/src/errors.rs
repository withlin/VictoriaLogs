use std::{error::Error, fmt};

/// Result alias used across the Rust rewrite.
pub type Result<T> = std::result::Result<T, StorageError>;

/// High-level error type for the Rust storage implementation.
#[derive(Debug)]
pub enum StorageError {
    InvalidConfig(String),
    MissingParameter(&'static str),
    ReadOnly(String),
    NotAvailableInMode(&'static str),
    NotImplemented(&'static str),
    Io(std::io::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::InvalidConfig(msg) => write!(f, "invalid configuration: {msg}"),
            StorageError::MissingParameter(name) => {
                write!(f, "missing required parameter `{name}`")
            }
            StorageError::ReadOnly(msg) => write!(f, "{msg}"),
            StorageError::NotAvailableInMode(msg) => write!(f, "{msg}"),
            StorageError::NotImplemented(msg) => write!(f, "{msg}"),
            StorageError::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            StorageError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        StorageError::Io(err)
    }
}

impl StorageError {
    /// Maps error variants to HTTP-style status codes to keep parity with the Go handler.
    pub fn status_code(&self) -> u16 {
        match self {
            StorageError::InvalidConfig(_) => 400,
            StorageError::MissingParameter(_) => 400,
            StorageError::ReadOnly(_) => 429,
            StorageError::NotAvailableInMode(_) => 400,
            StorageError::NotImplemented(_) => 501,
            StorageError::Io(_) => 500,
        }
    }
}
