//! Global search overlay (Ctrl+K or /): library, movies, series and people as you type.

use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::TitleKind;

use crate::disc::{SearchDown, SearchItem, SearchUp};
use crate::theme::Palette;
use crate::ui::chip;
use crate::view::MediaHubView;

impl MediaHubView {
    pub(crate) fn search_overlay(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.disc.search;
        let groups = self.search_groups();
        let items: Vec<SearchItem> = groups.iter().map(|(_, i)| i.clone()).collect();
        let mut list = div().flex().flex_col().gap_1();
        let mut last_group = "";
        for (i, (group, item)) in groups.iter().enumerate() {
            let group = *group;
            if group != last_group {
                last_group = group;
                list = list.child(div().px_3().pt_3().pb_1().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.faint)).child(group.to_uppercase()));
            }
            let on = i == s.sel;
            let (seed, img_path, title, sub, badge, round): (String, Option<std::path::PathBuf>, String, String, Option<(String, u32)>, bool) = match item {
                SearchItem::Local(id) => match self.entry(*id) {
                    Some(e) => {
                        let progress = (e.item.watch_progress > 0 && !e.item.watched).then(|| "in progress".to_string());
                        (
                            e.display_title(),
                            e.poster(),
                            e.display_title(),
                            [e.subtitle(), progress]
                                .into_iter()
                                .flatten()
                                .collect::<Vec<_>>()
                                .join(" · "),
                            Some(("▶ Local".into(), p.good)),
                            false,
                        )
                    }
                    None => continue,
                },
                SearchItem::Imdb(id) => (id.clone(), None, format!("Find streams for {id}"), "Torrentio / Real-Debrid".into(), None, false),
                SearchItem::Title(c) => {
                    let t = &c.title;
                    let mut parts = vec![if t.kind == TitleKind::Tv { "Series".to_string() } else { "Movie".to_string() }];
                    if let Some(y) = t.year {
                        parts.push(y.to_string());
                    }
                    if let Some(r) = t.rating.filter(|r| *r > 0.0) {
                        parts.push(format!("★ {r:.1}"));
                    }
                    let badge = if self.disc.local_ids.contains(&(t.kind, t.tmdb_id)) {
                        Some(("In library".to_string(), p.good))
                    } else if self.disc.watch_ids.contains(&(t.kind, t.tmdb_id)) {
                        Some(("✓ Watchlist".to_string(), p.accent))
                    } else {
                        None
                    };
                    (t.name.clone(), c.poster.clone(), t.name.clone(), parts.join(" · "), badge, false)
                }
                SearchItem::Person(h) => {
                    let dept = h.card.department.clone().unwrap_or_default();
                    let known = h.card.known_for.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                    (h.card.name.clone(), h.photo.clone(), h.card.name.clone(), [dept, known].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · "), None, true)
                }
            };
            let lazy_url = match item {
                SearchItem::Title(c) => rmh_app::images::thumb_url(&c.title),
                _ => None,
            };
            let it = item.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("sr-{i}")))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(if on { p.accent_soft } else { p.surface }))
                    .border_1()
                    .border_color(rgb(if on { p.accent } else { p.surface }))
                    .hover(|d| d.bg(rgb(p.surface_raised)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_search_item(it.clone(), window, cx)))
                    .child(
                        div()
                            .w(px(40.0))
                            .h(px(if round { 40.0 } else { 60.0 }))
                            .flex_none()
                            .overflow_hidden()
                            .when(round, |d| d.rounded_full())
                            .when(!round, |d| d.rounded_md())
                            .child(self.lazy_poster(&seed, img_path, lazy_url, if matches!(item, SearchItem::Imdb(_)) { Some("◎") } else { None })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(title))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(sub)),
                    )
                    .when_some(badge, |d, (b, c)| d.child(chip(b, 0xFFFFFF10, c)))
                    .when(on, |d| d.child(div().text_xs().text_color(rgb(p.muted)).child("↵"))),
            );
        }
        let q = s.query.trim();
        let empty = items.is_empty();
        let hint = if q.is_empty() {
            Some("Type to search movies, series, people and your library. Typos are fine.".to_string())
        } else if s.loading && empty {
            Some("Searching…".to_string())
        } else if empty && q.chars().count() >= 2 && !s.loading {
            Some(format!("Nothing found for “{q}”"))
        } else if !self.svc.has_tmdb() {
            Some("Add a TMDB key in Settings to search movies, series and people.".to_string())
        } else {
            None
        };

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .pt(px(70.0))
            .bg(rgba(0x05060ACC))
            .child(
                div()
                    .id("search-overlay")
                    .key_context("SearchBox")
                    .on_action(cx.listener(|this, _: &SearchUp, _w, cx| this.search_move(-1, cx)))
                    .on_action(cx.listener(|this, _: &SearchDown, _w, cx| this.search_move(1, cx)))
                    .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_search(window, cx)))
                    .w(px(760.0))
                    .max_h(px(720.0))
                    .flex()
                    .flex_col()
                    .rounded_2xl()
                    .bg(rgb(p.surface))
                    .border_1()
                    .border_color(rgb(p.border))
                    .shadow_lg()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgb(p.border_soft))
                            .child(div().text_lg().text_color(rgb(p.accent)).child("⌕"))
                            .child(div().flex_1().child(Input::new(&s.input).w_full()))
                            .when(s.loading, |d| d.child(div().text_xs().text_color(rgb(p.warn)).child("●")))
                            .child(chip("Esc", 0xFFFFFF10, p.muted)),
                    )
                    .when_some(s.hits.as_ref().and_then(|h| h.corrected.clone()), |d, c| {
                        d.child(
                            div()
                                .px_5()
                                .pt_3()
                                .text_sm()
                                .text_color(rgb(p.muted))
                                .child(format!("Showing results for “{c}” – did you mean that?")),
                        )
                    })
                    .child(
                        div()
                            .id("search-results")
                            .flex_1()
                            .overflow_y_scrollbar()
                            .p_2()
                            .child(list)
                            .when_some(hint, |d, h| d.child(div().p_6().text_sm().text_color(rgb(p.muted)).child(h))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .px_4()
                            .py_2()
                            .border_t_1()
                            .border_color(rgb(p.border_soft))
                            .text_xs()
                            .text_color(rgb(p.faint))
                            .child("↑↓ select")
                            .child("↵ open")
                            .child("Esc close")
                            .child(div().flex_1())
                            .child("Library · TMDB movies, series & people"),
                    ),
            )
    }
}
