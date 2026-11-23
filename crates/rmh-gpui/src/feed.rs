//! Streaming-first Home (the default "S" layout): a hero carousel of what's
//! trending, Continue watching / Up next, then discovery rows (trending,
//! popular, new on streaming, recommendations, genres, …). Rows only hit TMDB
//! once they scroll into view; posters are small and lazy; list responses are
//! cached on disk for 6 h by the metadata client. The local library is a small
//! row at the very bottom (hidden when empty).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{up_next, BrowseResults, DiscoverQuery, ListKind, ReleaseKind, TitleCard, TitleKind};

use crate::theme::Palette;
use crate::ui::{chip, entry_progress};
use crate::view::{MediaHubView, Page};

/// Hero slides (top of TMDB "trending this week" that have a backdrop)
const HERO_SLIDES: usize = 7;
/// Auto-advance interval of the hero carousel
const HERO_EVERY: Duration = Duration::from_millis(crate::theme::tokens::HERO_DWELL_MS);
/// Medium poster card width on Home rows
pub const FEED_CARD: f32 = 158.0;

/// Card geometry from Settings → Appearance (style + size, Compact forces small)
#[derive(Clone, Copy)]
pub struct CardDims {
    pub w: f32,
    pub h: f32,
    pub landscape: bool,
}
/// Rows loading at once
const MAX_ROW_LOADS: usize = 3;

#[derive(Clone)]
pub enum Src {
    /// Movies and series of one TMDB list, interleaved
    Mixed(ListKind),
    List(TitleKind, ListKind),
    Discover(DiscoverQuery),
    /// TMDB recommendations for the last thing watched
    Because,
    /// Episodes of the user's shows aired / airing this week
    NewEpisodes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Idle,
    Loading,
    Ready,
    Failed,
}

pub struct FeedRow {
    pub key: &'static str,
    pub title: String,
    pub sub: Option<String>,
    pub src: Src,
    pub cards: Vec<TitleCard>,
    /// Optional caption per card (episode rows: "S02E05 · Mon")
    pub captions: Vec<String>,
    pub state: RowState,
    /// Seed title of a "Because you watched" row
    pub seed: Option<TitleCard>,
}

impl FeedRow {
    fn new(key: &'static str, title: &str, sub: Option<&str>, src: Src) -> Self {
        Self {
            key,
            title: title.into(),
            sub: sub.map(Into::into),
            src,
            cards: vec![],
            captions: vec![],
            state: RowState::Idle,
            seed: None,
        }
    }
}

pub struct HomeFeed {
    pub rows: Vec<FeedRow>,
    pub hero_idx: usize,
    pub hero_at: Instant,
    pub hero_hover: bool,
    /// Rows whose placeholder was painted on screen since the last pump
    wanted: Rc<RefCell<Vec<usize>>>,
    scrolls: RefCell<HashMap<String, ScrollHandle>>,
}

impl Default for HomeFeed {
    fn default() -> Self {
        let movie = |genres: Vec<i32>| Src::Discover(DiscoverQuery { kind: TitleKind::Movie, genres, ..Default::default() });
        let tv = |genres: Vec<i32>| Src::Discover(DiscoverQuery { kind: TitleKind::Tv, genres, ..Default::default() });
        let rows = vec![
            FeedRow::new("new_eps", "New episodes this week", Some("shows you follow"), Src::NewEpisodes),
            FeedRow::new("trend_day", "Trending today", Some("movies & series"), Src::Mixed(ListKind::TrendingDay)),
            FeedRow::new("because", "Because you watched", None, Src::Because),
            FeedRow::new("pop_movies", "Popular movies", None, Src::List(TitleKind::Movie, ListKind::Popular)),
            FeedRow::new("pop_tv", "Popular series", None, Src::List(TitleKind::Tv, ListKind::Popular)),
            FeedRow::new(
                "digital",
                "New on streaming",
                Some("digital releases · last 60 days"),
                Src::Discover(DiscoverQuery { kind: TitleKind::Movie, digital_within_days: Some(60), ..Default::default() }),
            ),
            FeedRow::new("theaters", "Now in theaters", None, Src::List(TitleKind::Movie, ListKind::NowPlaying)),
            FeedRow::new("top_movies", "Top rated movies", None, Src::List(TitleKind::Movie, ListKind::TopRated)),
            FeedRow::new("top_tv", "Top rated series", None, Src::List(TitleKind::Tv, ListKind::TopRated)),
            FeedRow::new("g_action", "Action & adventure", None, movie(vec![28])),
            FeedRow::new("g_comedy", "Comedy", None, movie(vec![35])),
            FeedRow::new("g_scifi", "Sci-Fi & fantasy series", None, tv(vec![10765])),
            FeedRow::new("g_crime", "Crime series", None, tv(vec![80])),
            FeedRow::new("g_anim", "Animation", None, movie(vec![16])),
        ];
        Self {
            rows,
            hero_idx: 0,
            hero_at: Instant::now(),
            hero_hover: false,
            wanted: Rc::new(RefCell::new(vec![])),
            scrolls: RefCell::new(HashMap::new()),
        }
    }
}

impl HomeFeed {
    fn scroll(&self, key: &str) -> ScrollHandle {
        self.scrolls.borrow_mut().entry(key.to_string()).or_default().clone()
    }
}

/// (watched, progress 0..1) per TMDB title, from the library and Trakt
type ProgressMap = HashMap<(TitleKind, i32), (bool, Option<f32>)>;

fn interleave(a: Vec<TitleCard>, b: Vec<TitleCard>) -> Vec<TitleCard> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut a, mut b) = (a.into_iter(), b.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => break,
            (x, y) => out.extend(x.into_iter().chain(y)),
        }
    }
    out
}

impl MediaHubView {
    fn is_stream_home(&self) -> bool {
        self.page == Page::Home && self.svc.config().home_layout.is_feed()
    }

    /// Hero slides: trending this week with a backdrop
    pub(crate) fn hero_slides(&self) -> Vec<&TitleCard> {
        self.trending.iter().filter(|c| c.title.backdrop_path.is_some()).take(HERO_SLIDES).collect()
    }

    pub(crate) fn hero_step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.hero_slides().len();
        if n == 0 {
            return;
        }
        self.feed.hero_idx = (self.feed.hero_idx as i32 + delta).rem_euclid(n as i32) as usize;
        self.feed.hero_at = Instant::now();
        cx.notify();
    }

    /// Called from the 250 ms poller: rotate the hero, start rows that came into view
    pub(crate) fn pump_feed(&mut self, cx: &mut Context<Self>) {
        if !self.is_stream_home() {
            self.feed.wanted.borrow_mut().clear();
            return;
        }
        if !self.feed.hero_hover && self.feed.hero_at.elapsed() >= HERO_EVERY && self.hero_slides().len() > 1 {
            self.hero_step(1, cx);
        }
        let wanted: Vec<usize> = std::mem::take(&mut *self.feed.wanted.borrow_mut());
        let mut loading = self.feed.rows.iter().filter(|r| r.state == RowState::Loading).count();
        for i in wanted {
            if loading >= MAX_ROW_LOADS {
                break;
            }
            if self.feed.rows.get(i).is_some_and(|r| r.state == RowState::Idle) {
                self.load_feed_row(i, cx);
                loading += 1;
            }
        }
    }

    /// Most recent title the user watched (library history first, then Trakt)
    fn because_seed(&self) -> Option<TitleCard> {
        let mut hist: Vec<&rmh_app::LibraryEntry> =
            self.entries.iter().filter(|e| (e.item.watched || e.item.watch_progress > 0) && e.kind().is_some()).collect();
        hist.sort_by_key(|e| std::cmp::Reverse(e.item.updated_at));
        for e in hist {
            let (Some(kind), Some(id)) = (e.kind(), e.link.as_ref().and_then(|l| l.tmdb_id)) else { continue };
            let title = rmh_app::Title {
                kind,
                tmdb_id: id as i32,
                name: e.link.as_ref().and_then(|l| l.show_title.clone()).unwrap_or_else(|| e.display_title()),
                year: None,
                overview: None,
                poster_path: None,
                backdrop_path: None,
                rating: None,
                genre_ids: vec![],
                original_language: None,
                votes: None,
            };
            return Some(TitleCard { poster: e.poster(), title });
        }
        let playback: Vec<i64> = self.trakt_lists.playback.iter().filter_map(|e| e.tmdb_id()).collect();
        self.trakt_cards
            .iter()
            .find(|c| playback.contains(&(c.title.tmdb_id as i64)))
            .or_else(|| self.disc.watch_cards.first())
            .cloned()
    }

    fn load_feed_row(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(src) = self.feed.rows.get(i).map(|r| r.src.clone()) else { return };
        self.feed.rows[i].state = RowState::Loading;
        type Fut = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(Vec<TitleCard>, Vec<String>), String>>>>;
        let fut: Fut = match src {
            Src::Mixed(l) => {
                let a = self.svc.browse_list(TitleKind::Movie, l, 1);
                let b = self.svc.browse_list(TitleKind::Tv, l, 1);
                Box::pin(async move {
                    let (a, b) = (a.await, b.await);
                    if let (Err(e), Err(_)) = (&a, &b) {
                        return Err(e.clone());
                    }
                    let cards = |r: Result<BrowseResults, String>| r.map(|r| r.cards).unwrap_or_default();
                    Ok((interleave(cards(a), cards(b)), vec![]))
                })
            }
            Src::List(k, l) => {
                let f = self.svc.browse_list(k, l, 1);
                Box::pin(async move { f.await.map(|r| (r.cards, vec![])) })
            }
            Src::Discover(q) => {
                let f = self.svc.browse_discover(q);
                Box::pin(async move { f.await.map(|r| (r.cards, vec![])) })
            }
            Src::Because => {
                let Some(seed) = self.because_seed() else {
                    self.feed.rows[i].state = RowState::Ready;
                    return;
                };
                self.feed.rows[i].title = format!("Because you watched {}", seed.title.name);
                let f = self.svc.browse_recommendations(seed.title.kind, seed.title.tmdb_id);
                self.feed.rows[i].seed = Some(seed);
                Box::pin(async move { f.await.map(|r| (r.cards, vec![])) })
            }
            Src::NewEpisodes => {
                if !self.trakt.logged_in && !self.disc.watch_cards.iter().any(|c| c.title.kind == TitleKind::Tv) {
                    self.feed.rows[i].state = RowState::Ready;
                    return;
                }
                self.feed.rows[i].sub = Some(if self.trakt.logged_in { "from your Trakt calendar".into() } else { "shows on your watchlist".into() });
                let today = Local::now().date_naive();
                let months: Vec<(i32, u32)> = {
                    let prev = today - chrono::Duration::days(7);
                    let mut v = vec![(today.year(), today.month())];
                    if prev.month() != today.month() {
                        v.insert(0, (prev.year(), prev.month()));
                    }
                    v
                };
                let futs: Vec<_> = months.into_iter().map(|(y, m)| self.svc.calendar_month(y, m, false)).collect();
                let watch = self.disc.watch_ids.clone();
                let local = self.disc.local_ids.clone();
                Box::pin(async move {
                    let mut items = vec![];
                    for f in futs {
                        items.extend(f.await?.items);
                    }
                    let (from, to) = (today - chrono::Duration::days(7), today + chrono::Duration::days(3));
                    let mut eps: Vec<rmh_app::CalendarItem> = items
                        .into_iter()
                        .filter(|i| matches!(i.kind, ReleaseKind::Episode | ReleaseKind::Premiere))
                        .filter(|i| i.date >= from && i.date <= to)
                        .filter(|i| i.mine || i.tmdb_id.is_some_and(|id| watch.contains(&(i.title_kind, id)) || local.contains(&(i.title_kind, id))))
                        .filter(|i| i.title.is_some())
                        .collect();
                    // aired (newest first), then the next few days
                    eps.sort_by_key(|i| if i.date <= today { (0, -(i.date.num_days_from_ce() as i64)) } else { (1, i.date.num_days_from_ce() as i64) });
                    let mut seen = std::collections::HashSet::new();
                    let mut cards = vec![];
                    let mut caps = vec![];
                    for i in eps {
                        let Some(t) = i.title.clone() else { continue };
                        if !seen.insert(t.tmdb_id) {
                            continue;
                        }
                        let when = if i.date == today { "today".to_string() } else { i.date.format("%a %-d %b").to_string() };
                        let ep = i.detail.split(" · ").next().unwrap_or("").to_string();
                        caps.push(if ep.is_empty() { when } else { format!("{ep} · {when}") });
                        cards.push(TitleCard { poster: i.poster.clone(), title: t });
                    }
                    Ok((cards, caps))
                })
            }
        };
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                if let Some(row) = this.feed.rows.get_mut(i) {
                    match res {
                        Ok((cards, caps)) => {
                            row.cards = cards;
                            row.captions = caps;
                            row.state = RowState::Ready;
                        }
                        Err(e) => {
                            tracing::warn!("home row {}: {e}", row.key);
                            row.state = RowState::Failed;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "See all" for a row
    fn feed_see_all(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.feed.rows.get(i) else { return };
        match row.src.clone() {
            Src::Mixed(l) => self.browse_list_all(TitleKind::Movie, l, window, cx),
            Src::List(k, l) => self.browse_list_all(k, l, window, cx),
            Src::Discover(q) => self.browse_query_all(q, window, cx),
            Src::Because => {
                if let Some(seed) = row.seed.clone() {
                    self.open_title(&seed, None, window, cx);
                }
            }
            Src::NewEpisodes => self.navigate(Page::Calendar, window, cx),
        }
    }

    /// ▶ on the hero: movies go straight to "Play best", series open their episodes
    pub(crate) fn hero_play(&mut self, card: &TitleCard, window: &mut Window, cx: &mut Context<Self>) {
        if card.title.kind == TitleKind::Movie {
            self.pending_play = Some((card.title.kind, card.title.tmdb_id));
        }
        self.open_title(card, None, window, cx);
    }

    pub(crate) fn card_dims(&self) -> CardDims {
        let cfg = self.svc.config();
        let landscape = cfg.card_style == "landscape";
        let size_owned = self.effective_card_size();
        let size = if cfg.home_layout == rmh_app::HomeLayout::Compact {
            "s"
        } else {
            size_owned.as_str()
        };
        let w = match (landscape, size) {
            (false, "s") => 126.0,
            (false, "l") => 196.0,
            (false, _) => FEED_CARD,
            (true, "s") => 220.0,
            (true, "l") => 336.0,
            (true, _) => 272.0,
        };
        CardDims { w, h: if landscape { w * 0.5625 } else { w * 1.5 }, landscape }
    }

    fn progress_map(&self) -> ProgressMap {
        let mut m = ProgressMap::new();
        for e in &self.entries {
            let (Some(kind), Some(id)) = (e.kind(), e.link.as_ref().and_then(|l| l.tmdb_id)) else { continue };
            if kind != TitleKind::Movie {
                continue;
            }
            m.insert((kind, id as i32), (e.item.watched, entry_progress(e)));
        }
        for e in &self.trakt_lists.playback {
            let (Some(id), Some(pct)) = (e.tmdb_id(), e.progress) else { continue };
            let kind = if e.movie.is_some() { TitleKind::Movie } else { TitleKind::Tv };
            m.entry((kind, id as i32)).or_insert((false, Some((pct as f32 / 100.0).clamp(0.02, 1.0))));
        }
        m
    }

    // ----------------------------------------------------------------- render

    pub(crate) fn home_stream(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let cfg = self.svc.config();
        let prog = self.progress_map();
        let layout = cfg.home_layout;
        let mut col = div().flex().flex_col().when(layout == rmh_app::HomeLayout::Compact, |d| d.gap_5()).when(layout != rmh_app::HomeLayout::Compact, |d| d.gap_8());
        if layout != rmh_app::HomeLayout::Compact {
            col = col.child(self.hero_carousel(if layout == rmh_app::HomeLayout::Cinematic { 600.0 } else { 440.0 }, p, cx));
        }

        let mut did_watchlist = false;
        let mut did_trending = false;

        for key in &cfg.home_rows {
            match key.as_str() {
                "continue" => {
                    let mut items: Vec<AnyElement> =
                        self.continue_entries().iter().take(12).map(|e| self.wide_entry_card(e, "s-cont", p, cx)).collect();
                    // Fold Trakt playback into Continue when both are enabled (local first)
                    if self.trakt.logged_in && cfg.home_row_visible("trakt_playback") {
                        let local: std::collections::HashSet<i64> =
                            self.entries.iter().filter_map(|e| e.link.as_ref().and_then(|l| l.tmdb_id)).collect();
                        let playback: Vec<i64> = self.trakt_lists.playback.iter().filter_map(|e| e.tmdb_id()).collect();
                        for c in &self.trakt_cards {
                            let id = c.title.tmdb_id as i64;
                            if playback.contains(&id) && !local.contains(&id) {
                                items.push(self.feed_card(c, "s-tpb", Some("Continue · Trakt"), &prog, p, cx));
                            }
                        }
                    }
                    if !items.is_empty() {
                        let n = items.len();
                        col = col.child(self.feed_section(
                            "s-cont",
                            "Continue watching",
                            Some(format!("{n} in progress · local first, then Trakt")),
                            None,
                            items,
                            p,
                            cx,
                        ));
                    }
                }
                "trakt_playback" if !cfg.home_row_visible("continue") => {
                    if self.trakt.logged_in && !self.trakt_cards.is_empty() {
                        let local: std::collections::HashSet<i64> =
                            self.entries.iter().filter_map(|e| e.link.as_ref().and_then(|l| l.tmdb_id)).collect();
                        let playback: Vec<i64> = self.trakt_lists.playback.iter().filter_map(|e| e.tmdb_id()).collect();
                        let items: Vec<AnyElement> = self
                            .trakt_cards
                            .iter()
                            .filter(|c| {
                                let id = c.title.tmdb_id as i64;
                                playback.contains(&id) && !local.contains(&id)
                            })
                            .map(|c| self.feed_card(c, "s-tpb", Some("Continue · Trakt"), &prog, p, cx))
                            .collect();
                        if !items.is_empty() {
                            col = col.child(self.feed_section(
                                "s-tpb",
                                "Continue on Trakt",
                                Some("started elsewhere · stream it here".into()),
                                None,
                                items,
                                p,
                                cx,
                            ));
                        }
                    }
                }
                "trakt_playback" => {}
                "up_next" => {
                    let next = up_next(&self.entries);
                    if !next.is_empty() {
                        let items = next.iter().map(|e| self.entry_card(e, "s-next", 170.0, p, cx)).collect();
                        col = col.child(self.feed_section(
                            "s-next",
                            "Up next",
                            Some("next episode of shows you're watching".into()),
                            None,
                            items,
                            p,
                            cx,
                        ));
                    }
                }
                "watchlist" => {
                    did_watchlist = true;
                    col = col.when_some(self.watchlist_row(&prog, p, cx), |c, r| c.child(r));
                }
                "trending" => {
                    did_trending = true;
                    if self.trending.len() > HERO_SLIDES {
                        let items = self.trending.iter().map(|c| self.feed_card(c, "s-tw", None, &prog, p, cx)).collect();
                        col = col.child(self.feed_section("s-tw", "Trending this week", None, Some(usize::MAX), items, p, cx));
                    }
                }
                "library" => {
                    if !self.entries.is_empty() {
                        let w = self.poster_width(120.0);
                        let items = self.recent_entries().iter().take(14).map(|e| self.entry_card(e, "s-lib", w, p, cx)).collect();
                        col = col.child(self.feed_section(
                            "s-lib",
                            "On this device",
                            Some(format!("{} files in your folders", self.entries.len())),
                            Some(usize::MAX - 1),
                            items,
                            p,
                            cx,
                        ));
                    }
                }
                k if k.starts_with("smart:") => {
                    let entries = self.smart_collection_entries(k);
                    if !entries.is_empty() {
                        let title = rmh_app::smart_collection_builtins()
                            .iter()
                            .find(|(id, _)| *id == k)
                            .map(|(_, label)| *label)
                            .unwrap_or("Collection");
                        let w = self.poster_width(140.0);
                        let sid = format!("s-{k}");
                        let items = entries.iter().map(|e| self.entry_card(e, &sid, w, p, cx)).collect();
                        col = col.child(self.feed_section(
                            &sid,
                            title,
                            Some(format!("{} on this device", entries.len())),
                            None,
                            items,
                            p,
                            cx,
                        ));
                    }
                }
                k if k.starts_with("trakt_list:") => {
                    let id = &k["trakt_list:".len()..];
                    let cards = self.svc.cached_list_cards(id);
                    if !cards.is_empty() {
                        let name = self
                            .disc
                            .lists
                            .cache
                            .lists
                            .iter()
                            .find(|l| rmh_app::MediaService::user_list_id(l) == id)
                            .map(|l| l.name.clone())
                            .unwrap_or_else(|| id.to_string());
                        let sid = format!("s-tl-{id}");
                        let items = cards.iter().map(|c| self.feed_card(c, &sid, None, &prog, p, cx)).collect();
                        col = col.child(self.feed_section(
                            &sid,
                            &name,
                            Some(format!("{} titles · Trakt list", cards.len())),
                            None,
                            items,
                            p,
                            cx,
                        ));
                    }
                }
                _ => {}
            }
        }

        // Official Trakt catalogs addon rows (after configured rows)
        if self.trakt.logged_in && cfg.enable_trakt_catalogs {
            if !self.trakt_history_cards.is_empty() {
                let items = self.trakt_history_cards.iter().map(|c| self.feed_card(c, "s-th", Some("Watched on Trakt"), &prog, p, cx)).collect();
                col = col.child(self.feed_section("s-th", "Trakt history", Some("recently watched".into()), None, items, p, cx));
            }
            if !self.trakt_rec_cards.is_empty() {
                let items = self.trakt_rec_cards.iter().map(|c| self.feed_card(c, "s-tr", None, &prog, p, cx)).collect();
                col = col.child(self.feed_section("s-tr", "Trakt recommendations", Some("based on your history".into()), None, items, p, cx));
            }
        }

        if !self.svc.has_tmdb() {
            col = col.child(
                self.card(p)
                    .child(self.section_title("Add TMDB for discovery rows", p))
                    .child(div().text_sm().text_color(rgb(p.muted)).child(
                        "Add a free TMDB API key in Settings → Services to see trending, popular and new titles here.",
                    )),
            );
            if !did_watchlist {
                col = col.when_some(self.watchlist_row(&prog, p, cx), |c, r| c.child(r));
            }
        } else {
            for (i, row) in self.feed.rows.iter().enumerate() {
                if !did_watchlist && row.key == "g_action" {
                    col = col.when_some(self.watchlist_row(&prog, p, cx), |c, r| c.child(r));
                    did_watchlist = true;
                }
                if !did_trending && row.key == "pop_movies" && cfg.home_row_visible("trending") && self.trending.len() > HERO_SLIDES {
                    let items = self.trending.iter().map(|c| self.feed_card(c, "s-tw", None, &prog, p, cx)).collect();
                    col = col.child(self.feed_section("s-tw", "Trending this week", None, Some(usize::MAX), items, p, cx));
                    did_trending = true;
                }
                if let Some(el) = self.feed_row(i, row, &prog, p, cx) {
                    col = col.child(el);
                }
            }
        }

        col.into_any_element()
    }

    fn watchlist_row(&self, prog: &ProgressMap, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.svc.config().home_row_visible("watchlist") || self.disc.watch_cards.is_empty() {
            return None;
        }
        let items = self.disc.watch_cards.iter().map(|c| self.feed_card(c, "s-wl", None, prog, p, cx)).collect();
        let sub = format!("{} titles{}", self.disc.watch_cards.len(), if self.trakt.logged_in { " · synced with Trakt" } else { "" });
        Some(self.feed_section("s-wl", "Your watchlist", Some(sub), Some(usize::MAX - 2), items, p, cx))
    }

    fn feed_row(&self, i: usize, row: &FeedRow, prog: &ProgressMap, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        match row.state {
            RowState::Ready | RowState::Failed if row.cards.is_empty() => None,
            RowState::Idle | RowState::Loading => {
                // placeholder that asks for its data once it's (nearly) on screen
                let wanted = self.feed.wanted.clone();
                let sentinel = canvas(
                    move |b, window, _| {
                        let vp = window.viewport_size();
                        if b.origin.y < vp.height + px(400.0) && b.origin.y + b.size.height > px(-400.0) {
                            wanted.borrow_mut().push(i);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full();
                let dims = self.card_dims();
                let skel: Vec<AnyElement> = (0..9).map(|_| skeleton_card(dims, p)).collect();
                Some(
                    div()
                        .relative()
                        .child(self.feed_section(row.key, &row.title, row.sub.clone(), None, skel, p, cx))
                        .child(sentinel)
                        .into_any_element(),
                )
            }
            _ => {
                let items = row
                    .cards
                    .iter()
                    .enumerate()
                    .map(|(j, c)| self.feed_card(c, row.key, row.captions.get(j).map(|s| s.as_str()), prog, p, cx))
                    .collect();
                Some(self.feed_section(row.key, &row.title, row.sub.clone(), Some(i), items, p, cx))
            }
        }
    }

    /// Row header (title, subtitle, See all, ‹ ›) + horizontally scrolling body.
    /// `see_all`: feed row index, usize::MAX = trending week, MAX-1 = library, MAX-2 = watchlist
    #[allow(clippy::too_many_arguments)]
    fn feed_section(
        &self,
        key: &str,
        title: &str,
        sub: Option<String>,
        see_all: Option<usize>,
        items: Vec<AnyElement>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handle = self.feed.scroll(key);
        let arrow = |id: String, label: &'static str, dir: f32, handle: ScrollHandle| {
            div()
                .id(SharedString::from(id))
                .size(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .bg(rgb(p.surface))
                .border_1()
                .border_color(rgb(p.border_soft))
                .text_color(rgb(p.muted))
                .hover(|s| s.border_color(rgb(p.accent)).text_color(rgb(p.text)))
                .on_click(cx.listener(move |_this, _: &ClickEvent, _w, cx| {
                    let off = handle.offset();
                    let max = handle.max_offset();
                    let step = (f32::from(handle.bounds().size.width) * 0.8).max(320.0);
                    let x = (f32::from(off.x) - dir * step).clamp(-f32::from(max.x), 0.0);
                    handle.set_offset(point(px(x), off.y));
                    cx.notify();
                }))
                .child(label)
        };
        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .child(self.section_title(title.to_string(), p))
            .when_some(sub, |d, s| d.child(div().text_xs().text_color(rgb(p.muted)).child(s)))
            .child(div().flex_1())
            .when_some(see_all, |d, i| {
                d.child(
                    div()
                        .id(SharedString::from(format!("see-all-{key}")))
                        .px_2()
                        .text_xs()
                        .cursor_pointer()
                        .text_color(rgb(p.accent))
                        .hover(|s| s.text_color(rgb(p.text)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| match i {
                            usize::MAX => this.browse_list_all(TitleKind::Movie, ListKind::TrendingWeek, window, cx),
                            x if x == usize::MAX - 1 => this.navigate(Page::Library, window, cx),
                            x if x == usize::MAX - 2 => this.navigate(Page::Watchlist, window, cx),
                            i => this.feed_see_all(i, window, cx),
                        }))
                        .child(if i == usize::MAX - 1 { "Open library ›" } else { "See all ›" }),
                )
            })
            .child(arrow(format!("arl-{key}"), "‹", -1.0, handle.clone()))
            .child(arrow(format!("arr-{key}"), "›", 1.0, handle.clone()));
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(
                div()
                    .id(SharedString::from(format!("frow-{key}")))
                    .w_full()
                    .overflow_x_scroll()
                    .track_scroll(&handle)
                    .child(div().flex().gap_3().pt_1().pb_2().children(items)),
            )
            .into_any_element()
    }

    /// Poster card for Home rows: grows on hover, small lazy W185 poster,
    /// rating / watched / progress badges
    fn feed_card(&self, c: &TitleCard, key: &str, caption: Option<&str>, prog: &ProgressMap, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let card = c.clone();
        let wl_card = c.clone();
        let t = &c.title;
        let dims = self.card_dims();
        let w = dims.w;
        let avail = self.title_availability(t.kind, t.tmdb_id);
        let local = avail.local;
        let listed = avail.on_watchlist;
        let (watched, progress) = prog.get(&(t.kind, t.tmdb_id)).copied().unwrap_or((false, None));
        let gid = SharedString::from(format!("fc-{key}-{}-{}", t.kind.as_str(), t.tmdb_id));
        let wl_id = SharedString::from(format!("fwl-{key}-{}-{}", t.kind.as_str(), t.tmdb_id));
        let caption = caption.map(|s| s.to_string()).unwrap_or_else(|| {
            format!("{}{}", if t.kind == TitleKind::Tv { "Series" } else { "Movie" }, t.year.map(|y| format!(" · {y}")).unwrap_or_default())
        });
        // the image box is inset by 6 px and fills its slot on hover: the card
        // grows without pushing its neighbours around
        let inset = 6.0;
        div()
            .id(gid.clone())
            .group(gid.clone())
            .w(px(w))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_title(&card, None, window, cx)))
            .child(
                div().relative().w(px(w)).h(px(dims.h)).child(
                    div()
                        .absolute()
                        .top(px(inset))
                        .bottom(px(inset))
                        .left(px(inset))
                        .right(px(inset))
                        .rounded_xl()
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(p.border_soft))
                        .group_hover(gid.clone(), |s| s.top_0().bottom_0().left_0().right_0().border_color(rgb(p.accent)).shadow_lg())
                        .map(|d| {
                            let backdrop = dims.landscape.then(|| rmh_app::images::row_backdrop_url(t, w > 300.0)).flatten();
                            match backdrop {
                                Some(url) => d.child(self.lazy_poster(&t.name, None, Some(url), None)).child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .right_0()
                                        .bottom_0()
                                        .h(px(dims.h * 0.55))
                                        .bg(linear_gradient(0.0, linear_color_stop(rgba(0x000000D0), 0.0), linear_color_stop(rgba(0x00000000), 1.0)))
                                        .child(
                                            div()
                                                .absolute()
                                                .left_2()
                                                .right(px(40.0))
                                                .bottom_2()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0xFFFFFF))
                                                .truncate()
                                                .child(t.name.clone()),
                                        ),
                                ),
                                None => d.child(self.lazy_poster(&t.name, c.poster.clone(), rmh_app::images::row_poster_url(t), None)),
                            }
                        })
                        .when_some(t.rating.filter(|r| *r > 0.0), |d, r| {
                            d.child(div().absolute().top_1().right_1().child(chip(format!("★ {r:.1}"), 0x000000B0, p.gold)))
                        })
                        .when(watched, |d| d.child(div().absolute().top_1().left_1().child(chip("✓ Watched", 0x0F2A1EE6, p.good))))
                        .when(!watched && local, |d| d.child(div().absolute().top_1().left_1().child(chip("▶ Local", 0x0F2A1EE6, p.good))))
                        .child(
                            div()
                                .id(wl_id)
                                .absolute()
                                .bottom_2()
                                .right_2()
                                .size(px(26.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .cursor_pointer()
                                .bg(rgba(if listed { (p.accent << 8) | 0xEE } else { 0x000000AA }))
                                .text_color(rgb(if listed { p.on_accent } else { p.text }))
                                .text_sm()
                                .hover(|s| s.bg(rgb(p.accent)).text_color(rgb(p.on_accent)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                                    cx.stop_propagation();
                                    this.toggle_watchlist(&wl_card, cx);
                                }))
                                .child(if listed { "✓" } else { "+" }),
                        )
                        .when_some(progress, |d, f| {
                            d.child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .right_0()
                                    .h(px(4.0))
                                    .bg(rgba(0x00000099))
                                    .child(div().h_full().w(relative(f)).bg(rgb(p.accent))),
                            )
                        }),
                ),
            )
            .child(
                div()
                    .px(px(inset))
                    .flex()
                    .flex_col()
                    .child(div().text_sm().truncate().child(t.name.clone()))
                    .child(div().text_xs().truncate().text_color(rgb(p.muted)).child(caption)),
            )
            .into_any_element()
    }

    fn hero_carousel(&self, h: f32, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let slides = self.hero_slides();
        let big = h > 500.0;
        if slides.is_empty() {
            if !self.svc.has_tmdb() {
                return div().into_any_element();
            }
            return div().h(px(h)).w_full().rounded_2xl().bg(rgb(p.surface)).into_any_element();
        }
        let n = slides.len();
        let idx = self.feed.hero_idx.min(n - 1);
        let cur = slides[idx].clone();
        // backdrops: the current slide and the next one (prefetch)
        let url = rmh_app::images::hero_backdrop_url(&cur.title);
        if let Some(u) = &url {
            self.want_image(u);
        }
        if let Some(u) = slides.get((idx + 1) % n).and_then(|c| rmh_app::images::hero_backdrop_url(&c.title)) {
            self.want_image(&u);
        }
        let backdrop = url.as_deref().and_then(|u| self.lazy.get(u)).or_else(|| (idx == 0).then(|| self.hero_backdrop.clone()).flatten());
        let t = cur.title.clone();
        let listed = self.disc.watch_ids.contains(&(t.kind, t.tmdb_id));
        let (play, open, wl) = (cur.clone(), cur.clone(), cur.clone());
        let nav_btn = |id: &'static str, label: &'static str, delta: i32| {
            div()
                .id(id)
                .size(px(40.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .bg(rgba(0x00000066))
                .text_color(rgb(p.text))
                .text_lg()
                .hover(|s| s.bg(rgba(0x000000AA)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.hero_step(delta, cx)))
                .child(label)
        };
        let mut dots = div().flex().gap_2().items_center();
        for i in 0..n {
            dots = dots.child(
                div()
                    .id(SharedString::from(format!("hero-dot-{i}")))
                    .h(px(6.0))
                    .w(px(if i == idx { 22.0 } else { 6.0 }))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(rgba(if i == idx { 0xFFFFFFEE } else { 0xFFFFFF55 }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.feed.hero_idx = i;
                        this.feed.hero_at = Instant::now();
                        cx.notify();
                    })),
            );
        }
        let kind_label = if t.kind == TitleKind::Tv { "SERIES" } else { "MOVIE" };
        div()
            .id("hero")
            .relative()
            .h(px(h))
            .w_full()
            .rounded_2xl()
            .overflow_hidden()
            .bg(rgb(p.surface))
            .on_hover(cx.listener(|this, hovered: &bool, _w, _cx| this.feed.hero_hover = *hovered))
            .when_some(backdrop, |d, b| d.child(img(b).absolute().size_full().object_fit(ObjectFit::Cover)))
            .child(div().absolute().size_full().bg(linear_gradient(
                90.0,
                linear_color_stop(rgba((p.canvas << 8) | 0xF2), 0.0),
                linear_color_stop(rgba(p.canvas << 8), 0.7),
            )))
            .child(div().absolute().size_full().bg(linear_gradient(
                0.0,
                linear_color_stop(rgba((p.canvas << 8) | 0xE0), 0.0),
                linear_color_stop(rgba(p.canvas << 8), 0.45),
            )))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .p_10()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .max_w(px(if big { 760.0 } else { 680.0 }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(chip(format!("#{} TRENDING THIS WEEK", idx + 1), (p.accent << 8) | 0x55, p.text))
                            .child(chip(kind_label, 0x000000AA, p.text)),
                    )
                    .child(
                        div()
                            .when(big, |d| d.text_size(px(crate::theme::tokens::TYPE_DISPLAY + 8.0)))
                            .when(!big, |d| d.text_3xl())
                            .font_weight(FontWeight::BOLD)
                            .line_clamp(2)
                            .child(t.name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .items_center()
                            .text_sm()
                            .text_color(rgb(p.muted))
                            .when_some(t.rating.filter(|r| *r > 0.0), |d, r| d.child(div().text_color(rgb(p.gold)).child(format!("★ {r:.1}"))))
                            .when_some(t.year, |d, y| d.child(y.to_string()))
                            .when_some(t.votes.filter(|v| *v > 0), |d, v| d.child(format!("{} votes", crate::browse::fmt_count(v)))),
                    )
                    .child(div().text_sm().text_color(rgb(p.text)).line_clamp(3).child(t.overview.clone().unwrap_or_default()))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .pt_1()
                            .child(
                                self.button("hero-play", if t.kind == TitleKind::Movie { "▶  Play best" } else { "▶  Episodes" }, true, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.hero_play(&play, window, cx))),
                            )
                            .child(
                                self.button("hero-wl", if listed { "✓ On watchlist" } else { "+ Watchlist" }, false, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.toggle_watchlist(&wl, cx))),
                            )
                            .child(
                                self.button("hero-info", "ⓘ  Details", false, p)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_title(&open, None, window, cx))),
                            ),
                    ),
            )
            .child(div().absolute().left_3().top(px(h / 2.0 - 20.0)).child(nav_btn("hero-prev", "‹", -1)))
            .child(div().absolute().right_3().top(px(h / 2.0 - 20.0)).child(nav_btn("hero-next", "›", 1)))
            .child(div().absolute().right_8().bottom_6().child(dots))
            .into_any_element()
    }

}

fn skeleton_card(d: CardDims, p: Palette) -> AnyElement {
    div()
        .w(px(d.w))
        .flex_none()
        .flex()
        .flex_col()
        .gap_2()
        .p(px(6.0))
        .child(div().h(px(d.h - 12.0)).rounded_xl().bg(rgb(p.surface)))
        .child(div().h(px(12.0)).w(px(d.w * 0.6)).rounded_md().bg(rgb(p.surface)))
        .into_any_element()
}
