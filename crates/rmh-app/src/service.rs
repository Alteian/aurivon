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
use std::collections::HashMap;
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
    /// Video file inside the torrent (from the scraper)
    pub file_idx: Option<u32>,
    /// Resolution, HDR, codec, audio, source and size parsed from the name
    pub traits: crate::streams::StreamTraits,
    pub(crate) score: u32,
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
            file_idx: r.file_idx,
            traits: crate::streams::StreamTraits::parse(&r.title, r.size.or_else(|| r.size_str.as_deref().and_then(crate::streams::parse_size))),
            score: r.parsed.quality_score(),
        }
    }
}

/// Where a resolved stream can be played from
#[derive(Debug, Clone)]
pub struct ResolvedStream {
    pub url: String,
    pub title: String,
    /// Served by Real-Debrid (otherwise the local torrent server or a direct URL)
    pub debrid: bool,
    /// Torrent stream id on the local server, if any
    pub torrent_stream: Option<String>,
    /// The candidate that won
    pub hit_title: String,
}

pub(crate) struct Inner {
    pub(crate) rt: tokio::runtime::Runtime,
    pub(crate) db_path: PathBuf,
    pub(crate) db: OnceCell<Database>,
    pub(crate) config: RwLock<AppConfig>,
    pub(crate) scraper: RwLock<Option<Arc<ScraperAddon>>>,
    pub(crate) torrent: Arc<Mutex<Option<Arc<TorrentEngine>>>>,
    /// What "Play" is doing right now (checking Real-Debrid, finding peers, buffering)
    pub(crate) start_status: Arc<RwLock<Option<String>>>,
    pub(crate) trakt: RwLock<Option<rmh_trakt::TraktClient>>,
    pub(crate) http: reqwest::Client,
    /// Poster / still image cache
    pub(crate) image_dir: PathBuf,
    pub(crate) trakt_token_path: PathBuf,
    /// Cached TMDB JSON responses
    pub(crate) tmdb_cache: PathBuf,
    /// Downloaded subtitles
    pub(crate) subs_dir: PathBuf,
    /// Local watchlist + Simkl / AniList tokens live next to the config
    pub(crate) data_dir: PathBuf,
    /// Every title TMDB has shown us (typo-tolerant search suggestions)
    pub(crate) seen: RwLock<std::collections::HashMap<(rmh_metadata::Kind, i32), rmh_metadata::Title>>,
    pub(crate) opensubtitles: RwLock<Option<Arc<rmh_services::opensubtitles::OpenSubtitles>>>,
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
            dirs::cache_dir().unwrap_or(base.clone()).join("aurivon").join("images")
        } else {
            base.join("images")
        };
        let trakt_token_path = if db_path == Database::default_path().unwrap_or_default() {
            rmh_trakt::TraktToken::default_path().unwrap_or(base.join("trakt.json"))
        } else {
            base.join("trakt.json")
        };
        let default_db = db_path == Database::default_path().unwrap_or_default();
        let cache_base = image_dir.parent().map(Path::to_path_buf).unwrap_or(base.clone());
        let data_dir = if default_db {
            dirs::config_dir().map(|d| d.join("aurivon")).unwrap_or(base.clone())
        } else {
            base.clone()
        };
        let http = reqwest::Client::builder()
            .user_agent(concat!("Aurivon/", env!("CARGO_PKG_VERSION")))
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
                start_status: Arc::new(RwLock::new(None)),
                trakt: RwLock::new(None),
                http,
                tmdb_cache: cache_base.join("tmdb"),
                subs_dir: cache_base.join("subtitles"),
                data_dir,
                seen: RwLock::new(Default::default()),
                opensubtitles: RwLock::new(None),
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
        let os = cfg.opensubtitles_key().and_then(|key| {
            rmh_services::opensubtitles::OpenSubtitles::new(key, cfg.opensubtitles_username.clone(), cfg.opensubtitles_password.clone())
                .map(Arc::new)
                .ok()
        });
        if let Ok(mut o) = self.inner.opensubtitles.write() {
            *o = os;
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
            crate::streams::sort_best(&mut hits);
            Ok(hits)
        })
    }

    /// Turn a stream candidate into a URL mpv can play: Real-Debrid direct
    /// link when it is cached there, otherwise the local torrent server once
    /// the first few MB are buffered.
    pub fn resolve_stream(&self, hit: &StreamHit) -> impl Future<Output = ServiceResult<ResolvedStream>> + 'static {
        self.fast_start(vec![hit.clone()])
    }

    /// What the current start-up is doing, for a status line
    pub fn start_status(&self) -> Option<String> {
        self.inner.start_status.read().ok().and_then(|s| s.clone())
    }

    /// Check the given candidates against Real-Debrid's cache (each probe adds
    /// and removes the magnet). Returns info hash -> cached.
    pub fn probe_rd(&self, hits: &[StreamHit]) -> impl Future<Output = ServiceResult<HashMap<String, bool>>> + 'static {
        let items: Vec<(String, Option<u32>)> = hits
            .iter()
            .filter(|h| !h.magnet.starts_with("http"))
            .map(|h| (h.info_hash.clone(), h.file_idx))
            .collect();
        let scraper = self.inner.scraper.read().ok().and_then(|s| s.clone());
        self.run(async move {
            match scraper.filter(|s| s.has_real_debrid()) {
                Some(s) => Ok(s.probe_rd(&items).await),
                None => Ok(HashMap::new()),
            }
        })
    }

    /// Warm the torrent engine and fetch the metadata of the torrents "Play
    /// best" would try, while the user is still looking at the list
    pub fn prefetch(&self, hits: &[StreamHit]) {
        let magnets: Vec<String> = hits
            .iter()
            .filter(|h| !h.rd_cached && h.magnet.starts_with("magnet:"))
            .take(PREFETCH)
            .map(|h| h.magnet.clone())
            .collect();
        let torrent = self.inner.torrent.clone();
        self.inner.rt.spawn(async move {
            let engine = {
                let mut guard = torrent.lock().await;
                if guard.is_none() {
                    let mut engine = TorrentEngine::with_defaults();
                    if let Err(e) = engine.initialize().await {
                        tracing::warn!("Torrent engine failed: {e}");
                        return;
                    }
                    *guard = Some(Arc::new(engine));
                }
                guard.clone().unwrap()
            };
            engine.prefetch(&magnets).await;
        });
    }

    /// Stop every torrent stream and delete its data
    pub fn stop_torrents(&self) -> impl Future<Output = ServiceResult<()>> + 'static {
        let torrent = self.inner.torrent.clone();
        self.run(async move {
            let engine = torrent.lock().await.clone();
            if let Some(e) = engine {
                e.remove_all_except(None).await;
            }
            Ok(())
        })
    }

    /// Start the first playable stream out of `candidates` (best first):
    ///
    /// 1. ask Real-Debrid about the top few and use the best cached one;
    /// 2. otherwise race the top torrents: each one fetches metadata from
    ///    its trackers, then buffers the first 4 MiB and last 2 MiB of the
    ///    video; the first to finish plays, the others are dropped.
    pub fn fast_start(&self, candidates: Vec<StreamHit>) -> impl Future<Output = ServiceResult<ResolvedStream>> + 'static {
        let scraper = self.inner.scraper.read().ok().and_then(|s| s.clone());
        let torrent = self.inner.torrent.clone();
        let status = self.inner.start_status.clone();
        self.run(async move {
            let set = |s: Option<String>| {
                if let Ok(mut g) = status.write() {
                    *g = s;
                }
            };
            let r = fast_start_inner(candidates, scraper, torrent, &set).await;
            set(None);
            r
        })
    }
}

/// Real-Debrid candidates checked before falling back to P2P
const RD_PROBES: usize = 10;
/// Torrents raced against each other
const P2P_RACE: usize = 4;
/// Torrents whose metadata is fetched while the picker is open
const PREFETCH: usize = 3;
const HEAD_BYTES: u64 = 4 * 1024 * 1024;
const TAIL_BYTES: u64 = 2 * 1024 * 1024;

async fn fast_start_inner(
    candidates: Vec<StreamHit>,
    scraper: Option<Arc<ScraperAddon>>,
    torrent: Arc<Mutex<Option<Arc<TorrentEngine>>>>,
    set: &(dyn Fn(Option<String>) + Sync),
) -> ServiceResult<ResolvedStream> {
    let started = std::time::Instant::now();
    let Some(first) = candidates.first() else { return Err("No stream to play".into()) };
    if first.magnet.starts_with("http") {
        return Ok(ResolvedStream { url: first.magnet.clone(), title: first.title.clone(), debrid: false, torrent_stream: None, hit_title: first.title.clone() });
    }

    // 1. Real-Debrid
    if let Some(scraper) = scraper.filter(|s| s.has_real_debrid()) {
        set(Some("Checking Real-Debrid cache…".into()));
        let probe: Vec<(String, Option<u32>)> = candidates.iter().take(RD_PROBES).map(|h| (h.info_hash.clone(), h.file_idx)).collect();
        let cached = scraper.probe_rd(&probe).await;
        for hit in candidates.iter().filter(|h| cached.get(&h.info_hash).copied().unwrap_or(false)) {
            set(Some("Opening Real-Debrid link…".into()));
            let mut r = ScrapedResult::from_hash(&hit.title, &hit.info_hash, &hit.source);
            r.rd_cached = true;
            r.file_idx = hit.file_idx;
            match scraper.resolve_stream(&r).await {
                Ok(url) if url.starts_with("http") => {
                    info!("Fast start: Real-Debrid link for {} in {:.1}s", hit.title, started.elapsed().as_secs_f64());
                    if let Some(engine) = torrent.lock().await.clone() {
                        engine.remove_all_except(None).await;
                    }
                    return Ok(ResolvedStream { url, title: hit.title.clone(), debrid: true, torrent_stream: None, hit_title: hit.title.clone() });
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("Real-Debrid resolve failed, trying the next one: {e}"),
            }
        }
    }

    // 2. P2P race
    set(Some("Starting torrent engine…".into()));
    let engine = {
        let mut guard = torrent.lock().await;
        if guard.is_none() {
            let mut engine = TorrentEngine::with_defaults();
            engine.initialize().await.map_err(|e| format!("Torrent engine failed: {e}"))?;
            *guard = Some(Arc::new(engine));
        }
        guard.clone().unwrap()
    };
    engine.stop_streams().await;

    let racers: Vec<StreamHit> = candidates.into_iter().take(P2P_RACE).collect();
    let n = racers.len();
    let progress: Arc<RwLock<Vec<Racer>>> = Arc::new(RwLock::new(vec![Racer::default(); n]));
    let race = futures::future::select_ok(racers.into_iter().enumerate().map(|(i, hit)| {
        let engine = engine.clone();
        let progress = progress.clone();
        Box::pin(async move {
            let stream = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                engine.add_magnet(&hit.magnet, hit.file_idx.map(|i| i as usize)),
            )
            .await
            .map_err(|_| format!("{}: no metadata after 90 s", hit.title))?
            .map_err(|e| format!("{}: {e}", hit.title))?;
            let id = stream.info.id.clone();
            info!("Fast start: metadata for {} after {:.1}s", hit.title, started.elapsed().as_secs_f64());
            let p = progress.clone();
            let buffered = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                engine.prebuffer(&id, HEAD_BYTES, TAIL_BYTES, move |b| {
                    if let Ok(mut v) = p.write() {
                        v[i] = Racer { buffer: Some(b) };
                    }
                }),
            )
            .await;
            match buffered {
                Ok(Ok(())) => Ok((hit, stream)),
                Ok(Err(e)) => {
                    engine.remove_stream(&id).await;
                    Err(format!("{}: {e}", hit.title))
                }
                Err(_) => {
                    engine.remove_stream(&id).await;
                    Err(format!("{}: too slow to buffer", hit.title))
                }
            }
        })
    }));
    tokio::pin!(race);
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
    let won = loop {
        tokio::select! {
            r = &mut race => break r,
            _ = tick.tick() => {
                let v = progress.read().map(|v| v.clone()).unwrap_or_default();
                set(Some(race_status(&v, started.elapsed().as_secs())));
            }
        }
    };
    match won {
        Ok(((hit, stream), _rest)) => {
            // Losers are dropped mid-flight: clean their torrents up
            engine.remove_all_except(Some(&stream.info.id)).await;
            info!("Fast start: torrent {} buffered in {:.1}s", hit.title, started.elapsed().as_secs_f64());
            Ok(ResolvedStream {
                url: stream.info.stream_url.clone(),
                title: stream.info.filename.clone(),
                debrid: false,
                torrent_stream: Some(stream.info.id.clone()),
                hit_title: hit.title,
            })
        }
        Err(e) => {
            engine.remove_all_except(None).await;
            Err(format!("Could not start any stream ({e})"))
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Racer {
    buffer: Option<rmh_torrent::BufferProgress>,
}

/// One status line for the torrents racing to start
fn race_status(racers: &[Racer], secs: u64) -> String {
    let best = racers
        .iter()
        .filter_map(|r| r.buffer)
        .max_by_key(|b| (b.buffered * 1000).checked_div(b.target).unwrap_or(1000));
    match best {
        Some(b) => {
            let pct = (b.buffered * 100).checked_div(b.target).unwrap_or(100).min(100);
            format!(
                "Buffering {pct}% · {:.1} MB/s · {} peers{}",
                b.download_speed as f64 / 1_000_000.0,
                b.peers,
                if b.seen > b.peers { format!(" ({} found)", b.seen) } else { String::new() }
            )
        }
        None => {
            let n = racers.len();
            format!("Finding peers… {secs} s · trying {n} torrent{}", if n == 1 { "" } else { "s" })
        }
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
