//! Main window: sidebar navigation, pages, now-playing bar.

use std::time::Duration;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{
    format_time, is_audio, resume_position, Library, MediaItem, MediaService, MovieHit, NowPlaying, Player,
    PlayerConfig, ProgressUpdate, StreamHit, TickOutcome,
};
use uuid::Uuid;

use crate::theme::{initials, tile_colors, Palette, DARK};

const SEEK_STEPS: f32 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Library,
    Discover,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Home, Page::Library, Page::Discover, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Library => "Library",
            Page::Discover => "Discover",
            Page::Settings => "Settings",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Home => "⌂",
            Page::Library => "▤",
            Page::Discover => "◎",
            Page::Settings => "⚙",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Page::Home => "Pick up where you left off",
            Page::Library => "Everything in your local folders",
            Page::Discover => "Find movies and stream them",
            Page::Settings => "Libraries, keys and player",
        }
    }
}

pub struct MediaHubView {
    svc: MediaService,
    player: Player,
    page: Page,
    libraries: Vec<Library>,
    media: Vec<MediaItem>,
    busy: Option<String>,
    status: Option<String>,
    error: Option<String>,

    path_input: Entity<InputState>,
    filter_input: Entity<InputState>,
    discover_input: Entity<InputState>,
    tmdb_input: Entity<InputState>,
    rd_input: Entity<InputState>,
    mpv_input: Entity<InputState>,

    movies: Vec<MovieHit>,
    selected_movie: Option<usize>,
    streams: Vec<StreamHit>,
    searching: bool,

    now: Option<NowPlaying>,
    seek_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    seek_dragging: bool,
    volume: f64,
    mpv_version: Option<String>,

    focus_handle: FocusHandle,
    _poller: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl MediaHubView {
    pub fn new(svc: MediaService, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = svc.config();
        let mpv_path = config.mpv_path();
        let volume = config.volume.unwrap_or(100.0).clamp(0.0, 130.0);
        let player = Player::new(PlayerConfig { mpv_path: mpv_path.clone(), volume: volume.min(100.0) as u8, ..Default::default() });

        let path_input = cx.new(|cx| InputState::new(window, cx).placeholder("Folder path, e.g. ~/Videos"));
        let filter_input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter library… (Enter plays the top match)"));
        let discover_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search movies by title, or paste an IMDb id (tt0133093)"));
        let tmdb_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("TMDB API key")
                .masked(true)
                .default_value(config.tmdb_api_key.clone().unwrap_or_default())
        });
        let rd_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Real-Debrid API key (optional)")
                .masked(true)
                .default_value(config.real_debrid_api_key.clone().unwrap_or_default())
        });
        let mpv_input = cx.new(|cx| InputState::new(window, cx).placeholder("mpv").default_value(mpv_path.clone()));

        let seek_slider = cx.new(|_| SliderState::new().min(0.0).max(SEEK_STEPS).step(1.0));
        let volume_slider =
            cx.new(|_| SliderState::new().min(0.0).max(130.0).step(1.0).default_value(volume as f32));

        let subscriptions = vec![
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
                // Enter plays the top match: type-to-play from the keyboard
                InputEvent::PressEnter { .. } => {
                    if let Some(id) = this.filtered_media(cx).first().map(|m| m.id) {
                        this.play_item(id, window, cx);
                    }
                }
                _ => {}
            }),
            cx.subscribe(&seek_slider, |this, _, ev: &SliderEvent, cx| match ev {
                SliderEvent::Change(_) => this.seek_dragging = true,
                SliderEvent::Release(v) => {
                    this.seek_dragging = false;
                    if let Some(d) = this.now.as_ref().and_then(|n| n.duration) {
                        let target = (v.start() / SEEK_STEPS) as f64 * d;
                        let _ = this.player.seek(target);
                        if let Some(n) = this.now.as_mut() {
                            n.position = target;
                        }
                    }
                    cx.notify();
                }
            }),
            cx.subscribe(&volume_slider, |this, _, ev: &SliderEvent, cx| {
                let v = match ev {
                    SliderEvent::Change(v) | SliderEvent::Release(v) => v.start() as f64,
                };
                this.set_volume(v, matches!(ev, SliderEvent::Release(_)), cx);
            }),
        ];

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
            libraries: Vec::new(),
            media: Vec::new(),
            busy: Some("Loading library…".into()),
            status: None,
            error: None,
            path_input,
            filter_input,
            discover_input,
            tmdb_input,
            rd_input,
            mpv_input,
            movies: Vec::new(),
            selected_movie: None,
            streams: Vec::new(),
            searching: false,
            now: None,
            seek_slider,
            volume_slider,
            seek_dragging: false,
            volume,
            focus_handle,
            _poller: poller,
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view
    }

    // ------------------------------------------------------------------ state

    fn notify_error(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.error = Some(msg.into());
        self.busy = None;
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let libs = self.svc.libraries();
        let media = self.svc.media();
        cx.spawn(async move |this, cx| {
            let libs = libs.await;
            let media = media.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match (libs, media) {
                    (Ok(l), Ok(m)) => {
                        this.libraries = l;
                        this.media = m;
                    }
                    (Err(e), _) | (_, Err(e)) => this.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

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

    fn add_library_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.path_input.read(cx).value().to_string();
        self.path_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.add_library(path, cx);
    }

    fn browse_folder(&mut self, cx: &mut Context<Self>) {
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
                Ok(Ok(None)) => {}
                Ok(Err(e)) => this.notify_error(
                    format!("Folder picker unavailable ({e}). Type the path instead."),
                    cx,
                ),
                Err(_) => {}
            })
            .ok();
        })
        .detach();
    }

    fn remove_library(&mut self, id: Uuid, cx: &mut Context<Self>) {
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

    fn scan_library(&mut self, lib: Library, cx: &mut Context<Self>) {
        self.busy = Some(format!("Scanning {}…", lib.name));
        cx.notify();
        let fut = self.svc.scan_library(&lib);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(n) => this.status = Some(format!("Scan of “{}” complete · {} new", lib.name, n)),
                    Err(e) => this.error = Some(e),
                }
                this.reload(cx);
            })
            .ok();
        })
        .detach();
    }

    fn save_progress(&mut self, update: Option<ProgressUpdate>, cx: &mut Context<Self>) {
        let Some(update) = update else { return };
        // Mirror locally so cards and resume reflect it immediately
        if let Some(item) = self.media.iter_mut().find(|m| m.id == update.media_id) {
            item.watch_progress = update.position;
            item.watched |= update.watched;
        }
        let fut = self.svc.save_progress(update);
        cx.spawn(async move |_, _| {
            if let Err(e) = fut.await {
                tracing::warn!("Failed to save progress: {e}");
            }
        })
        .detach();
    }

    // --------------------------------------------------------------- playback

    fn play_item(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.media.iter().find(|m| m.id == id).cloned() else { return };
        let previous = self.now.as_mut().and_then(|n| n.progress(false));
        self.save_progress(previous, cx);
        match self.player.play_at_position(std::path::Path::new(&item.file_path), resume_position(item.watch_progress)) {
            Ok(()) => {
                self.error = None;
                self.status = Some(format!("Playing {}", item.title));
                self.now = Some(NowPlaying::new(item.title.clone(), Some(id), self.volume));
            }
            Err(e) => self.error = Some(format!("Playback error: {e:#}")),
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn play_url(&mut self, url: String, title: String, cx: &mut Context<Self>) {
        let previous = self.now.as_mut().and_then(|n| n.progress(false));
        self.save_progress(previous, cx);
        match self.player.play_url(&url) {
            Ok(()) => {
                self.status = Some(format!("Streaming {title}"));
                self.now = Some(NowPlaying::new(title, None, self.volume));
            }
            Err(e) => self.error = Some(format!("Playback error: {e:#}")),
        }
        cx.notify();
    }

    fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(n) = self.now.as_mut() {
            let _ = self.player.toggle_pause();
            n.paused = !n.paused;
            cx.notify();
        }
    }

    fn seek_relative(&mut self, secs: f64, cx: &mut Context<Self>) {
        if self.now.is_some() {
            let _ = self.player.seek_relative(secs);
            cx.notify();
        }
    }

    fn set_volume(&mut self, v: f64, persist: bool, cx: &mut Context<Self>) {
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

    fn nudge_volume(&mut self, delta: f64, window: &mut Window, cx: &mut Context<Self>) {
        let v = (self.volume + delta).clamp(0.0, 130.0);
        self.volume_slider.update(cx, |s, cx| s.set_value(v as f32, window, cx));
        self.set_volume(v, true, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(mut n) = self.now.take() {
            let update = n.progress(false);
            self.save_progress(update, cx);
            self.status = Some(format!("Stopped {}", n.title));
        }
        let _ = self.player.stop();
        cx.notify();
    }

    fn on_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(now) = self.now.as_mut() else { return };
        let snap = self.player.snapshot();
        match now.tick(&snap) {
            TickOutcome::Continue => {}
            TickOutcome::Save(update) => self.save_progress(Some(update), cx),
            TickOutcome::Ended(update) => {
                let title = now.title.clone();
                self.now = None;
                self.save_progress(update, cx);
                self.status = Some(format!("Finished {title}"));
            }
            TickOutcome::Failed(err) => {
                self.now = None;
                let _ = self.player.stop();
                self.error = Some(err);
            }
        }
        if let Some(n) = &self.now {
            if !self.seek_dragging {
                let frac = n.duration.filter(|d| *d > 0.0).map(|d| (n.position / d).clamp(0.0, 1.0)).unwrap_or(0.0);
                self.seek_slider.update(cx, |s, cx| s.set_value(frac as f32 * SEEK_STEPS, window, cx));
            }
        }
        cx.notify();
    }

    // --------------------------------------------------------------- discover

    fn search_discover(&mut self, cx: &mut Context<Self>) {
        let query = self.discover_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            return;
        }
        self.searching = true;
        self.error = None;
        self.streams.clear();
        self.selected_movie = None;
        if query.starts_with("tt") && query[2..].chars().all(|c| c.is_ascii_digit()) {
            self.movies.clear();
            self.load_streams(query, cx);
            return;
        }
        let fut = self.svc.search_movies(query);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.searching = false;
                match res {
                    Ok(m) => {
                        this.status = Some(format!("{} movies found", m.len()));
                        this.movies = m;
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

    fn select_movie(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(movie) = self.movies.get(idx) else { return };
        let Some(imdb) = movie.imdb_id.clone() else {
            self.notify_error("No IMDb id for this title, can't look up streams", cx);
            return;
        };
        self.selected_movie = Some(idx);
        self.searching = true;
        self.streams.clear();
        self.load_streams(imdb, cx);
    }

    fn load_streams(&mut self, imdb: String, cx: &mut Context<Self>) {
        let fut = self.svc.search_streams(imdb);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.searching = false;
                match res {
                    Ok(s) => {
                        this.status = Some(format!("{} streams found", s.len()));
                        this.streams = s;
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

    fn play_stream(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(hit) = self.streams.get(idx).cloned() else { return };
        self.busy = Some(format!("Preparing stream: {}", hit.title));
        cx.notify();
        let fut = self.svc.resolve_stream(&hit);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.busy = None;
                match res {
                    Ok(r) => this.play_url(r.url, r.title, cx),
                    Err(e) => this.notify_error(e, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    // --------------------------------------------------------------- settings

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let mut cfg = self.svc.config();
        let read = |e: &Entity<InputState>, cx: &Context<Self>| {
            let v = e.read(cx).value().trim().to_string();
            (!v.is_empty()).then_some(v)
        };
        cfg.tmdb_api_key = read(&self.tmdb_input, cx);
        cfg.real_debrid_api_key = read(&self.rd_input, cx);
        cfg.mpv_path = read(&self.mpv_input, cx);
        let mpv = cfg.mpv_path();
        match self.svc.save_config(cfg) {
            Ok(()) => {
                self.mpv_version = Player::mpv_version(&mpv);
                let mut pc = self.player.get_config();
                pc.mpv_path = mpv;
                self.player.set_config(pc);
                self.status = Some("Settings saved".into());
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    // -------------------------------------------------------------- keyboard

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        if ks.modifiers.control {
            let page = match ks.key.as_str() {
                "1" => Some(Page::Home),
                "2" => Some(Page::Library),
                "3" => Some(Page::Discover),
                "," | "4" => Some(Page::Settings),
                _ => None,
            };
            if let Some(p) = page {
                self.navigate(p, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        if ks.key == "escape" {
            window.focus(&self.focus_handle, cx);
            return;
        }
        // Media keys only when no text field has focus
        if !self.focus_handle.is_focused(window) || self.now.is_none() {
            return;
        }
        match ks.key.as_str() {
            "space" => self.toggle_pause(cx),
            "left" => self.seek_relative(-10.0, cx),
            "right" => self.seek_relative(10.0, cx),
            "up" => self.nudge_volume(5.0, window, cx),
            "down" => self.nudge_volume(-5.0, window, cx),
            "s" => self.stop(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    // ------------------------------------------------------------------- view

    fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut nav = div().flex().flex_col().gap_1();
        for page in Page::ALL {
            let selected = self.page == page;
            nav = nav.child(
                div()
                    .id(format!("nav-{}", page.label()))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(if selected { p.active } else { p.sidebar }))
                    .text_color(rgb(if selected { p.text } else { p.muted }))
                    .hover(|s| s.bg(rgb(p.border_soft)).text_color(rgb(p.text)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.navigate(page, window, cx)))
                    .child(
                        div()
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(rgb(if selected { p.accent_soft } else { p.surface_raised }))
                            .text_color(rgb(if selected { p.accent } else { p.muted }))
                            .child(page.glyph()),
                    )
                    .child(div().flex_1().text_sm().child(page.label()))
                    .when(selected, |d| d.child(div().w(px(3.0)).h(px(14.0)).rounded_full().bg(rgb(p.accent)))),
            );
        }

        let watched = self.media.iter().filter(|m| m.watched).count();
        div()
            .w(px(232.0))
            .flex_none()
            .flex()
            .flex_col()
            .h_full()
            .px_4()
            .py_5()
            .gap_6()
            .bg(rgb(p.sidebar))
            .border_r_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_2()
                    .child(
                        div()
                            .size(px(36.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_lg()
                            .bg(linear_gradient(
                                135.0,
                                linear_color_stop(rgb(p.accent), 0.0),
                                linear_color_stop(rgb(0x4CC9F0), 1.0),
                            ))
                            .text_color(rgb(p.on_accent))
                            .font_weight(FontWeight::BOLD)
                            .child("▶"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("RustMediaHub"))
                            .child(div().text_xs().text_color(rgb(p.muted)).child("Media center")),
                    ),
            )
            .child(nav)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border_soft))
                    .text_xs()
                    .text_color(rgb(p.muted))
                    .child(format!("{} items · {} libraries", self.media.len(), self.libraries.len()))
                    .child(format!("{watched} watched"))
                    .child(
                        div()
                            .text_color(rgb(if self.mpv_version.is_some() { p.good } else { p.danger }))
                            .child(match &self.mpv_version {
                                Some(v) => format!("● {v}"),
                                None => "● mpv not found".to_string(),
                            }),
                    ),
            )
    }

    fn top_bar(&self, p: Palette) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_8()
            .py_5()
            .border_b_1()
            .border_color(rgb(p.border_soft))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(self.page.label()))
                    .child(div().text_sm().text_color(rgb(p.muted)).child(self.page.subtitle())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .when_some(self.busy.clone(), |d, b| {
                        d.child(div().size(px(8.0)).rounded_full().bg(rgb(p.warn))).child(b)
                    })
                    .when(self.busy.is_none(), |d| {
                        d.when_some(self.status.clone(), |d, s| {
                            d.child(div().size(px(8.0)).rounded_full().bg(rgb(p.good))).child(s)
                        })
                    }),
            )
    }

    fn button(&self, id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool, p: Palette) -> Stateful<Div> {
        let d = div()
            .id(id.into())
            .flex()
            .items_center()
            .justify_center()
            .px_4()
            .py_2()
            .rounded_lg()
            .text_sm()
            .cursor_pointer()
            .child(label.into());
        if primary {
            d.bg(rgb(p.accent))
                .text_color(rgb(p.on_accent))
                .font_weight(FontWeight::MEDIUM)
                .hover(|s| s.opacity(0.9))
        } else {
            d.bg(rgb(p.surface_raised))
                .border_1()
                .border_color(rgb(p.border))
                .text_color(rgb(p.text))
                .hover(|s| s.border_color(rgb(p.accent)))
        }
    }

    fn section_title(&self, title: impl Into<SharedString>, p: Palette) -> impl IntoElement {
        div().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.text)).child(title.into())
    }

    fn add_library_row(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(380.0)).child(Input::new(&self.path_input).w_full()))
            .child(
                self.button("add-lib", "Add folder", true, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.add_library_from_input(window, cx))),
            )
            .child(
                self.button("browse-lib", "Browse…", false, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| this.browse_folder(cx))),
            )
    }

    fn media_card(&self, item: &MediaItem, key: &str, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let id = item.id;
        let (c1, c2) = tile_colors(&item.title);
        let audio = is_audio(item);
        let playing = self.now.as_ref().and_then(|n| n.media_id) == Some(id);
        let progress = item
            .runtime_minutes
            .filter(|m| *m > 0)
            .map(|m| (item.watch_progress as f32 / (m as f32 * 60.0)).clamp(0.0, 1.0));
        div()
            .id(format!("card-{key}-{id}"))
            .w(px(176.0))
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.play_item(id, window, cx)))
            .child(
                div()
                    .relative()
                    .h(px(250.0))
                    .w_full()
                    .rounded_xl()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(if playing { p.accent } else { p.border_soft }))
                    .bg(linear_gradient(160.0, linear_color_stop(rgb(c1), 0.0), linear_color_stop(rgb(c2), 1.0)))
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgba(0xFFFFFFCC))
                            .child(if audio { "♪".to_string() } else { initials(&item.title) }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_2()
                            .left_2()
                            .px_2()
                            .py(px(2.0))
                            .rounded_md()
                            .bg(rgba(0x00000088))
                            .text_xs()
                            .text_color(rgb(p.text))
                            .child(if audio { "AUDIO" } else { "VIDEO" }),
                    )
                    .when(item.watched, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top_2()
                                .right_2()
                                .px_2()
                                .py(px(2.0))
                                .rounded_md()
                                .bg(rgb(p.good))
                                .text_xs()
                                .text_color(rgb(0x062014))
                                .child("✓ Watched"),
                        )
                    })
                    .when(playing, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_2()
                                .left_2()
                                .px_2()
                                .py(px(2.0))
                                .rounded_md()
                                .bg(rgb(p.accent))
                                .text_xs()
                                .text_color(rgb(p.on_accent))
                                .child("▶ Playing"),
                        )
                    })
                    .when_some(progress.filter(|_| !item.watched && item.watch_progress > 0), |d, frac| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left_0()
                                .w_full()
                                .h(px(4.0))
                                .bg(rgba(0xFFFFFF22))
                                .child(div().h_full().w(relative(frac)).bg(rgb(p.accent))),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().text_color(rgb(p.text)).truncate().child(item.title.clone()))
                    .child(
                        div().text_xs().text_color(rgb(p.muted)).child(if item.watch_progress > 0 && !item.watched {
                            format!("Resume at {}", format_time(item.watch_progress as f64))
                        } else {
                            human_size(item.file_size)
                        }),
                    ),
            )
    }

    fn grid(&self, items: &[&MediaItem], key: &str, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut g = div().flex().flex_wrap().gap_5();
        for item in items {
            g = g.child(self.media_card(item, key, p, cx));
        }
        g
    }

    fn empty_state(&self, title: &str, body: &str, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .py_16()
            .px_8()
            .rounded_2xl()
            .border_1()
            .border_color(rgb(p.border_soft))
            .bg(rgb(p.surface))
            .child(div().text_3xl().text_color(rgb(p.accent)).child("▤"))
            .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .child(div().text_sm().text_color(rgb(p.muted)).child(body.to_string()))
            .child(self.add_library_row(p, cx))
    }

    fn home_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.media.is_empty() {
            return self
                .empty_state("Welcome to RustMediaHub", "Add a folder with movies, shows or music to get started.", p, cx)
                .into_any_element();
        }
        let mut cont: Vec<&MediaItem> = self.media.iter().filter(|m| m.watch_progress > 0 && !m.watched).collect();
        cont.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        cont.truncate(10);
        let mut recent: Vec<&MediaItem> = self.media.iter().collect();
        recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        recent.truncate(12);

        div()
            .flex()
            .flex_col()
            .gap_8()
            .when(!cont.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(self.section_title("Continue watching", p))
                        .child(self.grid(&cont, "cont", p, cx)),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.section_title("Recently added", p))
                    .child(self.grid(&recent, "recent", p, cx)),
            )
            .into_any_element()
    }

    /// Library items matching the filter box, sorted by title
    fn filtered_media(&self, cx: &App) -> Vec<&MediaItem> {
        let filter = self.filter_input.read(cx).value().to_lowercase();
        let mut items: Vec<&MediaItem> = self
            .media
            .iter()
            .filter(|m| filter.is_empty() || m.title.to_lowercase().contains(&filter) || m.file_name.to_lowercase().contains(&filter))
            .collect();
        items.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
        items
    }

    fn library_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.media.is_empty() && self.libraries.is_empty() {
            return self
                .empty_state("Your library is empty", "Type a folder path or browse for one. It's scanned right away.", p, cx)
                .into_any_element();
        }
        let items = self.filtered_media(cx);
        let count = items.len();

        div()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(div().w(px(320.0)).child(Input::new(&self.filter_input).w_full()))
                    .child(self.add_library_row(p, cx)),
            )
            .child(div().text_sm().text_color(rgb(p.muted)).child(format!("{count} items")))
            .child(self.grid(&items, "lib", p, cx))
            .into_any_element()
    }

    fn discover_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div().flex().flex_col().gap_6().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().flex_1().child(Input::new(&self.discover_input).w_full()))
                .child(
                    self.button("discover-search", if self.searching { "Searching…" } else { "Search" }, true, p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.search_discover(cx))),
                ),
        );
        if !self.svc.has_tmdb() {
            col = col.child(
                div()
                    .p_4()
                    .rounded_lg()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border))
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .child("Title search needs a TMDB API key (Settings). IMDb ids (tt…) work without it."),
            );
        }

        if !self.movies.is_empty() {
            let mut list = div().flex().flex_wrap().gap_4();
            for (i, m) in self.movies.iter().enumerate().take(24) {
                let selected = self.selected_movie == Some(i);
                let (c1, c2) = tile_colors(&m.title);
                list = list.child(
                    div()
                        .id(format!("movie-{i}"))
                        .w(px(260.0))
                        .flex()
                        .gap_3()
                        .p_3()
                        .rounded_xl()
                        .cursor_pointer()
                        .bg(rgb(if selected { p.active } else { p.surface }))
                        .border_1()
                        .border_color(rgb(if selected { p.accent } else { p.border_soft }))
                        .hover(|s| s.border_color(rgb(p.accent)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.select_movie(i, cx)))
                        .child(
                            div()
                                .w(px(54.0))
                                .h(px(80.0))
                                .flex_none()
                                .rounded_md()
                                .bg(linear_gradient(160.0, linear_color_stop(rgb(c1), 0.0), linear_color_stop(rgb(c2), 1.0)))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(rgba(0xFFFFFFCC))
                                .font_weight(FontWeight::BOLD)
                                .child(initials(&m.title)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .min_w_0()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(m.title.clone()))
                                .child(div().text_xs().text_color(rgb(p.muted)).child(format!(
                                    "{}{}",
                                    m.year.map(|y| y.to_string()).unwrap_or_default(),
                                    m.rating.map(|r| format!(" · ★ {r:.1}")).unwrap_or_default()
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(p.faint))
                                        .line_clamp(2)
                                        .child(m.overview.clone().unwrap_or_default()),
                                ),
                        ),
                );
            }
            col = col.child(self.section_title("Movies", p)).child(list);
        }

        if self.searching && self.streams.is_empty() {
            col = col.child(div().text_sm().text_color(rgb(p.muted)).child("Looking for streams…"));
        }

        if !self.streams.is_empty() {
            let mut rows = div().flex().flex_col().gap_1();
            for (i, s) in self.streams.iter().enumerate().take(60) {
                rows = rows.child(
                    div()
                        .id(format!("stream-{i}"))
                        .flex()
                        .items_center()
                        .gap_4()
                        .px_4()
                        .py_3()
                        .rounded_lg()
                        .bg(rgb(p.surface))
                        .cursor_pointer()
                        .hover(|st| st.bg(rgb(p.surface_raised)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.play_stream(i, cx)))
                        .child(
                            div()
                                .w(px(64.0))
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(if s.rd_cached { p.good } else { p.muted }))
                                .child(if s.rd_cached { "⚡ RD" } else { "P2P" }),
                        )
                        .child(div().w(px(200.0)).text_sm().truncate().child(s.quality.clone()))
                        .child(div().w(px(90.0)).text_xs().text_color(rgb(p.muted)).child(s.size.clone().unwrap_or_default()))
                        .child(
                            div()
                                .w(px(70.0))
                                .text_xs()
                                .text_color(rgb(p.muted))
                                .child(s.seeders.map(|n| format!("▲ {n}")).unwrap_or_default()),
                        )
                        .child(div().flex_1().min_w_0().text_xs().text_color(rgb(p.faint)).truncate().child(s.title.clone()))
                        .child(div().text_sm().text_color(rgb(p.accent)).child("Play ▶")),
                );
            }
            col = col.child(self.section_title(format!("Streams ({})", self.streams.len()), p)).child(rows);
        }
        col.into_any_element()
    }

    fn card(&self, p: Palette) -> Div {
        div().flex().flex_col().gap_4().p_6().rounded_2xl().bg(rgb(p.surface)).border_1().border_color(rgb(p.border_soft))
    }

    fn settings_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut libs = div().flex().flex_col().gap_2();
        if self.libraries.is_empty() {
            libs = libs.child(div().text_sm().text_color(rgb(p.muted)).child("No libraries yet."));
        }
        for lib in &self.libraries {
            let (id, scan_lib) = (lib.id, lib.clone());
            let count = self.media.iter().filter(|m| m.file_path.starts_with(&lib.path)).count();
            libs = libs.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(p.surface_raised))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(lib.name.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(format!("{} · {count} items", lib.path))),
                    )
                    .child(
                        self.button(format!("scan-{id}"), "Rescan", false, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.scan_library(scan_lib.clone(), cx))),
                    )
                    .child(
                        self.button(format!("rm-{id}"), "Remove", false, p)
                            .text_color(rgb(p.danger))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.remove_library(id, cx))),
                    ),
            );
        }

        let field = |label: &'static str, hint: &'static str, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                .child(Input::new(input).w_full())
                .child(div().text_xs().text_color(rgb(p.faint)).child(hint))
        };

        div()
            .flex()
            .flex_col()
            .gap_6()
            .max_w(px(860.0))
            .child(
                self.card(p)
                    .child(self.section_title("Libraries", p))
                    .child(libs)
                    .child(self.add_library_row(p, cx)),
            )
            .child(
                self.card(p)
                    .child(self.section_title("Services", p))
                    .child(field("TMDB API key", "Used for title search in Discover. Stored in ~/.config/rustmediahub.", &self.tmdb_input))
                    .child(field("Real-Debrid API key", "Optional. Cached torrents then play instantly over HTTPS.", &self.rd_input))
                    .child(field("mpv executable", "Leave as mpv to use the one on PATH.", &self.mpv_input))
                    .child(
                        div().flex().child(
                            self.button("save-settings", "Save settings", true, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.save_settings(cx))),
                        ),
                    ),
            )
            .child(
                self.card(p)
                    .child(self.section_title("Status", p))
                    .child(status_line(
                        p,
                        self.mpv_version.is_some(),
                        match &self.mpv_version {
                            Some(v) => format!("Player: {v} (separate window, JSON IPC)"),
                            None => "Player: mpv not found. Install it: sudo apt install mpv".to_string(),
                        },
                    ))
                    .child(status_line(
                        p,
                        self.svc.has_tmdb(),
                        if self.svc.has_tmdb() { "TMDB: configured" } else { "TMDB: not configured" }.to_string(),
                    ))
                    .child(status_line(
                        p,
                        self.svc.has_real_debrid(),
                        if self.svc.has_real_debrid() { "Real-Debrid: configured" } else { "Real-Debrid: not configured (P2P only)" }
                            .to_string(),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.faint))
                            .child("Shortcuts: Space play/pause · ←/→ seek 10s · ↑/↓ volume · S stop · Ctrl+1–4 pages · Esc leaves a text field"),
                    ),
            )
            .into_any_element()
    }

    fn now_playing_bar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.now.as_ref().expect("checked by caller");
        let ctrl = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .size(px(36.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .text_sm()
                .text_color(rgb(p.text))
                .hover(|s| s.bg(rgb(p.surface_raised)))
                .child(label)
        };
        let starting = !n.seen_active;
        div()
            .flex()
            .items_center()
            .gap_5()
            .px_6()
            .py_3()
            .bg(rgb(p.sidebar))
            .border_t_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .w(px(240.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child({
                        let (c1, c2) = tile_colors(&n.title);
                        div()
                            .size(px(44.0))
                            .flex_none()
                            .rounded_md()
                            .bg(linear_gradient(160.0, linear_color_stop(rgb(c1), 0.0), linear_color_stop(rgb(c2), 1.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .child(initials(&n.title))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(n.title.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).child(if starting {
                                "Starting mpv…"
                            } else if n.paused {
                                "Paused"
                            } else {
                                "Now playing"
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(ctrl("np-back", "«10").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.seek_relative(-10.0, cx))))
                    .child(
                        div()
                            .id("np-play")
                            .size(px(42.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(rgb(p.text))
                            .text_color(rgb(p.canvas))
                            .hover(|s| s.opacity(0.9))
                            .child(if n.paused { "▶" } else { "❚❚" })
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.toggle_pause(cx))),
                    )
                    .child(ctrl("np-fwd", "10»").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.seek_relative(10.0, cx))))
                    .child(ctrl("np-stop", "■").on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.stop(cx)))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_xs().text_color(rgb(p.muted)).child(format_time(n.position)))
                    .child(div().flex_1().child(Slider::new(&self.seek_slider)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .child(n.duration.map(format_time).unwrap_or_else(|| "--:--".into())),
                    ),
            )
            .child(
                div()
                    .w(px(170.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().text_color(rgb(p.muted)).child(format!("Vol {:.0}", self.volume)))
                    .child(div().flex_1().child(Slider::new(&self.volume_slider))),
            )
    }
}

fn status_line(p: Palette, ok: bool, text: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .child(div().size(px(8.0)).rounded_full().bg(rgb(if ok { p.good } else { p.danger })))
        .child(text)
}

fn human_size(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

impl Render for MediaHubView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = DARK;
        // Keep keyboard shortcuts alive: if nothing has focus (startup, or the
        // focused input was removed), focus the shell.
        if window.focused(cx).is_none() {
            window.focus(&self.focus_handle, cx);
        }
        let content = match self.page {
            Page::Home => self.home_page(p, cx),
            Page::Library => self.library_page(p, cx),
            Page::Discover => self.discover_page(p, cx),
            Page::Settings => self.settings_page(p, cx),
        };

        let mut main = div()
            .flex_1()
            .flex()
            .flex_col()
            .h_full()
            .min_w_0()
            .child(self.top_bar(p))
            .child(div().flex_1().w_full().overflow_y_scrollbar().child(div().px_8().py_6().child(content)));
        if self.now.is_some() {
            main = main.child(self.now_playing_bar(p, cx));
        }

        let mut shell = div()
            .track_focus(&self.focus_handle)
            .key_context("MediaHub")
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .flex()
            .size_full()
            .bg(rgb(p.canvas))
            .text_color(rgb(p.text))
            .child(self.sidebar(p, cx))
            .child(main);

        if let Some(err) = self.error.clone() {
            shell = shell.child(
                div()
                    .id("error-toast")
                    .absolute()
                    .top_5()
                    .right_5()
                    .max_w(px(440.0))
                    .flex()
                    .items_start()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(p.danger))
                    .bg(rgb(p.surface_raised))
                    .text_sm()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.error = None;
                        cx.notify();
                    }))
                    .child(div().text_color(rgb(p.danger)).child("●"))
                    .child(div().flex_1().child(err))
                    .child(div().text_color(rgb(p.muted)).child("✕")),
            );
        }
        shell
    }
}
