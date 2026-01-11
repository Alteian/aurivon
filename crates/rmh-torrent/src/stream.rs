//! Stream information and status

use serde::{Deserialize, Serialize};

/// Information about a torrent stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    /// Unique stream ID (info hash + file index)
    pub id: String,
    
    /// Torrent info hash
    pub info_hash: String,
    
    /// File index within the torrent
    pub file_index: usize,
    
    /// Filename
    pub filename: String,
    
    /// Total file size in bytes
    pub size: u64,
    
    /// Local HTTP URL for streaming
    pub stream_url: String,
    
    /// Current download progress (0.0 - 1.0)
    pub progress: f64,
    
    /// Download speed in bytes/sec
    pub download_speed: u64,
    
    /// Upload speed in bytes/sec
    pub upload_speed: u64,
    
    /// Number of connected peers
    pub peers: u32,
    
    /// Number of seeds
    pub seeds: u32,
    
    /// Stream status
    pub status: StreamStatus,
}

/// Status of a torrent stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamStatus {
    /// Initializing (parsing magnet, connecting to peers)
    Initializing,
    /// Downloading metadata
    Metadata,
    /// Buffering initial pieces for playback
    Buffering,
    /// Ready to stream / actively streaming
    Streaming,
    /// Download complete
    Complete,
    /// Paused
    Paused,
    /// Error occurred
    Error,
}

impl Default for StreamStatus {
    fn default() -> Self {
        Self::Initializing
    }
}

/// Active torrent stream handle
#[derive(Clone, Debug)]
pub struct TorrentStream {
    /// Stream info
    pub info: StreamInfo,
    
    /// Handle to the underlying torrent
    pub(crate) handle: TorrentHandle,
}

/// Internal handle to librqbit torrent
#[derive(Clone, Debug)]
pub(crate) struct TorrentHandle {
    pub id: usize,
}

impl TorrentStream {
    /// Get the local streaming URL for MPV
    pub fn stream_url(&self) -> &str {
        &self.info.stream_url
    }
    
    /// Get current progress (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        self.info.progress
    }
    
    /// Check if ready to stream (buffered enough)
    pub fn is_ready(&self) -> bool {
        matches!(
            self.info.status,
            StreamStatus::Streaming | StreamStatus::Complete
        )
    }
    
    /// Check if download is complete
    pub fn is_complete(&self) -> bool {
        self.info.status == StreamStatus::Complete
    }
}
