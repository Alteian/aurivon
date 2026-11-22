//! Shared widgets and the window shell (sidebar, top bar, mini player, toasts).

use std::path::PathBuf;

use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::slider::Slider;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{format_time, is_audio, LibraryEntry, TitleCard, TitleKind};

use crate::theme::{initials, tile_colors, Palette};
use crate::view::{MediaHubView, Nav, Page};

pub fn gradient(seed: &str) -> Background {
    let (c1, c2) = tile_colors(seed);
    linear_gradient(160.0, linear_color_stop(rgb(c1), 0.0), linear_color_stop(rgb(c2), 1.0))
}

/// Poster image, or a gradient tile with initials
pub fn poster(seed: &str, path: Option<PathBuf>, glyph: Option<&str>) -> Div {
    let base = div().relative().size_full().overflow_hidden().bg(gradient(seed));
    match path {
        Some(p) => base.child(img(p).size_full().object_fit(ObjectFit::Cover)),
        None => base.flex().items_center().justify_center().child(
            div()
                .text_3xl()
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(0xFFFFFFCC))
                .child(glyph.map(str::to_string).unwrap_or_else(|| initials(seed))),
        ),
    }
}

/// The Aurivon mark (PNG embedded in the binary)
pub fn logo(size: f32) -> Img {
    static LOGO: std::sync::LazyLock<std::sync::Arc<Image>> = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, include_bytes!("../assets/logo-128.png").to_vec()))
    });
    img(LOGO.clone()).size(px(size)).flex_none()
}

pub fn chip(text: impl Into<SharedString>, bg: u32, fg: u32) -> Div {
    div().px_2().py(px(2.0)).rounded_md().bg(rgba(bg)).text_xs().text_color(rgb(fg)).child(text.into())
}

pub fn progress_line(frac: f32, p: Palette) -> Div {
    div()
        .absolute()
        .bottom_0()
        .left_0()
        .w_full()
        .h(px(4.0))
        .bg(rgba(0xFFFFFF22))
        .child(div().h_full().w(relative(frac.clamp(0.0, 1.0))).bg(rgb(p.accent)))
}

pub fn entry_progress(e: &LibraryEntry) -> Option<f32> {
    let total = e.item.runtime_minutes.filter(|m| *m > 0).map(|m| m as f32 * 60.0)?;
    (e.item.watch_progress > 0 && !e.item.watched).then(|| (e.item.watch_progress as f32 / total).clamp(0.02, 1.0))
}

impl MediaHubView {
    pub(crate) fn button(&self, id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool, p: Palette) -> Stateful<Div> {
        let d = div()
            .id(id.into())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_2()
            .px_4()
            .py_2()
            .rounded_lg()
            .text_sm()
            .cursor_pointer()
            .child(label.into());
        if primary {
            d.bg(rgb(p.accent)).text_color(rgb(p.on_accent)).font_weight(FontWeight::MEDIUM).hover(|s| s.opacity(0.9))
        } else {
            d.bg(rgb(p.surface_raised))
                .border_1()
                .border_color(rgb(p.border))
                .text_color(rgb(p.text))
                .hover(|s| s.border_color(rgb(p.accent)))
        }
    }

    /// Segmented / pill option
    pub(crate) fn pill(&self, id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, p: Palette) -> Stateful<Div> {
        div()
            .id(id.into())
            .px_3()
            .py(px(6.0))
            .rounded_full()
            .text_sm()
            .cursor_pointer()
            .border_1()
            .border_color(rgb(if on { p.accent } else { p.border }))
            .bg(rgb(if on { p.accent_soft } else { p.surface_raised }))
            .text_color(rgb(if on { p.text } else { p.muted }))
            .hover(|s| s.border_color(rgb(p.accent)))
            .child(label.into())
    }

    pub(crate) fn section_title(&self, title: impl Into<SharedString>, p: Palette) -> Div {
        div().text_lg().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.text)).child(title.into())
    }

    pub(crate) fn card(&self, p: Palette) -> Div {
        div().flex().flex_col().gap_4().p_6().rounded_2xl().bg(rgb(p.surface)).border_1().border_color(rgb(p.border_soft))
    }

    pub(crate) fn add_library_row(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().w(px(340.0)).child(gpui_kit::component::input::Input::new(&self.path_input).w_full()))
            .child(
                self.button("add-lib", "Add folder", true, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.add_library_from_input(window, cx))),
            )
            .child(self.button("browse-lib", "Browse…", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.browse_folder(cx))))
    }

    pub(crate) fn empty_state(&self, title: &str, body: &str, p: Palette, cx: &mut Context<Self>) -> AnyElement {
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
            .child(logo(72.0))
            .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
            .child(div().text_sm().text_color(rgb(p.muted)).child(body.to_string()))
            .child(self.add_library_row(p, cx))
            .into_any_element()
    }

    /// Portrait card for a library entry. `open` = details page, else play.
    pub(crate) fn entry_card(&self, e: &LibraryEntry, key: &str, width: f32, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = e.item.id;
        let audio = is_audio(&e.item);
        let playing = self.now.as_ref().and_then(|n| n.media_id) == Some(id);
        let title = e.display_title();
        let sub = e.subtitle();
        let progress = entry_progress(e);
        let resume = (e.item.watch_progress > 0 && !e.item.watched).then(|| format!("Resume {}", format_time(e.item.watch_progress as f64)));
        div()
            .id(SharedString::from(format!("card-{key}-{id}")))
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                // Double-click (or a click on something we can't describe) plays
                if ev.click_count() >= 2 {
                    this.play_item(id, window, cx)
                } else {
                    this.open_entry(id, window, cx)
                }
            }))
            .child(
                div()
                    .relative()
                    .h(px(width * 1.5))
                    .w_full()
                    .rounded_xl()
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(if playing { p.accent } else { p.border_soft }))
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .child(poster(&title, e.poster(), audio.then_some("♪")))
                    .child(div().absolute().top_2().left_2().child(chip(
                        match e.kind() {
                            Some(TitleKind::Tv) => "SERIES",
                            Some(TitleKind::Movie) => "MOVIE",
                            None if audio => "AUDIO",
                            None => "VIDEO",
                        },
                        0x000000AA,
                        p.text,
                    )))
                    .when(e.item.watched, |d| d.child(div().absolute().top_2().right_2().child(chip("✓", 0x5FD39AEE, 0x062014))))
                    .when(playing, |d| d.child(div().absolute().bottom_2().left_2().child(chip("▶ Playing", (p.accent << 8) | 0xFF, p.on_accent))))
                    .when_some(progress, |d, f| d.child(progress_line(f, p))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().text_color(rgb(p.text)).truncate().child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(p.muted))
                            .truncate()
                            .child(resume.or(sub).unwrap_or_else(|| human_size(e.item.file_size))),
                    ),
            )
            .into_any_element()
    }

    /// Wide "continue watching" card with a big progress bar
    pub(crate) fn wide_entry_card(&self, e: &LibraryEntry, key: &str, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let id = e.item.id;
        let title = e.display_title();
        let frac = entry_progress(e);
        let left = e.item.runtime_minutes.filter(|m| *m > 0).map(|m| {
            let rem = (m as i64 * 60 - e.item.watch_progress).max(0);
            format!("{} min left", (rem + 59) / 60)
        });
        div()
            .id(SharedString::from(format!("wide-{key}-{id}")))
            .w(px(340.0))
            .flex_none()
            .flex()
            .gap_3()
            .p_3()
            .rounded_xl()
            .bg(rgb(p.surface))
            .border_1()
            .border_color(rgb(p.border_soft))
            .hover(|s| s.border_color(rgb(p.accent)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.play_item(id, window, cx)))
            .child(div().w(px(84.0)).h(px(126.0)).flex_none().rounded_lg().overflow_hidden().child(poster(&title, e.poster(), None)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).truncate().child(title.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(e.subtitle().unwrap_or_default()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(p.faint))
                                    .line_clamp(2)
                                    .child(e.item.overview.clone().unwrap_or_default()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .h(px(4.0))
                                    .w_full()
                                    .rounded_full()
                                    .bg(rgb(p.border))
                                    .child(div().h_full().rounded_full().w(relative(frac.unwrap_or(0.0))).bg(rgb(p.accent))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(rgb(p.muted))
                                    .child(format!("▶ Resume {}", format_time(e.item.watch_progress as f64)))
                                    .child(left.unwrap_or_default()),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Card for a TMDB title (Discover, Trakt lists)
    pub(crate) fn title_card(&self, c: &TitleCard, key: &str, width: f32, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        self.title_card_sel(c, key, width, false, p, cx)
    }

    /// Poster card; `sel` = keyboard focus ring
    pub(crate) fn title_card_sel(&self, c: &TitleCard, key: &str, width: f32, sel: bool, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let card = c.clone();
        let wl_card = c.clone();
        let t = &c.title;
        let local = self.disc.local_ids.contains(&(t.kind, t.tmdb_id));
        let listed = self.disc.watch_ids.contains(&(t.kind, t.tmdb_id));
        let wl_id = SharedString::from(format!("wl-{key}-{}-{}", t.kind.as_str(), t.tmdb_id));
        div()
            .id(SharedString::from(format!("title-{key}-{}-{}", t.kind.as_str(), t.tmdb_id)))
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_title(&card, None, window, cx)))
            .child(
                div()
                    .relative()
                    .h(px(width * 1.5))
                    .w_full()
                    .rounded_xl()
                    .overflow_hidden()
                    .border_2()
                    .border_color(rgb(if sel { p.text } else { p.border_soft }))
                    .when(sel, |d| d.shadow_lg())
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .child(self.lazy_poster(&t.name, c.poster.clone(), rmh_app::images::poster_url(t), None))
                    .when(local, |d| d.child(div().absolute().bottom_2().left_2().child(chip("▶ In library", 0x0F2A1EE6, p.good))))
                    .child(
                        div()
                            .id(wl_id)
                            .absolute()
                            .bottom_2()
                            .right_2()
                            .size(px(28.0))
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
                    .when_some(t.rating.filter(|r| *r > 0.0), |d, r| {
                        d.child(div().absolute().top_2().right_2().child(chip(format!("★ {r:.1}"), 0x000000AA, p.gold)))
                    })
                    .child(div().absolute().top_2().left_2().child(chip(
                        if t.kind == TitleKind::Tv { "SERIES" } else { "MOVIE" },
                        0x000000AA,
                        p.text,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().truncate().child(t.name.clone()))
                    .child(div().text_xs().text_color(rgb(p.muted)).child(t.year.map(|y| y.to_string()).unwrap_or_default())),
            )
            .into_any_element()
    }

    /// Horizontal scrolling row
    pub(crate) fn row(&self, id: &str, items: Vec<AnyElement>) -> AnyElement {
        div()
            .id(SharedString::from(format!("row-{id}")))
            .w_full()
            .overflow_x_scroll()
            .child(div().flex().gap_4().pb_2().children(items))
            .into_any_element()
    }

    pub(crate) fn grid(&self, items: Vec<AnyElement>) -> AnyElement {
        div().flex().flex_wrap().gap_5().children(items).into_any_element()
    }

    pub(crate) fn section(&self, title: impl Into<SharedString>, extra: Option<String>, body: AnyElement, p: Palette) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_3()
                    .child(self.section_title(title, p))
                    .when_some(extra, |d, e| d.child(div().text_xs().text_color(rgb(p.muted)).child(e))),
            )
            .child(body)
            .into_any_element()
    }

    // ------------------------------------------------------------- the shell

    fn nav_selected(&self, item: Nav) -> bool {
        match item {
            Nav::Page(page) => self.page == page,
            Nav::Search => self.disc.search.open,
            Nav::Movies => self.page == Page::Discover && self.disc.browse.kind == TitleKind::Movie,
            Nav::Series => self.page == Page::Discover && self.disc.browse.kind == TitleKind::Tv,
        }
    }

    fn nav_items(&self) -> Vec<Nav> {
        Nav::ALL.into_iter().filter(|i| *i != Nav::Page(Page::Player) || self.now.is_some()).collect()
    }

    /// Logo mark + optional wordmark
    pub(crate) fn brand_lockup(&self, size: f32, wordmark: bool, p: Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(logo(size))
            .when(wordmark, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_base().font_weight(FontWeight::BOLD).text_color(rgb(p.gold)).child("AURIVON"))
                        .child(div().text_xs().text_color(rgb(p.muted)).child("Media center")),
                )
            })
    }

    fn sidebar(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let rail = self.svc.config().nav_style == "rail";
        let mut nav = div().flex().flex_col().gap_1();
        for item in self.nav_items() {
            let selected = self.nav_selected(item);
            let accent_glyph = selected || item == Nav::Page(Page::Player);
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("nav-{}", item.label())))
                    .flex()
                    .items_center()
                    .when(rail, |d| d.justify_center().py_2())
                    .when(!rail, |d| d.gap_3().px_3().py_2())
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(if selected { p.active } else { p.sidebar }))
                    .text_color(rgb(if selected { p.text } else { p.muted }))
                    .hover(|s| s.bg(rgb(p.border_soft)).text_color(rgb(p.text)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.nav_to(item, window, cx)))
                    .child(
                        div()
                            .size(px(if rail { 36.0 } else { 28.0 }))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(rgb(if selected { p.accent_soft } else { p.surface_raised }))
                            .text_color(rgb(if accent_glyph { p.accent } else { p.muted }))
                            .child(item.glyph()),
                    )
                    .when(!rail, |d| {
                        d.child(div().flex_1().text_sm().child(item.label()))
                            .when(item == Nav::Search, |d| d.child(div().text_xs().text_color(rgb(p.faint)).child("Ctrl K")))
                            .when(selected, |d| d.child(div().w(px(3.0)).h(px(14.0)).rounded_full().bg(rgb(p.accent))))
                    }),
            );
        }

        let watched = self.entries.iter().filter(|e| e.item.watched).count();
        let trakt = match (&self.trakt.username, self.trakt.logged_in) {
            (Some(u), true) => Some((true, format!("● Trakt: {u}"))),
            (None, true) => Some((true, "● Trakt connected".to_string())),
            _ if self.trakt.configured => Some((false, "● Trakt: not logged in".to_string())),
            _ => None,
        };
        div()
            .w(px(if rail { crate::theme::tokens::RAIL_W } else { crate::theme::tokens::SIDEBAR_W }))
            .flex_none()
            .flex()
            .flex_col()
            .h_full()
            .when(rail, |d| d.px_2().items_center())
            .when(!rail, |d| d.px_4())
            .py_5()
            .gap_6()
            .bg(rgb(p.sidebar))
            .border_r_1()
            .border_color(rgb(p.border))
            .child(div().px_2().child(self.brand_lockup(if rail { 40.0 } else { 38.0 }, !rail, p)))
            .child(nav)
            .child(div().flex_1())
            .when(!rail, |d| {
                d.child(
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
                        .child(format!("{} items · {} libraries", self.entries.len(), self.libraries.len()))
                        .child(format!("{watched} watched"))
                        .child(div().text_color(rgb(if self.mpv_version.is_some() { p.good } else { p.danger })).child(
                            match &self.mpv_version {
                                Some(v) => format!("● {v}{}", if self.embedded() { " · embedded" } else { "" }),
                                None => "● mpv not found".to_string(),
                            },
                        ))
                        .when_some(trakt, |d, (ok, t)| d.child(div().text_color(rgb(if ok { p.good } else { p.warn })).child(t))),
                )
            })
            .into_any_element()
    }

    /// "Top bar" navigation style: brand + horizontal nav above the page
    fn top_nav(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut nav = div().flex().items_center().gap_1();
        for item in self.nav_items() {
            let selected = self.nav_selected(item);
            nav = nav.child(
                div()
                    .id(SharedString::from(format!("tnav-{}", item.label())))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py(px(6.0))
                    .rounded_full()
                    .cursor_pointer()
                    .text_sm()
                    .bg(rgb(if selected { p.accent_soft } else { p.sidebar }))
                    .text_color(rgb(if selected { p.accent } else { p.muted }))
                    .hover(|s| s.text_color(rgb(p.text)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.nav_to(item, window, cx)))
                    .child(div().child(item.glyph()))
                    .child(item.label()),
            );
        }
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_6()
            .px_6()
            .py_3()
            .bg(rgb(p.sidebar))
            .border_b_1()
            .border_color(rgb(p.border))
            .child(self.brand_lockup(32.0, true, p))
            .child(nav)
            .into_any_element()
    }

    fn top_bar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let title = match self.page {
            Page::Details => self.details.as_ref().map(|d| d.name.clone()).unwrap_or_default(),
            Page::Person => self.disc.person.as_ref().map(|d| d.name.clone()).unwrap_or_default(),
            Page::Player => self.now.as_ref().map(|n| n.title.clone()).unwrap_or_default(),
            Page::Discover => if self.disc.browse.kind == TitleKind::Movie { "Movies" } else { "Series" }.to_string(),
            other => other.label().to_string(),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_8()
            .py_4()
            .border_b_1()
            .border_color(rgb(p.border_soft))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).truncate().child(title))
                    .when(!self.page.subtitle().is_empty(), |d| d.child(div().text_sm().text_color(rgb(p.muted)).child(self.page.subtitle()))),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .when_some(self.busy.clone(), |d, b| d.child(div().size(px(8.0)).rounded_full().bg(rgb(p.warn))).child(b))
                    .when(self.busy.is_none(), |d| {
                        d.when_some(self.status.clone(), |d, s| d.child(div().size(px(8.0)).rounded_full().bg(rgb(p.good))).child(s))
                    })
                    .child(
                        self.icon_btn(
                            "top-fullscreen",
                            if self.app_fullscreen || self.theater { "⤡" } else { "⛶" },
                            "Fullscreen (F11, Esc to leave)",
                            p,
                        )
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.toggle_fullscreen(window, cx))),
                    ),
            )
    }

    /// Compact player bar shown on other pages while something plays
    fn mini_player(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.now.as_ref().expect("caller checks");
        let poster_path = self.now_entry.as_ref().and_then(|e| e.poster());
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
                    .id("mini-open")
                    .w(px(260.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.navigate(Page::Player, window, cx)))
                    .child(div().w(px(36.0)).h(px(54.0)).flex_none().rounded_md().overflow_hidden().child(poster(&n.title, poster_path, None)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(n.title.clone()))
                            .child(div().text_xs().text_color(rgb(p.accent)).child(if self.embedded() { "Open player ↗" } else { "Player controls ↗" })),
                    ),
            )
            .child(self.transport(p, false, cx))
            .child(div().flex_1().child(self.seek_row(p, cx)))
            .child(
                div()
                    .w(px(150.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_xs().text_color(rgb(p.muted)).child(format!("{:.0}", self.volume)))
                    .child(div().flex_1().child(Slider::new(&self.volume_slider))),
            )
    }

    fn error_toast(&self, err: String, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("error-toast")
            .absolute()
            .top_5()
            .right_5()
            .max_w(px(460.0))
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
            .child(div().text_color(rgb(p.muted)).child("✕"))
    }
}

pub fn human_size(bytes: i64) -> String {
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
        let p = self.palette(window);
        if p.light != self.kit_light {
            // inputs / sliders from gpui-kit follow the light / dark mode
            self.kit_light = p.light;
            gpui_kit::component::theme::Theme::change(
                if p.light { gpui_kit::component::theme::ThemeMode::Light } else { gpui_kit::component::theme::ThemeMode::Dark },
                Some(window),
                cx,
            );
        }
        self.ensure_video_host(window);
        if self.disc.search.open && self.disc.search.focus_pending {
            self.disc.search.focus_pending = false;
            let input = self.disc.search.input.clone();
            input.update(cx, |s, cx| s.focus(window, cx));
        } else if window.focused(cx).is_none() {
            window.focus(&self.focus_handle, cx);
        }
        // The native video surface is only visible on the player page
        let on_player = self.page == Page::Player && self.now.is_some();
        // browse grid columns for keyboard navigation
        let vw = f32::from(window.viewport_size().width);
        let side = match self.svc.config().nav_style.as_str() {
            _ if self.theater => 0.0,
            "top" => 0.0,
            "rail" => crate::theme::tokens::RAIL_W,
            _ => crate::theme::tokens::SIDEBAR_W,
        };
        let content = vw - side - 64.0 - 14.0;
        self.disc.grid_cols = (((content + crate::browse::GRID_GAP) / (crate::browse::GRID_CARD + crate::browse::GRID_GAP)).floor() as usize).max(1);
        if self.disc.search.open {
            if let Some(v) = &self.video {
                v.hide();
            }
        }
        if !on_player {
            if let Some(v) = &self.video {
                v.hide();
            }
        }

        let main: AnyElement = if on_player {
            // Player page manages its own layout (video area can't scroll)
            div()
                .flex_1()
                .flex()
                .flex_col()
                .h_full()
                .min_w_0()
                .when(!self.theater, |d| d.child(self.top_bar(p, cx)))
                .child(self.player_page(p, window, cx))
                .into_any_element()
        } else {
            let content = match self.page {
                Page::Home => self.home_page(p, cx),
                Page::Library => self.library_page(p, cx),
                Page::Watchlist => self.watchlist_page(p, cx),
                Page::Discover => self.browse_page(p, window, cx),
                Page::Calendar => self.calendar_page(p, cx),
                Page::Details => self.details_page(p, cx),
                Page::Person => self.person_page(p, cx),
                Page::Settings => self.settings_page(p, cx),
                Page::Player => self.home_page(p, cx),
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .h_full()
                .min_w_0()
                .child(self.top_bar(p, cx))
                .when_some(self.update_banner(p, cx), |d, b| d.child(b))
                .child(div().id("content").flex_1().w_full().overflow_y_scrollbar().child(div().px_8().py_6().child(content)))
                .when(self.now.is_some(), |d| d.child(self.mini_player(p, cx)))
                .into_any_element()
        };

        let top_nav = self.svc.config().nav_style == "top";
        let mut shell = div()
            .track_focus(&self.focus_handle)
            .key_context("MediaHub")
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .flex()
            .size_full()
            .bg(rgb(p.canvas))
            .text_color(rgb(p.text))
            .when(!self.theater && !top_nav, |d| d.child(self.sidebar(p, cx)))
            .child(if top_nav && !self.theater {
                div().flex_1().flex().flex_col().h_full().min_w_0().child(self.top_nav(p, cx)).child(main).into_any_element()
            } else {
                main
            });
        if self.disc.search.open {
            shell = shell.child(self.search_overlay(p, cx));
        }
        if let Some(err) = self.error.clone() {
            shell = shell.child(self.error_toast(err, p, cx));
        }
        shell
    }
}
