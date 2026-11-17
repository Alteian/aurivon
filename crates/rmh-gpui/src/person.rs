//! Person page: photo, bio, known for and the full filmography.

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::TitleKind;

use crate::theme::Palette;
use crate::ui::{chip, poster};
use crate::view::MediaHubView;

impl MediaHubView {
    pub(crate) fn person_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(st) = &self.disc.person else { return div().into_any_element() };
        let page = st.page.as_ref();
        let person = page.map(|x| &x.person);
        let mut facts: Vec<String> = vec![];
        if let Some(d) = person.and_then(|x| x.department.clone()) {
            facts.push(d);
        }
        if let Some(b) = person.and_then(|x| x.birthday.clone()) {
            let age = person.and_then(|x| age(&b, x.deathday.as_deref()));
            facts.push(match (person.and_then(|x| x.deathday.clone()), age) {
                (Some(d), Some(a)) => format!("{b} – {d} ({a})"),
                (None, Some(a)) => format!("Born {b} ({a})"),
                _ => format!("Born {b}"),
            });
        }
        if let Some(pl) = person.and_then(|x| x.place_of_birth.clone()) {
            facts.push(pl);
        }
        if let Some(n) = page.map(|x| x.credits.len()).filter(|n| *n > 0) {
            facts.push(format!("{n} credits"));
        }
        // first paragraphs only, up to ~700 characters
        let bio = person.and_then(|x| x.biography.clone()).filter(|b| !b.trim().is_empty()).map(|b| {
            let mut out = String::new();
            for para in b.split("\n").map(str::trim).filter(|p| !p.is_empty()) {
                if !out.is_empty() && out.len() + para.len() > 700 {
                    break;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(para);
            }
            out
        });

        let header = div()
            .flex()
            .gap_8()
            .p_8()
            .rounded_2xl()
            .bg(linear_gradient(120.0, linear_color_stop(rgb(p.accent_soft), 0.0), linear_color_stop(rgb(p.surface), 0.7)))
            .border_1()
            .border_color(rgb(p.border_soft))
            .child(
                div()
                    .w(px(200.0))
                    .h(px(280.0))
                    .flex_none()
                    .rounded_xl()
                    .overflow_hidden()
                    .shadow_lg()
                    .child(poster(&st.name, page.and_then(|x| x.photo.clone()), None)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_3xl().font_weight(FontWeight::BOLD).child(st.name.clone()))
                    .child(div().text_sm().text_color(rgb(p.muted)).child(facts.join(" · ")))
                    .child(div().text_sm().line_clamp(7).child(bio.unwrap_or_else(|| {
                        if st.loading { "Loading…".into() } else { "No biography on TMDB.".into() }
                    })))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button("person-back", "← Back", false, p).on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                let back = this.back_page;
                                this.navigate(back, window, cx)
                            })))
                            .when_some(person.and_then(|x| x.imdb_id.clone()), |d, imdb| {
                                d.child(self.button("person-imdb", "IMDb ↗", false, p).on_click(cx.listener(move |_, _: &ClickEvent, _w, cx| {
                                    cx.open_url(&format!("https://www.imdb.com/name/{imdb}/"))
                                })))
                            }),
                    ),
            );
        let mut col = div().flex().flex_col().gap_6().child(header);
        let Some(page) = page else { return col.into_any_element() };

        if !page.known_for.is_empty() {
            let cards = page.known_for.iter().map(|c| self.title_card(c, "known", 150.0, p, cx)).collect();
            col = col.child(self.section("Known for", None, self.row("known", cards), p));
        }

        // filmography, filterable by department
        let mut depts: Vec<String> = vec![];
        for (_, _, d) in &page.credits {
            if !depts.contains(d) {
                depts.push(d.clone());
            }
        }
        let mut pills = div().flex().flex_wrap().gap_2().child(
            self.pill("dept-all", format!("All {}", page.credits.len()), st.dept.is_none(), p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                if let Some(s) = this.disc.person.as_mut() {
                    s.dept = None;
                }
                cx.notify();
            })),
        );
        for d in depts {
            let n = page.credits.iter().filter(|c| c.2 == d).count();
            let on = st.dept.as_deref() == Some(d.as_str());
            let dd = d.clone();
            pills = pills.child(self.pill(SharedString::from(format!("dept-{d}")), format!("{d} {n}"), on, p).on_click(cx.listener(
                move |this, _: &ClickEvent, _w, cx| {
                    if let Some(s) = this.disc.person.as_mut() {
                        s.dept = Some(dd.clone());
                    }
                    cx.notify();
                },
            )));
        }
        let mut list = div().flex().flex_col().gap_1();
        for (i, (card, role, dept)) in page.credits.iter().enumerate() {
            if st.dept.as_ref().is_some_and(|d| d != dept) {
                continue;
            }
            let c = card.clone();
            let t = &card.title;
            let local = self.disc.local_ids.contains(&(t.kind, t.tmdb_id));
            list = list.child(
                div()
                    .id(SharedString::from(format!("credit-{i}")))
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(p.surface_raised)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| this.open_title(&c, None, window, cx)))
                    .child(div().w(px(44.0)).text_sm().text_color(rgb(p.muted)).child(t.year.map(|y| y.to_string()).unwrap_or_else(|| "—".into())))
                    .child(div().w(px(32.0)).h(px(48.0)).flex_none().rounded_md().overflow_hidden().child(poster(&t.name, card.poster.clone(), None)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(t.name.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(role.clone())),
                    )
                    .when(local, |d| d.child(chip("In library", 0x5FD39A22, p.good)))
                    .child(chip(if t.kind == TitleKind::Tv { "Series" } else { "Movie" }, 0xFFFFFF10, p.muted))
                    .when_some(t.rating.filter(|r| *r > 0.0), |d, r| d.child(div().w(px(44.0)).text_xs().text_color(rgb(p.warn)).child(format!("★ {r:.1}")))),
            );
        }
        col = col.child(self.section("Filmography", None, div().flex().flex_col().gap_3().child(pills).child(self.card(p).p_3().gap_0().child(list)).into_any_element(), p));
        col.into_any_element()
    }
}

fn age(birth: &str, death: Option<&str>) -> Option<i32> {
    use chrono::Datelike;
    let b = chrono::NaiveDate::parse_from_str(birth, "%Y-%m-%d").ok()?;
    let e = match death {
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?,
        None => chrono::Local::now().date_naive(),
    };
    let mut a = e.year() - b.year();
    if (e.month(), e.day()) < (b.month(), b.day()) {
        a -= 1;
    }
    Some(a)
}
