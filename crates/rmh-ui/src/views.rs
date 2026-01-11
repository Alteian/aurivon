//! View definitions and traits

/// Available views in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Library,
    Discover,
    Settings,
}

/// Library view state
pub struct LibraryView {
    pub search_query: String,
    pub sort_by: SortBy,
    pub filter: MediaFilter,
}

impl Default for LibraryView {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            sort_by: SortBy::Title,
            filter: MediaFilter::All,
        }
    }
}

/// Discover view state for searching movies/shows
#[derive(Debug, Clone, Default)]
pub struct DiscoverView {
    pub search_query: String,
    pub is_searching: bool,
    pub selected_result: Option<usize>,
}

/// Settings view state  
#[derive(Debug, Clone)]
pub struct SettingsView {
    pub tmdb_api_key_input: String,
    pub real_debrid_api_key_input: String,
    pub mpv_path_input: String,
}

impl Default for SettingsView {
    fn default() -> Self {
        Self {
            tmdb_api_key_input: String::new(),
            real_debrid_api_key_input: String::new(),
            mpv_path_input: "mpv".to_string(),
        }
    }
}

/// Sort options for media grid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Title,
    DateAdded,
    ReleaseDate,
    Rating,
    FileSize,
}

/// Filter options for media grid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaFilter {
    #[default]
    All,
    Watched,
    Unwatched,
    Movies,
    TvShows,
}
