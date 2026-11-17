//! UI-agnostic application layer for RustMediaHub.
//!
//! Front ends (GPUI today, iced legacy) only deal with this crate and
//! `rmh-player`: they render state and forward user intent. All I/O (SQLite,
//! scanning, TMDB, Trakt, scrapers, torrents) runs on a private Tokio runtime
//! inside [`MediaService`], so the UI toolkit's executor never needs Tokio.

pub mod calendar;
pub mod catalog;
pub mod discovery;
pub mod fuzzy;
pub mod social;
pub mod subs;
pub mod watchlist;
pub mod config;
pub mod service;
pub mod session;
pub mod trakt;
pub mod update;

pub use catalog::{next_episode, up_next, LibraryEntry, SeasonSummary, TitleCard, TitleKind};
pub use calendar::{month_grid, CalendarItem, CalendarMonth, ReleaseKind};
pub use config::{lang_code, AppConfig, HomeLayout, VideoMode};
pub use discovery::{
    library_ids, search_library, BrowseResults, BrowseSort, CastMember, Credit, DiscoverQuery, Extras, Facets, Genre, ListKind,
    PersonHit, PersonPage, Provider, SearchHits, TitleExtras, Video,
};
pub use social::{ServiceStatus, SimklPin, WatchEvent};
pub use subs::{SubContext, SubtitleHit};
pub use watchlist::WatchItem;
pub use rmh_services::opensubtitles::language_name as language_label;
pub use rmh_services::simkl::Scrobble as SocialScrobble;
pub use service::{expand_home, is_audio, MediaService, MovieHit, ResolvedStream, ServiceResult, StreamHit};
pub use session::{format_time, resume_position, NowPlaying, ProgressUpdate, TickOutcome};
pub use trakt::{SyncReport, TraktAction, TraktDeviceCode, TraktEntry, TraktItem, TraktLists, TraktStatus};

pub use rmh_core::{Library, MediaItem};
pub use rmh_metadata::{Details, Episode, Title};
pub use rmh_player::{Chapter, PlaybackSnapshot, Player, PlayerConfig, Track, TrackKind};
