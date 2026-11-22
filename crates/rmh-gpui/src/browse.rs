//! Browse: curated rows, filter chips (genre, year, rating, runtime, language,
//! streaming service), sort and a keyboard-navigable results grid.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{BrowseSort, TitleKind};

use crate::disc::Facet;
use crate::theme::Palette;
use crate::view::MediaHubView;

pub const GRID_CARD: f32 = 170.0;
pub const GRID_GAP: f32 = 20.0;

impl MediaHubView {
    /// Fake search field that opens the global search
    pub(crate) fn search_launcher(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("search-launcher")
            .flex_1()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .rounded_lg()
            .cursor_pointer()
            .bg(rgb(p.surface_raised))
            .border_1()
            .border_color(rgb(p.border))
            .hover(|s| s.border_color(rgb(p.accent)))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.open_search(window, cx)))
            .child(div().text_color(rgb(p.accent)).child("⌕"))
            .child(div().flex_1().text_sm().text_color(rgb(p.muted)).child("Search movies, series, people…"))
            .child(div().text_xs().text_color(rgb(p.faint)).child("Ctrl+K  ·  /"))
    }

    pub(crate) fn browse_page(&self, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let b = &self.disc.browse;
        let kind = b.kind;
        let mut col = div().flex().flex_col().gap_5();

        // movies / series + search
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .p_1()
                        .rounded_full()
                        .bg(rgb(p.surface))
                        .border_1()
                        .border_color(rgb(p.border_soft))
                        .child(
                            self.pill("bk-movies", "Movies", kind == TitleKind::Movie, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.set_browse_kind(TitleKind::Movie, cx))),
                        )
                        .child(
                            self.pill("bk-series", "Series", kind == TitleKind::Tv, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.set_browse_kind(TitleKind::Tv, cx))),
                        ),
                )
                .child(self.search_launcher(p, cx)),
        );

        if !self.svc.has_tmdb() {
            return col
                .child(self.card(p).child(div().text_sm().text_color(rgb(p.muted)).child(
                    "Browsing, filters and search need a TMDB API key (Settings → Services). IMDb ids (tt…) in search still find streams without it.",
                )))
                .into_any_element();
        }

        // filter chips
        let mut chips = div().flex().flex_wrap().items_center().gap_2();
        for f in Facet::ALL {
            let summary = self.facet_summary(f);
            let open = b.panel == Some(f);
            let label = match &summary {
                Some(s) => format!("{}: {s}  ▾", f.label()),
                None => format!("{}  ▾", f.label()),
            };
            chips = chips.child(
                div()
                    .id(SharedString::from(format!("facet-{}", f.label())))
                    .px_3()
                    .py(px(6.0))
                    .rounded_full()
                    .text_sm()
                    .cursor_pointer()
                    .border_1()
                    .border_color(rgb(if open || summary.is_some() { p.accent } else { p.border }))
                    .bg(rgb(if summary.is_some() { p.accent_soft } else if open { p.surface_raised } else { p.surface }))
                    .text_color(rgb(if summary.is_some() || open { p.text } else { p.muted }))
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        let b = &mut this.disc.browse;
                        b.panel = if b.panel == Some(f) { None } else { Some(f) };
                        b.opt_sel = 0;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        if b.query.is_filtered() || b.list.is_some() {
            chips = chips.child(self.pill("facet-clear", "✕ Clear", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.clear_filters(cx))));
        }
        col = col.child(chips);

        // open filter panel
        if let Some(f) = b.panel {
            let mut opts = div().flex().flex_wrap().gap_2();
            let logos: Vec<Option<std::path::PathBuf>> = if f == Facet::Provider {
                b.facets
                    .as_ref()
                    .map(|x| if kind == TitleKind::Movie { &x.movie_providers } else { &x.tv_providers })
                    .map(|v| v.iter().map(|(_, l)| l.clone()).collect())
                    .unwrap_or_default()
            } else {
                vec![]
            };
            let options = self.facet_options(f);
            if options.is_empty() {
                opts = opts.child(div().text_sm().text_color(rgb(p.muted)).child("Loading…"));
            }
            for (i, (label, on)) in options.into_iter().enumerate() {
                let kb = i == b.opt_sel;
                let logo = logos.get(i).cloned().flatten();
                opts = opts.child(
                    div()
                        .id(SharedString::from(format!("opt-{i}")))
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py(px(6.0))
                        .rounded_lg()
                        .text_sm()
                        .cursor_pointer()
                        .border_1()
                        .border_color(rgb(if kb { p.text } else if on { p.accent } else { p.border }))
                        .bg(rgb(if on { p.accent_soft } else { p.surface_raised }))
                        .text_color(rgb(if on { p.text } else { p.muted }))
                        .hover(|s| s.border_color(rgb(p.accent)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.toggle_facet(f, i, cx)))
                        .when_some(logo, |d, l| d.child(img(l).size(px(22.0)).rounded_md()))
                        .when(on, |d| d.child(div().text_color(rgb(p.accent)).child("✓")))
                        .child(label),
                );
            }
            col = col.child(
                self.card(p)
                    .gap_3()
                    .p_4()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(f.label()))
                            .child(div().text_xs().text_color(rgb(p.faint)).child("←→ choose · Enter toggle · F next filter · Esc close")),
                    )
                    .child(opts),
            );
        }

        // sort
        let mut sorts = div().flex().flex_wrap().items_center().gap_2().child(div().text_xs().text_color(rgb(p.faint)).mr_1().child("SORT"));
        for s in BrowseSort::ALL {
            sorts = sorts.child(
                self.pill(SharedString::from(format!("sort-{}", s.label())), s.label(), b.query.sort == s, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_sort(s, cx))),
            );
        }
        col = col.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_4()
                .child(sorts)
                .child(div().text_xs().text_color(rgb(p.faint)).child("F filters · O sort · V movies/series · arrows + Enter · W watchlist · X clear")),
        );

        // results or curated rows
        let vw = f32::from(window.viewport_size().width);
        let _ = vw;
        if b.filtered() {
            let cards: Vec<AnyElement> =
                b.results.iter().enumerate().map(|(i, c)| self.title_card_sel(c, "grid", GRID_CARD, b.sel == Some(i), p, cx)).collect();
            let head = if b.loading && b.results.is_empty() {
                "Loading…".to_string()
            } else {
                format!("{} {}", fmt_count(b.total), if kind == TitleKind::Movie { "movies" } else { "series" })
            };
            let mut body = div().flex().flex_col().gap_5().child(div().flex().flex_wrap().gap(px(GRID_GAP)).children(cards));
            if b.page < b.total_pages {
                body = body.child(
                    div().flex().justify_center().child(
                        self.button("load-more", if b.loading { "Loading…" } else { "Load more" }, false, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.run_discover(true, cx))),
                    ),
                );
            } else if !b.loading && b.results.is_empty() {
                body = body.child(div().text_sm().text_color(rgb(p.muted)).child("No titles match these filters."));
            }
            let title = match b.list {
                Some(l) => l.label(kind).to_string(),
                None if b.query.digital_within_days.is_some() => "New on streaming".to_string(),
                None => "Results".to_string(),
            };
            col = col.child(self.section(title, Some(head), body.into_any_element(), p));
        } else {
            for (l, cards) in &b.rows {
                let items: Vec<AnyElement> = if cards.is_empty() {
                    (0..7).map(|_| skeleton(150.0, p)).collect()
                } else {
                    cards.iter().map(|c| self.title_card(c, &format!("row-{l:?}"), 150.0, p, cx)).collect()
                };
                col = col.child(self.section(l.label(kind), None, self.row(&format!("br-{l:?}"), items), p));
            }
            // genre cloud
            if let Some(f) = &b.facets {
                let gs = if kind == TitleKind::Movie { &f.movie_genres } else { &f.tv_genres };
                let mut cloud = div().flex().flex_wrap().gap_2();
                for g in gs {
                    let id = g.id;
                    cloud = cloud.child(
                        self.pill(SharedString::from(format!("gc-{id}")), g.name.clone(), false, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.browse_genre(kind, id, window, cx))),
                    );
                }
                col = col.child(self.section("Browse by genre", None, cloud.into_any_element(), p));
            }
            for (gid, name, cards) in &b.genre_rows {
                if cards.is_empty() {
                    continue;
                }
                let items = cards.iter().map(|c| self.title_card(c, &format!("g{gid}"), 150.0, p, cx)).collect();
                col = col.child(self.section(name.clone(), None, self.row(&format!("gr-{gid}"), items), p));
            }
        }
        if self.stream_ctx.is_some() && self.details.is_none() {
            col = col.child(self.streams_list(p, cx));
        }
        col.into_any_element()
    }
}

fn skeleton(width: f32, p: Palette) -> AnyElement {
    div()
        .w(px(width))
        .flex_none()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().h(px(width * 1.5)).rounded_xl().bg(rgb(p.surface)))
        .child(div().h(px(12.0)).w(px(width * 0.7)).rounded_md().bg(rgb(p.surface)))
        .into_any_element()
}

pub fn fmt_count(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}
