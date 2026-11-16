//! UI-agnostic application layer for RustMediaHub.
//!
//! Front ends (GPUI today, iced legacy) only deal with this crate and
//! `rmh-player`: they render state and forward user intent. All I/O (SQLite,
//! scanning, TMDB, scrapers, torrents) runs on a private Tokio runtime inside
//! [`MediaService`], so the UI toolkit's executor never needs Tokio.

pub mod config;
pub mod service;
pub mod session;

pub use config::AppConfig;
pub use service::{expand_home, is_audio, MediaService, MovieHit, ResolvedStream, ServiceResult, StreamHit};
pub use session::{format_time, resume_position, NowPlaying, ProgressUpdate, TickOutcome};

pub use rmh_core::{Library, MediaItem};
pub use rmh_player::{PlaybackSnapshot, Player, PlayerConfig};
