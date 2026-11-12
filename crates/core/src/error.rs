use std::io;

/// Errors produced by the compression engine.
///
/// Per-file errors are converted into [`crate::Outcome::Failed`] by the executor; only option
/// validation surfaces as a hard error to callers.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("invalid option: {0}")]
    InvalidOption(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("could not decode image: {0}")]
    Decode(String),

    #[error("could not encode image: {0}")]
    Encode(String),
}

impl CoreError {
    pub(crate) fn decode(e: impl std::fmt::Display) -> Self {
        Self::Decode(e.to_string())
    }

    pub(crate) fn encode(e: impl std::fmt::Display) -> Self {
        Self::Encode(e.to_string())
    }
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;
