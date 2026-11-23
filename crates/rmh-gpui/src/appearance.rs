//! Settings → Appearance: theme, accent, home layout, cards and navigation.
//! Every choice is saved and applied immediately (the app is the preview).

use gpui_kit::*;
use rmh_app::HomeLayout;

use crate::theme::{tokens, Palette, ACCENTS, THEMES};
use crate::view::MediaHubView;

fn theme_swatch(t: Palette) -> Div {
    // a tiny mock of the app in that theme: sidebar, hero, two cards
    div()
        .w(px(150.0))
        .h(px(92.0))
        .rounded(px(tokens::RADIUS_MD))
        .overflow_hidden()
        .flex()
        .bg(rgb(t.canvas))
        .child(div().w(px(30.0)).h_full().bg(rgb(t.sidebar)).flex().flex_col().gap_1().p_1().children(
            (0..4).map(move |i| div().h(px(6.0)).rounded_sm().bg(rgb(if i == 0 { t.accent } else { t.surface_raised }))),
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .p(px(6.0))
                .child(
                    div()
                        .h(px(36.0))
                        .rounded_sm()
                        .bg(linear_gradient(90.0, linear_color_stop(rgb(t.surface_raised), 0.0), linear_color_stop(rgb(t.accent_soft), 1.0)))
                        .p_1()
                        .child(div().w(px(40.0)).h(px(5.0)).rounded_sm().bg(rgb(t.text)))
                        .child(div().mt_1().w(px(22.0)).h(px(7.0)).rounded_sm().bg(rgb(t.accent))),
                )
                .child(div().flex().gap_1().children((0..4).map(move |_| div().w(px(20.0)).h(px(30.0)).rounded_sm().bg(rgb(t.surface))))),
        )
}

impl MediaHubView {
    fn option_row(&self, label: &str, body: impl IntoElement, p: Palette) -> Div {
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(div().w(px(130.0)).flex_none().text_sm().font_weight(FontWeight::MEDIUM).text_color(rgb(p.muted)).child(label.to_string()))
            .child(body)
    }

    pub(crate) fn appearance_card(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let cfg = self.svc.config();

        // themes
        let mut themes = div().flex().flex_wrap().gap_3();
        for (id, label, t) in THEMES {
            let on = cfg.theme == id;
            themes = themes.child(
                div()
                    .id(SharedString::from(format!("theme-{id}")))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_1()
                    .rounded(px(tokens::RADIUS_LG))
                    .cursor_pointer()
                    .border_2()
                    .border_color(rgb(if on { p.accent } else { p.border_soft }))
                    .hover(|s| s.border_color(rgb(p.accent)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_appearance(|c| c.theme = id.to_string(), cx)))
                    .child(theme_swatch(t))
                    .child(div().px_1().pb_1().text_xs().text_color(rgb(if on { p.text } else { p.muted })).child(label)),
            );
        }

        // accent
        let mut accents = div().flex().items_center().gap_2().child(
            self.pill("accent-theme", "Theme default", cfg.accent_color.is_none(), p)
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.set_appearance(|c| c.accent_color = None, cx))),
        );
        for a in ACCENTS {
            let hex = format!("#{a:06X}");
            let on = cfg.accent_color.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&hex));
            accents = accents.child(
                div()
                    .id(SharedString::from(format!("accent-{a:06X}")))
                    .size(px(26.0))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(rgb(a))
                    .border_2()
                    .border_color(rgb(if on { p.text } else { p.canvas }))
                    .hover(|s| s.border_color(rgb(p.text)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                        let hex = hex.clone();
                        this.set_appearance(|c| c.accent_color = Some(hex), cx)
                    })),
            );
        }

        let pills = |this: &Self, key: &'static str, opts: &[(&'static str, &'static str)], cur: &str, set: fn(&mut rmh_app::AppConfig, &str), cx: &mut Context<Self>| {
            let mut row = div().flex().flex_wrap().gap_2();
            for (v, label) in opts.iter().copied() {
                row = row.child(
                    this.pill(SharedString::from(format!("{key}-{v}")), label, cur == v, p)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_appearance(|c| set(c, v), cx))),
                );
            }
            row
        };

        let mut layouts = div().flex().flex_wrap().gap_2();
        for l in HomeLayout::ALL {
            layouts = layouts.child(
                self.pill(SharedString::from(format!("set-layout-{}", l.letter())), l.label(), cfg.home_layout == l, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_home_layout(l, cx))),
            );
        }

        self.card(p)
            .gap_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(self.brand_lockup(44.0, true, p))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(rgb(p.faint)).child("Changes apply instantly")),
            )
            .child(self.section_title("Appearance", p))
            .child(self.option_row("Theme", themes, p))
            .child(self.option_row("Accent", accents, p))
            .child(
                self.toggle_row("tg-follow-system", "Follow system light / dark", "use the Light theme while the desktop is in light mode", cfg.theme_follow_system, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.set_appearance(|c| c.theme_follow_system = !c.theme_follow_system, cx))),
            )
            .child(self.option_row("Home layout", layouts, p))
            .child(self.option_row(
                "Cards",
                pills(self, "card-style", &[("poster", "Posters"), ("landscape", "Landscape backdrops")], &cfg.card_style, |c, v| c.card_style = v.into(), cx),
                p,
            ))
            .child(self.option_row(
                "Card size",
                pills(self, "card-size", &[("s", "Small"), ("m", "Medium"), ("l", "Large")], &cfg.card_size, |c, v| c.card_size = v.into(), cx),
                p,
            ))
            .child(self.option_row(
                "Navigation",
                pills(self, "nav-style", &[("expanded", "Sidebar"), ("rail", "Icon rail"), ("top", "Top bar")], &cfg.nav_style, |c, v| c.nav_style = v.into(), cx),
                p,
            ))
            .child(
                self.toggle_row(
                    "tg-couch",
                    "Couch mode",
                    "Larger cards for TV distance; on Library use ←→↑↓ to move focus and Enter to open",
                    cfg.couch_mode,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.set_appearance(|c| c.couch_mode = !c.couch_mode, cx);
                })),
            )
            .into_any_element()
    }
}
