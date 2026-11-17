//! Torrent streaming engine for Aurivon
//!
//! This crate provides Stremio-like torrent streaming capabilities:
//! - Sequential piece downloading for instant playback
//! - Built-in HTTP server for MPV to stream from
//! - Magnet link and .torrent file support
//! - Automatic piece prioritization for video files
//!
//! ## How it works
//!
//! 1. Parse magnet link or .torrent file
//! 2. Start downloading with sequential piece priority
//! 3. Spawn local HTTP server serving the video file
//! 4. Return `http://localhost:PORT/stream` URL to player
//! 5. MPV plays the stream while download continues

mod engine;
mod server;
mod stream;
mod error;
mod addon;

pub use engine::{TorrentEngine, TorrentEngineConfig};
pub use server::StreamServer;
pub use stream::{TorrentStream, StreamInfo, StreamStatus};
pub use error::{TorrentError, TorrentResult};
pub use addon::TorrentAddon;

/// Video file extensions to prioritize in torrents
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "wmv", "flv", "webm", "m4v",
    "mpg", "mpeg", "ts", "m2ts", "vob", "3gp", "ogv",
];

/// Check if a filename is a video file
pub fn is_video_file(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    VIDEO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// Find the largest video file in a list of files
pub fn find_best_video_file(files: &[(usize, String, u64)]) -> Option<(usize, &str, u64)> {
    files
        .iter()
        .filter(|(_, name, _)| is_video_file(name))
        .max_by_key(|(_, _, size)| *size)
        .map(|(idx, name, size)| (*idx, name.as_str(), *size))
}
