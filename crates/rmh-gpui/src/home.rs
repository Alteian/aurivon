//! Home (three switchable layouts), Library and Discover pages.

use std::collections::BTreeMap;

use gpui_kit::component::input::Input;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{up_next, HomeLayout, LibraryEntry, TitleKind};

use crate::theme::Palette;
use crate::ui::{chip, poster};
use crate::view::MediaHubView;

impl MediaHubView {
    pub(crate) fn continue_entries(&self) -> Vec<&LibraryEntry> {
        let mut v: Vec<&LibraryEntry> = self.entries.iter().filter(|e| e.item.watch_progress > 0 && !e.item.watched).collect();
        v.sort_by(|a, b| b.item.updated_at.cmp(&a.item.updated_at));
        v
    }

    pub(crate) fn recent_entries(&self) -> Vec<&LibraryEntry> {
        let mut v: Vec<&LibraryEntry> = self.entries.iter().collect();
        v.sort_by(|a, b| b.item.created_at.cmp(&a.item.created_at));
        v.truncate(16);
        v
    }

    pub(crate) fn layout_switcher(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.svc.config().home_layout;
        let mut row = div().flex().items_center().gap_2().child(div().text_xs().text_color(rgb(p.muted)).child("Home layout"));
        for l in HomeLayout::ALL {
            let letter = l.letter();
            row = row.child(
                self.pill(SharedString::from(format!("layout-{letter}")), l.label(), l == current, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_home_layout(l, cx))),
            );
        }
        row
    }

    pub(crate) fn home_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let layout = self.svc.config().home_layout;
        if layout.is_feed() {
            return div().flex().flex_col().gap_6().child(self.home_quick_bar(p, cx)).child(self.home_stream(p, cx)).into_any_element();
        }
        let body = match layout {
            HomeLayout::Stream | HomeLayout::Cinematic | HomeLayout::Compact => self.home_stream(p, cx),
            HomeLayout::Focus => self.home_focus(p, cx),
            HomeLayout::Discover => self.home_discover(p, cx),
            HomeLayout::Library => self.home_library(p, cx),
        };
        let cfg = self.svc.config();
        let mut col = div().flex().flex_col().gap_6().child(self.layout_switcher(p, cx)).child(body);
        if cfg.home_row_visible("watchlist") && !self.disc.watch_cards.is_empty() {
            let cards = self.disc.watch_cards.iter().map(|c| self.title_card(c, "home-wl", 150.0, p, cx)).collect();
            col = col.child(self.section("Your watchlist", Some(format!("{} titles · synced with Trakt", self.disc.watch_cards.len())), self.row("home-wl", cards), p));
        }
        col.into_any_element()
    }

    /// Trakt continue-watching (watchlist is unified into the local list)
    fn trakt_rows(&self, p: Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.trakt.logged_in || self.trakt_cards.is_empty() || !self.svc.config().home_row_visible("trakt_playback") {
            return vec![];
        }
        let local: std::collections::HashSet<i64> =
            self.entries.iter().filter_map(|e| e.link.as_ref().and_then(|l| l.tmdb_id)).collect();
        let playback_ids: Vec<i64> = self.trakt_lists.playback.iter().filter_map(|e| e.tmdb_id()).collect();
        let mut resume = vec![];
        for c in &self.trakt_cards {
            let id = c.title.tmdb_id as i64;
            if local.contains(&id) {
                continue;
            }
            if playback_ids.contains(&id) {
                resume.push(self.title_card(c, "trakt-pb", 150.0, p, cx));
            }
        }
        let mut out = vec![];
        if !resume.is_empty() {
            out.push(self.section("Continue on Trakt", Some("started elsewhere · stream it here".into()), self.row("trakt-pb", resume), p));
        }
        out
    }

    /// A: Continue Watching + Up Next
    fn home_focus(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.entries.is_empty() && !self.trakt.logged_in {
            return self.empty_state("Welcome to Aurivon", "Add a folder with movies, shows or music to get started.", p, cx);
        }
        let cfg = self.svc.config();
        let cont = self.continue_entries();
        let next = up_next(&self.entries);
        let mut col = div().flex().flex_col().gap_8();
        if cfg.home_row_visible("continue") {
            col = col.child(self.section(
                "Continue watching",
                Some(format!("{} in progress", cont.len())),
                if cont.is_empty() {
                    div().text_sm().text_color(rgb(p.muted)).child("Nothing in progress. Start something below.").into_any_element()
                } else {
                    self.row("cont", cont.iter().take(12).map(|e| self.wide_entry_card(e, "cont", p, cx)).collect())
                },
                p,
            ));
        }
        if cfg.home_row_visible("up_next") {
            col = col.child(self.section(
                "Up next",
                Some("next unwatched episode of shows you're watching".into()),
                if next.is_empty() {
                    div()
                        .text_sm()
                        .text_color(rgb(p.muted))
                        .child("Finish an episode and the next one shows up here (autoplay plays it for you).")
                        .into_any_element()
                } else {
                    self.row("next", next.iter().map(|e| self.entry_card(e, "next", 170.0, p, cx)).collect())
                },
                p,
            ));
        }
        for r in self.trakt_rows(p, cx) {
            col = col.child(r);
        }
        let recent = self.recent_entries();
        if !recent.is_empty() {
            col = col.child(self.section(
                "Recently added",
                None,
                self.row("recent", recent.iter().map(|e| self.entry_card(e, "recent", 150.0, p, cx)).collect()),
                p,
            ));
        }
        col.into_any_element()
    }

    fn hero(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(top) = self.trending.first().cloned() else {
            return self
                .card(p)
                .child(self.section_title("Discover needs TMDB", p))
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(p.muted))
                        .child("Add a free TMDB API key in Settings → Services to see trending movies and shows here."),
                )
                .into_any_element();
        };
        let t = top.title.clone();
        let (kind, id) = (t.kind, t.tmdb_id);
        let open = top.clone();
        div()
            .relative()
            .h(px(380.0))
            .w_full()
            .rounded_2xl()
            .overflow_hidden()
            .bg(rgb(p.surface))
            .when_some(self.hero_backdrop.clone(), |d, b| d.child(img(b).absolute().size_full().object_fit(ObjectFit::Cover)))
            .child(div().absolute().size_full().bg(linear_gradient(
                90.0,
                linear_color_stop(rgba((p.canvas << 8) | 0xF5), 0.0),
                linear_color_stop(rgba(p.canvas << 8), 0.75),
            )))
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_end()
                    .gap_6()
                    .p_8()
                    .child(div().w(px(150.0)).h(px(225.0)).flex_none().rounded_xl().overflow_hidden().shadow_lg().child(self.lazy_poster(&t.name, top.poster.clone(), rmh_app::images::poster_url(&t), None)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .max_w(px(560.0))
                            .child(chip("#1 TRENDING THIS WEEK", (p.accent << 8) | 0x55, p.text))
                            .child(div().text_3xl().font_weight(FontWeight::BOLD).child(t.name.clone()))
                            .child(div().text_sm().text_color(rgb(p.muted)).child(format!(
                                "{}{}{}",
                                if kind == TitleKind::Tv { "Series" } else { "Movie" },
                                t.year.map(|y| format!(" · {y}")).unwrap_or_default(),
                                t.rating.map(|r| format!(" · ★ {r:.1}")).unwrap_or_default()
                            )))
                            .child(div().text_sm().text_color(rgb(p.text)).line_clamp(3).child(t.overview.clone().unwrap_or_default()))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        self.button("hero-open", "▶  Watch", true, p)
                                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_title(&open, None, window, cx))),
                                    )
                                    .when(self.trakt.logged_in, |d| {
                                        d.child(
                                            self.button("hero-wl", "+ Watchlist", false, p)
                                                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.trakt_watchlist_add(kind, id, cx))),
                                        )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// B: Trending / Discover with a hero banner
    fn home_discover(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div().flex().flex_col().gap_8().child(self.hero(p, cx));
        let cont = self.continue_entries();
        if !cont.is_empty() {
            col = col.child(self.section(
                "Continue watching",
                None,
                self.row("bcont", cont.iter().take(10).map(|e| self.entry_card(e, "bcont", 140.0, p, cx)).collect()),
                p,
            ));
        }
        if self.svc.config().home_row_visible("trending") && self.trending.len() > 1 {
            let movies: Vec<AnyElement> =
                self.trending.iter().skip(1).filter(|c| c.title.kind == TitleKind::Movie).map(|c| self.title_card(c, "trm", 160.0, p, cx)).collect();
            let shows: Vec<AnyElement> =
                self.trending.iter().skip(1).filter(|c| c.title.kind == TitleKind::Tv).map(|c| self.title_card(c, "trs", 160.0, p, cx)).collect();
            if !movies.is_empty() {
                col = col.child(self.section("Trending movies", None, self.row("trm", movies), p));
            }
            if !shows.is_empty() {
                col = col.child(self.section("Trending shows", None, self.row("trs", shows), p));
            }
        }
        for r in self.trakt_rows(p, cx) {
            col = col.child(r);
        }
        col.into_any_element()
    }

    /// C: library grid first, small continue row on top
    fn home_library(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.entries.is_empty() {
            return self.empty_state("Your library is empty", "Add a folder to fill this grid.", p, cx);
        }
        let cont = self.continue_entries();
        let mut col = div().flex().flex_col().gap_6();
        if !cont.is_empty() {
            col = col.child(self.section(
                "Continue watching",
                None,
                self.row("ccont", cont.iter().take(4).map(|e| self.wide_entry_card(e, "ccont", p, cx)).collect()),
                p,
            ));
        }
        let all: Vec<&LibraryEntry> = self.entries.iter().collect();
        col.child(self.section("Library", Some(format!("{} items", self.entries.len())), self.grid(self.library_cards(&all, "home", p, cx)), p))
            .into_any_element()
    }

    /// Movies as cards, each series collapsed into one show card
    pub(crate) fn library_cards(&self, entries: &[&LibraryEntry], key: &str, p: Palette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut shows: BTreeMap<i64, Vec<&LibraryEntry>> = BTreeMap::new();
        let mut cards: Vec<(String, AnyElement)> = vec![];
        for e in entries {
            match (e.kind(), e.link.as_ref().and_then(|l| l.tmdb_id)) {
                (Some(TitleKind::Tv), Some(id)) => shows.entry(id).or_default().push(e),
                _ => cards.push((e.display_title().to_lowercase(), self.entry_card(e, key, 168.0, p, cx))),
            }
        }
        for (_, eps) in shows {
            let first = eps[0];
            let title = first.display_title();
            let watched = eps.iter().filter(|e| e.item.watched).count();
            let open_id = up_next(&eps.iter().map(|e| (*e).clone()).collect::<Vec<_>>())
                .first()
                .map(|e| e.item.id)
                .unwrap_or(first.item.id);
            let card = div()
                .id(SharedString::from(format!("show-{key}-{}", first.item.id)))
                .w(px(168.0))
                .flex_none()
                .flex()
                .flex_col()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_entry(open_id, window, cx)))
                .child(
                    div()
                        .relative()
                        .h(px(252.0))
                        .w_full()
                        .rounded_xl()
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(p.border_soft))
                        .hover(|s| s.border_color(rgb(p.accent)))
                        .child(poster(&title, first.poster(), None))
                        .child(div().absolute().top_2().left_2().child(chip("SERIES", 0x000000AA, p.text)))
                        .child(div().absolute().bottom_2().right_2().child(chip(format!("{} ep", eps.len()), 0x000000CC, p.text))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().truncate().child(title.clone()))
                        .child(div().text_xs().text_color(rgb(p.muted)).child(format!("{watched}/{} watched", eps.len()))),
                )
                .into_any_element();
            cards.push((title.to_lowercase(), card));
        }
        cards.sort_by(|a, b| a.0.cmp(&b.0));
        cards.into_iter().map(|(_, c)| c).collect()
    }

    pub(crate) fn library_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        if self.entries.is_empty() && self.libraries.is_empty() {
            return self.empty_state("Your library is empty", "Type a folder path or browse for one. It's scanned right away.", p, cx);
        }
        let cfg = self.svc.config();
        let items = self.filtered_entries(cx);
        let count = items.len();
        let type_pills: Vec<AnyElement> = [("all", "All"), ("movie", "Movies"), ("tv", "Shows"), ("other", "Other")]
            .iter()
            .map(|(id, label)| {
                let id = (*id).to_string();
                self.pill(SharedString::from(format!("lib-type-{id}")), (*label).to_string(), cfg.library_type_filter == id, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        let id = id.clone();
                        this.update_config(|c| c.library_type_filter = id, false, cx);
                    }))
                    .into_any_element()
            })
            .collect();
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(type_pills)
                    .child(
                        self.pill("lib-hide-watched", "Hide watched", cfg.library_hide_watched, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                            this.update_config(|c| c.library_hide_watched = !c.library_hide_watched, false, cx);
                        })),
                    )
                    .child(self.button("lib-rematch", "Re-match", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.match_library(true, cx)))),
            )
            .child(div().text_sm().text_color(rgb(p.muted)).child(format!("{count} files · click for details, double-click to play")))
            .child(self.grid(self.library_cards(&items, "lib", p, cx)))
            .into_any_element()
    }

    pub(crate) fn watchlist_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let cards = &self.disc.watch_cards;
        let mut col = div().flex().flex_col().gap_6().child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(div().text_sm().text_color(rgb(p.muted)).child(if self.trakt.logged_in {
                    format!("{} titles · linked to Trakt", cards.len())
                } else {
                    format!("{} titles · connect Trakt in Settings to sync", cards.len())
                }))
                .when(self.trakt.logged_in, |d| {
                    d.child(
                        self.button("wl-sync", "Sync with Trakt", true, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.sync_watchlist_only(cx))),
                    )
                }),
        );
        if cards.is_empty() {
            col = col.child(div().text_sm().text_color(rgb(p.muted)).child("Press W on any title to add it. Sync pulls your Trakt watchlist here."));
        } else {
            col = col.child(self.grid(cards.iter().map(|c| self.title_card(c, "wl-page", 168.0, p, cx)).collect()));
        }
        col.into_any_element()
    }
}
