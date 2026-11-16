//! Async application services backed by a private Tokio runtime.
//!
//! Every method returns a `'static` future that can be awaited from *any*
//! executor (GPUI's, iced's, or a plain `block_on`): the actual work runs on
//! the service's own Tokio runtime and the returned future just waits for it.

use crate::config::AppConfig;
use rmh_core::{Library, MediaItem, MediaType};
use rmh_database::{Database, MediaRepository};
use rmh_metadata::{MovieSearchResult, PosterSize, TmdbClient};
use rmh_scanner::Scanner;
use rmh_scraper::{ScrapedResult, ScraperAddon, ScraperAddonConfig, SearchQuery};
use rmh_torrent::TorrentEngine;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::{Mutex, OnceCell};
use tracing::info;
use uuid::Uuid;

pub type ServiceResult<T> = Result<T, String>;

/// A movie found via TMDB
#[derive(Debug, Clone)]
pub struct MovieHit {
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub title: String,
    pub year: Option<u16>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub rating: Option<f32>,
}

impl From<&MovieSearchResult> for MovieHit {
    fn from(r: &MovieSearchResult) -> Self {
        Self {
            tmdb_id: r.tmdb_id,
            imdb_id: r.imdb_id.clone(),
            title: r.title.clone(),
            year: r.year,
            overview: r.overview.clone(),
            poster_url: r.poster_url(PosterSize::W342),
            rating: r.vote_average,
        }
    }
}

/// A playable stream candidate (torrent / debrid)
#[derive(Debug, Clone)]
pub struct StreamHit {
    pub title: String,
    pub quality: String,
    pub size: Option<String>,
    pub seeders: Option<u32>,
    pub source: String,
    pub rd_cached: bool,
    pub magnet: String,
    pub info_hash: String,
    score: u32,
}

impl From<&ScrapedResult> for StreamHit {
    fn from(r: &ScrapedResult) -> Self {
        Self {
            title: r.title.clone(),
            quality: r.parsed.quality_string(),
            size: r.size_str.clone(),
            seeders: r.seeders,
            source: r.source.clone(),
            rd_cached: r.rd_cached,
            magnet: r.magnet.clone(),
            info_hash: r.info_hash.clone(),
            score: r.parsed.quality_score(),
        }
    }
}

/// Where a resolved stream can be played from
#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub url: String,
    pub title: String,
}

pub(crate) struct Inner {
    pub(crate) rt: tokio::runtime::Runtime,
    pub(crate) db_path: PathBuf,
    pub(crate) db: OnceCell<Database>,
    pub(crate) config: RwLock<AppConfig>,
    pub(crate) scraper: RwLock<Option<Arc<ScraperAddon>>>,
    pub(crate) torrent: Arc<Mutex<Option<TorrentEngine>>>,
    pub(crate) trakt: RwLock<Option<rmh_trakt::TraktClient>>,
    pub(crate) http: reqwest::Client,
    /// Poster / still image cache
    pub(crate) image_dir: PathBuf,
    pub(crate) trakt_token_path: PathBuf,
}

/// Cloneable handle to the application services
#[derive(Clone)]
pub struct MediaService {
    pub(crate) inner: Arc<Inner>,
}

impl MediaService {
    /// Service using the default database location and saved config
    pub fn new() -> anyhow::Result<Self> {
        let config = AppConfig::load().unwrap_or_else(|e| {
            tracing::warn!("Failed to load config: {e}, using defaults");
            AppConfig::default()
        });
        Self::with_paths(Database::default_path()?, config)
    }

    /// Service with an explicit database path (tests, portable installs)
    pub fn with_paths(db_path: PathBuf, config: AppConfig) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rmh-service")
            .enable_all()
            .build()?;
        let base = db_path.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        let image_dir = if db_path == Database::default_path().unwrap_or_default() {
            dirs::cache_dir().unwrap_or(base.clone()).join("rustmediahub").join("images")
        } else {
            base.join("images")
        };
        let trakt_token_path = if db_path == Database::default_path().unwrap_or_default() {
            rmh_trakt::TraktToken::default_path().unwrap_or(base.join("trakt.json"))
        } else {
            base.join("trakt.json")
        };
        let http = reqwest::Client::builder()
            .user_agent(concat!("RustMediaHub/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        let svc = Self {
            inner: Arc::new(Inner {
                rt,
                db_path,
                db: OnceCell::new(),
                config: RwLock::new(config),
                scraper: RwLock::new(None),
                torrent: Arc::new(Mutex::new(None)),
                trakt: RwLock::new(None),
                http,
                image_dir,
                trakt_token_path,
            }),
        };
        svc.rebuild_scraper();
        Ok(svc)
    }

    /// Run a future on the service runtime and await it from any executor
    pub fn run<T, F>(&self, fut: F) -> impl Future<Output = ServiceResult<T>> + 'static
    where
        T: Send + 'static,
        F: Future<Output = ServiceResult<T>> + Send + 'static,
    {
        let handle = self.inner.rt.spawn(fut);
        async move { handle.await.map_err(|e| format!("background task failed: {e}"))? }
    }

    pub(crate) fn db(&self) -> impl Future<Output = ServiceResult<Database>> + Send + 'static {
        let inner = self.inner.clone();
        async move {
            inner
                .db
                .get_or_try_init(|| async { Database::new(&inner.db_path).await })
                .await
                .cloned()
                .map_err(|e| format!("Database error: {e:#}"))
        }
    }

    // ---------------------------------------------------------------- config

    pub fn config(&self) -> AppConfig {
        self.inner.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Persist config and re-create clients that depend on API keys
    pub fn save_config(&self, config: AppConfig) -> ServiceResult<()> {
        config.save().map_err(|e| format!("{e:#}"))?;
        if let Ok(mut c) = self.inner.config.write() {
            *c = config;
        }
        self.rebuild_scraper();
        Ok(())
    }

    fn rebuild_scraper(&self) {
        let cfg = self.config();
        let mut sc = ScraperAddonConfig::default();
        if let Some(key) = cfg.real_debrid_api_key.clone().filter(|k| !k.trim().is_empty()) {
            sc = sc.with_real_debrid(key);
        }
        if let Ok(mut s) = self.inner.scraper.write() {
            *s = Some(Arc::new(ScraperAddon::new(sc)));
        }
        let trakt = cfg.trakt_credentials().and_then(|(id, secret)| {
            rmh_trakt::TraktClient::new(id, secret, self.inner.trakt_token_path.clone())
                .map_err(|e| tracing::warn!("Trakt client: {e:#}"))
                .ok()
        });
        if let Ok(mut t) = self.inner.trakt.write() {
            *t = trakt;
        }
    }

    pub fn has_tmdb(&self) -> bool {
        self.config().tmdb_api_key.is_some_and(|k| !k.trim().is_empty())
    }

    pub fn has_real_debrid(&self) -> bool {
        self.config().real_debrid_api_key.is_some_and(|k| !k.trim().is_empty())
    }

    // --------------------------------------------------------------- library

    pub fn libraries(&self) -> impl Future<Output = ServiceResult<Vec<Library>>> + 'static {
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            MediaRepository::new(db.pool()).get_all_libraries().await.map_err(|e| format!("{e:#}"))
        })
    }

    pub fn media(&self) -> impl Future<Output = ServiceResult<Vec<MediaItem>>> + 'static {
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            MediaRepository::new(db.pool()).get_all_media().await.map_err(|e| format!("{e:#}"))
        })
    }

    /// Add a folder as a library and scan it. Returns the library and number of items found.
    pub fn add_library(&self, path: impl Into<String>) -> impl Future<Output = ServiceResult<(Library, usize)>> + 'static {
        let raw: String = path.into();
        let db = self.db();
        self.run(async move {
            let path = expand_home(raw.trim());
            if !Path::new(&path).is_dir() {
                return Err(format!("Not a folder: {path}"));
            }
            let path = std::fs::canonicalize(&path).map(|p| p.to_string_lossy().to_string()).unwrap_or(path);
            let db = db.await?;
            let repo = MediaRepository::new(db.pool());
            let existing = repo.get_all_libraries().await.map_err(|e| format!("{e:#}"))?;
            if existing.iter().any(|l| l.path == path) {
                return Err(format!("Library already added: {path}"));
            }
            let name = Path::new(&path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Library")
                .to_string();
            let library = Library::new(name, path.clone(), MediaType::Movie);
            repo.insert_library(&library).await.map_err(|e| format!("{e:#}"))?;
            let added = Scanner::new(db.clone())
                .scan_directory(library.id, Path::new(&path), None)
                .await
                .map_err(|e| format!("{e:#}"))?;
            info!("Added library {} with {} items", path, added.len());
            Ok((library, added.len()))
        })
    }

    pub fn remove_library(&self, id: Uuid) -> impl Future<Output = ServiceResult<()>> + 'static {
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            MediaRepository::new(db.pool()).delete_library(id).await.map_err(|e| format!("{e:#}"))
        })
    }

    /// Rescan a library; returns number of new items
    pub fn scan_library(&self, library: &Library) -> impl Future<Output = ServiceResult<usize>> + 'static {
        let (id, path) = (library.id, library.path.clone());
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            Scanner::new(db)
                .scan_directory(id, Path::new(&path), None)
                .await
                .map(|items| items.len())
                .map_err(|e| format!("{e:#}"))
        })
    }

    pub fn save_progress(&self, update: crate::ProgressUpdate) -> impl Future<Output = ServiceResult<()>> + 'static {
        let db = self.db();
        self.run(async move {
            let db = db.await?;
            let repo = MediaRepository::new(db.pool());
            repo.update_watch_progress(update.media_id, update.position).await.map_err(|e| format!("{e:#}"))?;
            if update.watched {
                repo.update_watched_status(update.media_id, true).await.map_err(|e| format!("{e:#}"))?;
            }
            Ok(())
        })
    }

    // -------------------------------------------------------------- discover

    pub fn search_movies(&self, query: impl Into<String>) -> impl Future<Output = ServiceResult<Vec<MovieHit>>> + 'static {
        let query: String = query.into();
        let key = self.config().tmdb_api_key.filter(|k| !k.trim().is_empty());
        self.run(async move {
            let key = key.ok_or("TMDB API key not set. Add it in Settings.")?;
            let tmdb = TmdbClient::new(key).map_err(|e| format!("{e:#}"))?;
            let results = tmdb.search_movie_with_imdb(&query).await.map_err(|e| format!("TMDB search failed: {e:#}"))?;
            Ok(results.iter().map(MovieHit::from).collect())
        })
    }

    /// Find stream candidates for an IMDb id, best first
    pub fn search_streams(&self, imdb_id: impl Into<String>) -> impl Future<Output = ServiceResult<Vec<StreamHit>>> + 'static {
        self.search_streams_for(imdb_id, None)
    }

    /// Stream candidates for a movie (`episode` None) or a series episode (season, episode)
    pub fn search_streams_for(
        &self,
        imdb_id: impl Into<String>,
        episode: Option<(u32, u32)>,
    ) -> impl Future<Output = ServiceResult<Vec<StreamHit>>> + 'static {
        let imdb_id: String = imdb_id.into();
        let scraper = self.inner.scraper.read().ok().and_then(|s| s.clone());
        self.run(async move {
            let scraper = scraper.ok_or("Scraper not initialised")?;
            let query = match episode {
                Some((s, e)) => SearchQuery::series(&imdb_id, "", s, e),
                None => SearchQuery::movie(&imdb_id, ""),
            };
            let results = scraper
                .search(&query)
                .await
                .map_err(|e| format!("Stream search failed: {e}"))?;
            let mut hits: Vec<StreamHit> = results.iter().map(StreamHit::from).collect();
            hits.sort_by(|a, b| {
                b.rd_cached
                    .cmp(&a.rd_cached)
                    .then(b.score.cmp(&a.score))
                    .then(b.seeders.unwrap_or(0).cmp(&a.seeders.unwrap_or(0)))
            });
            Ok(hits)
        })
    }

    /// Turn a stream candidate into a URL mpv can play: Real-Debrid direct link
    /// when cached, otherwise the local torrent streaming server.
    pub fn resolve_stream(&self, hit: &StreamHit) -> impl Future<Output = ServiceResult<ResolvedStream>> + 'static {
        let hit = hit.clone();
        let scraper = self.inner.scraper.read().ok().and_then(|s| s.clone());
        let torrent = self.inner.torrent.clone();
        self.run(async move {
            if hit.magnet.starts_with("http") {
                return Ok(ResolvedStream { url: hit.magnet.clone(), title: hit.title.clone() });
            }
            if hit.rd_cached {
                if let Some(scraper) = scraper.filter(|s| s.has_real_debrid()) {
                    let mut r = ScrapedResult::from_hash(&hit.title, &hit.info_hash, &hit.source);
                    r.rd_cached = true;
                    r.magnet = hit.magnet.clone();
                    match scraper.resolve_stream(&r).await {
                        Ok(url) if url.starts_with("http") => return Ok(ResolvedStream { url, title: hit.title.clone() }),
                        Ok(_) => {}
                        Err(e) => tracing::warn!("Real-Debrid resolve failed, falling back to torrent: {e}"),
                    }
                }
            }
            let mut guard = torrent.lock().await;
            if guard.is_none() {
                let mut engine = TorrentEngine::with_defaults();
                engine.initialize().await.map_err(|e| format!("Torrent engine failed: {e}"))?;
                *guard = Some(engine);
            }
            let engine = guard.as_ref().unwrap();
            let stream = engine.add_magnet(&hit.magnet, None).await.map_err(|e| format!("Torrent failed: {e}"))?;
            Ok(ResolvedStream { url: stream.info.stream_url.clone(), title: stream.info.filename.clone() })
        })
    }
}

/// Expand a leading `~` to the home directory
pub fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), rest);
        }
    }
    path.to_string()
}

/// Is this media item an audio file?
pub fn is_audio(item: &MediaItem) -> bool {
    Path::new(&item.file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(rmh_core::is_audio_file)
        .unwrap_or(false)
}
