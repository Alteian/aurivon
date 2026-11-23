//! State + actions for discovery: global search, browse, person pages, title
//! extras, the release calendar, watchlist, subtitles search and the extra
//! services (Simkl / AniList). Rendering lives in `search`, `browse`,
//! `person`, `calendar` and `details`.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rmh_app::{
    search_library, BrowseSort, CalendarMonth, DiscoverQuery, Facets, ListKind, PersonHit, PersonPage, SearchHits, ServiceStatus,
    SimklPin, SubContext, SubtitleHit, TitleCard, TitleExtras, TitleKind,
};
use uuid::Uuid;

use crate::view::{MediaHubView, Page};

actions!(rmh, [SearchUp, SearchDown]);

/// Up/Down inside the search box move the selection instead of the caret
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SearchUp, Some("SearchBox > Input")),
        KeyBinding::new("down", SearchDown, Some("SearchBox > Input")),
        KeyBinding::new("ctrl-p", SearchUp, Some("SearchBox > Input")),
        KeyBinding::new("ctrl-n", SearchDown, Some("SearchBox > Input")),
    ]);
}

/// One row in the search overlay
#[derive(Clone)]
pub enum SearchItem {
    Local(Uuid),
    Title(TitleCard),
    Person(PersonHit),
    Imdb(String),
}

pub struct SearchState {
    pub open: bool,
    pub input: Entity<InputState>,
    pub query: String,
    pub hits: Option<SearchHits>,
    pub local: Vec<Uuid>,
    pub loading: bool,
    pub sel: usize,
    seq: u64,
    /// Focus the input on the next render (it may not exist yet)
    pub focus_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Genre,
    Year,
    Rating,
    Runtime,
    Language,
    Provider,
}

impl Facet {
    pub const ALL: [Facet; 6] = [Facet::Genre, Facet::Year, Facet::Rating, Facet::Runtime, Facet::Language, Facet::Provider];
    pub fn label(self) -> &'static str {
        match self {
            Facet::Genre => "Genre",
            Facet::Year => "Year",
            Facet::Rating => "Rating",
            Facet::Runtime => "Runtime",
            Facet::Language => "Language",
            Facet::Provider => "Streaming on",
        }
    }
}

pub const LANGUAGES: [(&str, &str); 12] = [
    ("en", "English"),
    ("cs", "Czech"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("de", "German"),
    ("it", "Italian"),
    ("da", "Danish"),
    ("sv", "Swedish"),
    ("hi", "Hindi"),
    ("zh", "Chinese"),
];

/// (label, from, to)
pub fn year_options() -> Vec<(String, Option<u16>, Option<u16>)> {
    let y = chrono::Local::now().year() as u16;
    vec![
        ("This year".into(), Some(y), Some(y)),
        ("Last 5 years".into(), Some(y - 4), Some(y)),
        ("2020s".into(), Some(2020), Some(2029)),
        ("2010s".into(), Some(2010), Some(2019)),
        ("2000s".into(), Some(2000), Some(2009)),
        ("1990s".into(), Some(1990), Some(1999)),
        ("1980s".into(), Some(1980), Some(1989)),
        ("Before 1980".into(), None, Some(1979)),
    ]
}
pub const RATINGS: [f32; 4] = [6.0, 7.0, 7.5, 8.0];
/// (label, min, max)
pub const RUNTIMES: [(&str, Option<u32>, Option<u32>); 4] =
    [("Under 30 min", None, Some(30)), ("Under 90 min", None, Some(90)), ("90–150 min", Some(90), Some(150)), ("Over 150 min", Some(150), None)];

pub struct BrowseState {
    pub kind: TitleKind,
    pub facets: Option<Facets>,
    /// Curated rows for `rows_kind`
    pub rows: Vec<(ListKind, Vec<TitleCard>)>,
    pub genre_rows: Vec<(i32, String, Vec<TitleCard>)>,
    pub rows_kind: Option<TitleKind>,
    pub query: DiscoverQuery,
    /// "See all" of a curated list (trending, popular, …) instead of /discover
    pub list: Option<ListKind>,
    pub results: Vec<TitleCard>,
    pub page: u32,
    pub total_pages: u32,
    pub total: u32,
    pub loading: bool,
    /// Keyboard selection in the results grid
    pub sel: Option<usize>,
    /// Filter panel open (keyboard: F)
    pub panel: Option<Facet>,
    pub opt_sel: usize,
    pub seq: u64,
}

impl BrowseState {
    pub fn filtered(&self) -> bool {
        self.list.is_some() || self.query.is_filtered() || self.query.sort != BrowseSort::Popularity
    }
}

pub struct PersonState {
    pub id: i32,
    pub name: String,
    pub loading: bool,
    pub page: Option<PersonPage>,
    pub dept: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalView {
    Month,
    Agenda,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalTypes {
    All,
    Movies,
    Series,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalRelease {
    All,
    Theatrical,
    Digital,
    Premiere,
}

/// Lists / Watchlist hub filters
pub struct ListsState {
    pub scope: rmh_app::ListScope,
    pub type_filter: rmh_app::ListTypeFilter,
    pub sort: rmh_app::ListSort,
    pub query: String,
    pub cache: rmh_app::ListsCache,
    pub cards: Vec<TitleCard>,
    pub loading: bool,
    pub new_name: Entity<InputState>,
}

pub struct CalendarState {
    pub year: i32,
    pub month: u32,
    pub data: Option<CalendarMonth>,
    pub loading: bool,
    pub view: CalView,
    pub mine_only: bool,
    pub types: CalTypes,
    pub release: CalRelease,
    pub sel_day: NaiveDate,
    /// Highlighted item of the selected day (Enter opens it)
    pub sel_item: usize,
    /// Watchlist changed: refetch (TMDB fallback only lists known shows)
    pub stale: bool,
}

pub struct SocialInputs {
    pub region: Entity<InputState>,
    pub os_key: Entity<InputState>,
    pub os_user: Entity<InputState>,
    pub os_pass: Entity<InputState>,
    pub simkl_id: Entity<InputState>,
    pub anilist_id: Entity<InputState>,
    pub anilist_token: Entity<InputState>,
    pub torbox: Entity<InputState>,
    pub alldebrid: Entity<InputState>,
    pub addon_url: Entity<InputState>,
}

pub struct Disc {
    pub search: SearchState,
    pub browse: BrowseState,
    pub person: Option<PersonState>,
    pub extras: Option<((TitleKind, i32), TitleExtras)>,
    pub extras_loading: bool,
    pub calendar: CalendarState,
    pub lists: ListsState,
    pub watch_cards: Vec<TitleCard>,
    pub watch_ids: HashSet<(TitleKind, i32)>,
    pub local_ids: HashSet<(TitleKind, i32)>,
    /// Trakt collection (digital “owned”) — not on-disk library
    pub collection_ids: HashSet<(TitleKind, i32)>,
    pub subs: Vec<SubtitleHit>,
    pub subs_loading: bool,
    pub subs_msg: Option<String>,
    /// File we already auto-searched subtitles for
    pub subs_auto_for: Option<String>,
    pub simkl: ServiceStatus,
    pub anilist: ServiceStatus,
    pub simkl_pin: Option<SimklPin>,
    pub inputs: SocialInputs,
    /// Columns of the browse grid at the current window width
    pub grid_cols: usize,
}

fn cli_search() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--search")?;
    args.get(i + 1).cloned()
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

impl Disc {
    pub fn new(svc: &rmh_app::MediaService, window: &mut Window, cx: &mut Context<MediaHubView>) -> (Self, Vec<Subscription>) {
        let cfg = svc.config();
        let input = |cx: &mut Context<MediaHubView>, window: &mut Window, ph: &str, val: String, masked: bool| {
            let ph = ph.to_string();
            cx.new(move |cx| InputState::new(window, cx).placeholder(ph).masked(masked).default_value(val))
        };
        // `aurivon --search "dune"` opens the app with the search overlay
        let initial = cli_search();
        let search_input = input(cx, window, "Search movies, series, people and your library…", initial.clone().unwrap_or_default(), false);
        let inputs = SocialInputs {
            region: input(cx, window, "CZ", cfg.region.clone(), false),
            os_key: input(cx, window, "OpenSubtitles API key", opt(&cfg.opensubtitles_api_key), true),
            os_user: input(cx, window, "OpenSubtitles username (optional)", opt(&cfg.opensubtitles_username), false),
            os_pass: input(cx, window, "OpenSubtitles password (optional)", opt(&cfg.opensubtitles_password), true),
            simkl_id: input(cx, window, "Simkl client id", opt(&cfg.simkl_client_id), true),
            anilist_id: input(cx, window, "AniList client id", opt(&cfg.anilist_client_id), false),
            anilist_token: input(cx, window, "Paste the AniList token here", String::new(), true),
            torbox: input(cx, window, "TorBox API key (optional)", opt(&cfg.torbox_api_key), true),
            alldebrid: input(cx, window, "AllDebrid API key (optional)", opt(&cfg.alldebrid_api_key), true),
            addon_url: input(cx, window, "https://…/manifest.json", String::new(), false),
        };
        let subs = vec![cx.subscribe_in(&search_input, window, |this: &mut MediaHubView, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::Change => this.on_search_change(cx),
            InputEvent::PressEnter { .. } => this.search_activate(window, cx),
            _ => {}
        })];
        let today = chrono::Local::now().date_naive();
        let kind_default = TitleKind::Movie;
        let disc = Disc {
            search: SearchState { open: initial.is_some(), input: search_input, query: String::new(), hits: None, local: vec![], loading: false, sel: 0, seq: 0, focus_pending: initial.is_some() },
            browse: BrowseState {
                kind: kind_default,
                facets: None,
                rows: vec![],
                genre_rows: vec![],
                rows_kind: None,
                query: DiscoverQuery { kind: kind_default, region: cfg.region.clone(), ..Default::default() },
                list: None,
                results: vec![],
                page: 0,
                total_pages: 0,
                total: 0,
                loading: false,
                sel: None,
                panel: None,
                opt_sel: 0,
                seq: 0,
            },
            person: None,
            extras: None,
            extras_loading: false,
            calendar: CalendarState {
                year: today.year(),
                month: today.month(),
                data: None,
                loading: false,
                view: CalView::Month,
                mine_only: false,
                types: CalTypes::All,
                release: CalRelease::All,
                sel_day: today,
                sel_item: 0,
                stale: false,
            },
            lists: ListsState {
                scope: rmh_app::ListScope::Watchlist,
                type_filter: rmh_app::ListTypeFilter::All,
                sort: rmh_app::ListSort::Added,
                query: String::new(),
                cache: svc.lists_cache(),
                cards: vec![],
                loading: false,
                new_name: input(cx, window, "New list name", String::new(), false),
            },
            watch_cards: vec![],
            watch_ids: svc.watchlist().iter().map(|w| (w.title.kind, w.title.tmdb_id)).collect(),
            local_ids: HashSet::new(),
            collection_ids: HashSet::new(),
            subs: vec![],
            subs_loading: false,
            subs_msg: None,
            subs_auto_for: None,
            simkl: ServiceStatus::default(),
            anilist: ServiceStatus::default(),
            simkl_pin: None,
            inputs,
            grid_cols: 6,
        };
        (disc, subs)
    }
}

impl MediaHubView {
    // ------------------------------------------------------------- search

    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disc.search.open = true;
        if let Some(v) = &self.video {
            v.hide();
        }
        self.disc.search.focus_pending = true;
        let input = self.disc.search.input.clone();
        input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disc.search.open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn on_search_change(&mut self, cx: &mut Context<Self>) {
        let q = self.disc.search.input.read(cx).value().trim().to_string();
        if q == self.disc.search.query {
            return;
        }
        self.disc.search.query = q.clone();
        self.disc.search.sel = 0;
        self.disc.search.local = search_library(&self.entries, &q).into_iter().take(5).map(|(e, _)| e.item.id).collect();
        self.disc.search.seq += 1;
        let seq = self.disc.search.seq;
        if q.chars().count() < 2 || !self.svc.has_tmdb() {
            self.disc.search.hits = None;
            self.disc.search.loading = false;
            cx.notify();
            return;
        }
        self.disc.search.loading = true;
        let svc = self.svc.clone();
        cx.spawn(async move |this, cx| {
            // debounce: only search once typing pauses
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let current = this.update(cx, |this, _| this.disc.search.seq == seq).unwrap_or(false);
            if !current {
                return;
            }
            let res = svc.search_everything(q).await;
            this.update(cx, |this, cx| {
                if this.disc.search.seq != seq {
                    return;
                }
                this.disc.search.loading = false;
                match res {
                    Ok(h) => this.disc.search.hits = Some(h),
                    Err(e) => tracing::warn!("search failed: {e}"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn search_items(&self) -> Vec<SearchItem> {
        self.search_groups().into_iter().map(|(_, i)| i).collect()
    }

    /// Rows of the search overlay with their group heading:
    /// library, (people when the query is a name), top results, movies, series, people
    pub(crate) fn search_groups(&self) -> Vec<(&'static str, SearchItem)> {
        let s = &self.disc.search;
        let mut out: Vec<(&'static str, SearchItem)> = s.local.iter().map(|id| ("In your library", SearchItem::Local(*id))).collect();
        let q = s.query.trim();
        if q.starts_with("tt") && q.len() > 3 && q[2..].chars().all(|c| c.is_ascii_digit()) {
            out.push(("IMDb id", SearchItem::Imdb(q.to_string())));
        }
        let Some(h) = &s.hits else { return out };
        let local: HashSet<(TitleKind, i32)> = s
            .local
            .iter()
            .filter_map(|id| self.entry(*id))
            .filter_map(|e| Some((e.kind()?, e.link.as_ref()?.tmdb_id? as i32)))
            .collect();
        let titles: Vec<&TitleCard> = h.titles.iter().filter(|c| !local.contains(&(c.title.kind, c.title.tmdb_id))).take(14).collect();
        let people: Vec<&PersonHit> = h.people.iter().take(4).collect();
        let best_title = titles.first().map(|c| rmh_app::fuzzy::score(q, &c.title.name)).unwrap_or(0.0);
        let best_person = people.first().map(|p| rmh_app::fuzzy::score(q, &p.card.name)).unwrap_or(0.0);
        let people_first = best_person > best_title + 0.02 || (best_person >= 0.9 && people.first().is_some_and(|p| p.card.department.as_deref() != Some("Acting")));
        if people_first {
            out.extend(people.iter().map(|p| ("People", SearchItem::Person((*p).clone()))));
        }
        let top = titles.len().min(3);
        out.extend(titles[..top].iter().map(|c| ("Top results", SearchItem::Title((*c).clone()))));
        out.extend(titles[top..].iter().filter(|c| c.title.kind == TitleKind::Movie).map(|c| ("Movies", SearchItem::Title((*c).clone()))));
        out.extend(titles[top..].iter().filter(|c| c.title.kind == TitleKind::Tv).map(|c| ("Series", SearchItem::Title((*c).clone()))));
        if !people_first {
            out.extend(people.iter().map(|p| ("People", SearchItem::Person((*p).clone()))));
        }
        out
    }

    pub(crate) fn search_move(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.search_items().len();
        if n == 0 {
            return;
        }
        let cur = self.disc.search.sel as i32 + delta;
        self.disc.search.sel = cur.rem_euclid(n as i32) as usize;
        cx.notify();
    }

    pub(crate) fn search_activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.search_items();
        let Some(item) = items.get(self.disc.search.sel).or(items.first()).cloned() else { return };
        self.open_search_item(item, window, cx);
    }

    pub(crate) fn open_search_item(&mut self, item: SearchItem, window: &mut Window, cx: &mut Context<Self>) {
        self.disc.search.open = false;
        window.focus(&self.focus_handle, cx);
        match item {
            SearchItem::Local(id) => self.open_entry(id, window, cx),
            SearchItem::Title(c) => self.open_title(&c, None, window, cx),
            SearchItem::Person(p) => self.open_person(p.card.id, p.card.name.clone(), window, cx),
            SearchItem::Imdb(id) => {
                self.navigate(Page::Discover, window, cx);
                self.load_streams(
                    id.clone(),
                    None,
                    crate::view::StreamContext { label: id, trakt: None, imdb: None, episode: None, tvdb_id: None },
                    cx,
                );
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------------- browse

    pub(crate) fn ensure_browse(&mut self, cx: &mut Context<Self>) {
        if !self.svc.has_tmdb() {
            return;
        }
        if self.disc.browse.facets.is_none() {
            let fut = self.svc.browse_facets();
            cx.spawn(async move |this, cx| {
                let res = fut.await;
                this.update(cx, |this, cx| {
                    match res {
                        Ok(f) => {
                            this.disc.browse.facets = Some(f);
                            this.load_genre_rows(cx);
                        }
                        Err(e) => tracing::warn!("facets: {e}"),
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        let kind = self.disc.browse.kind;
        if self.disc.browse.rows_kind != Some(kind) {
            self.disc.browse.rows_kind = Some(kind);
            self.disc.browse.rows.clear();
            self.disc.browse.genre_rows.clear();
            let lists: Vec<ListKind> = if kind == TitleKind::Movie {
                vec![ListKind::TrendingDay, ListKind::TrendingWeek, ListKind::Popular, ListKind::NowPlaying, ListKind::Upcoming, ListKind::TopRated]
            } else {
                vec![ListKind::TrendingDay, ListKind::TrendingWeek, ListKind::Popular, ListKind::NowPlaying, ListKind::Upcoming, ListKind::TopRated]
            };
            for (i, l) in lists.into_iter().enumerate() {
                self.disc.browse.rows.push((l, vec![]));
                let fut = self.svc.browse_list(kind, l, 1);
                cx.spawn(async move |this, cx| {
                    let res = fut.await;
                    this.update(cx, |this, cx| {
                        if this.disc.browse.rows_kind == Some(kind) {
                            if let (Ok(r), Some(row)) = (res, this.disc.browse.rows.get_mut(i)) {
                                row.1 = r.cards;
                            }
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            self.load_genre_rows(cx);
        }
    }

    fn load_genre_rows(&mut self, cx: &mut Context<Self>) {
        let kind = self.disc.browse.kind;
        let Some(f) = &self.disc.browse.facets else { return };
        if !self.disc.browse.genre_rows.is_empty() {
            return;
        }
        // a few popular genres as their own rows
        let wanted: &[i32] = if kind == TitleKind::Movie { &[28, 35, 878, 27, 16, 18] } else { &[10759, 35, 10765, 80, 16, 18] };
        let genres = if kind == TitleKind::Movie { &f.movie_genres } else { &f.tv_genres };
        let picks: Vec<(i32, String)> = wanted.iter().filter_map(|id| genres.iter().find(|g| g.id == *id).map(|g| (g.id, g.name.clone()))).collect();
        for (i, (gid, name)) in picks.into_iter().enumerate() {
            self.disc.browse.genre_rows.push((gid, name, vec![]));
            let q = DiscoverQuery { kind, genres: vec![gid], ..Default::default() };
            let fut = self.svc.browse_discover(q);
            cx.spawn(async move |this, cx| {
                let res = fut.await;
                this.update(cx, |this, cx| {
                    if this.disc.browse.rows_kind == Some(kind) {
                        if let (Ok(r), Some(row)) = (res, this.disc.browse.genre_rows.get_mut(i)) {
                            row.2 = r.cards;
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub(crate) fn set_browse_kind(&mut self, kind: TitleKind, cx: &mut Context<Self>) {
        if self.disc.browse.kind == kind {
            return;
        }
        let b = &mut self.disc.browse;
        b.kind = kind;
        b.query.kind = kind;
        // genre and provider ids differ between movies and series
        b.query.genres.clear();
        b.query.providers.clear();
        b.sel = None;
        self.ensure_browse(cx);
        if self.disc.browse.filtered() {
            self.run_discover(false, cx);
        }
        cx.notify();
    }

    /// Run (or extend with `more`) the filtered results
    pub(crate) fn run_discover(&mut self, more: bool, cx: &mut Context<Self>) {
        let b = &mut self.disc.browse;
        if !b.filtered() {
            b.results.clear();
            b.total = 0;
            cx.notify();
            return;
        }
        if more && b.page >= b.total_pages {
            return;
        }
        b.seq += 1;
        let seq = b.seq;
        let mut q = b.query.clone();
        q.kind = b.kind;
        q.page = if more { b.page + 1 } else { 1 };
        if !more {
            b.sel = None;
        }
        b.loading = true;
        let list = b.list;
        let fut: std::pin::Pin<Box<dyn std::future::Future<Output = Result<rmh_app::BrowseResults, String>>>> = match list {
            Some(l) => Box::pin(self.svc.browse_list(q.kind, l, q.page)),
            None => Box::pin(self.svc.browse_discover(q)),
        };
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let b = &mut this.disc.browse;
                if b.seq != seq {
                    return;
                }
                b.loading = false;
                match res {
                    Ok(r) => {
                        if !more {
                            b.results.clear();
                        }
                        b.results.extend(r.cards);
                        b.page = r.page;
                        b.total_pages = r.total_pages;
                        b.total = r.total_results;
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

    /// Options of a facet: (label, selected)
    pub(crate) fn facet_options(&self, f: Facet) -> Vec<(String, bool)> {
        let b = &self.disc.browse;
        let q = &b.query;
        match f {
            Facet::Genre => {
                let gs = b.facets.as_ref().map(|x| if b.kind == TitleKind::Movie { &x.movie_genres } else { &x.tv_genres });
                gs.map(|gs| gs.iter().map(|g| (g.name.clone(), q.genres.contains(&g.id))).collect()).unwrap_or_default()
            }
            Facet::Year => year_options().into_iter().map(|(l, a, z)| (l, q.year_from == a && q.year_to == z && (a.is_some() || z.is_some()))).collect(),
            Facet::Rating => RATINGS.iter().map(|r| (format!("★ {r}+"), q.min_rating == Some(*r))).collect(),
            Facet::Runtime => RUNTIMES.iter().map(|(l, a, z)| (l.to_string(), q.runtime_min == *a && q.runtime_max == *z)).collect(),
            Facet::Language => LANGUAGES.iter().map(|(c, n)| (n.to_string(), q.language.as_deref() == Some(*c))).collect(),
            Facet::Provider => {
                let ps = b.facets.as_ref().map(|x| if b.kind == TitleKind::Movie { &x.movie_providers } else { &x.tv_providers });
                ps.map(|ps| ps.iter().map(|(p, _)| (p.name.clone(), q.providers.contains(&p.id))).collect()).unwrap_or_default()
            }
        }
    }

    /// Toggle option `i` of a facet and refresh results
    pub(crate) fn toggle_facet(&mut self, f: Facet, i: usize, cx: &mut Context<Self>) {
        let b = &mut self.disc.browse;
        let q = &mut b.query;
        match f {
            Facet::Genre => {
                let gs = b.facets.as_ref().map(|x| if b.kind == TitleKind::Movie { x.movie_genres.clone() } else { x.tv_genres.clone() });
                if let Some(g) = gs.and_then(|gs| gs.get(i).cloned()) {
                    if let Some(pos) = q.genres.iter().position(|x| *x == g.id) {
                        q.genres.remove(pos);
                    } else {
                        q.genres.push(g.id);
                    }
                }
            }
            Facet::Year => {
                if let Some((_, a, z)) = year_options().get(i).cloned() {
                    if q.year_from == a && q.year_to == z {
                        (q.year_from, q.year_to) = (None, None);
                    } else {
                        (q.year_from, q.year_to) = (a, z);
                    }
                }
            }
            Facet::Rating => {
                if let Some(r) = RATINGS.get(i) {
                    q.min_rating = if q.min_rating == Some(*r) { None } else { Some(*r) };
                }
            }
            Facet::Runtime => {
                if let Some((_, a, z)) = RUNTIMES.get(i) {
                    if q.runtime_min == *a && q.runtime_max == *z {
                        (q.runtime_min, q.runtime_max) = (None, None);
                    } else {
                        (q.runtime_min, q.runtime_max) = (*a, *z);
                    }
                }
            }
            Facet::Language => {
                if let Some((c, _)) = LANGUAGES.get(i) {
                    q.language = if q.language.as_deref() == Some(*c) { None } else { Some(c.to_string()) };
                }
            }
            Facet::Provider => {
                let ps = b.facets.as_ref().map(|x| if b.kind == TitleKind::Movie { x.movie_providers.clone() } else { x.tv_providers.clone() });
                if let Some((p, _)) = ps.and_then(|ps| ps.get(i).cloned()) {
                    if let Some(pos) = q.providers.iter().position(|x| *x == p.id) {
                        q.providers.remove(pos);
                    } else {
                        q.providers.push(p.id);
                    }
                }
            }
        }
        self.disc.browse.list = None;
        self.run_discover(false, cx);
    }

    /// Summary of the active values of a facet, for its chip
    pub(crate) fn facet_summary(&self, f: Facet) -> Option<String> {
        let on: Vec<String> = self.facet_options(f).into_iter().filter(|(_, s)| *s).map(|(l, _)| l).collect();
        match on.len() {
            0 => None,
            1 => Some(on[0].clone()),
            n => Some(format!("{} +{}", on[0], n - 1)),
        }
    }

    pub(crate) fn set_sort(&mut self, s: BrowseSort, cx: &mut Context<Self>) {
        self.disc.browse.query.sort = s;
        self.disc.browse.list = None;
        self.run_discover(false, cx);
    }

    pub(crate) fn clear_filters(&mut self, cx: &mut Context<Self>) {
        let b = &mut self.disc.browse;
        b.query = DiscoverQuery { kind: b.kind, region: b.query.region.clone(), ..Default::default() };
        b.list = None;
        b.panel = None;
        self.run_discover(false, cx);
    }

    /// Jump to Browse with one genre selected (from the genre cloud / details)
    pub(crate) fn browse_genre(&mut self, kind: TitleKind, genre: i32, window: &mut Window, cx: &mut Context<Self>) {
        self.set_browse_kind(kind, cx);
        let b = &mut self.disc.browse;
        b.query.genres = vec![genre];
        b.list = None;
        self.navigate(Page::Discover, window, cx);
        self.run_discover(false, cx);
    }

    /// Browse grid of a whole curated list ("See all" on Home)
    pub(crate) fn browse_list_all(&mut self, kind: TitleKind, list: ListKind, window: &mut Window, cx: &mut Context<Self>) {
        self.set_browse_kind(kind, cx);
        let b = &mut self.disc.browse;
        b.query = DiscoverQuery { kind, region: b.query.region.clone(), ..Default::default() };
        b.list = Some(list);
        b.panel = None;
        self.navigate(Page::Discover, window, cx);
        self.run_discover(false, cx);
    }

    /// Browse grid for a /discover query ("See all" on a Home genre row)
    pub(crate) fn browse_query_all(&mut self, q: DiscoverQuery, window: &mut Window, cx: &mut Context<Self>) {
        self.set_browse_kind(q.kind, cx);
        let b = &mut self.disc.browse;
        let region = b.query.region.clone();
        b.query = DiscoverQuery { region, ..q };
        b.list = None;
        b.panel = None;
        self.navigate(Page::Discover, window, cx);
        self.run_discover(false, cx);
    }

    /// Movies / Series in the sidebar: the Browse home for that kind
    pub(crate) fn browse_kind_home(&mut self, kind: TitleKind, window: &mut Window, cx: &mut Context<Self>) {
        self.set_browse_kind(kind, cx);
        if self.disc.browse.list.take().is_some() {
            self.run_discover(false, cx);
        }
        self.navigate(Page::Discover, window, cx);
    }

    /// Arrow keys / Enter / W on the Browse page. Returns true when handled.
    pub(crate) fn browse_key(&mut self, key: &str, shift: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // filter panel open: ←/→ choose an option, Enter/Space toggles, F next facet, Esc closes
        if let Some(f) = self.disc.browse.panel {
            let n = self.facet_options(f).len().max(1);
            match key {
                "left" => self.disc.browse.opt_sel = (self.disc.browse.opt_sel + n - 1) % n,
                "right" => self.disc.browse.opt_sel = (self.disc.browse.opt_sel + 1) % n,
                "enter" | "space" => {
                    let i = self.disc.browse.opt_sel;
                    self.toggle_facet(f, i, cx);
                }
                "f" => self.cycle_panel(shift),
                "escape" | "up" | "down" => self.disc.browse.panel = None,
                _ => return false,
            }
            cx.notify();
            return true;
        }
        match key {
            "f" => self.cycle_panel(shift),
            "o" => {
                let all = BrowseSort::ALL;
                let i = all.iter().position(|s| *s == self.disc.browse.query.sort).unwrap_or(0);
                let next = all[(i + if shift { all.len() - 1 } else { 1 }) % all.len()];
                self.set_sort(next, cx);
            }
            "v" => {
                let k = if self.disc.browse.kind == TitleKind::Movie { TitleKind::Tv } else { TitleKind::Movie };
                self.set_browse_kind(k, cx);
            }
            "x" => self.clear_filters(cx),
            "l" => self.run_discover(true, cx),
            "left" | "right" | "up" | "down" => {
                let n = self.disc.browse.results.len();
                if n == 0 {
                    return false;
                }
                let cols = self.disc.grid_cols.max(1) as i64;
                let cur = self.disc.browse.sel.map(|s| s as i64);
                let next = match (cur, key) {
                    (None, _) => 0,
                    (Some(c), "left") => c - 1,
                    (Some(c), "right") => c + 1,
                    (Some(c), "up") => c - cols,
                    (Some(c), _) => c + cols,
                };
                let next = next.clamp(0, n as i64 - 1) as usize;
                self.disc.browse.sel = Some(next);
                // near the end: fetch the next page
                if next + cols as usize >= n {
                    self.run_discover(true, cx);
                }
            }
            "enter" => {
                let Some(c) = self.disc.browse.sel.and_then(|i| self.disc.browse.results.get(i)).cloned() else { return false };
                self.open_title(&c, None, window, cx);
            }
            "w" => {
                let Some(c) = self.disc.browse.sel.and_then(|i| self.disc.browse.results.get(i)).cloned() else { return false };
                self.toggle_watchlist(&c, cx);
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn cycle_panel(&mut self, back: bool) {
        let all = Facet::ALL;
        let b = &mut self.disc.browse;
        b.opt_sel = 0;
        b.panel = match b.panel {
            None => Some(if back { all[all.len() - 1] } else { all[0] }),
            Some(f) => {
                let i = all.iter().position(|x| *x == f).unwrap_or(0) as i64 + if back { -1 } else { 1 };
                (0..all.len() as i64).contains(&i).then(|| all[i as usize])
            }
        };
    }

    // ------------------------------------------------------------- watchlist

    pub(crate) fn toggle_watchlist(&mut self, c: &TitleCard, cx: &mut Context<Self>) {
        match self.svc.toggle_watchlist(&c.title) {
            Ok(added) => {
                let key = (c.title.kind, c.title.tmdb_id);
                if added {
                    // a new series may have episodes the TMDB-based calendar doesn't know yet
                    let known = self.disc.calendar.data.as_ref().is_some_and(|d| d.items.iter().any(|i| i.tmdb_id == Some(key.1)));
                    if key.0 == TitleKind::Tv && !self.trakt.logged_in && !known {
                        self.disc.calendar.stale = true;
                    }
                    self.disc.watch_ids.insert(key);
                    self.disc.watch_cards.insert(0, c.clone());
                    let mut where_ = vec!["your watchlist"];
                    if self.trakt.logged_in {
                        where_.push("Trakt");
                    }
                    if self.disc.simkl.logged_in {
                        where_.push("Simkl");
                    }
                    self.status = Some(format!("Added {} to {}", c.title.name, where_.join(" + ")));
                } else {
                    self.disc.watch_ids.remove(&key);
                    self.disc.watch_cards.retain(|w| (w.title.kind, w.title.tmdb_id) != key);
                    self.status = Some(format!("Removed {} from your watchlist", c.title.name));
                }
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    pub(crate) fn load_watchlist(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.watchlist_cards();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                if let Ok(c) = res {
                    this.disc.watch_ids = c.iter().map(|c| (c.title.kind, c.title.tmdb_id)).collect();
                    this.disc.watch_cards = c;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ------------------------------------------------------------- person / extras

    pub(crate) fn open_person(&mut self, id: i32, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.disc.person = Some(PersonState { id, name, loading: true, page: None, dept: None });
        self.navigate(Page::Person, window, cx);
        let fut = self.svc.person_page(id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                if let Some(p) = this.disc.person.as_mut().filter(|p| p.id == id) {
                    p.loading = false;
                    match res {
                        Ok(page) => {
                            p.name = page.person.name.clone();
                            p.page = Some(page);
                        }
                        Err(e) => this.error = Some(e),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn load_extras(&mut self, kind: TitleKind, id: i32, cx: &mut Context<Self>) {
        if self.disc.extras.as_ref().is_some_and(|(k, _)| *k == (kind, id)) {
            return;
        }
        self.disc.extras = None;
        self.disc.extras_loading = true;
        let fut = self.svc.title_extras(kind, id);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let current = this.details.as_ref().is_some_and(|d| d.tmdb_id == Some(id));
                if current {
                    this.disc.extras_loading = false;
                    match res {
                        Ok(x) => this.disc.extras = Some(((kind, id), x)),
                        Err(e) => tracing::warn!("extras: {e}"),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ------------------------------------------------------------- calendar

    pub(crate) fn load_calendar(&mut self, force: bool, cx: &mut Context<Self>) {
        if !self.svc.has_tmdb() && !self.trakt.logged_in {
            return;
        }
        let (y, m) = (self.disc.calendar.year, self.disc.calendar.month);
        let force = force || std::mem::take(&mut self.disc.calendar.stale);
        if !force && self.disc.calendar.data.as_ref().is_some_and(|d| d.year == y && d.month == m) {
            return;
        }
        self.disc.calendar.loading = true;
        let fut = self.svc.calendar_month(y, m, force);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let c = &mut this.disc.calendar;
                if c.year == y && c.month == m {
                    c.loading = false;
                    match res {
                        Ok(d) => c.data = Some(d),
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

    pub(crate) fn calendar_shift(&mut self, months: i32, cx: &mut Context<Self>) {
        let c = &mut self.disc.calendar;
        let idx = c.year * 12 + c.month as i32 - 1 + months;
        c.year = idx.div_euclid(12);
        c.month = (idx.rem_euclid(12) + 1) as u32;
        let today = chrono::Local::now().date_naive();
        c.sel_day = if today.year() == c.year && today.month() == c.month {
            today
        } else {
            NaiveDate::from_ymd_opt(c.year, c.month, 1).unwrap_or(today)
        };
        c.sel_item = 0;
        self.load_calendar(false, cx);
        cx.notify();
    }

    pub(crate) fn calendar_today(&mut self, cx: &mut Context<Self>) {
        let today = chrono::Local::now().date_naive();
        let c = &mut self.disc.calendar;
        let months = (today.year() * 12 + today.month() as i32) - (c.year * 12 + c.month as i32);
        self.calendar_shift(months, cx);
    }

    /// Items passing the calendar filters
    pub(crate) fn calendar_items(&self) -> Vec<rmh_app::CalendarItem> {
        let c = &self.disc.calendar;
        let Some(d) = &c.data else { return vec![] };
        let mine = |i: &rmh_app::CalendarItem| {
            i.mine || i.tmdb_id.is_some_and(|id| self.disc.watch_ids.contains(&(i.title_kind, id)) || self.disc.local_ids.contains(&(i.title_kind, id)))
        };
        d.items
            .iter()
            .filter(|i| !c.mine_only || mine(i))
            .filter(|i| match c.types {
                CalTypes::All => true,
                CalTypes::Movies => i.kind.is_movie(),
                CalTypes::Series => !i.kind.is_movie(),
            })
            .filter(|i| match c.release {
                CalRelease::All => true,
                CalRelease::Theatrical => i.kind == rmh_app::ReleaseKind::Theatrical || !i.kind.is_movie(),
                CalRelease::Digital => i.kind == rmh_app::ReleaseKind::Digital || !i.kind.is_movie(),
                CalRelease::Premiere => i.kind == rmh_app::ReleaseKind::Premiere || i.kind == rmh_app::ReleaseKind::Episode,
            })
            .cloned()
            .map(|mut i| {
                i.mine = mine(&i);
                i
            })
            .collect()
    }

    pub(crate) fn open_calendar_item(&mut self, it: &rmh_app::CalendarItem, window: &mut Window, cx: &mut Context<Self>) {
        let card = match &it.title {
            Some(t) => TitleCard { title: t.clone(), poster: it.poster.clone() },
            None => return,
        };
        self.open_title(&card, None, window, cx);
        // series: jump straight to the season of that episode
        if !it.kind.is_movie() {
            let season = it.detail.strip_prefix('S').and_then(|r| r.split('E').next()).and_then(|n| n.parse::<u32>().ok());
            if let (Some(s), Some(d)) = (season, self.details.as_mut()) {
                d.season = Some(s);
            }
        }
    }

    /// Calendar keys: arrows move the day, PgUp/PgDn month, V view, M mine, T today, Enter opens, W watchlist
    pub(crate) fn calendar_key(&mut self, key: &str, shift: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let c = &mut self.disc.calendar;
        let day = |d: NaiveDate, n: i64| d + chrono::Duration::days(n);
        match key {
            "left" | "right" | "up" | "down" if c.view == CalView::Month || key == "left" || key == "right" => {
                let n = match key {
                    "left" => -1,
                    "right" => 1,
                    "up" => -7,
                    _ => 7,
                };
                c.sel_day = day(c.sel_day, n);
                c.sel_item = 0;
                if c.sel_day.month() != c.month || c.sel_day.year() != c.year {
                    let target = c.sel_day;
                    let months = (target.year() * 12 + target.month() as i32) - (c.year * 12 + c.month as i32);
                    c.year = target.year();
                    c.month = target.month();
                    let _ = months;
                    self.load_calendar(false, cx);
                    self.disc.calendar.sel_day = target;
                }
            }
            "up" | "down" => {
                // agenda: walk items of the selected day, then neighbouring days with releases
                let items = self.calendar_items();
                let c = &mut self.disc.calendar;
                let today_items = items.iter().filter(|i| i.date == c.sel_day).count();
                if key == "down" && c.sel_item + 1 < today_items {
                    c.sel_item += 1;
                } else if key == "up" && c.sel_item > 0 {
                    c.sel_item -= 1;
                } else {
                    let next = if key == "down" {
                        items.iter().map(|i| i.date).find(|d| *d > c.sel_day)
                    } else {
                        items.iter().map(|i| i.date).filter(|d| *d < c.sel_day).last()
                    };
                    if let Some(d) = next {
                        c.sel_day = d;
                        c.sel_item = if key == "down" { 0 } else { items.iter().filter(|i| i.date == d).count().saturating_sub(1) };
                    }
                }
            }
            "pageup" => self.calendar_shift(-1, cx),
            "pagedown" => self.calendar_shift(1, cx),
            "v" => c.view = if c.view == CalView::Month { CalView::Agenda } else { CalView::Month },
            "m" => c.mine_only = !c.mine_only,
            "t" => self.calendar_today(cx),
            "r" => self.load_calendar(true, cx),
            "y" => {
                c.types = match c.types {
                    CalTypes::All => CalTypes::Movies,
                    CalTypes::Movies => CalTypes::Series,
                    CalTypes::Series => CalTypes::All,
                }
            }
            "d" => {
                c.release = match (c.release, shift) {
                    (CalRelease::All, _) => CalRelease::Theatrical,
                    (CalRelease::Theatrical, _) => CalRelease::Digital,
                    (CalRelease::Digital, _) => CalRelease::Premiere,
                    (CalRelease::Premiere, _) => CalRelease::All,
                }
            }
            "enter" | "w" => {
                let sel = c.sel_item;
                let day = c.sel_day;
                let Some(it) = self.calendar_items().into_iter().filter(|i| i.date == day).nth(sel) else { return false };
                if key == "enter" {
                    self.open_calendar_item(&it, window, cx);
                } else if let Some(t) = it.title.clone() {
                    self.toggle_watchlist(&TitleCard { title: t, poster: it.poster.clone() }, cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    // ------------------------------------------------------------- subtitles

    fn sub_context(&self) -> SubContext {
        let e = self.now_entry.as_ref();
        let link = e.and_then(|e| e.link.as_ref());
        let is_tv = link.is_some_and(|l| l.kind == "tv");
        SubContext {
            file: e.map(|e| std::path::PathBuf::from(&e.item.file_path)),
            tmdb_id: link.and_then(|l| l.tmdb_id).map(|t| t as i32),
            season: if is_tv { link.and_then(|l| l.season).map(|s| s as u32) } else { None },
            episode: if is_tv { link.and_then(|l| l.episode).map(|s| s as u32) } else { None },
            query: if link.and_then(|l| l.tmdb_id).is_none() { self.now.as_ref().map(|n| n.title.clone()) } else { None },
        }
    }

    pub(crate) fn search_subtitles(&mut self, auto: bool, cx: &mut Context<Self>) {
        if !self.svc.has_opensubtitles() {
            self.disc.subs_msg = Some("Add an OpenSubtitles API key in Settings → Services to search subtitles.".into());
            cx.notify();
            return;
        }
        self.disc.subs_loading = true;
        self.disc.subs_msg = None;
        let fut = self.svc.subtitle_search(self.sub_context());
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                this.disc.subs_loading = false;
                match res {
                    Ok(list) => {
                        this.disc.subs_msg = Some(if list.is_empty() { "No subtitles found.".into() } else { format!("{} subtitles found", list.len()) });
                        let best = list.first().cloned();
                        this.disc.subs = list;
                        if auto {
                            if let Some(b) = best {
                                this.download_subtitle(b, cx);
                            }
                        }
                    }
                    Err(e) => this.disc.subs_msg = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn download_subtitle(&mut self, sub: SubtitleHit, cx: &mut Context<Self>) {
        self.disc.subs_msg = Some(format!("Downloading {}…", sub.label()));
        let fut = self.svc.subtitle_download(sub.clone());
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(path) => {
                        let _ = this.player.add_subtitle(&path.to_string_lossy());
                        this.disc.subs_msg = Some(format!("Loaded {}", sub.label()));
                        this.status = Some("Subtitles loaded from OpenSubtitles".into());
                    }
                    Err(e) => this.disc.subs_msg = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// When a file has no subtitle track in a preferred language, fetch one.
    /// Also applies forced-only / dual-sub defaults once tracks are known.
    pub(crate) fn maybe_auto_subtitles(&mut self, cx: &mut Context<Self>) {
        let cfg = self.svc.config();
        let Some(path) = self.snap.path.clone() else { return };
        if self.snap.tracks.is_empty() || self.now_entry.is_none() {
            return;
        }
        if self.disc.subs_auto_for.as_deref() == Some(&path) {
            return;
        }
        self.disc.subs_auto_for = Some(path);
        self.disc.subs.clear();

        if cfg.sub_forced_only {
            if let Some(id) = self.snap.tracks.iter().find(|t| t.kind == rmh_app::TrackKind::Sub && t.forced).map(|t| t.id) {
                let _ = self.player.set_track(rmh_app::TrackKind::Sub, Some(id));
                for t in self.snap.tracks.iter_mut().filter(|t| t.kind == rmh_app::TrackKind::Sub) {
                    t.selected = t.id == id;
                }
            }
            let _ = self.player.set_sub_forced_only(true);
        }
        if cfg.dual_subtitles {
            let primary = self.snap.tracks.iter().find(|t| t.kind == rmh_app::TrackKind::Sub && t.selected).map(|t| t.id);
            if let Some(sec) = self.pick_secondary_sub(primary) {
                let _ = self.player.set_secondary_sub(Some(sec));
                self.snap.secondary_sid = Some(sec);
            }
        }

        if !cfg.subtitles_auto || !self.svc.has_opensubtitles() {
            return;
        }
        let langs = cfg.subtitle_languages();
        let has_pref = self.snap.tracks.iter().filter(|t| t.kind == rmh_app::TrackKind::Sub).any(|t| {
            t.lang.as_deref().map(rmh_app::lang_code).is_some_and(|l| langs.first() == Some(&l))
        });
        if !has_pref {
            self.search_subtitles(true, cx);
        }
    }

    // ------------------------------------------------------------- services

    pub(crate) fn refresh_social(&mut self, cx: &mut Context<Self>) {
        let (a, b) = (self.svc.simkl_status(), self.svc.anilist_status());
        cx.spawn(async move |this, cx| {
            let (sa, sb) = (a.await, b.await);
            this.update(cx, |this, cx| {
                this.disc.simkl = sa.unwrap_or_default();
                this.disc.anilist = sb.unwrap_or_default();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn save_service_settings(&mut self, cx: &mut Context<Self>) {
        let read = |e: &Entity<InputState>, cx: &Context<Self>| {
            let v = e.read(cx).value().trim().to_string();
            (!v.is_empty()).then_some(v)
        };
        let i = &self.disc.inputs;
        let region = read(&i.region, cx).map(|r| r.to_uppercase()).unwrap_or_else(|| "CZ".into());
        let (k, u, pw, s, a, tb, ad) = (
            read(&i.os_key, cx),
            read(&i.os_user, cx),
            read(&i.os_pass, cx),
            read(&i.simkl_id, cx),
            read(&i.anilist_id, cx),
            read(&i.torbox, cx),
            read(&i.alldebrid, cx),
        );
        self.update_config(
            move |c| {
                c.region = region;
                c.opensubtitles_api_key = k;
                c.opensubtitles_username = u;
                c.opensubtitles_password = pw;
                c.simkl_client_id = s;
                c.anilist_client_id = a;
                c.torbox_api_key = tb;
                c.alldebrid_api_key = ad;
            },
            false,
            cx,
        );
        self.disc.browse.facets = None;
        self.disc.calendar.data = None;
        self.refresh_social(cx);
    }

    pub(crate) fn simkl_connect(&mut self, cx: &mut Context<Self>) {
        let fut = self.svc.simkl_begin_login();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            let pin = match res {
                Ok(p) => p,
                Err(e) => {
                    this.update(cx, |this, cx| {
                        this.error = Some(e);
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let Ok(wait) = this.update(cx, |this, cx| {
                this.disc.simkl_pin = Some(pin.clone());
                cx.open_url(&pin.verification_url);
                cx.notify();
                this.svc.simkl_finish_login(pin.clone())
            }) else {
                return;
            };
            let res = wait.await;
            this.update(cx, |this, cx| {
                this.disc.simkl_pin = None;
                match res {
                    Ok(()) => this.status = Some("Simkl connected".into()),
                    Err(e) => this.error = Some(e),
                }
                this.refresh_social(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn anilist_open_auth(&mut self, cx: &mut Context<Self>) {
        match self.svc.anilist_auth_url() {
            Ok(u) => cx.open_url(&u),
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    pub(crate) fn anilist_save_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.disc.inputs.anilist_token.read(cx).value().trim().to_string();
        if token.is_empty() {
            return;
        }
        self.disc.inputs.anilist_token.update(cx, |s, cx| s.set_value("", window, cx));
        let fut = self.svc.anilist_set_token(token);
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                match res {
                    Ok(user) => this.status = Some(format!("AniList connected as {user}")),
                    Err(e) => this.error = Some(e),
                }
                this.refresh_social(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
