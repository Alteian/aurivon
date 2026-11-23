//! Main window state and actions. Rendering lives in `ui`, `home`, `player`,
//! `details` and `settings`.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;
use rmh_app::{
    is_audio, next_episode, resume_position, Details, Episode, HomeLayout, Library, LibraryEntry, MediaService,
    NowPlaying, PlaybackSnapshot, Player, PlayerConfig, ProgressUpdate, StreamHit, TickOutcome, TitleCard, TitleKind,
    TraktItem, TraktLists, TraktStatus, VideoMode,
};
use uuid::Uuid;

use crate::embed::VideoHost;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Library,
    Watchlist,
    Discover,
    Calendar,
    Player,
    Details,
    Person,
    Addons,
    Settings,
}

/// Which Settings category panel is open (category cards at the top).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Libraries,
    Player,
    Integrations,
    Streams,
    Storage,
    Updates,
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 8] = [
        SettingsTab::Appearance,
        SettingsTab::Libraries,
        SettingsTab::Player,
        SettingsTab::Integrations,
        SettingsTab::Streams,
        SettingsTab::Storage,
        SettingsTab::Updates,
        SettingsTab::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Libraries => "Libraries",
            SettingsTab::Player => "Player",
            SettingsTab::Integrations => "Integrations",
            SettingsTab::Streams => "Streams",
            SettingsTab::Storage => "Storage",
            SettingsTab::Updates => "Updates",
            SettingsTab::About => "About",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Theme, layout, cards, navigation",
            SettingsTab::Libraries => "Local folders to scan",
            SettingsTab::Player => "mpv, autoplay, skip intro",
            SettingsTab::Integrations => "TMDB, debrid, Simkl…",
            SettingsTab::Streams => "Quality filters and sort",
            SettingsTab::Storage => "Caches and disk caps",
            SettingsTab::Updates => "Auto-update preferences",
            SettingsTab::About => "Status and keyboard shortcuts",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "◐",
            SettingsTab::Libraries => "▤",
            SettingsTab::Player => "▶",
            SettingsTab::Integrations => "⛓",
            SettingsTab::Streams => "⚡",
            SettingsTab::Storage => "💾",
            SettingsTab::Updates => "↻",
            SettingsTab::About => "ℹ",
        }
    }
}

/// Sidebar entries: pages plus Search and Movies / Series (Browse pre-set)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Page(Page),
    Search,
    Movies,
    Series,
}

impl Nav {
    /// Streaming first, the local library last (Ctrl+1… follows this order, Search excluded)
    pub const ALL: [Nav; 10] = [
        Nav::Page(Page::Home),
        Nav::Search,
        Nav::Movies,
        Nav::Series,
        Nav::Page(Page::Calendar),
        Nav::Page(Page::Watchlist),
        Nav::Page(Page::Library),
        Nav::Page(Page::Player),
        Nav::Page(Page::Addons),
        Nav::Page(Page::Settings),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Nav::Page(p) => p.label(),
            Nav::Search => "Search",
            Nav::Movies => "Movies",
            Nav::Series => "Series",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Nav::Page(p) => p.glyph(),
            Nav::Search => "⌕",
            Nav::Movies => "◎",
            Nav::Series => "▣",
        }
    }
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Library => "Library",
            Page::Watchlist => "Lists",
            Page::Discover => "Browse",
            Page::Calendar => "Calendar",
            Page::Player => "Now Playing",
            Page::Details => "Details",
            Page::Person => "Person",
            Page::Addons => "Addons",
            Page::Settings => "Settings",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Page::Home => "⌂",
            Page::Library => "▤",
            Page::Watchlist => "★",
            Page::Discover => "◎",
            Page::Calendar => "▦",
            Page::Player => "▶",
            Page::Details => "ℹ",
            Page::Person => "☺",
            Page::Addons => "⧉",
            Page::Settings => "⚙",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Page::Home => "What to watch tonight",
            Page::Library => "On this device — files in your folders",
            Page::Watchlist => "Streaming shelf — watchlist and named Trakt lists",
            Page::Discover => "Trending, genres, filters and streaming services",
            Page::Calendar => "New episodes, premieres, cinema and digital releases",
            Page::Player => "",
            Page::Details => "",
            Page::Person => "",
            Page::Addons => "Trakt account, official catalogs and HTTP addons",
            Page::Settings => "Appearance, libraries, player and integrations",
        }
    }
}

/// Right-hand panel on the player page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideTab {
    Chapters,
    Audio,
    Subtitles,
    Speed,
}

/// What the details page shows
#[derive(Clone)]
pub struct DetailsState {
    pub kind: Option<TitleKind>,
    pub tmdb_id: Option<i32>,
    /// Local library item (movie) this page was opened from
    pub local: Option<Uuid>,
    pub name: String,
    pub poster: Option<PathBuf>,
    pub details: Option<Details>,
    pub backdrop: Option<PathBuf>,
    pub loading: bool,
    pub season: Option<u32>,
    pub episodes: Vec<Episode>,
    /// Trakt watched progress for series (next episode)
    pub trakt_progress: Option<rmh_app::ShowProgress>,
    pub related: Vec<TitleCard>,
}

/// Where a stream list came from (for the Trakt item / title)
#[derive(Clone)]
pub struct StreamContext {
    pub label: String,
    pub trakt: Option<TraktItem>,
    /// For next-episode presearch
    pub imdb: Option<String>,
    pub episode: Option<(u32, u32)>,
    pub tvdb_id: Option<i32>,
}

pub struct MediaHubView {
    pub(crate) svc: MediaService,
    pub(crate) player: Player,
    pub(crate) page: Page,
    pub(crate) back_page: Page,
    pub(crate) settings_tab: SettingsTab,
    pub(crate) libraries: Vec<Library>,
    pub(crate) entries: Vec<LibraryEntry>,
    pub(crate) busy: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) matching: bool,
    pub(crate) match_attempted: bool,
    /// Libraries are rescanned once per launch so new files show up
    startup_scanned: bool,

    pub(crate) path_input: Entity<InputState>,
    pub(crate) filter_input: Entity<InputState>,
    pub(crate) discover_input: Entity<InputState>,
    pub(crate) tmdb_input: Entity<InputState>,
    pub(crate) rd_input: Entity<InputState>,
    pub(crate) mpv_input: Entity<InputState>,
    pub(crate) trakt_id_input: Entity<InputState>,
    pub(crate) trakt_code_input: Entity<InputState>,
    pub(crate) alang_input: Entity<InputState>,
    pub(crate) slang_input: Entity<InputState>,

    // discover / details
    pub(crate) results: Vec<TitleCard>,
    pub(crate) trending: Vec<TitleCard>,
    pub(crate) hero_backdrop: Option<PathBuf>,
    /// Streaming home: hero carousel + lazy discovery rows
    pub(crate) feed: crate::feed::HomeFeed,
    /// "Play best" requested from the hero: start once streams are found
    pub(crate) pending_play: Option<(TitleKind, i32)>,
    pub(crate) autoplay_ready: bool,
    /// gpui-kit components currently in light mode
    pub(crate) kit_light: bool,
    pub(crate) searching: bool,
    pub(crate) details: Option<DetailsState>,
    pub(crate) streams: Vec<StreamHit>,
    /// A stream that is being started: (what, live status line)
    pub(crate) starting: Option<(String, Option<String>)>,
    pub(crate) stream_ctx: Option<StreamContext>,
    pub(crate) streams_loading: bool,
    /// Soft-fail notes from the last stream search (Torrentio / addon)
    pub(crate) stream_warnings: Vec<String>,
    /// When set, next title pick from search links this library file to TMDB
    pub(crate) linking_media: Option<Uuid>,

    // trakt
    pub(crate) trakt: TraktStatus,
    /// True while browser PKCE login is waiting for the callback
    pub(crate) trakt_waiting: bool,
    pub(crate) trakt_lists: TraktLists,
    pub(crate) trakt_cards: Vec<TitleCard>,
    pub(crate) trakt_history_cards: Vec<TitleCard>,
    pub(crate) trakt_rec_cards: Vec<TitleCard>,
    /// Quiet background sync in flight
    trakt_syncing: bool,
    /// Last successful background / full sync start
    trakt_sync_at: Option<Instant>,

    // playback
    pub(crate) now: Option<NowPlaying>,
    pub(crate) snap: PlaybackSnapshot,
    pub(crate) now_entry: Option<LibraryEntry>,
    pub(crate) now_source: Option<String>,
    pub(crate) eof_handled: bool,
    pub(crate) autoplay: Option<(Instant, Uuid)>,
    pub(crate) hover_seek: Option<f64>,
    pub(crate) thumb: Option<(u64, PathBuf)>,
    pub(crate) thumb_pending: bool,
    pub(crate) seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub(crate) volume_slider: Entity<SliderState>,
    pub(crate) volume: f64,
    pub(crate) side_tab: SideTab,
    pub(crate) theater: bool,
    /// F11: the whole app fullscreen (theater also goes fullscreen on its own)
    pub(crate) app_fullscreen: bool,
    pub(crate) lazy: crate::lazy::LazyImages,
    /// Stream picker filter (starts from the default saved in Settings)
    pub(crate) stream_filter: rmh_app::StreamFilter,
    pub(crate) stream_filters_open: bool,
    /// Image cache size, refreshed when Settings opens
    pub(crate) image_cache: Option<rmh_app::CacheUsage>,
    /// Full storage report for Settings → Storage
    pub(crate) storage_report: Option<rmh_app::StorageReport>,
    /// Skip-intro segments from TheIntroDB for the current playback
    pub(crate) intro_segments: Option<rmh_app::IntroSegments>,
    /// Prefetched streams for the next episode
    pub(crate) next_ep_streams: Vec<StreamHit>,
    pub(crate) video: Option<Rc<VideoHost>>,
    pub(crate) video_checked: bool,
    pub(crate) mpv_version: Option<String>,

    pub(crate) disc: crate::disc::Disc,
    pub(crate) upd: crate::updates::UpdateUi,

    pub(crate) focus_handle: FocusHandle,
    /// Couch mode: focused library entry for arrow-key navigation
    pub(crate) couch_focus: Option<Uuid>,
    _poller: Task<()>,
    _subscriptions: Vec<Subscription>,
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

impl MediaHubView {
    pub fn new(svc: MediaService, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = svc.config();
        let mpv_path = config.mpv_path();
        let volume = config.volume.unwrap_or(100.0).clamp(0.0, 130.0);
        let player = Player::new(Self::player_config(&svc, None, volume));

        let input = |cx: &mut Context<Self>, window: &mut Window, ph: &str, val: String, masked: bool| {
            let ph = ph.to_string();
            cx.new(move |cx| InputState::new(window, cx).placeholder(ph).masked(masked).default_value(val))
        };
        let path_input = input(cx, window, "Folder path, e.g. ~/Videos", String::new(), false);
        let filter_input = input(cx, window, "Filter library… (Enter plays the top match)", String::new(), false);
        let discover_input = input(cx, window, "Search movies & shows, or paste an IMDb id (tt0133093)", String::new(), false);
        let tmdb_input = input(cx, window, "TMDB API key or read access token", opt(&config.tmdb_api_key), true);
        let rd_input = input(cx, window, "Real-Debrid API key (optional)", opt(&config.real_debrid_api_key), true);
        let mpv_input = input(cx, window, "mpv", mpv_path.clone(), false);
        let trakt_id_input = input(cx, window, "Trakt client id", opt(&config.trakt_client_id), true);
        let trakt_code_input = input(cx, window, "Paste OAuth code if browser handoff fails", String::new(), false);
        let alang_input = input(cx, window, "e.g. cze,eng", opt(&config.audio_lang), false);
        let slang_input = input(cx, window, "e.g. eng,cze", opt(&config.sub_lang), false);

        let volume_slider = cx.new(|_| SliderState::new().min(0.0).max(130.0).step(1.0).default_value(volume as f32));

        let (disc, disc_subs) = crate::disc::Disc::new(&svc, window, cx);
        let mut subscriptions = vec![
            cx.subscribe_in(&path_input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.add_library_from_input(window, cx);
                }
            }),
            cx.subscribe_in(&discover_input, window, |this, _, ev: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.search_discover(cx);
                }
            }),
            cx.subscribe_in(&filter_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    if let Some(id) = this.filtered_entries(cx).first().map(|e| e.item.id) {
                        this.play_item(id, window, cx);
                    }
                }
                _ => {}
            }),
            cx.subscribe(&volume_slider, |this, _, ev: &SliderEvent, cx| {
                let v = match ev {
                    SliderEvent::Change(v) | SliderEvent::Release(v) => v.start() as f64,
                };
                this.set_volume(v, matches!(ev, SliderEvent::Release(_)), cx);
            }),
        ];

        subscriptions.extend(disc_subs);
        let poller = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            if this.update_in(cx, |this, window, cx| this.on_tick(window, cx)).is_err() {
                break;
            }
        });

        let focus_handle = cx.focus_handle().tab_stop(true);
        window.focus(&focus_handle, cx);

        let mut view = Self {
            mpv_version: Player::mpv_version(&mpv_path),
            svc,
            player,
            page: Page::Home,
            back_page: Page::Home,
            settings_tab: SettingsTab::default(),
            libraries: Vec::new(),
            entries: Vec::new(),
            busy: Some("Loading library…".into()),
            status: None,
            error: None,
            matching: false,
            match_attempted: false,
            startup_scanned: false,
            path_input,
            filter_input,
            discover_input,
            tmdb_input,
            rd_input,
            mpv_input,
            trakt_id_input,
            trakt_code_input,
            alang_input,
            slang_input,
            results: Vec::new(),
            trending: Vec::new(),
            hero_backdrop: None,
            feed: Default::default(),
            pending_play: None,
            autoplay_ready: false,
            kit_light: false,
            searching: false,
            details: None,
            streams: Vec::new(),
            stream_ctx: None,
            streams_loading: false,
            stream_warnings: Vec::new(),
            linking_media: None,
            trakt: TraktStatus::default(),
            trakt_waiting: false,
            trakt_lists: TraktLists::default(),
            trakt_cards: Vec::new(),
            trakt_history_cards: Vec::new(),
            trakt_rec_cards: Vec::new(),
            trakt_syncing: false,
            trakt_sync_at: None,
            now: None,
            snap: PlaybackSnapshot::default(),
            now_entry: None,
            now_source: None,
            eof_handled: false,
            autoplay: None,
            hover_seek: None,
            thumb: None,
            thumb_pending: false,
            seek_bounds: Rc::new(Cell::new(Bounds::default())),
            volume_slider,
            volume,
            side_tab: SideTab::Chapters,
            theater: false,
            app_fullscreen: false,
            lazy: Default::default(),
            stream_filter: config.stream_filter.clone(),
            stream_filters_open: false,
            starting: None,
            image_cache: None,
            storage_report: None,
            intro_segments: None,
            next_ep_streams: vec![],
            video: None,
            video_checked: false,
            disc,
            upd: Default::default(),
            focus_handle,
            couch_focus: None,
            _poller: poller,
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        // Trending feeds the hero (streaming + Discover home layouts)
        if view.svc.config().home_layout == HomeLayout::Discover || view.svc.config().home_layout.is_feed() {
            view.load_trending(cx);
        }
        view.prune_image_cache(cx);
        view.refresh_trakt(cx);
        view.refresh_social(cx);
        if view.disc.search.open {
            view.on_search_change(cx);
        }
        view.load_watchlist(cx);
        view.start_update_loop(cx);
        view
    }

    /// mpv settings derived from the app config
    pub(crate) fn player_config(svc: &MediaService, wid: Option<i64>, volume: f64) -> PlayerConfig {
        let cfg = svc.config();
        let mut extra = vec![];
        if let Some(a) = cfg.audio_lang.as_ref().filter(|v| !v.trim().is_empty()) {
            extra.push(format!("--alang={}", a.trim()));
        }
        if let Some(s) = cfg.sub_lang.as_ref().filter(|v| !v.trim().is_empty()) {
            extra.push(format!("--slang={}", s.trim()));
        }
        if cfg.sub_delay.abs() > 0.001 {
            extra.push(format!("--sub-delay={}", cfg.sub_delay));
        }
        if cfg.sub_forced_only {
            extra.push("--sub-forced-only=yes".into());
        }
        PlayerConfig {
            mpv_path: cfg.mpv_path(),
            hwdec: cfg.mpv_hwdec.clone(),
            volume: volume.min(100.0) as u8,
            osc: cfg.mpv_osc,
            fullscreen: wid.is_none() && cfg.video_mode == VideoMode::Window && cfg.window_fullscreen,
            wid,
            extra_args: extra,
            ..Default::default()
        }
    }

    /// Create the embedded video surface once the window exists
    pub(crate) fn ensure_video_host(&mut self, window: &mut Window) {
        if self.video_checked {
            return;
        }
        self.video_checked = true;
        if self.svc.config().video_mode != VideoMode::Embedded {
            return;
        }
        match VideoHost::new(window) {
            Some(host) => {
                let wid = host.xid();
                self.video = Some(Rc::new(host));
                self.player.set_config(Self::player_config(&self.svc, Some(wid), self.volume));
            }
            None => {
                self.status = Some("Embedded video unavailable here; mpv opens its own window".into());
            }
        }
    }

    pub(crate) fn embedded(&self) -> bool {
        self.video.is_some()
    }

    // ------------------------------------------------------------------ state

    pub(crate) fn notify_error(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.error = Some(msg.into());
        self.busy = None;
        cx.notify();
    }

    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let libs = self.svc.libraries();
        let entries = self.svc.library_entries();
        cx.spawn(async move |this, cx| {
            let libs = libs.await;
            let entries = entries.await;
            this.update(cx, |this, cx| {
                if !this.matching {
                    this.busy = None;
                }
                match (libs, entries) {
                    (Ok(l), Ok(e)) => {
                        this.libraries = l;
                        this.disc.local_ids = rmh_app::library_ids(&e);
                        this.entries = e;
                        if this.disc.search.open {
                            let q = this.disc.search.query.clone();
                            this.disc.search.local =
                                rmh_app::search_library(&this.entries, &q).into_iter().take(5).map(|(e, _)| e.item.id).collect();
                        }
                        if !this.startup_scanned {
                            this.startup_scanned = true;
                            for lib in this.libraries.clone() {
                                this.scan_library(lib, cx);
                            }
                        }
                        this.maybe_match(cx);
                    }
                    (Err(e), _) | (_, Err(e)) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Match new files against TMDB once per change (posters, details)
    fn maybe_match(&mut self, cx: &mut Context<Self>) {
        let cfg = self.svc.config();
        if self.matching || self.match_attempted || !cfg.auto_match || !self.svc.has_tmdb() {
            return;
        }
        if self.entries.iter().any(|e| e.link.is_none() && !is_audio(&e.item)) {
            self.match_library(false, cx);
        }
    }

    pub(crate) fn match_library(&mut self, force: bool, cx: &mut Context<Self>) {
        if self.matching {
            return;
        }
        self.matching = true;
        self.match_attempted = true;
        self.busy = Some("Fetching posters & details from TMDB…".into());
        cx.notify();
        let fut = self.svc.match_library(force);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.matching = false;
                this.busy = None;
                match res {
                    Ok((m, u)) => {
                        this.status = Some(format!("TMDB: {m} matched{}", if u > 0 { format!(", {u} not found") } else { String::new() }))
                    }
                    Err(e) => this.error = Some(e),
                }
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn nav_to(&mut self, item: Nav, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            Nav::Page(p) => self.navigate(p, window, cx),
            Nav::Search => self.open_search(window, cx),
            Nav::Movies => self.browse_kind_home(TitleKind::Movie, window, cx),
            Nav::Series => self.browse_kind_home(TitleKind::Tv, window, cx),
        }
    }

    pub(crate) fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page == Page::Player && self.now.is_none() {
            return;
        }
        if page != self.page && !matches!(self.page, Page::Player | Page::Details | Page::Person) {
            self.back_page = self.page;
        }
        if page != Page::Player && self.theater {
            self.set_theater(false, window, cx);
        }
        self.page = page;
        match page {
            Page::Discover => self.ensure_browse(cx),
            Page::Calendar => self.load_calendar(false, cx),
            Page::Watchlist => {
                if self.trakt.logged_in && self.disc.lists.cache.lists.is_empty() {
                    self.refresh_lists_from_trakt(cx);
                } else {
                    self.reload_lists_hub(cx);
                }
            }
            Page::Settings => self.refresh_cache_usage(),
            _ => {}
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The active palette (theme + accent + system light/dark)
    pub(crate) fn palette(&self, window: &Window) -> crate::theme::Palette {
        let cfg = self.svc.config();
        let light = matches!(window.appearance(), WindowAppearance::Light | WindowAppearance::VibrantLight);
        crate::theme::palette_for(&cfg.theme, cfg.accent_color.as_deref(), cfg.theme_follow_system, light)
    }

    /// Change appearance settings and save them (applies live)
    pub(crate) fn set_appearance(&mut self, f: impl FnOnce(&mut rmh_app::AppConfig), cx: &mut Context<Self>) {
        let mut cfg = self.svc.config();
        f(&mut cfg);
        if let Err(e) = self.svc.save_config(cfg) {
            self.error = Some(e);
        }
        cx.notify();
    }

    pub(crate) fn set_home_layout(&mut self, layout: HomeLayout, cx: &mut Context<Self>) {
        let mut cfg = self.svc.config();
        cfg.home_layout = layout;
        if let Err(e) = self.svc.save_config(cfg) {
            self.error = Some(e);
        }
        if layout == HomeLayout::Discover || layout.is_feed() {
            if self.trending.is_empty() {
                self.load_trending(cx);
            } else if layout == HomeLayout::Discover && self.hero_backdrop.is_none() {
                self.load_hero(cx);
            }
        }
        cx.notify();
    }

    // ---------------------------------------------------------------- library

    fn add_library(&mut self, path: String, cx: &mut Context<Self>) {
        if path.trim().is_empty() {
            return;
        }
        self.busy = Some(format!("Scanning {}…", path.trim()));
        self.error = None;
        cx.notify();
        let fut = self.svc.add_library(path);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok((lib, n)) => {
                        this.status = Some(format!("Added “{}” · {} items", lib.name, n));
                        this.match_attempted = false;
                        this.reload(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn add_library_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.path_input.read(cx).value().to_string();
        self.path_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.add_library(path, cx);
    }

    pub(crate) fn browse_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add to library".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = rx.await;
            this.update(cx, |this, cx| match picked {
                Ok(Ok(Some(paths))) => {
                    if let Some(p) = paths.first() {
                        this.add_library(p.to_string_lossy().to_string(), cx);
                    }
                }
                Ok(Err(e)) => this.notify_error(format!("Folder picker unavailable ({e}). Type the path instead."), cx),
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn remove_library(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let fut = self.svc.remove_library(id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(()) => this.status = Some("Library removed".into()),
                    Err(e) => this.error = Some(e),
                }
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn scan_library(&mut self, lib: Library, cx: &mut Context<Self>) {
        self.busy = Some(format!("Scanning {}…", lib.name));
        cx.notify();
        let fut = self.svc.scan_library(&lib);
        let name = lib.name.clone();
        cx.spawn(async move |this, cx| {
            let poller = this.clone();
            let poll = cx.spawn(async move |cx| loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let cont = poller
                    .update(cx, |this, cx| {
                        if let Some(line) = this.svc.scan_status() {
                            this.busy = Some(line);
                            cx.notify();
                            true
                        } else {
                            this.busy.as_ref().is_some_and(|b| b.starts_with("Scanning") || b.contains('/'))
                        }
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            });
            let res = fut.await;
            let _ = poll.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(n) => this.status = Some(format!("Scan of “{name}” complete · {n} new")),
                    Err(e) => this.error = Some(e),
                }
                this.match_attempted = false;
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    fn save_progress(&mut self, update: Option<ProgressUpdate>, cx: &mut Context<Self>) {
        let Some(update) = update else { return };
        let push_item = if update.watched {
            self.entries.iter().find(|e| e.item.id == update.media_id).and_then(|e| e.trakt_item())
        } else {
            None
        };
        if let Some(e) = self.entries.iter_mut().find(|e| e.item.id == update.media_id) {
            e.item.watch_progress = update.position;
            e.item.watched |= update.watched;
            e.item.updated_at = chrono_now();
        }
        if let Some(item) = push_item {
            self.svc.trakt_push_watched(item);
        }
        let fut = self.svc.save_progress(update);
        cx.spawn(async move |_, _| {
            if let Err(e) = fut.await {
                tracing::warn!("Failed to save progress: {e}");
            }
        })
        .detach();
    }

    /// Offline/online shelf signals for a TMDB title.
    pub(crate) fn title_availability(&self, kind: TitleKind, tmdb_id: i32) -> rmh_app::TitleAvailability {
        rmh_app::TitleAvailability::resolve(kind, tmdb_id, &self.disc.local_ids, &self.disc.watch_ids, &self.disc.collection_ids)
    }

    /// Couch mode: arrow-key focus on the Library grid (columns ≈ 6).
    pub(crate) fn couch_library_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ids: Vec<Uuid> = self.filtered_entries(cx).iter().map(|e| e.item.id).collect();
        if ids.is_empty() {
            return false;
        }
        let cols = 6usize;
        let cur = self.couch_focus.and_then(|id| ids.iter().position(|x| *x == id)).unwrap_or(0);
        let next = match key {
            "left" => cur.saturating_sub(1),
            "right" => (cur + 1).min(ids.len() - 1),
            "up" => cur.saturating_sub(cols),
            "down" => (cur + cols).min(ids.len() - 1),
            "enter" => {
                let id = self.couch_focus.unwrap_or(ids[cur]);
                self.open_entry(id, window, cx);
                return true;
            }
            _ => return false,
        };
        self.couch_focus = Some(ids[next]);
        cx.notify();
        true
    }

    /// Library entries matching the filter box + type / watched / match filters
    pub(crate) fn filtered_entries(&self, cx: &App) -> Vec<&LibraryEntry> {
        let filter = self.filter_input.read(cx).value().to_lowercase();
        let cfg = self.svc.config();
        let hide_watched = cfg.library_hide_watched;
        let type_f = cfg.library_type_filter.clone();
        let match_f = cfg.library_match_filter.clone();
        self.entries
            .iter()
            .filter(|e| {
                if hide_watched && e.item.watched {
                    return false;
                }
                let type_ok = match type_f.as_str() {
                    "movie" => matches!(e.item.media_type, rmh_app::MediaType::Movie) && e.kind() != Some(TitleKind::Tv),
                    "tv" => {
                        e.kind() == Some(TitleKind::Tv)
                            || matches!(e.item.media_type, rmh_app::MediaType::Episode | rmh_app::MediaType::TvShow)
                    }
                    "other" => matches!(e.item.media_type, rmh_app::MediaType::Unknown),
                    _ => true,
                };
                if !type_ok {
                    return false;
                }
                let matched = rmh_app::entry_is_matched(e);
                let match_ok = match match_f.as_str() {
                    "matched" => matched,
                    "unmatched" => !matched,
                    _ => true,
                };
                if !match_ok {
                    return false;
                }
                filter.is_empty()
                    || e.display_title().to_lowercase().contains(&filter)
                    || e.item.file_name.to_lowercase().contains(&filter)
            })
            .collect()
    }

    pub(crate) fn entry(&self, id: Uuid) -> Option<&LibraryEntry> {
        self.entries.iter().find(|e| e.item.id == id)
    }

    // --------------------------------------------------------------- playback

    /// End the current session's bookkeeping (progress + Trakt stop)
    fn finish_session(&mut self, cx: &mut Context<Self>) {
        if let Some(mut n) = self.now.take() {
            if !self.eof_handled {
                let update = n.progress(false);
                self.save_progress(update, cx);
            }
            if let Some((item, action, pct)) = n.scrobble_stop() {
                self.svc.trakt_scrobble(action, item, pct);
            }
        }
        self.now_entry = None;
        self.now_source = None;
        self.autoplay = None;
        self.eof_handled = false;
        self.thumb = None;
        self.hover_seek = None;
        self.intro_segments = None;
        self.next_ep_streams.clear();
    }

    fn after_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.snap = PlaybackSnapshot::default();
        self.intro_segments = None;
        self.next_ep_streams.clear();
        if self.embedded() || self.page == Page::Player {
            self.navigate(Page::Player, window, cx);
        }
        window.focus(&self.focus_handle, cx);
        self.fetch_intro_and_presearch(cx);
        cx.notify();
    }

    fn fetch_intro_and_presearch(&mut self, cx: &mut Context<Self>) {
        let cfg = self.svc.config();
        if cfg.introdb_skip {
            let tvdb = self
                .stream_ctx
                .as_ref()
                .and_then(|c| c.tvdb_id)
                .or_else(|| self.details.as_ref().and_then(|d| d.details.as_ref()).and_then(|d| d.tvdb_id));
            let duration_ms = self.snap.duration.map(|d| (d * 1000.0) as u64).unwrap_or(0);
            if let Some(tvdb) = tvdb.filter(|id| *id > 0) {
                let fut = self.svc.intro_segments(tvdb, duration_ms.max(1));
                cx.spawn(async move |this, cx| {
                    let res = fut.await;
                    this.update(cx, |this, cx| {
                        if let Ok(segs) = res {
                            this.intro_segments = Some(segs);
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
        if cfg.next_episode_presearch {
            if let Some(ctx) = self.stream_ctx.clone() {
                if let (Some(imdb), Some((s, e))) = (ctx.imdb.clone(), ctx.episode) {
                    let fut = self.svc.search_streams_for(imdb, Some((s, e + 1)));
                    cx.spawn(async move |this, cx| {
                        let res = fut.await;
                        this.update(cx, |this, cx| {
                            if let Ok(report) = res {
                                this.next_ep_streams = report.hits;
                                if !this.next_ep_streams.is_empty() {
                                    tracing::info!("Presearched {} streams for next episode", this.next_ep_streams.len());
                                }
                            }
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
            }
        }
    }

    pub(crate) fn install_addon(&mut self, cx: &mut Context<Self>) {
        let url = self.disc.inputs.addon_url.read(cx).value().trim().to_string();
        if url.is_empty() {
            self.error = Some("Paste an addon manifest URL first".into());
            cx.notify();
            return;
        }
        self.busy = Some("Installing addon…".into());
        cx.notify();
        let fut = self.svc.install_addon(url);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(a) => {
                        this.status = Some(if a.url.contains("torrentio") || a.name.to_lowercase().contains("torrentio") {
                            format!("Installed {}. Built-in Torrentio turned off to avoid duplicate searches.", a.name)
                        } else {
                            format!("Installed {}", a.name)
                        });
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn install_catalog_entry(&mut self, entry_id: String, cx: &mut Context<Self>) {
        self.busy = Some("Installing addon…".into());
        cx.notify();
        let fut = self.svc.install_catalog_entry(entry_id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(a) => {
                        this.status = Some(if a.url.contains("torrentio") || a.name.to_lowercase().contains("torrentio") {
                            format!("Installed {}. Built-in Torrentio turned off to avoid duplicate searches.", a.name)
                        } else {
                            format!("Installed {}", a.name)
                        });
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn play_item(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.entry(id).cloned() else { return };
        self.finish_session(cx);
        let path = std::path::Path::new(&entry.item.file_path);
        match self.player.play_at_position(path, resume_position(entry.item.watch_progress)) {
            Ok(()) => {
                self.error = None;
                let title = match entry.subtitle().filter(|_| entry.kind() == Some(TitleKind::Tv)) {
                    Some(s) => format!("{} · {s}", entry.display_title()),
                    None => entry.display_title(),
                };
                self.status = Some(format!("Playing {title}"));
                self.now = Some(NowPlaying::new(title, Some(id), self.volume).with_trakt(entry.trakt_item()));
                self.now_source = Some(entry.item.file_path.clone());
                self.now_entry = Some(entry);
                self.after_start(window, cx);
            }
            Err(e) => self.error = Some(format!("Playback error: {e:#}")),
        }
        cx.notify();
    }

    pub(crate) fn play_url(&mut self, url: String, title: String, trakt: Option<TraktItem>, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_session(cx);
        match self.player.play_url(&url) {
            Ok(()) => {
                self.status = Some(format!("Streaming {title}"));
                self.now = Some(NowPlaying::new(title, None, self.volume).with_trakt(trakt));
                self.now_source = Some(url);
                self.after_start(window, cx);
            }
            Err(e) => self.error = Some(format!("Playback error: {e:#}")),
        }
        cx.notify();
    }

    pub(crate) fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.now.as_mut() {
            let _ = self.player.toggle_pause();
            n.paused = !n.paused;
            self.snap.paused = n.paused;
            cx.notify();
        }
    }

    pub(crate) fn seek_relative(&mut self, secs: f64, cx: &mut Context<Self>) {
        if self.now.is_some() {
            let _ = self.player.seek_relative(secs);
            cx.notify();
        }
    }

    pub(crate) fn seek_to(&mut self, secs: f64, cx: &mut Context<Self>) {
        if let Some(n) = self.now.as_mut() {
            let _ = self.player.seek(secs);
            n.position = secs;
            self.snap.position = secs;
            self.autoplay = None;
            cx.notify();
        }
    }

    pub(crate) fn set_volume(&mut self, v: f64, persist: bool, cx: &mut Context<Self>) {
        self.volume = v.clamp(0.0, 130.0);
        let _ = self.player.set_volume(self.volume);
        if let Some(n) = self.now.as_mut() {
            n.volume = self.volume;
        }
        if persist {
            let mut cfg = self.svc.config();
            cfg.volume = Some(self.volume);
            let _ = self.svc.save_config(cfg);
        }
        cx.notify();
    }

    pub(crate) fn nudge_volume(&mut self, delta: f64, window: &mut Window, cx: &mut Context<Self>) {
        let v = (self.volume + delta).clamp(0.0, 130.0);
        self.volume_slider.update(cx, |s, cx| s.set_value(v as f32, window, cx));
        self.set_volume(v, true, cx);
    }

    pub(crate) fn set_speed(&mut self, speed: f64, cx: &mut Context<Self>) {
        let _ = self.player.set_speed(speed);
        let _ = self.player.show_text(&format!("Speed {speed:.2}×"));
        self.snap.speed = speed;
        cx.notify();
    }

    pub(crate) fn nudge_speed(&mut self, up: bool, cx: &mut Context<Self>) {
        const STEPS: [f64; 9] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0];
        let cur = if self.snap.speed > 0.0 { self.snap.speed } else { 1.0 };
        let next = if up {
            STEPS.iter().copied().find(|s| *s > cur + 0.01).unwrap_or(3.0)
        } else {
            STEPS.iter().rev().copied().find(|s| *s < cur - 0.01).unwrap_or(0.25)
        };
        self.set_speed(next, cx);
    }

    pub(crate) fn frame_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let _ = self.player.frame_step(forward);
        cx.notify();
    }

    pub(crate) fn step_chapter(&mut self, delta: i64, cx: &mut Context<Self>) {
        let _ = self.player.step_chapter(delta);
        cx.notify();
    }

    pub(crate) fn go_chapter(&mut self, idx: usize, cx: &mut Context<Self>) {
        let _ = self.player.set_chapter(idx as i64);
        cx.notify();
    }

    /// Current chapter is an intro/recap: (button label, seek target)
    pub(crate) fn skip_target(&self) -> Option<(&'static str, f64)> {
        if let Some(idx) = self.snap.chapter.map(|c| c as usize) {
            if let Some(ch) = self.snap.chapters.get(idx) {
                let t = ch.title.to_lowercase();
                let is = |words: &[&str]| words.iter().any(|w| t == *w || t.contains(w));
                let label = if is(&["intro", "opening", "opening credits", "title sequence"]) || t == "op" {
                    Some("Skip intro")
                } else if is(&["recap", "previously"]) {
                    Some("Skip recap")
                } else if is(&["credits", "ending", "outro", "end credits"]) || t == "ed" {
                    Some("Skip credits")
                } else {
                    None
                };
                if let Some(label) = label {
                    let target = self.snap.chapters.get(idx + 1).map(|c| c.time).or(self.snap.duration.map(|d| (d - 1.0).max(0.0)))?;
                    return Some((label, target));
                }
            }
        }
        // TheIntroDB fallback when chapters are missing
        let pos = self.snap.position;
        let segs = self.intro_segments.as_ref()?;
        if let Some(end_ms) = segs.recap_end_ms {
            let end = end_ms as f64 / 1000.0;
            if pos < end {
                return Some(("Skip recap", end));
            }
        }
        if let Some(end_ms) = segs.intro_end_ms {
            let end = end_ms as f64 / 1000.0;
            if pos < end {
                return Some(("Skip intro", end));
            }
        }
        if let Some(start_ms) = segs.credits_start_ms {
            let start = start_ms as f64 / 1000.0;
            if pos >= start {
                let dur = self.snap.duration.unwrap_or(start + 1.0);
                return Some(("Skip credits", (dur - 1.0).max(start)));
            }
        }
        None
    }

    pub(crate) fn skip_segment(&mut self, cx: &mut Context<Self>) {
        if let Some((_, target)) = self.skip_target() {
            self.seek_to(target, cx);
        }
    }

    pub(crate) fn select_track(&mut self, kind: rmh_app::TrackKind, id: Option<i64>, cx: &mut Context<Self>) {
        let _ = self.player.set_track(kind, id);
        for t in self.snap.tracks.iter_mut().filter(|t| t.kind == kind) {
            t.selected = Some(t.id) == id;
        }
        // Dual: if enabled and primary changed, keep secondary on a different preferred lang track
        if kind == rmh_app::TrackKind::Sub && self.svc.config().dual_subtitles {
            if let Some(sec) = self.pick_secondary_sub(id) {
                let _ = self.player.set_secondary_sub(Some(sec));
                self.snap.secondary_sid = Some(sec);
            }
        }
        cx.notify();
    }

    /// Prefer a non-primary subtitle in the second preferred language (or any other track)
    pub(crate) fn pick_secondary_sub(&self, primary: Option<i64>) -> Option<i64> {
        let langs = self.svc.config().subtitle_languages();
        let second_lang = langs.get(1).cloned();
        let subs: Vec<_> = self.snap.tracks.iter().filter(|t| t.kind == rmh_app::TrackKind::Sub).collect();
        if let Some(want) = second_lang {
            if let Some(t) = subs.iter().find(|t| {
                Some(t.id) != primary && t.lang.as_deref().map(rmh_app::lang_code).as_deref() == Some(want.as_str())
            }) {
                return Some(t.id);
            }
        }
        subs.iter().find(|t| Some(t.id) != primary).map(|t| t.id)
    }

    pub(crate) fn select_secondary_sub(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        let _ = self.player.set_secondary_sub(id);
        self.snap.secondary_sid = id;
        if id.is_some() {
            self.update_config(|c| c.dual_subtitles = true, false, cx);
        }
        cx.notify();
    }

    pub(crate) fn nudge_sub_delay(&mut self, delta: f64, cx: &mut Context<Self>) {
        let next = ((self.snap.sub_delay + delta) * 10.0).round() / 10.0;
        let _ = self.player.set_sub_delay(next);
        self.snap.sub_delay = next;
        let _ = self.player.show_text(&format!("Sub delay {next:+.1}s"));
        self.update_config(|c| c.sub_delay = next, false, cx);
    }

    pub(crate) fn reset_sub_delay(&mut self, cx: &mut Context<Self>) {
        let _ = self.player.set_sub_delay(0.0);
        self.snap.sub_delay = 0.0;
        let _ = self.player.show_text("Sub delay 0.0s");
        self.update_config(|c| c.sub_delay = 0.0, false, cx);
    }

    pub(crate) fn toggle_sub_forced_only(&mut self, cx: &mut Context<Self>) {
        let on = !self.svc.config().sub_forced_only;
        let _ = self.player.set_sub_forced_only(on);
        let _ = self.player.show_text(if on { "Forced subs only" } else { "All subtitle events" });
        self.update_config(|c| c.sub_forced_only = on, false, cx);
    }

    /// Keyboard: step to the next audio / subtitle track (subtitles wrap through "off")
    pub(crate) fn cycle_track(&mut self, kind: rmh_app::TrackKind, cx: &mut Context<Self>) {
        let ids: Vec<i64> = self.snap.tracks.iter().filter(|t| t.kind == kind).map(|t| t.id).collect();
        if ids.is_empty() {
            return;
        }
        let cur = self.snap.tracks.iter().find(|t| t.kind == kind && t.selected).map(|t| t.id);
        let mut options: Vec<Option<i64>> = ids.into_iter().map(Some).collect();
        if kind == rmh_app::TrackKind::Sub {
            options.push(None);
        }
        let i = options.iter().position(|o| *o == cur).map(|i| (i + 1) % options.len()).unwrap_or(0);
        let next = options[i];
        self.side_tab = if kind == rmh_app::TrackKind::Sub { SideTab::Subtitles } else { SideTab::Audio };
        let label = next
            .and_then(|id| self.snap.tracks.iter().find(|t| t.kind == kind && t.id == id))
            .map(|t| t.label())
            .unwrap_or_else(|| "Off".into());
        let what = if kind == rmh_app::TrackKind::Sub { "Subtitles" } else { "Audio" };
        let _ = self.player.show_text(&format!("{what}: {label}"));
        self.select_track(kind, next, cx);
    }

    pub(crate) fn cycle_side_tab(&mut self, cx: &mut Context<Self>) {
        self.side_tab = match self.side_tab {
            SideTab::Chapters => SideTab::Audio,
            SideTab::Audio => SideTab::Subtitles,
            SideTab::Subtitles => SideTab::Speed,
            SideTab::Speed => SideTab::Chapters,
        };
        cx.notify();
    }

    pub(crate) fn load_subtitle_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Load subtitles".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| {
                    if let Some(p) = paths.first() {
                        let _ = this.player.add_subtitle(&p.to_string_lossy());
                        this.status = Some("Subtitles loaded".into());
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn set_theater(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.theater != on {
            self.theater = on;
            self.sync_window_fullscreen(window);
            // keep mpv's own flag in step so its double-click / F toggles agree
            if self.embedded() && self.now.is_some() && self.snap.fullscreen != on {
                self.snap.fullscreen = on;
                let _ = self.player.set_fullscreen(on);
            }
            if on && self.now.is_some() {
                let _ = self.player.show_text("Fullscreen · Esc or F to leave");
            }
            cx.notify();
        }
    }

    /// The window is fullscreen while the app (F11) or the player asks for it
    fn sync_window_fullscreen(&self, window: &mut Window) {
        let want = self.theater || self.app_fullscreen;
        if window.is_fullscreen() != want {
            window.toggle_fullscreen();
        }
    }

    /// F11 / the ⛶ button: fullscreen everywhere. On the player page this is
    /// the same as the player's fullscreen (video only, no app chrome).
    pub(crate) fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Player && self.now.is_some() {
            let on = !self.theater;
            self.app_fullscreen = false;
            self.set_theater(on, window, cx);
            return;
        }
        self.app_fullscreen = !self.app_fullscreen;
        self.sync_window_fullscreen(window);
        cx.notify();
    }

    fn prune_image_cache(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.prune_image_cache(None);
        cx.spawn(async move |_, _| {
            if let Err(e) = fut.await {
                tracing::warn!("Image cache cleanup failed: {e}");
            }
        })
        .detach();
    }

    pub(crate) fn stop(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(n) = &self.now {
            self.status = Some(format!("Stopped {}", n.title));
        }
        if self.now_source.as_deref().is_some_and(|s| s.starts_with("http://127.0.0.1:")) {
            // Stop seeding / downloading the torrent we were streaming
            let f = self.svc.stop_torrents();
            cx.spawn(async move |_, _| {
                let _ = f.await;
            })
            .detach();
        }
        self.finish_session(cx);
        let _ = self.player.stop();
        if let Some(v) = &self.video {
            v.hide();
        }
        if self.page == Page::Player {
            let back = self.back_page;
            self.navigate(back, window, cx);
        }
        self.set_theater(false, window, cx);
        self.reload(cx);
        cx.notify();
    }

    pub(crate) fn next_local_episode(&self) -> Option<LibraryEntry> {
        let cur = self.now_entry.as_ref()?;
        next_episode(&self.entries, cur).cloned()
    }

    /// Seek-bar hover: remember the time and fetch a preview frame
    pub(crate) fn hover_seek_at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let b = self.seek_bounds.get();
        let (Some(d), w) = (self.snap.duration.filter(|d| *d > 0.0), f32::from(b.size.width)) else { return };
        if w <= 0.0 {
            return;
        }
        let frac = ((f32::from(x) - f32::from(b.origin.x)) / w).clamp(0.0, 1.0) as f64;
        let secs = frac * d;
        self.hover_seek = Some(secs);
        self.request_thumb(secs, cx);
        cx.notify();
    }

    fn request_thumb(&mut self, secs: f64, cx: &mut Context<Self>) {
        let bucket = (secs / 5.0).floor() as u64 * 5;
        if self.thumb.as_ref().is_some_and(|(b, _)| *b == bucket) || self.thumb_pending {
            return;
        }
        let Some(src) = self.now_source.clone() else { return };
        self.thumb_pending = true;
        let fut = self.svc.thumbnail(src, secs);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.thumb_pending = false;
                if let Ok(path) = res {
                    this.thumb = Some((bucket, path));
                }
                // The pointer may have moved on meanwhile
                if let Some(h) = this.hover_seek {
                    if (h / 5.0).floor() as u64 * 5 != bucket {
                        this.request_thumb(h, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn click_seek_at(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let b = self.seek_bounds.get();
        let w = f32::from(b.size.width);
        if let (Some(d), true) = (self.snap.duration.filter(|d| *d > 0.0), w > 0.0) {
            let frac = ((f32::from(x) - f32::from(b.origin.x)) / w).clamp(0.0, 1.0) as f64;
            self.seek_to(frac * d, cx);
        }
    }

    pub(crate) fn on_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pump_feed(cx);
        self.pump_images(cx);
        self.maybe_trakt_background_sync(cx);
        if std::mem::take(&mut self.autoplay_ready) {
            self.pending_play = None;
            if self.page == Page::Details && !self.streams.is_empty() {
                self.play_best(window, cx);
            }
        }
        if let Some((what, line)) = self.starting.as_mut() {
            let now = self.svc.start_status();
            if now.is_some() && now != *line {
                self.busy = Some(format!("{what}: {}", now.as_deref().unwrap_or_default()));
                *line = now;
                cx.notify();
            }
        }
        let Some(now) = self.now.as_mut() else { return };
        let mut snap = self.player.snapshot();
        // Paused and then seeked to the very end: mpv may not report eof-reached
        // until playback resumes, so treat "paused at the last half second" as the end.
        if !snap.eof && snap.loaded && snap.paused && snap.duration.is_some_and(|d| d > 5.0 && d - snap.position < 0.6) {
            snap.eof = true;
        }
        let outcome = now.tick(&snap);
        let scrobble = now.scrobble_update();
        let fullscreen_changed = snap.fullscreen != self.snap.fullscreen;
        self.snap = snap;
        if let Some((item, action, pct)) = scrobble {
            self.social_scrobble(&item, action, pct);
            self.svc.trakt_scrobble(action, item, pct);
        }
        match outcome {
            TickOutcome::Continue => {}
            TickOutcome::Save(update) => self.save_progress(Some(update), cx),
            TickOutcome::Ended(update) => {
                let title = self.now.as_ref().map(|n| n.title.clone()).unwrap_or_default();
                if !self.eof_handled {
                    self.save_progress(update, cx);
                }
                self.eof_handled = true;
                self.finish_session(cx);
                if let Some(v) = &self.video {
                    v.hide();
                }
                if self.page == Page::Player {
                    let back = self.back_page;
                    self.navigate(back, window, cx);
                }
                self.set_theater(false, window, cx);
                self.status = Some(format!("Finished {title}"));
                cx.notify();
                return;
            }
            TickOutcome::Failed(err) => {
                self.finish_session(cx);
                let _ = self.player.stop();
                self.error = Some(err);
                if let Some(v) = &self.video {
                    v.hide();
                }
                cx.notify();
                return;
            }
        }
        // mpv's own fullscreen (double-click / F in the video) drives theater mode
        if fullscreen_changed && self.embedded() {
            let fs = self.snap.fullscreen;
            if self.page != Page::Player && fs {
                self.navigate(Page::Player, window, cx);
            }
            self.set_theater(fs, window, cx);
        }
        // End of file (mpv keeps the last frame): mark watched, maybe autoplay
        if self.snap.eof && !self.eof_handled {
            self.eof_handled = true;
            if let Some(n) = self.now.as_mut() {
                let update = n.progress(true);
                let stop = n.scrobble_stop();
                self.save_progress(update, cx);
                if let Some((item, action, _)) = stop {
                    self.social_scrobble(&item, action, 100.0);
                    self.svc.trakt_scrobble(action, item, 100.0);
                }
            }
            if self.svc.config().autoplay_next {
                if let Some(next) = self.next_local_episode() {
                    self.autoplay = Some((Instant::now() + Duration::from_secs(10), next.item.id));
                    let _ = self.player.show_text(&format!("Up next: {} – {}", next.display_title(), next.subtitle().unwrap_or_default()));
                }
            }
        } else if !self.snap.eof && self.eof_handled && self.snap.position > 1.0 {
            // user seeked back from the end
            if let Some(n) = self.now.as_ref() {
                if n.duration.is_some_and(|d| d - n.position > 5.0) {
                    self.eof_handled = false;
                    self.autoplay = None;
                }
            }
        }
        self.maybe_auto_subtitles(cx);
        if let Some((at, id)) = self.autoplay {
            if Instant::now() >= at {
                self.autoplay = None;
                self.play_item(id, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn play_next_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = self.autoplay.take().map(|(_, id)| id).or_else(|| self.next_local_episode().map(|e| e.item.id));
        if let Some(id) = next {
            self.play_item(id, window, cx);
            return;
        }
        // Use presearched streams for the next episode when available
        if !self.next_ep_streams.is_empty() {
            let candidates = std::mem::take(&mut self.next_ep_streams);
            if let Some(ctx) = self.stream_ctx.as_mut() {
                if let Some((s, e)) = ctx.episode.as_mut() {
                    *e += 1;
                    ctx.label = format!("{} S{:02}E{:02}", ctx.label.split(" · ").next().unwrap_or("Episode"), s, e);
                }
            }
            self.start_streams(candidates, window, cx);
        }
    }

    // --------------------------------------------------------------- discover

    pub(crate) fn load_trending(&mut self, cx: &mut Context<Self>) {
        if !self.svc.has_tmdb() {
            return;
        }
        let fut = self.svc.trending();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(t) => {
                        this.trending = t;
                        // the streaming home loads its own (lazy) backdrops
                        if this.svc.config().home_layout == HomeLayout::Discover {
                            this.load_hero(cx);
                        }
                    }
                    Err(e) => tracing::warn!("Trending failed: {e}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_hero(&mut self, cx: &mut Context<Self>) {
        let Some(top) = self.trending.first() else { return };
        let fut = self.svc.title_details(top.title.kind, top.title.tmdb_id);
        cx.spawn(async move |this, cx| {
            if let Ok((_, backdrop)) = fut.await {
                this.update(cx, |this, cx| {
                    this.hero_backdrop = backdrop;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn search_discover(&mut self, cx: &mut Context<Self>) {
        let query = self.discover_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.results.clear();
            cx.notify();
            return;
        }
        let is_imdb = query.starts_with("tt") && query[2..].chars().all(|c| c.is_ascii_digit());
        if is_imdb && !self.svc.has_tmdb() {
            self.load_streams(
                query.clone(),
                None,
                StreamContext { label: query, trakt: None, imdb: None, episode: None, tvdb_id: None },
                cx,
            );
            return;
        }
        self.searching = true;
        self.error = None;
        let fut = self.svc.search_titles(query);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.searching = false;
                match res {
                    Ok(r) => {
                        this.status = Some(format!("{} titles found", r.len()));
                        this.results = r;
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Open the details page for a TMDB title
    pub(crate) fn open_title(&mut self, card: &TitleCard, local: Option<Uuid>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(media_id) = self.linking_media.take() {
            let kind = card.title.kind;
            let tmdb = card.title.tmdb_id;
            let name = card.title.name.clone();
            self.busy = Some(format!("Linking to {name}…"));
            let fut = self.svc.link_media_to_title(media_id, kind, tmdb);
            cx.spawn(async move |this, cx| {
                let res = fut.await;
                this.update(cx, |this, cx| {
                    this.busy = None;
                    match res {
                        Ok(()) => {
                            this.status = Some(format!("Linked and locked match: {name}"));
                            this.reload(cx);
                        }
                        Err(e) => this.error = Some(e),
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        self.open_details(
            DetailsState {
                kind: Some(card.title.kind),
                tmdb_id: Some(card.title.tmdb_id),
                local,
                name: card.title.name.clone(),
                poster: card.poster.clone(),
                details: None,
                backdrop: None,
                loading: true,
                season: None,
                episodes: vec![],
                trakt_progress: None,
                related: vec![],
            },
            window,
            cx,
        );
    }

    /// Start Fix match: search TMDB, then pick a title to lock onto this file.
    pub(crate) fn begin_fix_match(&mut self, media_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let hint = self
            .entry(media_id)
            .map(|e| e.display_title())
            .unwrap_or_else(|| "title".into());
        self.linking_media = Some(media_id);
        self.discover_input.update(cx, |s, cx| {
            s.set_value(&hint, window, cx);
        });
        self.status = Some(format!("Pick a TMDB match for “{hint}” (search, then click a title)"));
        self.navigate(Page::Discover, window, cx);
        self.search_discover(cx);
    }

    pub(crate) fn toggle_match_lock(&mut self, media_id: Uuid, cx: &mut Context<Self>) {
        let locked = self
            .entry(media_id)
            .and_then(|e| e.link.as_ref())
            .is_some_and(|l| l.match_locked != 0);
        let fut = self.svc.set_match_locked(media_id, !locked);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(()) => {
                        this.status = Some(if locked { "Match unlocked".into() } else { "Match locked".into() });
                        this.reload(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open details for a library entry (or play it if we know nothing more)
    pub(crate) fn open_entry(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.entry(id).cloned() else { return };
        let (Some(kind), Some(tmdb)) = (e.kind(), e.link.as_ref().and_then(|l| l.tmdb_id)) else {
            // Unmatched: offer Fix match instead of only playing
            self.begin_fix_match(id, window, cx);
            return;
        };
        let season = e.link.as_ref().and_then(|l| l.season).map(|s| s as u32);
        self.open_details(
            DetailsState {
                kind: Some(kind),
                tmdb_id: Some(tmdb as i32),
                local: (kind == TitleKind::Movie).then_some(id),
                name: e.display_title(),
                poster: e.poster(),
                details: None,
                backdrop: None,
                loading: true,
                season,
                episodes: vec![],
                trakt_progress: None,
                related: vec![],
            },
            window,
            cx,
        );
    }

    fn open_details(&mut self, state: DetailsState, window: &mut Window, cx: &mut Context<Self>) {
        self.streams.clear();
        self.stream_warnings.clear();
        self.stream_ctx = None;
        let (kind, id) = (state.kind, state.tmdb_id);
        if self.pending_play.is_some() && (kind, id) != self.pending_play.map(|(k, i)| (Some(k), Some(i))).unwrap_or_default() {
            self.pending_play = None;
        }
        self.details = Some(state);
        self.navigate(Page::Details, window, cx);
        let (Some(kind), Some(id)) = (kind, id) else { return };
        if self.trakt.logged_in {
            if kind == TitleKind::Tv {
                let prog = self.svc.trakt_show_progress(id);
                cx.spawn(async move |this, cx| {
                    let res = prog.await;
                    this.update(cx, |this, cx| {
                        if let (Ok(p), Some(d)) = (res, this.details.as_mut()) {
                            d.trakt_progress = Some(p);
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            let related = self.svc.trakt_related(kind, id);
            cx.spawn(async move |this, cx| {
                let res = related.await;
                this.update(cx, |this, cx| {
                    if let Ok(entries) = res {
                        let cards: Vec<TitleCard> = entries
                            .into_iter()
                            .filter_map(|e| {
                                let tmdb = e.tmdb_id()? as i32;
                                let k = if e.movie.is_some() { TitleKind::Movie } else { TitleKind::Tv };
                                let name = e.movie.as_ref().or(e.show.as_ref()).and_then(|m| m.title.clone())?;
                                let year = e.movie.as_ref().or(e.show.as_ref()).and_then(|m| m.year).map(|y| y as u16);
                                Some(TitleCard {
                                    title: rmh_app::Title {
                                        kind: k,
                                        tmdb_id: tmdb,
                                        name,
                                        year,
                                        overview: None,
                                        poster_path: None,
                                        backdrop_path: None,
                                        rating: None,
                                        genre_ids: vec![],
                                        original_language: None,
                                        votes: None,
                                    },
                                    poster: None,
                                })
                            })
                            .collect();
                        if let Some(d) = this.details.as_mut() {
                            d.related = cards;
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        if !self.svc.has_tmdb() {
            if let Some(d) = self.details.as_mut() {
                d.loading = false;
            }
            return;
        }
        self.load_extras(kind, id, cx);
        let fut = self.svc.title_details(kind, id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let mut load_season = None;
                let mut autostream = None;
                if let Some(d) = this.details.as_mut().filter(|d| d.tmdb_id == Some(id)) {
                    d.loading = false;
                    match res {
                        Ok((det, backdrop)) => {
                            if this.pending_play == Some((kind, id)) && kind == TitleKind::Movie {
                                if let Some(imdb) = det.imdb_id.clone() {
                                    autostream = Some((
                                        imdb.clone(),
                                        StreamContext {
                                            label: d.name.clone(),
                                            trakt: Some(TraktItem::Movie { tmdb: id as i64 }),
                                            imdb: Some(imdb),
                                            episode: None,
                                            tvdb_id: det.tvdb_id,
                                        },
                                    ));
                                }
                            }
                            if kind == TitleKind::Tv {
                                let s = d.season.or_else(|| det.seasons.iter().find(|s| s.number > 0).map(|s| s.number));
                                d.season = s;
                                load_season = s;
                            }
                            d.details = Some(det);
                            d.backdrop = backdrop;
                        }
                        Err(e) => this.error = Some(e),
                    }
                }
                if let Some(s) = load_season {
                    this.select_season(s, cx);
                }
                match autostream {
                    Some((imdb, ctx)) => this.load_streams(imdb, None, ctx, cx),
                    None => this.pending_play = None,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn select_season(&mut self, season: u32, cx: &mut Context<Self>) {
        let Some(d) = self.details.as_mut() else { return };
        let Some(show) = d.tmdb_id else { return };
        d.season = Some(season);
        d.episodes.clear();
        let fut = self.svc.season_episodes(show, season);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                if let Some(d) = this.details.as_mut().filter(|d| d.tmdb_id == Some(show) && d.season == Some(season)) {
                    match res {
                        Ok(eps) => d.episodes = eps,
                        Err(e) => this.error = Some(e),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Local file for an episode of the show on the details page
    pub(crate) fn local_episode(&self, show: i32, season: u32, episode: u32) -> Option<&LibraryEntry> {
        self.entries.iter().find(|e| {
            e.link.as_ref().is_some_and(|l| {
                l.kind == "tv" && l.tmdb_id == Some(show as i64) && l.season == Some(season as i64) && l.episode == Some(episode as i64)
            })
        })
    }

    pub(crate) fn load_streams(&mut self, imdb: String, episode: Option<(u32, u32)>, ctx: StreamContext, cx: &mut Context<Self>) {
        self.streams_loading = true;
        self.streams.clear();
        self.stream_warnings.clear();
        self.stream_ctx = Some(ctx);
        self.error = None;
        self.busy = Some(format!("Finding streams for {}…", self.stream_ctx.as_ref().map(|c| c.label.as_str()).unwrap_or("title")));
        let fut = self.svc.search_streams_for(imdb, episode);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.streams_loading = false;
                this.busy = None;
                match res {
                    Ok(report) => {
                        this.stream_warnings = report.warnings;
                        this.streams = report.hits;
                        let n = this.streams.len();
                        if n == 0 {
                            this.status = Some(if this.stream_warnings.is_empty() {
                                "No streams found".into()
                            } else {
                                format!("No streams found ({})", this.stream_warnings.join("; "))
                            });
                        } else if this.stream_warnings.is_empty() {
                            this.status = Some(format!("{n} streams found"));
                        } else {
                            this.status = Some(format!("{n} streams found · {}", this.stream_warnings.join("; ")));
                        }
                        if this.pending_play.is_some() {
                            this.autoplay_ready = true;
                        }
                        this.probe_debrid(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Ask Real-Debrid about the few streams "Play best" would try first so
    /// the picker can show which ones start instantly
    fn probe_debrid(&mut self, cx: &mut Context<Self>) {
        let order = rmh_app::streams::fast_start_order(&self.streams, &self.stream_filter);
        // Bootstrap the DHT while the list is read
        self.svc.warm_up_torrents();
        if !self.svc.has_real_debrid() {
            return;
        }
        let top: Vec<StreamHit> = rmh_app::streams::rd_probe_set(&self.streams, &order, 6).into_iter().map(|i| self.streams[i].clone()).collect();
        let fut = self.svc.probe_rd(&top);
        cx.spawn(async move |this, cx| {
            let Ok(map) = fut.await else { return };
            this.update(cx, |this, cx| {
                let mut changed = false;
                for s in this.streams.iter_mut() {
                    if let Some(c) = map.get(&s.info_hash) {
                        changed |= s.rd_cached != *c;
                        s.rd_cached = *c;
                    }
                }
                if changed {
                    rmh_app::streams::sort_best(&mut this.streams);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Default Play: the hottest streams that pass the current filters
    pub(crate) fn play_best(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let order = rmh_app::streams::fast_start_order(&self.streams, &self.stream_filter);
        if order.is_empty() {
            self.error = Some("No stream matches the filters".into());
            cx.notify();
            return;
        }
        let candidates = order.into_iter().map(|i| self.streams[i].clone()).collect();
        self.start_streams(candidates, window, cx);
    }

    pub(crate) fn play_stream(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(hit) = self.streams.get(idx).cloned() else { return };
        self.start_streams(vec![hit], window, cx);
    }

    fn start_streams(&mut self, candidates: Vec<StreamHit>, window: &mut Window, cx: &mut Context<Self>) {
        if self.starting.is_some() {
            return;
        }
        let ctx = self.stream_ctx.clone();
        let what = ctx.as_ref().map(|c| c.label.clone()).unwrap_or_else(|| candidates[0].title.clone());
        self.busy = Some(format!("Starting {what}…"));
        self.starting = Some((format!("Starting {what}"), None));
        self.error = None;
        cx.notify();
        let started = Instant::now();
        let fut = self.svc.fast_start(candidates);
        cx.spawn_in(window, async move |this, cx| {
            let res = fut.await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.starting = None;
                match res {
                    Ok(r) => {
                        tracing::info!(
                            "stream ready in {:.1}s via {}: {}",
                            started.elapsed().as_secs_f64(),
                            if r.debrid { "Real-Debrid" } else { "torrent" },
                            r.hit_title
                        );
                        let title = ctx.as_ref().map(|c| c.label.clone()).unwrap_or(r.title);
                        this.play_url(r.url, title, ctx.and_then(|c| c.trakt), window, cx)
                    }
                    Err(e) => this.notify_error(e, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    // ------------------------------------------------------------------ trakt

    pub(crate) fn refresh_trakt(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.trakt_status();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                if let Ok(s) = res {
                    let logged = s.logged_in;
                    this.trakt = s;
                    if logged {
                        this.load_trakt_lists(cx);
                        this.maybe_trakt_background_sync(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_trakt_lists(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.trakt_lists();
        let coll_fut = self.svc.trakt_collection_ids();
        let svc = self.svc.clone();
        cx.spawn(async move |this, cx| {
            let Ok(lists) = fut.await else { return };
            let collection_ids = coll_fut.await.unwrap_or_default();
            let map_ids = |entries: &[rmh_app::TraktEntry], n: usize| -> Vec<(TitleKind, i32)> {
                entries
                    .iter()
                    .filter_map(|e| {
                        let kind = if e.movie.is_some() { TitleKind::Movie } else { TitleKind::Tv };
                        Some((kind, e.tmdb_id()? as i32))
                    })
                    .take(n)
                    .collect()
            };
            let playback_wl: Vec<_> = lists.playback.iter().chain(lists.watchlist.iter()).cloned().collect();
            let cards = if svc.has_tmdb() {
                svc.cards_for(map_ids(&playback_wl, 24)).await.unwrap_or_default()
            } else {
                vec![]
            };
            let history = if svc.has_tmdb() {
                svc.cards_for(map_ids(&lists.history, 20)).await.unwrap_or_default()
            } else {
                vec![]
            };
            let recs = if svc.has_tmdb() {
                svc.cards_for(map_ids(&lists.recommendations, 20)).await.unwrap_or_default()
            } else {
                vec![]
            };
            this.update(cx, |this, cx| {
                this.trakt_lists = lists;
                this.trakt_cards = cards;
                this.trakt_history_cards = history;
                this.trakt_rec_cards = recs;
                this.disc.collection_ids = collection_ids;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Startup + quiet interval sync (watched + watchlist). Skips if already running.
    fn maybe_trakt_background_sync(&mut self, cx: &mut Context<Self>) {
        if self.trakt_syncing || !self.trakt.logged_in {
            return;
        }
        let low = self.svc.config().low_power;
        let interval = Duration::from_secs(if low { 2 * 3600 } else { 45 * 60 });
        let due = match self.trakt_sync_at {
            None => true,
            Some(t) => t.elapsed() >= interval,
        };
        if !due {
            return;
        }
        self.trakt_sync_quiet(cx);
    }

    fn trakt_sync_quiet(&mut self, cx: &mut Context<Self>) {
        self.trakt_syncing = true;
        self.trakt_sync_at = Some(Instant::now());
        let watched = self.svc.trakt_sync();
        let watchlist = self.svc.sync_watchlist();
        cx.spawn(async move |this, cx| {
            let _ = watched.await;
            let _ = watchlist.await;
            this.update(cx, |this, cx| {
                this.trakt_syncing = false;
                this.load_trakt_lists(cx);
                this.load_watchlist(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_connect(&mut self, cx: &mut Context<Self>) {
        self.save_settings(cx);
        let begin = self.svc.trakt_begin_login();
        let svc = self.svc.clone();
        cx.spawn(async move |this, cx| {
            let login = match begin.await {
                Ok(l) => l,
                Err(e) => {
                    this.update(cx, |this, cx| this.notify_error(e, cx)).ok();
                    return;
                }
            };
            this.update(cx, |this, cx| {
                this.trakt_waiting = true;
                this.status = Some("Complete sign-in in your browser…".into());
                cx.open_url(&login.auth_url);
                cx.notify();
            })
            .ok();
            let res = svc.trakt_finish_login().await;
            this.update(cx, |this, cx| {
                this.trakt_waiting = false;
                match res {
                    Ok(user) => {
                        this.status = Some(format!("Connected to Trakt{}", user.map(|u| format!(" as {u}")).unwrap_or_default()));
                        this.refresh_trakt(cx);
                        this.trakt_sync(cx);
                    }
                    Err(e) if e.to_lowercase().contains("cancelled") => {
                        // Paste-code stops the listener with this error; ignore if already logged in / not waiting.
                        if this.trakt.logged_in {
                            this.status = Some("Connected to Trakt".into());
                        } else if this.error.is_none() {
                            this.status = Some("Trakt login cancelled".into());
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_cancel_connect(&mut self, cx: &mut Context<Self>) {
        if !self.trakt_waiting {
            return;
        }
        let fut = self.svc.trakt_cancel_login();
        cx.spawn(async move |this, cx| {
            let _ = fut.await;
            this.update(cx, |this, cx| {
                this.trakt_waiting = false;
                this.status = Some("Trakt login cancelled".into());
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_paste_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.trakt_code_input.read(cx).value().trim().to_string();
        if code.is_empty() {
            self.error = Some("Paste the OAuth code from the browser page".into());
            cx.notify();
            return;
        }
        self.trakt_code_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.trakt_waiting = false;
        let fut = self.svc.trakt_complete_login(code);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.trakt_waiting = false;
                match res {
                    Ok(user) => {
                        this.status = Some(format!("Connected to Trakt{}", user.map(|u| format!(" as {u}")).unwrap_or_default()));
                        this.refresh_trakt(cx);
                        this.trakt_sync(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_disconnect(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.trakt_logout();
        cx.spawn(async move |this, cx| {
            let _ = fut.await;
            this.update(cx, |this, cx| {
                this.trakt_lists = TraktLists::default();
                this.trakt_cards.clear();
                this.trakt_history_cards.clear();
                this.trakt_rec_cards.clear();
                this.disc.collection_ids.clear();
                this.trakt_sync_at = None;
                this.status = Some("Disconnected from Trakt".into());
                this.refresh_trakt(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_sync(&mut self, cx: &mut Context<Self>) {
        self.busy = Some("Syncing with Trakt…".into());
        cx.notify();
        let watched = self.svc.trakt_sync();
        let watchlist = self.svc.sync_watchlist();
        cx.spawn(async move |this, cx| {
            let w = watched.await;
            let wl = watchlist.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match (w, wl) {
                    (Ok(r), Ok(wl)) => {
                        this.status = Some(format!(
                            "Trakt: {} watched←, {} watched→ · watchlist +{} / push {}",
                            r.pulled, r.pushed, wl.added_local, wl.pushed
                        ));
                    }
                    (Ok(r), Err(e)) => this.status = Some(format!("Watched sync ok ({}/{}), watchlist: {e}", r.pulled, r.pushed)),
                    (Err(e), Ok(wl)) => this.status = Some(format!("Watchlist +{} / push {}; watched: {e}", wl.added_local, wl.pushed)),
                    (Err(e), Err(e2)) => this.error = Some(format!("{e}; {e2}")),
                }
                this.reload(cx);
                this.load_watchlist(cx);
                this.load_trakt_lists(cx);
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn sync_watchlist_only(&mut self, cx: &mut Context<Self>) {
        self.busy = Some("Syncing watchlist…".into());
        cx.notify();
        let fut = self.svc.sync_watchlist();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(r) => this.status = Some(format!("Watchlist: +{} from Trakt, {} pushed", r.added_local, r.pushed)),
                    Err(e) => this.error = Some(e),
                }
                this.load_watchlist(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn trakt_watchlist_add(&mut self, kind: TitleKind, id: i32, cx: &mut Context<Self>) {
        let fut = self.svc.trakt_add_to_watchlist(kind, id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(()) => {
                        this.status = Some("Added to your Trakt watchlist".into());
                        this.load_trakt_lists(cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // --------------------------------------------------------------- settings

    pub(crate) fn save_settings(&mut self, cx: &mut Context<Self>) {
        let mut cfg = self.svc.config();
        let read = |e: &Entity<InputState>, cx: &Context<Self>| {
            let v = e.read(cx).value().trim().to_string();
            (!v.is_empty()).then_some(v)
        };
        let had_tmdb = self.svc.has_tmdb();
        cfg.tmdb_api_key = read(&self.tmdb_input, cx);
        cfg.real_debrid_api_key = read(&self.rd_input, cx);
        cfg.mpv_path = read(&self.mpv_input, cx);
        cfg.trakt_client_id = read(&self.trakt_id_input, cx);
        cfg.audio_lang = read(&self.alang_input, cx);
        cfg.sub_lang = read(&self.slang_input, cx);
        let mpv = cfg.mpv_path();
        match self.svc.save_config(cfg) {
            Ok(()) => {
                self.mpv_version = Player::mpv_version(&mpv);
                let wid = self.video.as_ref().map(|v| v.xid());
                self.player.set_config(Self::player_config(&self.svc, wid, self.volume));
                self.status = Some("Settings saved".into());
                if !had_tmdb && self.svc.has_tmdb() {
                    self.match_attempted = false;
                    self.load_trending(cx);
                    self.reload(cx);
                }
                self.refresh_trakt(cx);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Flip a boolean / enum setting and persist it
    pub(crate) fn update_config(&mut self, f: impl FnOnce(&mut rmh_app::AppConfig), restart_hint: bool, cx: &mut Context<Self>) {
        let mut cfg = self.svc.config();
        f(&mut cfg);
        match self.svc.save_config(cfg) {
            Ok(()) => {
                let wid = self.video.as_ref().map(|v| v.xid());
                self.player.set_config(Self::player_config(&self.svc, wid, self.volume));
                self.status = Some(if restart_hint { "Saved, restart Aurivon to apply" } else { "Saved" }.into());
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    // -------------------------------------------------------------- keyboard

    pub(crate) fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        if ks.key == "f11" {
            self.toggle_fullscreen(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.disc.search.open {
            if ks.key == "escape" {
                self.close_search(window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if ks.modifiers.control && ks.key == "k" {
            self.open_search(window, cx);
            cx.stop_propagation();
            return;
        }
        if ks.modifiers.control {
            let item = match ks.key.as_str() {
                "1" => Some(Nav::Page(Page::Home)),
                "2" => Some(Nav::Movies),
                "3" => Some(Nav::Series),
                "4" => Some(Nav::Page(Page::Calendar)),
                "5" => Some(Nav::Page(Page::Watchlist)),
                "6" => Some(Nav::Page(Page::Library)),
                "7" => Some(Nav::Page(Page::Player)),
                "8" => Some(Nav::Page(Page::Addons)),
                "9" | "," => Some(Nav::Page(Page::Settings)),
                _ => None,
            };
            if let Some(item) = item {
                self.nav_to(item, window, cx);
                cx.stop_propagation();
                return;
            }
            match ks.key.as_str() {
                // quick toggle between the three home layouts
                "l" => {
                    let cur = self.svc.config().home_layout;
                    let all = HomeLayout::ALL;
                    let i = all.iter().position(|l| *l == cur).unwrap_or(0);
                    self.set_home_layout(all[(i + 1) % all.len()], cx);
                    if self.page != Page::Home {
                        self.navigate(Page::Home, window, cx);
                    }
                    cx.stop_propagation();
                }
                // details for what's playing / the top Continue Watching item
                "i" => {
                    if let Some(id) = self.now_entry.as_ref().map(|e| e.item.id).or_else(|| self.resume_candidate()) {
                        self.open_entry(id, window, cx);
                        cx.stop_propagation();
                    }
                }
                _ => {}
            }
            return;
        }
        let root_focused = self.focus_handle.is_focused(window);
        if root_focused && !ks.modifiers.alt && !ks.modifiers.platform {
            if ks.key == "/" || ks.key_char.as_deref() == Some("/") {
                self.open_search(window, cx);
                cx.stop_propagation();
                return;
            }
            let handled = match self.page {
                Page::Discover => self.browse_key(ks.key.as_str(), ks.modifiers.shift, window, cx),
                Page::Calendar => self.calendar_key(ks.key.as_str(), ks.modifiers.shift, window, cx),
                Page::Library if self.svc.config().couch_mode => self.couch_library_key(ks.key.as_str(), window, cx),
                // hero carousel on the streaming home
                Page::Home if self.svc.config().home_layout.is_feed() && matches!(ks.key.as_str(), "left" | "right") => {
                    self.hero_step(if ks.key == "left" { -1 } else { 1 }, cx);
                    true
                }
                Page::Details if ks.key == "w" => {
                    self.details_toggle_watchlist(cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }
        // Enter on Home / Library resumes the top Continue Watching (or Up Next) item
        // (Library + couch mode uses couch_library_key for Enter instead)
        if ks.key == "enter"
            && matches!(self.page, Page::Home | Page::Library)
            && self.focus_handle.is_focused(window)
            && !(self.page == Page::Library && self.svc.config().couch_mode)
        {
            if let Some(id) = self.resume_candidate() {
                self.play_item(id, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if ks.key == "escape" {
            if self.theater {
                self.set_theater(false, window, cx);
            } else if self.app_fullscreen {
                self.app_fullscreen = false;
                self.sync_window_fullscreen(window);
                cx.notify();
            }
            window.focus(&self.focus_handle, cx);
            return;
        }
        if !self.focus_handle.is_focused(window) || self.now.is_none() {
            return;
        }
        let shift = ks.modifiers.shift;
        match ks.key.as_str() {
            "space" | "k" => self.toggle_pause(cx),
            "left" => self.seek_relative(if shift { -60.0 } else { -10.0 }, cx),
            "right" => self.seek_relative(if shift { 60.0 } else { 10.0 }, cx),
            "up" => self.nudge_volume(5.0, window, cx),
            "down" => self.nudge_volume(-5.0, window, cx),
            "," => self.frame_step(false, cx),
            "." => self.frame_step(true, cx),
            "[" => self.nudge_speed(false, cx),
            "]" => self.nudge_speed(true, cx),
            "backspace" => self.set_speed(1.0, cx),
            // same as mpv's default bindings: PgUp = next chapter, PgDn = previous
            "pageup" => self.step_chapter(1, cx),
            "pagedown" => self.step_chapter(-1, cx),
            "i" => self.skip_segment(cx),
            "a" => self.cycle_track(rmh_app::TrackKind::Audio, cx),
            "j" => self.cycle_track(rmh_app::TrackKind::Sub, cx),
            "z" => self.nudge_sub_delay(if shift { 0.1 } else { -0.1 }, cx),
            "x" => self.nudge_sub_delay(if shift { -0.1 } else { 0.1 }, cx),
            "t" => self.cycle_side_tab(cx),
            // search OpenSubtitles for what's playing
            "o" => {
                self.side_tab = SideTab::Subtitles;
                self.search_subtitles(false, cx);
            }
            "n" => self.play_next_now(window, cx),
            "f" => {
                if self.page != Page::Player {
                    self.navigate(Page::Player, window, cx);
                }
                let on = !self.theater;
                self.set_theater(on, window, cx);
            }
            "m" => {
                let muted = !self.snap.muted;
                let _ = self.player.set_muted(muted);
                self.snap.muted = muted;
            }
            "s" => self.stop(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl MediaHubView {
    /// What "resume" means right now: newest in-progress item, else the first Up Next episode
    pub(crate) fn resume_candidate(&self) -> Option<Uuid> {
        self.continue_entries()
            .first()
            .map(|e| e.item.id)
            .or_else(|| rmh_app::up_next(&self.entries).first().map(|e| e.item.id))
    }
}

fn chrono_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}


impl MediaHubView {
    /// Mirror Trakt scrobbles to Simkl / AniList
    fn social_scrobble(&self, item: &TraktItem, action: rmh_app::TraktAction, pct: f64) {
        let ev = match item {
            TraktItem::Movie { tmdb } => rmh_app::WatchEvent { kind: TitleKind::Movie, tmdb_id: *tmdb as i32, season: None, episode: None },
            TraktItem::Episode { show_tmdb, season, number } => {
                rmh_app::WatchEvent { kind: TitleKind::Tv, tmdb_id: *show_tmdb as i32, season: Some(*season), episode: Some(*number) }
            }
        };
        let action = match action {
            rmh_app::TraktAction::Start => rmh_app::SocialScrobble::Start,
            rmh_app::TraktAction::Pause => rmh_app::SocialScrobble::Pause,
            rmh_app::TraktAction::Stop => rmh_app::SocialScrobble::Stop,
        };
        self.svc.social_scrobble(action, ev, pct);
    }

    pub(crate) fn details_toggle_watchlist(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.details.as_ref() else { return };
        let Some(title) = d.details.as_ref().map(|x| x.title.clone()) else { return };
        let card = TitleCard { title, poster: d.poster.clone() };
        self.toggle_watchlist(&card, cx);
    }
}
