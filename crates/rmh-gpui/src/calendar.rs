//! Release calendar page: month grid + agenda, filters, local times.

use chrono::{Datelike, NaiveDate};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{month_grid, CalendarItem, ReleaseKind, TitleCard};

use crate::disc::{CalRelease, CalTypes, CalView};
use crate::theme::Palette;
use crate::ui::{chip, poster};
use crate::view::MediaHubView;

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub fn kind_color(k: ReleaseKind, p: Palette) -> u32 {
    match k {
        ReleaseKind::Episode => 0x4CC9F0,
        ReleaseKind::Premiere => p.accent,
        ReleaseKind::Theatrical => p.warn,
        ReleaseKind::Digital => p.good,
    }
}

impl MediaHubView {
    pub(crate) fn calendar_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.disc.calendar;
        let today = chrono::Local::now().date_naive();
        let items = self.calendar_items();
        let source = c.data.as_ref().map(|d| d.source.clone());
        let region = self.svc.config().region;

        // ---------------------------------------------------------- toolbar
        let nav = div()
            .flex()
            .items_center()
            .gap_2()
            .child(self.button("cal-prev", "‹", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.calendar_shift(-1, cx))))
            .child(div().w(px(200.0)).flex().justify_center().text_xl().font_weight(FontWeight::SEMIBOLD).child(format!(
                "{} {}",
                MONTHS[(c.month as usize).saturating_sub(1) % 12],
                c.year
            )))
            .child(self.button("cal-next", "›", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.calendar_shift(1, cx))))
            .child(self.button("cal-today", "Today", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.calendar_today(cx))))
            .when_some(source.clone(), |d, s| {
                let label = if s == "Trakt" { "via Trakt".to_string() } else { format!("via TMDB · {region}") };
                d.child(chip(label, 0xFFFFFF10, p.muted))
            })
            .when(c.loading, |d| d.child(div().text_sm().text_color(rgb(p.warn)).child("Loading…")));
        let seg = |this: &Self, id: &'static str, label: &'static str, on: bool| this.pill(id, label, on, p);
        let filters = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_4()
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(seg(self, "cv-month", "Month", c.view == CalView::Month).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.view = CalView::Month;
                        cx.notify();
                    })))
                    .child(seg(self, "cv-agenda", "Agenda", c.view == CalView::Agenda).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.view = CalView::Agenda;
                        cx.notify();
                    }))),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(seg(self, "cm-all", "Everything", !c.mine_only).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.mine_only = false;
                        cx.notify();
                    })))
                    .child(seg(self, "cm-mine", "My shows & watchlist", c.mine_only).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.mine_only = true;
                        cx.notify();
                    }))),
            )
            .child({
                let mut d = div().flex().gap_1();
                for (id, label, t) in [("ct-all", "All", CalTypes::All), ("ct-mov", "Movies", CalTypes::Movies), ("ct-ser", "Series", CalTypes::Series)] {
                    d = d.child(seg(self, id, label, c.types == t).on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.types = t;
                        cx.notify();
                    })));
                }
                d
            })
            .child({
                let mut d = div().flex().gap_1();
                for (id, label, r) in [("cr-all", "All releases", CalRelease::All), ("cr-th", "Cinema", CalRelease::Theatrical), ("cr-dg", "Digital", CalRelease::Digital)] {
                    d = d.child(seg(self, id, label, c.release == r).on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.release = r;
                        cx.notify();
                    })));
                }
                d
            })
            .child(div().flex_1())
            .child(self.button("cal-refresh", "↻", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.load_calendar(true, cx))));

        let legend = div()
            .flex()
            .flex_wrap()
            .gap_4()
            .text_xs()
            .text_color(rgb(p.muted))
            .children([ReleaseKind::Episode, ReleaseKind::Premiere, ReleaseKind::Theatrical, ReleaseKind::Digital].map(|k| {
                div().flex().items_center().gap_1().child(div().size(px(8.0)).rounded_full().bg(rgb(kind_color(k, p)))).child(k.label())
            }))
            .child(div().flex_1())
            .child(div().text_color(rgb(p.faint)).child(format!(
                "Times in {} · ←→↑↓ day · PgUp/PgDn month · V view · M mine · Y type · D release · Enter open · W watchlist",
                self.svc.config().timezone
            )));

        let mut col = div().flex().flex_col().gap_4().child(div().flex().items_center().justify_between().gap_4().child(nav)).child(filters).child(legend);

        if !self.svc.has_tmdb() && !self.trakt.logged_in {
            return col
                .child(self.card(p).child(div().text_sm().text_color(rgb(p.muted)).child(
                    "The release calendar needs Trakt (Settings → Services → Connect) or a TMDB API key.",
                )))
                .into_any_element();
        }

        let body = match c.view {
            CalView::Month => div()
                .flex()
                .gap_5()
                .items_start()
                .child(div().flex_1().min_w_0().child(self.month_grid(&items, today, p, cx)))
                .child(div().w(px(340.0)).flex_none().child(self.day_panel(&items, today, p, cx)))
                .into_any_element(),
            CalView::Agenda => self.agenda(&items, today, p, cx),
        };
        col = col.child(body);
        col.into_any_element()
    }

    fn month_grid(&self, items: &[CalendarItem], today: NaiveDate, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.disc.calendar;
        let (start, _) = month_grid(c.year, c.month);
        let mut head = div().flex();
        for d in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] {
            head = head.child(div().flex_1().px_2().pb_2().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.faint)).child(d));
        }
        let mut grid = div().flex().flex_col().gap_1().child(head);
        for w in 0..6 {
            let mut row = div().flex().gap_1();
            for dow in 0..7 {
                let day = start + chrono::Duration::days(w * 7 + dow);
                let in_month = day.month() == c.month;
                let is_today = day == today;
                let sel = day == c.sel_day;
                let todays: Vec<&CalendarItem> = items.iter().filter(|i| i.date == day).collect();
                let mut cell = div()
                    .id(SharedString::from(format!("day-{day}")))
                    .flex_1()
                    .min_w_0()
                    .h(px(112.0))
                    .flex()
                    .flex_col()
                    .gap(px(3.0))
                    .p(px(6.0))
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(if is_today { p.accent_soft } else if in_month { p.surface } else { p.canvas }))
                    .border_1()
                    .border_color(rgb(if sel { p.text } else if is_today { p.accent } else { p.border_soft }))
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        this.disc.calendar.sel_day = day;
                        this.disc.calendar.sel_item = 0;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(if is_today { FontWeight::BOLD } else { FontWeight::NORMAL })
                                    .text_color(rgb(if is_today { p.accent } else if in_month { p.text } else { p.faint }))
                                    .child(if is_today { format!("{} · Today", day.day()) } else { day.day().to_string() }),
                            )
                            .when(todays.len() > 3, |d| d.child(div().text_xs().text_color(rgb(p.faint)).child(format!("{}", todays.len())))),
                    );
                for it in todays.iter().take(3) {
                    cell = cell.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .px_1()
                            .rounded_sm()
                            .bg(rgba(0xFFFFFF08))
                            .child(div().size(px(6.0)).flex_none().rounded_full().bg(rgb(kind_color(it.kind, p))))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .truncate()
                                    .text_color(rgb(if in_month { if it.mine { p.text } else { p.muted } } else { p.faint }))
                                    .when(it.mine, |d| d.font_weight(FontWeight::MEDIUM))
                                    .child(it.name.clone()),
                            ),
                    );
                }
                if todays.len() > 3 {
                    cell = cell.child(div().text_xs().text_color(rgb(p.faint)).child(format!("+{} more", todays.len() - 3)));
                }
                row = row.child(cell);
            }
            grid = grid.child(row);
        }
        grid.into_any_element()
    }

    fn release_row(&self, it: &CalendarItem, id: String, highlighted: bool, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let item = it.clone();
        let watch = it.title.as_ref().map(|t| TitleCard { title: t.clone(), poster: it.poster.clone() });
        let on_list = it.tmdb_id.is_some_and(|tid| self.disc.watch_ids.contains(&(it.title_kind, tid)));
        let local = it.tmdb_id.is_some_and(|tid| self.disc.local_ids.contains(&(it.title_kind, tid)));
        div()
            .id(SharedString::from(id.clone()))
            .flex()
            .items_center()
            .gap_3()
            .p_2()
            .rounded_lg()
            .cursor_pointer()
            .bg(rgb(if highlighted { p.accent_soft } else { p.surface }))
            .border_1()
            .border_color(rgb(if highlighted { p.accent } else { p.border_soft }))
            .hover(|s| s.bg(rgb(p.surface_raised)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_calendar_item(&item, window, cx)))
            .child(div().w(px(40.0)).h(px(60.0)).flex_none().rounded_md().overflow_hidden().child(poster(&it.name, it.poster.clone(), None)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(it.name.clone()))
                    .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(it.detail.clone()))
                    .child(
                        div()
                            .flex()
                            .gap_1()
                            .child(chip(it.kind.label(), 0xFFFFFF10, kind_color(it.kind, p)))
                            .when_some(it.time.clone(), |d, t| d.child(chip(t, 0xFFFFFF10, p.text)))
                            .when(it.mine, |d| d.child(chip("Mine", 0x8B7CFF33, p.accent)))
                            .when(local, |d| d.child(chip("In library", 0x5FD39A22, p.good))),
                    ),
            )
            .when_some(watch, |d, card| {
                d.child(
                    div()
                        .id(SharedString::from(format!("{id}-wl")))
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_xs()
                        .cursor_pointer()
                        .bg(rgb(if on_list { p.accent_soft } else { p.surface_raised }))
                        .text_color(rgb(if on_list { p.accent } else { p.muted }))
                        .hover(|s| s.text_color(rgb(p.text)))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                            cx.stop_propagation();
                            this.toggle_watchlist(&card, cx);
                        }))
                        .child(if on_list { "✓ Watchlist" } else { "+ Watchlist" }),
                )
            })
            .into_any_element()
    }

    fn day_panel(&self, items: &[CalendarItem], today: NaiveDate, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.disc.calendar;
        let day = c.sel_day;
        let list: Vec<&CalendarItem> = items.iter().filter(|i| i.date == day).collect();
        let title = if day == today { format!("Today · {}", day.format("%a %-d %b")) } else { day.format("%A %-d %B").to_string() };
        let mut col = self.card(p).p_4().gap_2().child(
            div()
                .flex()
                .justify_between()
                .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(div().text_xs().text_color(rgb(p.muted)).child(format!("{} releases", list.len()))),
        );
        if list.is_empty() {
            col = col.child(div().py_4().text_sm().text_color(rgb(p.muted)).child(if c.loading { "Loading…" } else { "Nothing releases this day." }));
        }
        for (i, it) in list.iter().enumerate() {
            col = col.child(self.release_row(it, format!("dp-{i}"), i == c.sel_item, p, cx));
        }
        col.into_any_element()
    }

    fn agenda(&self, items: &[CalendarItem], today: NaiveDate, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let c = &self.disc.calendar;
        let mut col = div().flex().flex_col().gap_4();
        let mut days: Vec<NaiveDate> = items.iter().map(|i| i.date).filter(|d| d.month() == c.month).collect();
        days.dedup();
        if days.is_empty() {
            return col
                .child(div().p_6().text_sm().text_color(rgb(p.muted)).child(if c.loading { "Loading…" } else { "No releases match these filters this month." }))
                .into_any_element();
        }
        for day in days {
            let is_today = day == today;
            let list: Vec<&CalendarItem> = items.iter().filter(|i| i.date == day).collect();
            let mut grid = div().flex().flex_wrap().gap_2();
            for (i, it) in list.iter().enumerate() {
                let hl = day == c.sel_day && i == c.sel_item;
                grid = grid.child(div().w(px(380.0)).child(self.release_row(it, format!("ag-{day}-{i}"), hl, p, cx)));
            }
            col = col.child(
                div()
                    .flex()
                    .gap_5()
                    .child(
                        div()
                            .w(px(84.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_center()
                            .py_2()
                            .rounded_lg()
                            .bg(rgb(if is_today { p.accent_soft } else { p.surface }))
                            .border_1()
                            .border_color(rgb(if is_today { p.accent } else if day == c.sel_day { p.text } else { p.border_soft }))
                            .child(div().text_xs().text_color(rgb(if is_today { p.accent } else { p.muted })).child(if is_today {
                                "TODAY".to_string()
                            } else {
                                day.format("%a").to_string().to_uppercase()
                            }))
                            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(day.day().to_string()))
                            .child(div().text_xs().text_color(rgb(p.faint)).child(day.format("%b").to_string())),
                    )
                    .child(div().flex_1().min_w_0().child(grid)),
            );
        }
        col.into_any_element()
    }
}
