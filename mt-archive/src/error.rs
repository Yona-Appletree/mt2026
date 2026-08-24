/// Errors from archive I/O: filesystem access, WAV encoding, and JSON
/// (de)serialization.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("archive I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("WAV encoding error: {0}")]
    Wav(#[from] hound::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ArchiveError>;
