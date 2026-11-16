//! Main application state and logic

use crate::views::View;
use crate::config::AppConfig;
use anyhow::Result;
use iced::widget::{button, column, container, horizontal_space, row, text, text_input, Column, Row};
use iced::{Element, Length, Task};
use rmh_core::{Library, MediaItem, MediaType};
use rmh_database::{Database, MediaRepository};
use rmh_metadata::{TmdbClient, MovieSearchResult, PosterSize};
use rmh_player::{Player, PlayerConfig};
use rmh_scanner::Scanner;
use rmh_scraper::{ScraperAddon, ScraperAddonConfig, ScrapedResult, SearchQuery};
use rmh_torrent::{TorrentEngine, TorrentStream};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

/// Main application state
pub struct RustMediaHub {
    /// Current view being displayed
    current_view: View,
    /// All libraries
    libraries: Vec<Library>,
    /// Currently displayed media items
    media_items: Vec<MediaItem>,
    /// Selected media item
    selected_item: Option<Uuid>,
    /// Video player
    player: Player,
    /// Loading state
    is_loading: bool,
    /// Status message
    status_message: String,
    /// Error message (if any)
    error_message: Option<String>,
    /// App configuration
    config: AppConfig,
    /// Discover search query
    discover_query: String,
    /// TMDB movie search results (for title search)
    movie_search_results: Vec<MovieSearchResult>,
    /// Selected movie from TMDB search
    selected_movie: Option<usize>,
    /// Main window ID (XID) for MPV embedding
    window_id: Option<u64>,
    /// Discover stream results (from scraper)
    discover_results: Vec<ScrapedResult>,
    /// Is discover search in progress
    is_searching: bool,
    /// Scraper addon (initialized lazily)
    scraper: Option<Arc<RwLock<ScraperAddon>>>,
    /// TMDB client for movie search
    tmdb: Option<TmdbClient>,
    /// Selected discover result index
    selected_discover_result: Option<usize>,
    /// Torrent engine for P2P streaming
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    /// Currently active torrent stream
    active_stream: Option<TorrentStream>,
    /// Settings view state
    settings_view: crate::views::SettingsView,
    /// Frame buffer for software rendering (Wayland fallback)
    video_frame: Option<iced::widget::image::Handle>,
    /// Player state
    player_position: f64,
    player_duration: Option<f64>,
    is_paused: bool,
}



/// Messages that drive the application
#[derive(Debug, Clone)]
pub enum Message {
    // Navigation
    NavigateTo(View),

    // Database
    DatabaseInitialized(Result<(), String>),
    
    // Torrent Engine
    TorrentEngineInitialized(Result<(), String>),
    TorrentStreamReady(Result<TorrentStream, String>),
    
    // Library management
    LibrariesLoaded(Result<Vec<Library>, String>),
    AddLibrary(String, String), // name, path
    LibraryAdded(Result<Library, String>),
    RemoveLibrary(Uuid),
    LibraryRemoved(Result<Uuid, String>),
    
    // Media items
    MediaLoaded(Result<Vec<MediaItem>, String>),
    SelectMedia(Uuid),
    RefreshLibrary,
    
    // Scanning
    ScanLibrary(Uuid, String), // library_id, path
    ScanComplete(Result<usize, String>),
    
    // Playback
    PlayMedia(Uuid),
    PlayStreamUrl(String),
    PlaybackStarted(Result<(), String>),
    Tick, // Frame update loop
    TogglePause,
    Seek(f64),
    StopPlayback,
    
    // Settings
    UpdateTmdbApiKey(String),
    UpdateRealDebridApiKey(String),
    SaveSettings,
    SettingsSaved(Result<(), String>),
    
    // UI feedback
    SetStatus(String),
    ClearError,
    
    // Folder picker
    PickFolder,
    FolderPicked(Option<PathBuf>),
    
    // Discover/Search - Title search (TMDB)
    DiscoverQueryChanged(String),
    DiscoverSearch,
    MovieSearchResults(Result<Vec<MovieSearchResultData>, String>),
    SelectMovie(usize),
    
    // Discover/Search - Stream search (Scraper)
    SearchStreamsForMovie(String, String), // imdb_id, title
    DiscoverResultsLoaded(Result<Vec<ScrapedResultData>, String>),
    SelectDiscoverResult(usize),
    PlayDiscoverResult(usize),
    
    // Back navigation in discover
    BackToMovieSearch,
}

/// Serializable movie search result for message passing
#[derive(Debug, Clone)]
pub struct MovieSearchResultData {
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub vote_average: Option<f32>,
}

/// Serializable version of ScrapedResult for message passing
#[derive(Debug, Clone)]
pub struct ScrapedResultData {
    pub title: String,
    pub info_hash: String,
    pub magnet: String,
    pub quality: String,
    pub size: Option<String>,
    pub seeders: Option<u32>,
    pub source: String,
    pub rd_cached: bool,
}

impl RustMediaHub {
    /// Create a new application instance
    pub fn new() -> (Self, Task<Message>) {
        let config = AppConfig::load().unwrap_or_else(|e| {
             info!("Failed to load config: {}, using defaults", e);
             AppConfig::default()
        });
        
        // Initialize scraper if Real-Debrid key is available
        let scraper = config.real_debrid_api_key.as_ref().map(|key| {
            info!("Real-Debrid API key found, initializing scraper");
            let scraper_config = ScraperAddonConfig::default()
                .with_real_debrid(key.clone());
            Arc::new(RwLock::new(ScraperAddon::new(scraper_config)))
        });

        // Initialize TMDB client if API key is available
        let tmdb = config.tmdb_api_key.as_ref().and_then(|key| {
            info!("TMDB API key found, initializing client");
            TmdbClient::new(key.clone()).ok()
        });

        let app = Self {
            current_view: View::Library,
            libraries: Vec::new(),
            media_items: Vec::new(),
            selected_item: None,
            player: Player::with_defaults(),
            is_loading: true,
            status_message: "Initializing...".to_string(),
            error_message: None,
            config: config.clone(),
            discover_query: String::new(),
            movie_search_results: Vec::new(),
            selected_movie: None,
            window_id: None,
            video_frame: None,
            player_position: 0.0,
            player_duration: None,
            is_paused: false,
            discover_results: Vec::new(),
            is_searching: false,
            scraper,
            tmdb,
            selected_discover_result: None,
            torrent_engine: Arc::new(RwLock::new(TorrentEngine::with_defaults())),
            active_stream: None,
            settings_view: crate::views::SettingsView {
                tmdb_api_key_input: config.tmdb_api_key.clone().unwrap_or_default(),
                real_debrid_api_key_input: config.real_debrid_api_key.clone().unwrap_or_default(),
                mpv_path_input: "mpv".to_string(),
            },
        };

        // Initialize tasks
        let db_task = Task::perform(
            async {
                let db_path = Database::default_path()
                    .map_err(|e| e.to_string())?;
                Database::new(&db_path)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
            Message::DatabaseInitialized,
        );

        let engine = Arc::clone(&app.torrent_engine);
        let torrent_task = Task::perform(
            async move {
                let mut engine = engine.write().await;
                engine.initialize().await.map_err(|e| e.to_string())
            },
            Message::TorrentEngineInitialized,
        );

        (app, Task::batch(vec![db_task, torrent_task]))
    }

    /// Handle application title
    pub fn title(&self) -> String {
        "RustMediaHub".to_string()
    }

    /// Try to find and attach the window ID if not already set
    fn ensure_window_id(&mut self) {
        if self.window_id.is_none() {
             #[cfg(target_os = "linux")]
             // We search for the window by title. This requires the window to be mapped.
             // On macOS/Windows, this might require different implementation.
             if let Some(wid) = crate::window::find_window_by_title("RustMediaHub") {
                 info!("Found main window ID: {}", wid);
                 self.window_id = Some(wid);
                 
                 // Update player config with wid
                 let mut config = self.player.get_config();
                 config.wid = Some(wid as i64);
                 self.player.set_config(config);
             } else {
                // If we are on Wayland native, find_window_by_title might fail or return nothing useful for mpv.
                // We just log it. MPV will open in a separate window.
                info!("Could not find main window ID (using Wayland native?). MPV will launch in separate window.");
             }
        }
    }

    /// Update application state based on message
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NavigateTo(view) => {
                self.current_view = view;
                Task::none()
            }

            Message::TorrentEngineInitialized(result) => {
                if let Err(e) = result {
                    info!("Failed to initialize torrent engine: {}", e);
                    // We don't block the app, but streaming won't work
                } else {
                    info!("Torrent engine initialized");
                }
                Task::none()
            }

            Message::TorrentStreamReady(result) => {
                self.ensure_window_id();
                match result {
                    Ok(stream) => {
                        self.status_message = format!("Playing: {}", stream.info.filename);
                        let url = stream.info.stream_url.clone();
                        self.active_stream = Some(stream);
                        // Stop spinner if it was running
                        self.is_loading = false;
                        
                        if let Err(e) = self.player.play_url(&url) {
                            self.error_message = Some(format!("Playback error: {}", e));
                        }
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Failed to start stream: {}", e));
                        self.status_message = "Streaming failed".to_string();
                        // Stop spinner
                        self.is_loading = false;
                    }
                }
                Task::none()
            }

            Message::UpdateTmdbApiKey(key) => {
                self.settings_view.tmdb_api_key_input = key;
                Task::none()
            }

            Message::UpdateRealDebridApiKey(key) => {
                self.settings_view.real_debrid_api_key_input = key;
                Task::none()
            }

            Message::SaveSettings => {
                let mut config = self.config.clone();
                config.tmdb_api_key = Some(self.settings_view.tmdb_api_key_input.clone())
                    .filter(|s| !s.is_empty());
                config.real_debrid_api_key = Some(self.settings_view.real_debrid_api_key_input.clone())
                    .filter(|s| !s.is_empty());
                    
                self.config = config.clone();
                
                // Re-initialize clients
                if let Some(key) = &self.config.tmdb_api_key {
                     self.tmdb = TmdbClient::new(key.clone()).ok();
                }
                
                if let Some(key) = &self.config.real_debrid_api_key {
                    let scraper_config = ScraperAddonConfig::default()
                        .with_real_debrid(key.clone());
                    self.scraper = Some(Arc::new(RwLock::new(ScraperAddon::new(scraper_config))));
                }

                Task::perform(
                    async move {
                        config.save().map_err(|e| e.to_string())
                    },
                    Message::SettingsSaved
                )
            }
            
            Message::SettingsSaved(result) => {
                match result {
                    Ok(_) => self.status_message = "Settings saved".to_string(),
                    Err(e) => self.error_message = Some(format!("Failed to save settings: {}", e)),
                }
                Task::none()
            }

            Message::DatabaseInitialized(result) => {
                match result {
                    Ok(()) => {
                        self.status_message = "Database ready".to_string();
                        self.is_loading = false;
                        
                        // Load database and libraries
                        Task::perform(
                            async {
                                let db_path = Database::default_path()
                                    .map_err(|e| e.to_string())?;
                                let db = Database::new(&db_path)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let repo = MediaRepository::new(db.pool());
                                let libraries = repo.get_all_libraries()
                                    .await
                                    .map_err(|e| e.to_string())?;
                                Ok(libraries)
                            },
                            Message::LibrariesLoaded,
                        )
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Database error: {}", e));
                        self.is_loading = false;
                        Task::none()
                    }
                }
            }

            Message::LibrariesLoaded(result) => {
                match result {
                    Ok(libraries) => {
                        self.libraries = libraries;
                        self.status_message = format!("{} libraries loaded", self.libraries.len());
                        
                        // Load all media items
                        Task::perform(
                            async {
                                let db_path = Database::default_path()
                                    .map_err(|e| e.to_string())?;
                                let db = Database::new(&db_path)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let repo = MediaRepository::new(db.pool());
                                repo.get_all_media()
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            Message::MediaLoaded,
                        )
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                        Task::none()
                    }
                }
            }

            Message::MediaLoaded(result) => {
                match result {
                    Ok(items) => {
                        self.media_items = items;
                        self.status_message = format!("{} media items", self.media_items.len());
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                    }
                }
                Task::none()
            }

            Message::AddLibrary(name, path) => {
                let library = Library::new(name, path.clone(), MediaType::Movie);
                let library_clone = library.clone();
                
                Task::perform(
                    async move {
                        let db_path = Database::default_path()
                            .map_err(|e| e.to_string())?;
                        let db = Database::new(&db_path)
                            .await
                            .map_err(|e| e.to_string())?;
                        let repo = MediaRepository::new(db.pool());
                        repo.insert_library(&library_clone)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(library_clone)
                    },
                    Message::LibraryAdded,
                )
            }

            Message::LibraryAdded(result) => {
                match result {
                    Ok(library) => {
                        let lib_id = library.id;
                        let lib_path = library.path.clone();
                        self.libraries.push(library);
                        self.status_message = "Library added. Scanning...".to_string();
                        
                        // Start scanning
                        Task::perform(
                            async move {
                                let db_path = Database::default_path()
                                    .map_err(|e| e.to_string())?;
                                let db = Database::new(&db_path)
                                    .await
                                    .map_err(|e| e.to_string())?;
                                let scanner = Scanner::new(db);
                                let items = scanner.scan_directory(
                                    lib_id,
                                    std::path::Path::new(&lib_path),
                                    None,
                                ).await.map_err(|e| e.to_string())?;
                                Ok(items.len())
                            },
                            Message::ScanComplete,
                        )
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                        Task::none()
                    }
                }
            }

            Message::RemoveLibrary(id) => {
                Task::perform(
                    async move {
                        let db_path = Database::default_path()
                            .map_err(|e| e.to_string())?;
                        let db = Database::new(&db_path)
                            .await
                            .map_err(|e| e.to_string())?;
                        let repo = MediaRepository::new(db.pool());
                        repo.delete_library(id)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(id)
                    },
                    Message::LibraryRemoved,
                )
            }

            Message::LibraryRemoved(result) => {
                match result {
                    Ok(id) => {
                        self.libraries.retain(|l| l.id != id);
                        self.media_items.retain(|_| true); // TODO: filter by library
                        self.status_message = "Library removed".to_string();
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                    }
                }
                Task::none()
            }

            Message::SelectMedia(id) => {
                self.selected_item = Some(id);
                Task::none()
            }

            Message::RefreshLibrary => {
                self.is_loading = true;
                self.status_message = "Refreshing...".to_string();
                
                Task::perform(
                    async {
                        let db_path = Database::default_path()
                            .map_err(|e| e.to_string())?;
                        let db = Database::new(&db_path)
                            .await
                            .map_err(|e| e.to_string())?;
                        let repo = MediaRepository::new(db.pool());
                        repo.get_all_media()
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::MediaLoaded,
                )
            }

            Message::ScanLibrary(library_id, path) => {
                self.is_loading = true;
                self.status_message = "Scanning...".to_string();
                
                Task::perform(
                    async move {
                        let db_path = Database::default_path()
                            .map_err(|e| e.to_string())?;
                        let db = Database::new(&db_path)
                            .await
                            .map_err(|e| e.to_string())?;
                        let scanner = Scanner::new(db);
                        let items = scanner.scan_directory(
                            library_id,
                            std::path::Path::new(&path),
                            None,
                        ).await.map_err(|e| e.to_string())?;
                        Ok(items.len())
                    },
                    Message::ScanComplete,
                )
            }

            Message::ScanComplete(result) => {
                self.is_loading = false;
                match result {
                    Ok(count) => {
                        self.status_message = format!("Scan complete: {} new items", count);
                        // Refresh the media list
                        return self.update(Message::RefreshLibrary);
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                    }
                }
                Task::none()
            }

            Message::PlayMedia(id) => {
                self.ensure_window_id();
                if let Some(item) = self.media_items.iter().find(|m| m.id == id) {
                    let path = PathBuf::from(&item.file_path);
                    let progress = item.watch_progress;
                    
                    match self.player.play_at_position(&path, Some(progress.max(0) as u64)) {
                        Ok(()) => {
                            self.status_message = format!("Playing: {}", item.title);
                        }
                        Err(e) => {
                            self.error_message = Some(format!("Playback error: {}", e));
                        }
                    }
                }
                Task::none()
            }

            Message::PlaybackStarted(result) => {
                match result {
                    Ok(()) => {
                        self.status_message = "Playback started".to_string();
                    }
                    Err(e) => {
                        self.error_message = Some(e);
                    }
                }
                Task::none()
            }

            Message::Tick => {
                // If MPV is running (no window ID means separate window or SW), update state
                if self.window_id.is_none() {
                   
                   // Update playback state
                   if let Some(pos) = self.player.get_time_pos() {
                       self.player_position = pos;
                   }
                   if let Some(dur) = self.player.get_duration() {
                       self.player_duration = Some(dur);
                   }
                   self.is_paused = self.player.is_paused();
                }
                Task::none()
            }

            Message::TogglePause => {
                let _ = self.player.set_paused(!self.is_paused);
                self.is_paused = !self.is_paused; // Optimistic update
                Task::none()
            }

            Message::Seek(pos) => {
                let _ = self.player.seek(pos);
                self.player_position = pos; // Optimistic update
                Task::none()
            }

            Message::StopPlayback => {
                let _ = self.player.stop();
                self.video_frame = None;
                // self.active_stream = None; // User wants to keep cache? Maybe keep stream alive in background?
                // If we drop active_stream, the HTTP server and download stop.
                // Keeping it around allows resuming later or other players to connect.
                // But generally stopping playback means "done watching".
                // If user wants to just go back, maybe we should keep it.
                // For now, let's keep active_stream but clear video_frame to return to UI.
                Task::none()
            }

            Message::SetStatus(msg) => {
                self.status_message = msg;
                Task::none()
            }

            Message::ClearError => {
                self.error_message = None;
                Task::none()
            }

            Message::PickFolder => {
                // For now, we'll use a simple approach
                // In production, use rfd crate for native file dialogs
                Task::none()
            }

            Message::FolderPicked(path) => {
                if let Some(path) = path {
                    let name = path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Library")
                        .to_string();
                    return self.update(Message::AddLibrary(name, path.to_string_lossy().to_string()));
                }
                Task::none()
            }

            // Discover messages
            Message::DiscoverQueryChanged(query) => {
                self.discover_query = query;
                Task::none()
            }

            Message::DiscoverSearch => {
                if self.discover_query.trim().is_empty() {
                    self.error_message = Some("Please enter a search query".to_string());
                    return Task::none();
                }

                // Check if query is already an IMDB ID
                if self.discover_query.starts_with("tt") {
                    // Direct stream search with IMDB ID
                    let Some(ref scraper) = self.scraper else {
                        self.error_message = Some("Real-Debrid not configured. Add REAL_DEBRID_API_KEY to .env".to_string());
                        return Task::none();
                    };

                    self.is_searching = true;
                    self.status_message = format!("Searching streams for {}...", self.discover_query);
                    self.discover_results.clear();
                    self.movie_search_results.clear();
                    self.selected_movie = None;
                    
                    let imdb_id = self.discover_query.clone();
                    let scraper = Arc::clone(scraper);

                    return Task::perform(
                        async move {
                            let scraper = scraper.write().await;
                            let search_query = SearchQuery::movie(&imdb_id, "");
                            let results = scraper.search(&search_query).await
                                .map_err(|e| e.to_string())?;

                            let data: Vec<ScrapedResultData> = results.iter().map(|r| ScrapedResultData {
                                title: r.title.clone(),
                                info_hash: r.info_hash.clone(),
                                magnet: r.magnet.clone(),
                                quality: r.parsed.quality_string(),
                                size: r.size_str.clone(),
                                seeders: r.seeders,
                                source: r.source.clone(),
                                rd_cached: r.rd_cached,
                            }).collect();

                            Ok(data)
                        },
                        Message::DiscoverResultsLoaded,
                    );
                }

                // Otherwise, search TMDB for movies by title
                let Some(ref tmdb) = self.tmdb else {
                    self.error_message = Some("TMDB not configured. Add TMDB_API_KEY to .env".to_string());
                    return Task::none();
                };

                self.is_searching = true;
                self.status_message = format!("Searching movies for '{}'...", self.discover_query);
                self.movie_search_results.clear();
                self.selected_movie = None;
                self.discover_results.clear();
                
                let query = self.discover_query.clone();
                let tmdb = tmdb.clone();

                Task::perform(
                    async move {
                        let results = tmdb.search_movie_with_imdb(&query).await
                            .map_err(|e| e.to_string())?;
                        
                        // Convert to serializable format
                        let data: Vec<MovieSearchResultData> = results.iter().map(|r| MovieSearchResultData {
                            tmdb_id: r.tmdb_id,
                            imdb_id: r.imdb_id.clone(),
                            title: r.title.clone(),
                            overview: r.overview.clone(),
                            year: r.year,
                            poster_url: r.poster_url(rmh_metadata::PosterSize::W342),
                            vote_average: r.vote_average,
                        }).collect();
                        
                        Ok(data)
                    },
                    Message::MovieSearchResults,
                )
            }

            Message::MovieSearchResults(result) => {
                self.is_searching = false;
                match result {
                    Ok(results) => {
                        let count = results.len();
                        self.movie_search_results = results.iter().map(|r| MovieSearchResult {
                            tmdb_id: r.tmdb_id,
                            imdb_id: r.imdb_id.clone(),
                            title: r.title.clone(),
                            original_title: r.title.clone(), // Use title as original
                            overview: r.overview.clone(),
                            poster_path: None, // Already have URL in data
                            backdrop_path: None,
                            release_date: r.year.map(|y| format!("{}-01-01", y)),
                            vote_average: r.vote_average,
                            year: r.year,
                        }).collect();
                        
                        if count == 0 {
                            self.status_message = "No movies found".to_string();
                        } else {
                            self.status_message = format!("Found {} movies - select one to search streams", count);
                        }
                    }
                    Err(e) => {
                        self.error_message = Some(format!("TMDB search failed: {}", e));
                    }
                }
                Task::none()
            }

            Message::SelectMovie(index) => {
                self.selected_movie = Some(index);
                
                // Automatically trigger stream search
                if let Some(movie) = self.movie_search_results.get(index) {
                    if let Some(ref imdb_id) = movie.imdb_id {
                        let Some(ref scraper) = self.scraper else {
                            self.error_message = Some("Real-Debrid not configured".to_string());
                            return Task::none();
                        };

                        self.is_searching = true;
                        self.status_message = format!("Searching streams for '{}'...", movie.title);
                        self.discover_results.clear();
                        
                        let imdb_id = imdb_id.clone();
                        let scraper = Arc::clone(scraper);

                        return Task::perform(
                            async move {
                                let scraper = scraper.write().await;
                                let search_query = SearchQuery::movie(&imdb_id, "");
                                let results = scraper.search(&search_query).await
                                    .map_err(|e| e.to_string())?;

                                let data: Vec<ScrapedResultData> = results.iter().map(|r| ScrapedResultData {
                                    title: r.title.clone(),
                                    info_hash: r.info_hash.clone(),
                                    magnet: r.magnet.clone(),
                                    quality: r.parsed.quality_string(),
                                    size: r.size_str.clone(),
                                    seeders: r.seeders,
                                    source: r.source.clone(),
                                    rd_cached: r.rd_cached,
                                }).collect();

                                Ok(data)
                            },
                            Message::DiscoverResultsLoaded,
                        );
                    } else {
                        self.error_message = Some("No IMDB ID available for this movie".to_string());
                    }
                }
                Task::none()
            }

            Message::BackToMovieSearch => {
                self.selected_movie = None;
                self.discover_results.clear();
                self.status_message = format!("Found {} movies", self.movie_search_results.len());
                Task::none()
            }

            Message::SearchStreamsForMovie(imdb_id, title) => {
                let Some(ref scraper) = self.scraper else {
                    self.error_message = Some("Real-Debrid not configured".to_string());
                    return Task::none();
                };

                self.is_searching = true;
                self.status_message = format!("Searching streams for '{}'...", title);
                self.discover_results.clear();
                
                let scraper = Arc::clone(scraper);

                Task::perform(
                    async move {
                        let scraper = scraper.write().await;
                        let search_query = SearchQuery::movie(&imdb_id, "");
                        let results = scraper.search(&search_query).await
                            .map_err(|e| e.to_string())?;

                        let data: Vec<ScrapedResultData> = results.iter().map(|r| ScrapedResultData {
                            title: r.title.clone(),
                            info_hash: r.info_hash.clone(),
                            magnet: r.magnet.clone(),
                            quality: r.parsed.quality_string(),
                            size: r.size_str.clone(),
                            seeders: r.seeders,
                            source: r.source.clone(),
                            rd_cached: r.rd_cached,
                        }).collect();

                        Ok(data)
                    },
                    Message::DiscoverResultsLoaded,
                )
            }

            Message::DiscoverResultsLoaded(result) => {
                self.is_searching = false;
                match result {
                    Ok(results) => {
                        let count = results.len();
                        // Convert back to ScrapedResult for storage
                        self.discover_results = results.iter().map(|r| {
                            let mut result = ScrapedResult::from_hash(&r.title, &r.info_hash, &r.source);
                            result.rd_cached = r.rd_cached;
                            result.seeders = r.seeders;
                            result.size_str = r.size.clone();
                            result
                        }).collect();
                        
                        self.status_message = format!("Found {} streams", count);
                    }
                    Err(e) => {
                        self.error_message = Some(format!("Search failed: {}", e));
                    }
                }
                Task::none()
            }

            Message::SelectDiscoverResult(index) => {
                self.selected_discover_result = Some(index);
                Task::none()
            }

            Message::PlayDiscoverResult(index) => {
                if let Some(result) = self.discover_results.get(index) {
                    let url = if result.rd_cached {
                        // For RD cached, we need to resolve the stream
                        // For now, show the magnet
                        result.magnet.clone()
                    } else {
                        result.magnet.clone()
                    };
                    
                    self.status_message = format!("Playing: {}", result.title);
                    
                    // If it's a direct URL, play it
                    if url.starts_with("http") {
                        if let Err(e) = self.player.play_url(&url) {
                            self.error_message = Some(format!("Playback error: {}", e));
                        }
                        Task::none()
                    } else if url.starts_with("magnet:") {
                        let magnet = url.clone();
                        let engine = Arc::clone(&self.torrent_engine);
                        
                        self.status_message = format!("Resolving magnet: {}...", result.title);
                        self.is_loading = true;

                        Task::perform(
                            async move {
                                let engine = engine.read().await;
                                engine.add_magnet(&magnet, None).await.map_err(|e| e.to_string())
                            },
                            Message::TorrentStreamReady,
                        )
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }

            Message::PlayStreamUrl(url) => {
                self.ensure_window_id();
                if let Err(e) = self.player.play_url(&url) {
                    self.error_message = Some(format!("Playback error: {}", e));
                } else {
                    self.status_message = "Playing stream...".to_string();
                }
                Task::none()
            }
        }
    }

    /// Application subscription
    pub fn subscription(&self) -> iced::Subscription<Message> {
        let mut subs = Vec::new();
        
        // Only tick if player is active and we are in software rendering mode (no window ID)
        if self.window_id.is_none() && self.player.is_active() {
            subs.push(iced::time::every(std::time::Duration::from_millis(16)).map(|_| Message::Tick));
        }

        // Add keyboard listener if player is active (video_frame is Some)
        if self.video_frame.is_some() {
             subs.push(
                 iced::event::listen_with(|event, _status, _window| {
                    if let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, .. }) = event {
                        match key {
                           iced::keyboard::Key::Named(iced::keyboard::key::Named::Backspace) => Some(Message::StopPlayback),
                           iced::keyboard::Key::Named(iced::keyboard::key::Named::Space) => Some(Message::TogglePause),
                           iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => Some(Message::StopPlayback),
                           _ => None,
                        }
                    } else {
                        None
                    }
                 })
             );
        }

        iced::Subscription::batch(subs)
    }

    /// Render the application view
    pub fn view(&self) -> Element<'_, Message> {
        // If we have a video frame (software rendering), show it with controls
        if let Some(handle) = &self.video_frame {
             let image_widget = iced::widget::image(handle.clone())
                     .width(Length::Fill)
                     .height(Length::Fill)
                     .content_fit(iced::ContentFit::Contain);
            
             // Controls
             let play_pause_icon = if self.is_paused { "▶" } else { "⏸" };
             let play_btn = button(text(play_pause_icon).size(20))
                .on_press(Message::TogglePause)
                .padding(10);
                
             let stop_btn = button(text("⏹").size(20))
                .on_press(Message::StopPlayback)
                .padding(10);
             
             let current_time_str = format_duration(self.player_position);
             let total_time_str = self.player_duration.map(format_duration).unwrap_or_else(|| "--:--".to_string());
             
             let time_label = text(format!("{} / {}", current_time_str, total_time_str)).style(text::secondary);
             
             let slider = if let Some(duration) = self.player_duration {
                 iced::widget::slider(0.0..=duration, self.player_position, Message::Seek)
                     .step(1.0)
                     .width(Length::Fill)
             } else {
                 iced::widget::slider(0.0..=100.0, 0.0, |_| Message::Tick).width(Length::Fill) // Dummy slider
             };

             let controls = container(
                 row![
                    play_btn,
                    stop_btn,
                    time_label,
                    slider
                 ]
                 .spacing(10)
                 .align_y(iced::Alignment::Center)
                 .padding(10)
             )
             .style(container::dark)
             .width(Length::Fill);

             // Vertical layout: Image takes remaining space, controls at bottom
             return column![
                 container(image_widget).width(Length::Fill).height(Length::Fill).style(container::dark),
                 controls
             ]
             .width(Length::Fill)
             .height(Length::Fill)
             .into();
        }

        let content: Element<Message> = match self.current_view {
            View::Library => self.view_library(),
            View::Discover => self.view_discover(),
            View::Settings => self.view_settings(),
        };

        // Main layout with navigation
        let nav_bar = self.view_nav_bar();
        let status_bar = self.view_status_bar();

        let main_content = column![
            nav_bar,
            container(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20),
            status_bar,
        ];

        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::dark)
            .into()
    }

    /// Navigation bar
    fn view_nav_bar(&self) -> Element<'_, Message> {
        let library_btn = button(text("📁 Library"))
            .on_press(Message::NavigateTo(View::Library))
            .padding(10);

        let discover_btn = button(text("🔍 Discover"))
            .on_press(Message::NavigateTo(View::Discover))
            .padding(10);

        let settings_btn = button(text("⚙ Settings"))
            .on_press(Message::NavigateTo(View::Settings))
            .padding(10);

        let refresh_btn = button(text("↻ Refresh"))
            .on_press(Message::RefreshLibrary)
            .padding(10);

        // Show RD status
        let rd_status = if self.scraper.is_some() {
            text("⚡ RD").style(text::success)
        } else {
            text("⚡ RD").style(text::danger)
        };

        container(
            row![
                text("RustMediaHub").size(24),
                horizontal_space(),
                library_btn,
                discover_btn,
                settings_btn,
                refresh_btn,
                rd_status,
            ]
            .spacing(10)
            .padding(10)
        )
        .width(Length::Fill)
        .style(container::dark)
        .into()
    }

    /// Status bar at bottom
    fn view_status_bar(&self) -> Element<'_, Message> {
        let status = if self.is_loading {
            text(format!("⏳ {}", self.status_message))
        } else {
            text(&self.status_message)
        };

        let error: Element<Message> = if let Some(ref err) = self.error_message {
            row![
                text(format!("❌ {}", err)).style(text::danger),
                button(text("✕")).on_press(Message::ClearError).padding(5),
            ]
            .spacing(10)
            .into()
        } else {
            horizontal_space().into()
        };

        container(
            row![status, horizontal_space(), error]
                .spacing(10)
                .padding(10)
        )
        .width(Length::Fill)
        .style(container::dark)
        .into()
    }

    /// Library view with media grid
    fn view_library(&self) -> Element<'_, Message> {
        if self.media_items.is_empty() {
            return self.view_empty_library();
        }

        // Simple grid layout using rows
        let items_per_row = 5;
        let mut rows: Vec<Element<Message>> = Vec::new();
        
        let media_chunks: Vec<_> = self.media_items.chunks(items_per_row).collect();
        
        for chunk in media_chunks {
            let row_items: Vec<Element<Message>> = chunk
                .iter()
                .map(|item| self.view_media_card(item))
                .collect();
            
            let row_widget = Row::with_children(row_items).spacing(15);
            rows.push(row_widget.into());
        }

        let grid = Column::with_children(rows).spacing(15);

        iced::widget::scrollable(
            container(grid)
                .width(Length::Fill)
                .padding(10)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Empty library view with add button
    fn view_empty_library(&self) -> Element<'_, Message> {
        container(
            column![
                text("No media found").size(32),
                text("Add a library folder to get started").size(16),
                button(text("Add Library Folder"))
                    .on_press(Message::PickFolder)
                    .padding(15),
            ]
            .spacing(20)
            .align_x(iced::Alignment::Center)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    /// Individual media card
    fn view_media_card(&self, item: &MediaItem) -> Element<'_, Message> {
        let is_selected = self.selected_item == Some(item.id);
        let item_id = item.id;
        let title_text = item.title.clone();
        let is_watched = item.watched;

        let title = text(title_text)
            .size(14)
            .width(Length::Fixed(150.0));

        let watched_indicator = if is_watched {
            text("✓").style(text::success)
        } else {
            text("")
        };

        let card_content = column![
            container(
                // Placeholder for poster - would use image widget with actual poster
                column![
                    text("🎬").size(48),
                ]
                .align_x(iced::Alignment::Center)
            )
            .width(Length::Fixed(150.0))
            .height(Length::Fixed(225.0))
            .center_x(Length::Fixed(150.0))
            .center_y(Length::Fixed(225.0))
            .style(if is_selected { container::bordered_box } else { container::dark }),
            
            row![title, watched_indicator].spacing(5),
        ]
        .spacing(5);

        button(card_content)
            .on_press(Message::PlayMedia(item_id))
            .padding(5)
            .into()
    }

    /// Discover view with search
    fn view_discover(&self) -> Element<'_, Message> {
        // Search bar
        let search_input = text_input("Search movies by title or IMDB ID (e.g., 'Matrix' or 'tt0133093')...", &self.discover_query)
            .on_input(Message::DiscoverQueryChanged)
            .on_submit(Message::DiscoverSearch)
            .padding(15)
            .size(18)
            .width(Length::FillPortion(4));

        let search_btn = button(
            if self.is_searching { 
                text("⏳ Searching...") 
            } else { 
                text("🔍 Search") 
            }
        )
        .on_press(Message::DiscoverSearch)
        .padding(15);

        let search_bar = row![search_input, search_btn]
            .spacing(10)
            .padding(10);

        // Help text
        let help_text = if self.tmdb.is_none() && self.scraper.is_none() {
            text("⚠ TMDB and Real-Debrid not configured. Add API keys to .env file.")
                .style(text::danger)
        } else if self.tmdb.is_none() {
            text("⚠ TMDB not configured. Add TMDB_API_KEY for smart title search.")
                .style(text::danger)
        } else if self.scraper.is_none() {
            text("⚠ Real-Debrid not configured. Add REAL_DEBRID_API_KEY for streaming.")
                .style(text::danger)
        } else {
            text("Search by movie title or IMDB ID. Smart search powered by TMDB + Real-Debrid cache.")
        };

        // Results - show movie search results or stream results based on state
        let results_view: Element<Message> = if self.selected_movie.is_some() && !self.discover_results.is_empty() {
            // Show stream results after movie selection
            self.view_stream_results()
        } else if !self.movie_search_results.is_empty() {
            // Show movie search results
            self.view_movie_search_results()
        } else if !self.discover_results.is_empty() {
            // Direct IMDB ID search results
            self.view_discover_results()
        } else if self.is_searching {
            container(text("Searching...").size(20))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        } else {
            container(
                column![
                    text("🎬 Discover Movies & Shows").size(32),
                    help_text,
                    text(""),
                    text("🔍 Smart Search: Type a movie title to find it").size(16),
                    text("• 'Matrix' → Find all Matrix movies"),
                    text("• 'Inception' → Christopher Nolan's dream heist"),
                    text("• 'Interstellar' → Space exploration epic"),
                    text(""),
                    text("Or use direct IMDB IDs:").size(14),
                    text("• tt0133093 - The Matrix"),
                    text("• tt0111161 - The Shawshank Redemption"),
                ]
                .spacing(10)
            )
            .center_x(Length::Fill)
            .padding(50)
            .into()
        };

        column![
            search_bar,
            results_view,
        ]
        .spacing(10)
        .into()
    }

    /// Movie search results (from TMDB)
    fn view_movie_search_results(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<Message>> = Vec::new();

        // Header
        items.push(
            text("Select a movie to search for streams:")
                .size(16)
                .into()
        );

        for (index, movie) in self.movie_search_results.iter().enumerate() {
            let year = movie.year.map(|y| y.to_string()).unwrap_or_else(|| "?".to_string());
            let rating = movie.vote_average.map(|v| format!("⭐ {:.1}", v)).unwrap_or_default();
            let imdb_status = if movie.imdb_id.is_some() {
                text("✓ IMDB").style(text::success)
            } else {
                text("✗ No IMDB").style(text::danger)
            };
            
            let overview = movie.overview.as_ref()
                .map(|o| if o.len() > 100 { format!("{}...", &o[..100]) } else { o.clone() })
                .unwrap_or_default();

            let row_content = column![
                row![
                    text(&movie.title).size(18),
                    text(format!("({})", year)).size(14),
                    text(rating).size(14),
                    imdb_status,
                ].spacing(10),
                text(overview).size(12),
            ]
            .spacing(5)
            .padding(10);

            let row_btn = button(row_content)
                .on_press(Message::SelectMovie(index))
                .width(Length::Fill)
                .padding(0);

            items.push(row_btn.into());
        }

        let results_list = Column::with_children(items).spacing(10);

        iced::widget::scrollable(
            container(results_list)
                .width(Length::Fill)
                .padding(10)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Stream results after movie selection
    fn view_stream_results(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<Message>> = Vec::new();

        // Back button and movie info
        if let Some(idx) = self.selected_movie {
            if let Some(movie) = self.movie_search_results.get(idx) {
                let back_btn = button(text("← Back to movies"))
                    .on_press(Message::BackToMovieSearch)
                    .padding(10);
                
                let year = movie.year.map(|y| y.to_string()).unwrap_or_else(|| "?".to_string());
                let movie_info = row![
                    back_btn,
                    text(format!("Streams for: {} ({})", movie.title, year)).size(18),
                ].spacing(20);

                items.push(movie_info.into());
            }
        }

        // Stream results header
        items.push(
            row![
                text("Status").width(Length::Fixed(80.0)),
                text("Quality").width(Length::Fixed(200.0)),
                text("Size").width(Length::Fixed(100.0)),
                text("Seeds").width(Length::Fixed(80.0)),
                text("Source").width(Length::Fixed(100.0)),
                text("Title").width(Length::Fill),
            ]
            .spacing(10)
            .padding(10)
            .into()
        );

        for (index, result) in self.discover_results.iter().enumerate() {
            let status = if result.rd_cached {
                text("⚡ RD").style(text::success)
            } else {
                text("🧲 P2P")
            };

            let quality_str = result.parsed.quality_string();
            let quality = text(quality_str).width(Length::Fixed(200.0));
            
            let size_str = result.size_str.clone().unwrap_or_else(|| "?".to_string());
            let size = text(size_str).width(Length::Fixed(100.0));
            
            let seeders_str = result.seeders.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string());
            let seeders = text(seeders_str).width(Length::Fixed(80.0));
            
            let source = text(result.source.clone()).width(Length::Fixed(100.0));
            let title = text(result.title.clone()).width(Length::Fill);

            let row_content = row![
                status.width(Length::Fixed(80.0)),
                quality,
                size,
                seeders,
                source,
                title,
            ]
            .spacing(10)
            .padding(10);

            let row_btn = button(row_content)
                .on_press(Message::PlayDiscoverResult(index))
                .width(Length::Fill)
                .padding(0);

            items.push(row_btn.into());
        }

        let results_list = Column::with_children(items).spacing(5);

        iced::widget::scrollable(
            container(results_list)
                .width(Length::Fill)
                .padding(10)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Discover results list
    fn view_discover_results(&self) -> Element<'_, Message> {
        let mut items: Vec<Element<Message>> = Vec::new();

        // Header
        items.push(
            row![
                text("Status").width(Length::Fixed(80.0)),
                text("Quality").width(Length::Fixed(200.0)),
                text("Size").width(Length::Fixed(100.0)),
                text("Seeds").width(Length::Fixed(80.0)),
                text("Source").width(Length::Fixed(100.0)),
                text("Title").width(Length::Fill),
            ]
            .spacing(10)
            .padding(10)
            .into()
        );

        for (index, result) in self.discover_results.iter().enumerate() {
            let _is_selected = self.selected_discover_result == Some(index);
            
            let status = if result.rd_cached {
                text("⚡ RD").style(text::success)
            } else {
                text("🧲 P2P")
            };

            let quality_str = result.parsed.quality_string();
            let quality = text(quality_str)
                .width(Length::Fixed(200.0));
            
            let size_str = result.size_str.clone().unwrap_or_else(|| "?".to_string());
            let size = text(size_str)
                .width(Length::Fixed(100.0));
            
            let seeders_str = result.seeders.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string());
            let seeders = text(seeders_str)
                .width(Length::Fixed(80.0));
            
            let source = text(result.source.clone())
                .width(Length::Fixed(100.0));
            
            let title = text(result.title.clone())
                .width(Length::Fill);

            let row_content = row![
                status.width(Length::Fixed(80.0)),
                quality,
                size,
                seeders,
                source,
                title,
            ]
            .spacing(10)
            .padding(10);

            let row_btn = button(row_content)
                .on_press(Message::PlayDiscoverResult(index))
                .width(Length::Fill)
                .padding(0);

            items.push(row_btn.into());
        }

        let results_list = Column::with_children(items).spacing(5);

        iced::widget::scrollable(
            container(results_list)
                .width(Length::Fill)
                .padding(10)
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Settings view
    fn view_settings(&self) -> Element<'_, Message> {
        let libraries_section = column![
            text("Libraries").size(20),
            self.view_libraries_list(),
        ]
        .spacing(10);

        let config_section = column![
            text("Configuration").size(20),
            
            column![
                text("TMDB API Key"),
                text_input("Enter TMDB API Key", &self.settings_view.tmdb_api_key_input)
                    .on_input(Message::UpdateTmdbApiKey)
                    .padding(10),
            ].spacing(5),
            
            column![
                text("Real-Debrid API Key"),
                text_input("Enter Real-Debrid API Key", &self.settings_view.real_debrid_api_key_input)
                    .on_input(Message::UpdateRealDebridApiKey)
                    .secure(true)
                    .padding(10),
            ].spacing(5),

            button("Save Settings")
                .on_press(Message::SaveSettings)
                .padding(10),
        ]
        .spacing(20);

        let mpv_status = text("✓ Player: Integrated (libmpv2)").style(text::success);

        let rd_status = if self.scraper.is_some() {
            text("✓ Real-Debrid: Connected").style(text::success)
        } else {
            text("✗ Real-Debrid: Not configured").style(text::danger)
        };

        column![
            libraries_section,
            config_section,
            text("Status").size(20),
            mpv_status,
            rd_status,
        ]
        .spacing(20)
        .padding(20)
        .into()
    }

    /// Libraries list in settings
    fn view_libraries_list(&self) -> Element<'_, Message> {
        if self.libraries.is_empty() {
            return column![
                text("No libraries configured"),
                button(text("Add Library"))
                    .on_press(Message::PickFolder)
                    .padding(10),
            ]
            .spacing(10)
            .into();
        }

        let mut items: Vec<Element<Message>> = Vec::new();

        for lib in &self.libraries {
            let lib_row = row![
                text(&lib.name).width(Length::FillPortion(1)),
                text(&lib.path).width(Length::FillPortion(2)),
                button(text("Scan"))
                    .on_press(Message::ScanLibrary(lib.id, lib.path.clone()))
                    .padding(5),
                button(text("Remove"))
                    .on_press(Message::RemoveLibrary(lib.id))
                    .padding(5),
            ]
            .spacing(10)
            .padding(5);

            items.push(lib_row.into());
        }

        items.push(
            button(text("+ Add Library"))
                .on_press(Message::PickFolder)
                .padding(10)
                .into()
        );

        Column::with_children(items)
            .spacing(5)
            .into()
    }
}

impl Default for RustMediaHub {
    fn default() -> Self {
        Self::new().0
    }
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}
