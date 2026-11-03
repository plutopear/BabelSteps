//! Typed errors returned to the frontend. Messages never contain document
//! content (titles, step text, screenshots); only error kinds, counts and
//! file paths the user chose or the app made.

use babelsteps_core::export::ExportError;
use babelsteps_core::storage::StorageError;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// No recording is open.
    NoRecordingOpen,
    /// The caller passed something we refuse (bad path, empty title, ...).
    InvalidArgument,
    NotFound,
    /// Storage or file system trouble.
    Storage,
    /// Export failed before printing (images, files).
    Export,
    /// Another export is already running.
    Busy,
    /// Printing to PDF failed or is not available on this platform.
    Pdf,
    /// The folder was written by a newer version.
    TooNew,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        AppError { kind, message: message.into() }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl From<StorageError> for AppError {
    fn from(e: StorageError) -> Self {
        let kind = match &e {
            StorageError::TooNew { .. } => ErrorKind::TooNew,
            StorageError::NotABundle(_) => ErrorKind::NotFound,
            StorageError::Refused(_) => ErrorKind::InvalidArgument,
            _ => ErrorKind::Storage,
        };
        AppError::new(kind, e.to_string())
    }
}

impl From<ExportError> for AppError {
    fn from(e: ExportError) -> Self {
        AppError::new(ErrorKind::Export, e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::new(ErrorKind::Storage, format!("file error: {e}"))
    }
}

pub type AppResult<T> = Result<T, AppError>;
