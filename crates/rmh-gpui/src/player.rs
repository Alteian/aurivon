//! Player page: embedded video, transport, seek bar with chapter marks and
//! thumbnail previews, chapters / audio / subtitles / speed panel.

use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::slider::Slider;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{format_time, TrackKind};

use crate::theme::Palette;
use crate::ui::poster;
use gpui_kit::component::tooltip::Tooltip;

use crate::view::{MediaHubView, SideTab};

const SPEEDS: [f64; 8] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0];

impl MediaHubView {
    pub(crate) fn icon_btn(&self, id: &'static str, label: &'static str, tip: &'static str, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .min_w(px(36.0))
            .h(px(36.0))
            .px_2()
            .flex()
            .items_center()
            .justify_center()
            .rounded_lg()
            .cursor_pointer()
            .text_sm()
            .text_color(rgb(p.text))
            .hover(|s| s.bg(rgb(p.surface_raised)))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
            .child(label)
    }

    /// Play/pause, ±10 s, chapter skip (and frame step on the full player)
    pub(crate) fn transport(&self, p: Palette, full: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let paused = self.snap.paused || self.now.as_ref().is_some_and(|n| n.paused);
        let has_chapters = self.snap.chapters.len() > 1;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .when(full && has_chapters, |d| {
                d.child(
                    self.icon_btn("tp-prev-ch", "⏮", "Previous chapter (PgUp)", p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.step_chapter(-1, cx))),
                )
            })
            .child(
                self.icon_btn("tp-back", "↺10", "Back 10 s (←, Shift+← 60 s)", p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.seek_relative(-10.0, cx))),
            )
            .child(
                div()
                    .id("tp-play")
                    .size(px(44.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(rgb(p.text))
                    .text_color(rgb(p.canvas))
                    .hover(|s| s.opacity(0.9))
                    .child(if paused { "▶" } else { "❚❚" })
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.toggle_pause(cx))),
            )
            .child(
                self.icon_btn("tp-fwd", "10↻", "Forward 10 s (→, Shift+→ 60 s)", p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.seek_relative(10.0, cx))),
            )
            .when(full && has_chapters, |d| {
                d.child(
                    self.icon_btn("tp-next-ch", "⏭", "Next chapter (PgDn)", p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.step_chapter(1, cx))),
                )
            })
            .when(full, |d| {
                d.child(div().w(px(10.0)))
                    .child(
                        self.icon_btn("tp-fback", "◂|", "Previous frame (,)", p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.frame_step(false, cx))),
                    )
                    .child(
                        self.icon_btn("tp-ffwd", "|▸", "Next frame (.)", p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.frame_step(true, cx))),
                    )
            })
    }

    /// Time · seek bar (chapter marks, hover preview) · duration
    pub(crate) fn seek_row(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let pos = self.snap.position.max(self.now.as_ref().map(|n| n.position).unwrap_or(0.0));
        let dur = self.snap.duration.or(self.now.as_ref().and_then(|n| n.duration));
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().w(px(56.0)).text_xs().text_color(rgb(p.muted)).child(format_time(pos)))
            .child(div().flex_1().child(self.seek_bar(pos, dur, p, cx)))
            .child(div().w(px(56.0)).text_xs().text_color(rgb(p.muted)).child(dur.map(format_time).unwrap_or_else(|| "--:--".into())))
    }

    fn seek_bar(&self, pos: f64, dur: Option<f64>, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let d = dur.filter(|d| *d > 0.0);
        let frac = d.map(|d| (pos / d).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
        let cell = self.seek_bounds.clone();
        let mut bar = div()
            .id("seekbar")
            .relative()
            .h(px(20.0))
            .w_full()
            .cursor_pointer()
            .flex()
            .items_center()
            .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
            .child(
                div()
                    .w_full()
                    .h(px(5.0))
                    .rounded_full()
                    .bg(rgb(p.border))
                    .child(div().h_full().rounded_full().w(relative(frac)).bg(rgb(p.accent))),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _w, cx| this.hover_seek_at(ev.position.x, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                if !*hovered {
                    this.hover_seek = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _w, cx| this.click_seek_at(ev.position.x, cx)));
        // Chapter marks
        if let Some(d) = d {
            for (i, ch) in self.snap.chapters.iter().enumerate().skip(1) {
                let f = (ch.time / d).clamp(0.0, 1.0) as f32;
                bar = bar.child(
                    div()
                        .id(SharedString::from(format!("chmark-{i}")))
                        .absolute()
                        .left(relative(f))
                        .top(px(4.0))
                        .w(px(3.0))
                        .h(px(12.0))
                        .rounded_sm()
                        .bg(rgb(p.text))
                        .opacity(0.8),
                );
            }
        }
        bar.child(
            div()
                .absolute()
                .left(relative(frac))
                .top(px(3.0))
                .ml(px(-7.0))
                .size(px(14.0))
                .rounded_full()
                .bg(rgb(p.text))
                .border_2()
                .border_color(rgb(p.accent)),
        )
    }

    /// Floating preview above the seek bar while hovering it
    fn seek_preview(&self, p: Palette) -> Option<AnyElement> {
        let secs = self.hover_seek?;
        let d = self.snap.duration.filter(|d| *d > 0.0)?;
        let frac = (secs / d).clamp(0.0, 1.0) as f32;
        let chapter = self
            .snap
            .chapters
            .iter()
            .rev()
            .find(|c| c.time <= secs)
            .filter(|_| self.snap.chapters.len() > 1)
            .map(|c| c.title.clone());
        let bucket = (secs / 5.0).floor() as u64 * 5;
        let thumb = self.thumb.as_ref().filter(|(b, _)| b.abs_diff(bucket) <= 30).map(|(_, p)| p.clone());
        // Drawn over the controls row: the native video surface above would hide it
        Some(
            div()
                .absolute()
                .top(px(34.0))
                .left(relative(frac))
                .ml(px(-130.0))
                .w(px(260.0))
                .flex()
                .gap_2()
                .p_1()
                .rounded_lg()
                .bg(rgb(p.surface_raised))
                .border_1()
                .border_color(rgb(p.accent))
                .child(
                    div()
                        .w(px(124.0))
                        .h(px(70.0))
                        .flex_none()
                        .rounded_md()
                        .overflow_hidden()
                        .bg(rgb(0x000000))
                        .flex()
                        .items_center()
                        .justify_center()
                        .map(|el| match thumb {
                            Some(path) => el.child(img(path).size_full().object_fit(ObjectFit::Cover)),
                            None => el.child(div().text_xs().text_color(rgb(p.faint)).child("…")),
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap_1()
                        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.text)).child(format_time(secs)))
                        .when_some(chapter, |el, c| el.child(div().text_xs().text_color(rgb(p.muted)).line_clamp(2).child(c))),
                )
                .into_any_element(),
        )
    }

    fn video_area(&self, p: Palette, window: &mut Window) -> AnyElement {
        let Some(host) = self.video.clone() else {
            // Separate mpv window: show what is playing instead
            let n = self.now.as_ref().expect("caller checks");
            let poster_path = self.now_entry.as_ref().and_then(|e| e.poster());
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .gap_8()
                .bg(rgb(0x05060A))
                .child(div().w(px(200.0)).h(px(300.0)).rounded_xl().overflow_hidden().child(poster(&n.title, poster_path, None)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .max_w(px(420.0))
                        .child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(n.title.clone()))
                        .child(div().text_sm().text_color(rgb(p.muted)).child("Video is playing in mpv's own window (Settings → Player → Video: separate window). All controls below work on it."))
                )
                .into_any_element();
        };
        let scale = window.scale_factor();
        let covered = self.disc.search.open;
        div()
            .flex_1()
            .min_h(px(120.0))
            .bg(rgb(0x000000))
            .child(
                canvas(
                    move |b, _, _| b,
                    move |b, _, _, _| {
                        if covered {
                            host.hide();
                            return;
                        }
                        host.place(
                            f32::from(b.origin.x) * scale,
                            f32::from(b.origin.y) * scale,
                            f32::from(b.size.width) * scale,
                            f32::from(b.size.height) * scale,
                        );
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    fn side_panel(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = |this: &Self, t: SideTab, label: &'static str, count: usize, cx: &mut Context<Self>| {
            let on = this.side_tab == t;
            div()
                .id(SharedString::from(format!("tab-{label}")))
                .flex_1()
                .py_2()
                .flex()
                .justify_center()
                .text_xs()
                .cursor_pointer()
                .border_b_2()
                .border_color(rgb(if on { p.accent } else { p.border_soft }))
                .text_color(rgb(if on { p.text } else { p.muted }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                    this.side_tab = t;
                    cx.notify();
                }))
                .child(if count > 0 { format!("{label} {count}") } else { label.to_string() })
        };
        let tracks = |k: TrackKind| self.snap.tracks.iter().filter(|t| t.kind == k).count();
        let row = |id: String, label: String, sub: Option<String>, on: bool| {
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .rounded_lg()
                .cursor_pointer()
                .bg(rgb(if on { p.accent_soft } else { p.surface }))
                .hover(|s| s.bg(rgb(p.surface_raised)))
                .child(div().w(px(14.0)).text_color(rgb(p.accent)).child(if on { "●" } else { "" }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().text_sm().truncate().child(label))
                        .when_some(sub, |d, s| d.child(div().text_xs().text_color(rgb(p.muted)).child(s))),
                )
        };

        let mut body = div().flex().flex_col().gap_1();
        match self.side_tab {
            SideTab::Chapters => {
                if self.snap.chapters.is_empty() {
                    body = body.child(div().p_3().text_sm().text_color(rgb(p.muted)).child("This file has no chapters."));
                }
                for (i, ch) in self.snap.chapters.iter().enumerate() {
                    let on = self.snap.chapter == Some(i as i64);
                    body = body.child(
                        row(format!("ch-{i}"), ch.title.clone(), Some(format_time(ch.time)), on)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.go_chapter(i, cx))),
                    );
                }
            }
            SideTab::Audio | SideTab::Subtitles => {
                let kind = if self.side_tab == SideTab::Audio { TrackKind::Audio } else { TrackKind::Sub };
                if kind == TrackKind::Sub {
                    let off = !self.snap.tracks.iter().any(|t| t.kind == TrackKind::Sub && t.selected);
                    body = body.child(
                        row("sub-off".into(), "Off".into(), None, off)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.select_track(TrackKind::Sub, None, cx))),
                    );
                }
                for t in self.snap.tracks.iter().filter(|t| t.kind == kind) {
                    let id = t.id;
                    body = body.child(
                        row(format!("trk-{}-{id}", kind.property()), t.label(), Some(format!("#{id}")), t.selected)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.select_track(kind, Some(id), cx))),
                    );
                }
                if kind == TrackKind::Sub {
                    body = body.child(div().h(px(8.0))).child(
                        div()
                            .flex()
                            .gap_2()
                            // only offered once an OpenSubtitles key is configured
                            .when(self.svc.has_opensubtitles(), |d| {
                                d.child(
                                    self.button("sub-os", if self.disc.subs_loading { "Searching…" } else { "Search OpenSubtitles" }, true, p)
                                        .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.search_subtitles(false, cx))),
                                )
                            })
                            .child(
                                self.button("sub-load", "File…", false, p)
                                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.load_subtitle_file(cx))),
                            ),
                    );
                    if let Some(m) = self.disc.subs_msg.clone() {
                        body = body.child(div().px_1().pt_2().text_xs().text_color(rgb(p.muted)).child(m));
                    }
                    for (i, s) in self.disc.subs.iter().take(30).enumerate() {
                        let sub = s.clone();
                        let mut tags = vec![rmh_app::language_label(&s.language).to_string()];
                        if s.hash_match {
                            tags.push("exact match".into());
                        }
                        if s.trusted {
                            tags.push("trusted".into());
                        }
                        if s.hearing_impaired {
                            tags.push("SDH".into());
                        }
                        if s.ai_translated {
                            tags.push("AI".into());
                        }
                        tags.push(format!("{} ↓", s.downloads));
                        body = body.child(
                            row(format!("os-{i}"), s.release.clone(), Some(tags.join(" · ")), false)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.download_subtitle(sub.clone(), cx))),
                        );
                    }
                }
            }
            SideTab::Speed => {
                let cur = if self.snap.speed > 0.0 { self.snap.speed } else { 1.0 };
                for s in SPEEDS {
                    body = body.child(
                        row(format!("spd-{s}"), format!("{s}×"), (s == 1.0).then(|| "Normal".to_string()), (cur - s).abs() < 0.01)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_speed(s, cx))),
                    );
                }
                body = body.child(
                    div()
                        .p_3()
                        .text_xs()
                        .text_color(rgb(p.faint))
                        .child("[ / ] slower / faster · Backspace resets · , / . step one frame (pauses)"),
                );
            }
        }

        div()
            .w(px(300.0))
            .flex_none()
            .flex()
            .flex_col()
            .h_full()
            .bg(rgb(p.surface))
            .border_l_1()
            .border_color(rgb(p.border_soft))
            .child(
                div()
                    .flex()
                    .child(tab(self, SideTab::Chapters, "Chapters", self.snap.chapters.len(), cx))
                    .child(tab(self, SideTab::Audio, "Audio", tracks(TrackKind::Audio), cx))
                    .child(tab(self, SideTab::Subtitles, "Subs", tracks(TrackKind::Sub), cx))
                    .child(tab(self, SideTab::Speed, "Speed", 0, cx)),
            )
            .child(div().id("side-scroll").flex_1().overflow_y_scrollbar().p_2().child(body))
    }

    fn control_bar(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.now.as_ref().expect("caller checks");
        let starting = !n.seen_active;
        let speed = if self.snap.speed > 0.0 { self.snap.speed } else { 1.0 };
        let current_chapter = self
            .snap
            .chapter
            .and_then(|c| self.snap.chapters.get(c as usize))
            .filter(|_| self.snap.chapters.len() > 1)
            .map(|c| c.title.clone());
        let next_up = self.autoplay.and_then(|(at, id)| {
            let e = self.entry(id)?;
            let secs = at.saturating_duration_since(std::time::Instant::now()).as_secs() + 1;
            Some((format!("{} {}", e.display_title(), e.subtitle().unwrap_or_default()), secs))
        });
        let has_next = self.next_local_episode().is_some();

        div()
            .relative()
            .flex()
            .flex_col()
            .gap_2()
            .px_6()
            .pt_3()
            .pb_4()
            .min_h(px(112.0))
            .bg(rgb(p.sidebar))
            .border_t_1()
            .border_color(rgb(p.border))
            .child(self.seek_row(p, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .w(px(240.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).truncate().child(n.title.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(if starting {
                                "Starting mpv…".to_string()
                            } else {
                                current_chapter.unwrap_or_else(|| if n.paused { "Paused".into() } else { "Now playing".into() })
                            })),
                    )
                    .child(self.transport(p, true, cx))
                    .child(
                        self.icon_btn("tp-speed", "", "Playback speed ([ / ])", p)
                            .child(format!("{speed:.2}").trim_end_matches('0').trim_end_matches('.').to_string() + "×")
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                                this.side_tab = SideTab::Speed;
                                cx.notify();
                            })),
                    )
                    .when_some(self.skip_target(), |d, (label, _)| {
                        d.child(
                            self.button("skip-seg", format!("{label} ⏭"), true, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.skip_segment(cx))),
                        )
                    })
                    .when(has_next && self.autoplay.is_none(), |d| {
                        d.child(
                            self.icon_btn("tp-next-ep", "⏩ Next ep", "Next episode (N)", p)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.play_next_now(window, cx))),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .w(px(150.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_xs().text_color(rgb(p.muted)).child(if self.snap.muted { "🔇".to_string() } else { format!("{:.0}", self.volume) }))
                            .child(div().flex_1().child(Slider::new(&self.volume_slider))),
                    )
                    .child(
                        self.icon_btn("tp-theater", if self.theater { "⤡" } else { "⛶" }, "Fullscreen (F, Esc to leave)", p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                let on = !this.theater;
                                this.set_theater(on, window, cx)
                            })),
                    )
                    .child(
                        self.icon_btn("tp-stop", "■", "Stop (S)", p)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.stop(window, cx))),
                    ),
            )
            .when_some(next_up, |d, (title, secs)| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .rounded_lg()
                        .bg(rgb(p.accent_soft))
                        .text_sm()
                        .child(div().flex_none().font_weight(FontWeight::MEDIUM).child(format!("Up next in {secs}s")))
                        .child(div().flex_1().min_w_0().truncate().text_color(rgb(p.muted)).child(title))
                        .child(
                            self.button("next-now", "Play now", true, p)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.play_next_now(window, cx))),
                        )
                        .child(self.button("next-cancel", "Cancel", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                            this.autoplay = None;
                            cx.notify();
                        }))),
                )
            })
            .when_some(self.seek_preview(p), |d, prev| d.child(prev))
    }

    pub(crate) fn player_page(&self, p: Palette, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(div().flex_1().min_w_0().flex().flex_col().child(self.video_area(p, window)))
                    .when(!self.theater, |d| d.child(self.side_panel(p, cx))),
            )
            // Fullscreen: video only. mpv's on-screen controller takes over the
            // controls (keys keep working); without it the bar stays.
            .when(!(self.theater && self.embedded() && self.svc.config().mpv_osc), |d| d.child(self.control_bar(p, cx)))
            .into_any_element()
    }
}
