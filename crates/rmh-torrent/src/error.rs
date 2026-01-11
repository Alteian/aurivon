//! Error types for torrent operations

use thiserror::Error;

pub type TorrentResult<T> = Result<T, TorrentError>;

#[derive(Debug, Error)]
pub enum TorrentError {
    #[error("Invalid magnet link: {0}")]
    InvalidMagnet(String),

    #[error("Invalid torrent file: {0}")]
    InvalidTorrent(String),

    #[error("No video files found in torrent")]
    NoVideoFiles,

    #[error("File not found in torrent: {0}")]
    FileNotFound(String),

    #[error("Torrent not found: {0}")]
    TorrentNotFound(String),

    #[error("Engine not initialized")]
    NotInitialized,

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("Streaming failed: {0}")]
    StreamingFailed(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for TorrentError {
    fn from(e: anyhow::Error) -> Self {
        TorrentError::Internal(e.to_string())
    }
}
