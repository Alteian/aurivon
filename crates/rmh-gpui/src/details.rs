//! Title details: backdrop, info, play/resume, seasons & episodes, streams.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{format_time, TitleKind, TraktItem};

use crate::theme::Palette;
use crate::ui::{chip, poster};
use crate::view::{MediaHubView, Page, StreamContext};

impl MediaHubView {
    pub(crate) fn streams_list(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let label = self.stream_ctx.as_ref().map(|c| c.label.clone()).unwrap_or_default();
        if self.streams_loading {
            return div().text_sm().text_color(rgb(p.muted)).child(format!("Looking for streams for {label}…")).into_any_element();
        }
        if self.streams.is_empty() {
            return div().into_any_element();
        }
        let f = &self.stream_filter;
        let shown = rmh_app::streams::apply(&self.streams, f);
        let hidden = self.streams.len() - shown.len();
        let full = self.stream_filters_open;
        let saved = self.svc.config().stream_filter == *f;

        let toolbar = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                self.button("sf-more", if full { "Fewer filters ▴" } else { "More filters ▾" }, false, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.stream_filters_open = !this.stream_filters_open;
                        cx.notify();
                    })),
            )
            .when(f.is_active(), |d| {
                d.child(self.button("sf-clear", "Clear filters", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    let sort = this.stream_filter.sort;
                    this.stream_filter = rmh_app::StreamFilter { sort, ..Default::default() };
                    cx.notify();
                })))
            })
            .when(!saved, |d| {
                d.child(self.button("sf-save", "Save as default", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    let f = this.stream_filter.clone();
                    this.update_config(move |c| c.stream_filter = f, false, cx);
                    this.status = Some("Stream filters saved as default".into());
                })))
            })
            .child(div().flex_1())
            .child(div().text_xs().text_color(rgb(p.muted)).child(if hidden > 0 {
                format!("{} shown · {hidden} hidden by filters", shown.len())
            } else {
                format!("{} streams", shown.len())
            }));

        let mut rows = div().flex().flex_col().gap_1();
        if shown.is_empty() {
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(p.surface))
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .child(format!("No stream matches these filters ({hidden} hidden)."))
                    .child(self.button("sf-showall", "Show all", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        let sort = this.stream_filter.sort;
                        this.stream_filter = rmh_app::StreamFilter { sort, hide_cam: false, ..Default::default() };
                        cx.notify();
                    }))),
            );
        }
        let more = shown.len().saturating_sub(80);
        for i in shown.into_iter().take(80) {
            let s = &self.streams[i];
            let badges = s.traits.badges();
            let quality = if badges.is_empty() { s.quality.clone() } else { badges.join(" · ") };
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("stream-{i}")))
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(p.surface))
                    .cursor_pointer()
                    .hover(|st| st.bg(rgb(p.surface_raised)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.play_stream(i, window, cx)))
                    .child(
                        div()
                            .w(px(56.0))
                            .text_xs()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(if s.rd_cached { p.good } else { p.muted }))
                            .child(if s.rd_cached { "⚡ RD" } else { "P2P" }),
                    )
                    .child(div().w(px(230.0)).text_sm().truncate().child(quality))
                    .child(div().w(px(80.0)).text_xs().text_color(rgb(p.muted)).child(s.size.clone().unwrap_or_default()))
                    .child(div().w(px(60.0)).text_xs().text_color(rgb(p.muted)).child(s.seeders.map(|n| format!("▲ {n}")).unwrap_or_default()))
                    .child(div().flex_1().min_w_0().text_xs().text_color(rgb(p.faint)).truncate().child(s.title.clone()))
                    .child(div().text_sm().text_color(rgb(p.accent)).child("Play ▶")),
            );
        }
        if more > 0 {
            rows = rows.child(div().px_4().py_2().text_xs().text_color(rgb(p.faint)).child(format!("+ {more} more — narrow the filters to see them")));
        }
        let body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border_soft))
                    .child(self.stream_filter_panel(f, crate::filters::FilterTarget::Picker, full, p, cx))
                    .child(toolbar),
            )
            .child(rows);
        self.section(format!("Streams for {label}"), Some(format!("{}", self.streams.len())), body.into_any_element(), p)
    }

    pub(crate) fn details_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(d) = self.details.clone() else {
            return div().into_any_element();
        };
        let det = d.details.clone();
        let title = det.as_ref().map(|x| x.title.clone());
        let name = title.as_ref().map(|t| t.name.clone()).unwrap_or(d.name.clone());
        let kind = d.kind;
        let tmdb_id = d.tmdb_id;
        let imdb = det.as_ref().and_then(|x| x.imdb_id.clone());
        let local = d.local.and_then(|id| self.entry(id)).cloned();

        let meta = {
            let mut parts: Vec<String> = vec![];
            if let Some(k) = kind {
                parts.push(if k == TitleKind::Tv { "Series".into() } else { "Movie".into() });
            }
            if let Some(y) = title.as_ref().and_then(|t| t.year) {
                parts.push(y.to_string());
            }
            if let Some(r) = det.as_ref().and_then(|x| x.runtime_minutes).filter(|r| *r > 0) {
                parts.push(format!("{r} min"));
            }
            if let Some(r) = title.as_ref().and_then(|t| t.rating).filter(|r| *r > 0.0) {
                parts.push(format!("★ {r:.1}"));
            }
            if let Some(g) = det.as_ref().map(|x| x.genres.join(", ")).filter(|g| !g.is_empty()) {
                parts.push(g);
            }
            parts.join(" · ")
        };

        // Primary actions
        let mut actions = div().flex().flex_wrap().gap_2();
        if let Some(e) = &local {
            let id = e.item.id;
            let label = if e.item.watch_progress > 0 && !e.item.watched {
                format!("▶  Resume {}", format_time(e.item.watch_progress as f64))
            } else {
                "▶  Play".to_string()
            };
            actions = actions.child(self.button("det-play", label, true, p).on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.play_item(id, window, cx))));
        }
        if kind == Some(TitleKind::Movie) {
            if let Some(imdb) = imdb.clone() {
                let ctx = StreamContext { label: name.clone(), trakt: tmdb_id.map(|t| TraktItem::Movie { tmdb: t as i64 }) };
                actions = actions.child(
                    self.button("det-streams", "Find streams", local.is_none(), p)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.load_streams(imdb.clone(), None, ctx.clone(), cx))),
                );
            }
        }
        let extras = self.disc.extras.as_ref().filter(|(k, _)| Some(k.0) == kind && Some(k.1) == tmdb_id).map(|(_, x)| x);
        if let Some(v) = extras.and_then(|x| x.extras.best_trailer()).and_then(|v| v.url()) {
            actions = actions.child(self.button("det-trailer", "▶  Trailer", false, p).on_click(cx.listener(move |_, _: &ClickEvent, _w, cx| cx.open_url(&v))));
        }
        if let (Some(k), Some(id), Some(t)) = (kind, tmdb_id, title.clone()) {
            let listed = self.disc.watch_ids.contains(&(k, id));
            let card = rmh_app::TitleCard { title: t, poster: d.poster.clone() };
            actions = actions.child(
                self.button("det-wl", if listed { "✓ On watchlist" } else { "+ Watchlist" }, false, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.toggle_watchlist(&card, cx))),
            );
        }
        actions = actions.child(self.button("det-back", "← Back", false, p).on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
            let back = this.back_page;
            this.navigate(if back == Page::Details { Page::Home } else { back }, window, cx)
        })));

        let header = div()
            .relative()
            .w_full()
            .min_h(px(340.0))
            .rounded_2xl()
            .overflow_hidden()
            .bg(rgb(p.surface))
            .when_some(d.backdrop.clone(), |el, b| el.child(img(b).absolute().size_full().object_fit(ObjectFit::Cover)))
            .child(div().absolute().size_full().bg(linear_gradient(
                90.0,
                linear_color_stop(rgba(0x0A0B10F8), 0.0),
                linear_color_stop(rgba(0x0A0B1060), 1.0),
            )))
            .child(
                div()
                    .relative()
                    .flex()
                    .gap_8()
                    .p_8()
                    .child(div().w(px(190.0)).h(px(285.0)).flex_none().rounded_xl().overflow_hidden().shadow_lg().child(self.lazy_poster(&name, d.poster.clone(), title.as_ref().and_then(rmh_app::images::poster_url), None)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().text_3xl().font_weight(FontWeight::BOLD).child(name.clone()))
                            .when_some(det.as_ref().and_then(|x| x.tagline.clone()), |el, t| {
                                el.child(div().text_sm().italic().text_color(rgb(p.muted)).child(t))
                            })
                            .child(div().text_sm().text_color(rgb(p.muted)).child(meta))
                            .child(self.availability(kind, tmdb_id, local.is_some(), p))
                            .child(div().text_sm().line_clamp(5).child(
                                title.as_ref().and_then(|t| t.overview.clone()).unwrap_or_else(|| {
                                    if d.loading { "Loading details…".into() } else { String::new() }
                                }),
                            ))
                            .child(actions),
                    ),
            );

        let mut col = div().flex().flex_col().gap_6().child(header);

        // Seasons & episodes
        if let (Some(TitleKind::Tv), Some(show), Some(det)) = (kind, tmdb_id, det.as_ref()) {
            let mut seasons = div().flex().flex_wrap().gap_2();
            for s in &det.seasons {
                let n = s.number;
                let local_count = self
                    .entries
                    .iter()
                    .filter(|e| e.link.as_ref().is_some_and(|l| l.tmdb_id == Some(show as i64) && l.season == Some(n as i64)))
                    .count();
                let label = if local_count > 0 { format!("{} · {local_count} local", s.name) } else { s.name.clone() };
                seasons = seasons.child(
                    self.pill(SharedString::from(format!("season-{n}")), label, d.season == Some(n), p)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.select_season(n, cx))),
                );
            }
            let mut eps = div().flex().flex_col().gap_2();
            if d.episodes.is_empty() {
                eps = eps.child(div().text_sm().text_color(rgb(p.muted)).child("Loading episodes…"));
            }
            for ep in &d.episodes {
                let (s, n) = (ep.season, ep.number);
                let local_ep = self.local_episode(show, s, n).cloned();
                let label = format!("{} S{s:02}E{n:02}", name);
                let mut row = div()
                    .id(SharedString::from(format!("ep-{s}-{n}")))
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border_soft))
                    .child(div().w(px(36.0)).text_lg().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.muted)).child(format!("{n}")))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(ep.name.clone()))
                            .child(div().text_xs().text_color(rgb(p.faint)).line_clamp(2).child(ep.overview.clone().unwrap_or_default())),
                    )
                    .child(div().w(px(90.0)).text_xs().text_color(rgb(p.muted)).child(ep.air_date.clone().unwrap_or_default()));
                if let Some(e) = local_ep {
                    let id = e.item.id;
                    let watched = e.item.watched;
                    let resume = e.item.watch_progress > 0 && !watched;
                    row = row
                        .when(watched, |r| r.child(chip("✓ watched", 0x5FD39A33, p.good)))
                        .child(
                            self.button(SharedString::from(format!("ep-play-{s}-{n}")), if resume { "▶ Resume" } else { "▶ Play" }, true, p)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.play_item(id, window, cx))),
                        );
                } else if let Some(imdb) = imdb.clone() {
                    let ctx = StreamContext { label: label.clone(), trakt: Some(TraktItem::Episode { show_tmdb: show as i64, season: s, number: n }) };
                    row = row.child(
                        self.button(SharedString::from(format!("ep-str-{s}-{n}")), "Streams", false, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.load_streams(imdb.clone(), Some((s, n)), ctx.clone(), cx))),
                    );
                }
                eps = eps.child(row);
            }
            col = col.child(self.section("Seasons", None, seasons.into_any_element(), p)).child(eps);
        }

        if self.stream_ctx.is_some() {
            col = col.child(self.streams_list(p, cx));
        }
        if let Some(x) = extras.cloned() {
            col = col.child(self.extras_sections(&x, kind, p, cx));
        } else if self.disc.extras_loading {
            col = col.child(div().text_sm().text_color(rgb(p.muted)).child("Loading cast, trailers and similar titles…"));
        }
        col.into_any_element()
    }

    /// "In library" / "Real-Debrid" / "On Netflix" badges
    fn availability(&self, kind: Option<TitleKind>, tmdb_id: Option<i32>, local: bool, p: Palette) -> impl IntoElement {
        let extras = self.disc.extras.as_ref().filter(|(k, _)| Some(k.0) == kind && Some(k.1) == tmdb_id).map(|(_, x)| x);
        let local = local || matches!((kind, tmdb_id), (Some(k), Some(id)) if self.disc.local_ids.contains(&(k, id)));
        let mut row = div().flex().flex_wrap().items_center().gap_2();
        if local {
            row = row.child(chip("▶ Available locally", 0x5FD39A22, p.good));
        }
        if self.svc.has_real_debrid() {
            row = row.child(chip("⚡ Real-Debrid streams", 0x4CC9F022, 0x4CC9F0));
        }
        if let Some(x) = extras {
            for prov in x.extras.providers.iter().take(6) {
                let logo = x.provider_logos.get(&prov.id).cloned();
                row = row.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pl(px(2.0))
                        .pr_2()
                        .py(px(2.0))
                        .rounded_md()
                        .bg(rgba(0xFFFFFF10))
                        .text_xs()
                        .when_some(logo, |d, l| d.child(img(l).size(px(18.0)).rounded_sm()))
                        .child(prov.name.clone()),
                );
            }
        }
        row
    }

    fn extras_sections(&self, x: &rmh_app::TitleExtras, kind: Option<TitleKind>, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div().flex().flex_col().gap_6();
        // crew line
        let crew: Vec<String> = x.extras.crew.iter().take(5).map(|c| format!("{} ({})", c.name, c.role)).collect();
        if !crew.is_empty() || !x.extras.keywords.is_empty() {
            col = col.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when(!crew.is_empty(), |d| d.child(div().text_sm().child(crew.join(" · "))))
                    .when(!x.extras.keywords.is_empty(), |d| {
                        d.child(div().flex().flex_wrap().gap_1().children(x.extras.keywords.iter().take(10).map(|k| chip(k.clone(), 0xFFFFFF0C, p.muted))))
                    }),
            );
        }
        // cast
        if !x.extras.cast.is_empty() {
            let mut people = vec![];
            for (i, c) in x.extras.cast.iter().take(16).enumerate() {
                let (id, name) = (c.id, c.name.clone());
                people.push(
                    div()
                        .id(SharedString::from(format!("cast-{i}")))
                        .w(px(110.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_person(id, name.clone(), window, cx)))
                        .child(
                            div()
                                .size(px(96.0))
                                .rounded_full()
                                .overflow_hidden()
                                .border_2()
                                .border_color(rgb(p.border_soft))
                                .hover(|s| s.border_color(rgb(p.accent)))
                                .child(poster(&c.name, x.photos.get(&c.id).cloned(), None)),
                        )
                        .child(div().w_full().text_xs().font_weight(FontWeight::MEDIUM).text_center().truncate().child(c.name.clone()))
                        .child(div().w_full().text_xs().text_color(rgb(p.muted)).text_center().truncate().child(c.role.clone()))
                        .into_any_element(),
                );
            }
            col = col.child(self.section("Cast", Some("click a person for their filmography".into()), self.row("cast", people), p));
        }
        if !x.recommended.is_empty() {
            let cards = x.recommended.iter().map(|c| self.title_card(c, "rec", 150.0, p, cx)).collect();
            col = col.child(self.section("Recommended", Some("people who liked this also watched".into()), self.row("rec", cards), p));
        }
        if !x.similar.is_empty() {
            let cards = x.similar.iter().map(|c| self.title_card(c, "sim", 150.0, p, cx)).collect();
            let label = if kind == Some(TitleKind::Tv) { "Similar series" } else { "Similar movies" };
            col = col.child(self.section(label, None, self.row("sim", cards), p));
        }
        col.into_any_element()
    }
}
