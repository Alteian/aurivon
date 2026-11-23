//! UI-agnostic application layer for Aurivon.
//!
//! Front ends (GPUI today, iced legacy) only deal with this crate and
//! `rmh-player`: they render state and forward user intent. All I/O (SQLite,
//! scanning, TMDB, Trakt, scrapers, torrents) runs on a private Tokio runtime
//! inside [`MediaService`], so the UI toolkit's executor never needs Tokio.

pub mod addons;
pub mod calendar;
pub mod catalog;
pub mod discovery;
pub mod fuzzy;
pub mod images;
pub mod intro;
pub mod streams;
pub mod social;
pub mod storage;
pub mod subs;
pub mod lists;
pub mod watchlist;
pub mod config;
pub mod service;
pub mod session;
pub mod trakt;
pub mod update;
pub mod migrate;

pub use catalog::{next_episode, up_next, LibraryEntry, SeasonSummary, TitleCard, TitleKind};
pub use calendar::{month_grid, CalendarItem, CalendarMonth, ReleaseKind};
pub use config::{
    env_alias, env_flag, lang_code, smart_collection_builtins, AppConfig, HomeLayout, InstalledAddon, TraktWatchlistPolicy, VideoMode,
};
pub use storage::{repair_default_database, StorageBucket, StorageReport};
pub use images::CacheUsage;
pub use discovery::{
    entry_is_matched, library_ids, search_library, BrowseResults, BrowseSort, CastMember, Credit, DiscoverQuery, Extras,
    Facets, Genre, ListKind, PersonHit, PersonPage, Provider, SearchHits, TitleAvailability, TitleExtras, Video,
};
pub use streams::{AudioFilter, Codec, HdrFilter, Resolution, Source, StreamFilter, StreamSort, StreamTraits};
pub use social::{ServiceStatus, SimklPin, WatchEvent};
pub use subs::{SubContext, SubtitleHit};
pub use watchlist::{WatchItem, WatchlistSyncReport};
pub use lists::{ListScope, ListSort, ListTypeFilter, ListsCache};
pub use intro::IntroSegments;
pub use addons::{CatalogEntry, RemoteMeta, RemoteStream};
pub use rmh_services::opensubtitles::language_name as language_label;
pub use rmh_services::simkl::Scrobble as SocialScrobble;
pub use service::{
    expand_home, is_audio, MediaService, MovieHit, ResolvedStream, ServiceResult, StreamHit, StreamSearchReport,
};
pub use session::{format_time, resume_position, NowPlaying, ProgressUpdate, TickOutcome};
pub use trakt::{SyncReport, TraktAction, TraktEntry, TraktItem, TraktLists, TraktPkceLogin, TraktStatus};
pub use rmh_trakt::{ShowProgress, UserList};

pub use rmh_core::{Library, MediaItem, MediaType};
pub use rmh_metadata::{Details, Episode, Title};
pub use rmh_player::{Chapter, PlaybackSnapshot, Player, PlayerConfig, Track, TrackKind};
