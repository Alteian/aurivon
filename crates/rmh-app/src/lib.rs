//! UI-agnostic application layer for RustMediaHub.
//!
//! Front ends (GPUI today, iced legacy) only deal with this crate and
//! `rmh-player`: they render state and forward user intent. All I/O (SQLite,
//! scanning, TMDB, Trakt, scrapers, torrents) runs on a private Tokio runtime
//! inside [`MediaService`], so the UI toolkit's executor never needs Tokio.

pub mod catalog;
pub mod config;
pub mod service;
pub mod session;
pub mod trakt;

pub use catalog::{next_episode, up_next, LibraryEntry, SeasonSummary, TitleCard, TitleKind};
pub use config::{AppConfig, HomeLayout, VideoMode};
pub use service::{expand_home, is_audio, MediaService, MovieHit, ResolvedStream, ServiceResult, StreamHit};
pub use session::{format_time, resume_position, NowPlaying, ProgressUpdate, TickOutcome};
pub use trakt::{SyncReport, TraktAction, TraktDeviceCode, TraktEntry, TraktItem, TraktLists, TraktStatus};

pub use rmh_core::{Library, MediaItem};
pub use rmh_metadata::{Details, Episode, Title};
pub use rmh_player::{Chapter, PlaybackSnapshot, Player, PlayerConfig, Track, TrackKind};
