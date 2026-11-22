//! Settings: libraries, home layout, player, services (TMDB, Trakt, Real-Debrid).

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::{HomeLayout, VideoMode};

use crate::theme::Palette;
use crate::view::MediaHubView;

fn status_line(p: Palette, ok: bool, text: String) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_sm()
        .child(div().size(px(8.0)).rounded_full().bg(rgb(if ok { p.good } else { p.danger })))
        .child(text)
}

impl MediaHubView {
    pub(crate) fn toggle_row_pub(&self, id: &'static str, label: &'static str, hint: &'static str, on: bool, p: Palette) -> Stateful<Div> {
        self.toggle_row(id, label, hint, on, p)
    }

    pub(crate) fn toggle_row(&self, id: &'static str, label: &'static str, hint: &'static str, on: bool, p: Palette) -> Stateful<Div> {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_4()
            .cursor_pointer()
            .child(
                div()
                    .w(px(40.0))
                    .h(px(22.0))
                    .flex_none()
                    .rounded_full()
                    .p(px(3.0))
                    .bg(rgb(if on { p.accent } else { p.border }))
                    .flex()
                    .when(on, |d| d.justify_end())
                    .child(div().size(px(16.0)).rounded_full().bg(rgb(p.text))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                    .child(div().text_xs().text_color(rgb(p.faint)).child(hint)),
            )
    }

    fn field(&self, label: &'static str, hint: &'static str, input: &Entity<InputState>, p: Palette) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
            .child(Input::new(input).w_full())
            .child(div().text_xs().text_color(rgb(p.faint)).child(hint))
    }

    pub(crate) fn settings_page(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let cfg = self.svc.config();

        // Libraries
        let mut libs = div().flex().flex_col().gap_2();
        if self.libraries.is_empty() {
            libs = libs.child(div().text_sm().text_color(rgb(p.muted)).child("No libraries yet."));
        }
        for lib in &self.libraries {
            let (id, scan_lib) = (lib.id, lib.clone());
            let count = self.entries.iter().filter(|e| e.item.file_path.starts_with(&lib.path)).count();
            libs = libs.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(rgb(p.surface_raised))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(lib.name.clone()))
                            .child(div().text_xs().text_color(rgb(p.muted)).truncate().child(format!("{} · {count} items", lib.path))),
                    )
                    .child(
                        self.button(SharedString::from(format!("scan-{id}")), "Rescan", false, p)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.scan_library(scan_lib.clone(), cx))),
                    )
                    .child(
                        self.button(SharedString::from(format!("rm-{id}")), "Remove", false, p)
                            .text_color(rgb(p.danger))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.remove_library(id, cx))),
                    ),
            );
        }

        // Home layout
        let mut layouts = div().flex().flex_wrap().gap_2();
        for (i, l) in HomeLayout::ALL.into_iter().enumerate() {
            let letter = ["A", "B", "C"][i];
            layouts = layouts.child(
                self.pill(SharedString::from(format!("set-layout-{letter}")), format!("{letter} · {}", l.label()), cfg.home_layout == l, p)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| this.set_home_layout(l, cx))),
            );
        }

        // Player
        let embedded_ok = self.embedded();
        let player = self
            .card(p)
            .child(self.section_title("Player", p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(120.0)).text_sm().font_weight(FontWeight::MEDIUM).child("Video"))
                    .child(
                        self.pill("vm-emb", "Inside the app", cfg.video_mode == VideoMode::Embedded, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.video_mode = VideoMode::Embedded, true, cx))),
                    )
                    .child(
                        self.pill("vm-win", "Separate mpv window", cfg.video_mode == VideoMode::Window, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.video_mode = VideoMode::Window, true, cx))),
                    ),
            )
            .child(div().text_xs().text_color(rgb(p.faint)).child(if embedded_ok {
                if cfg!(windows) {
                    "Embedded: mpv draws into the app window (Win32 HWND)."
                } else {
                    "Embedded: mpv draws into the app window (X11 / XWayland)."
                }
            } else if cfg.video_mode == VideoMode::Embedded {
                "Embedding isn't available in this session; using a separate window."
            } else {
                "Separate window: mpv's own window with its on-screen controller."
            }))
            .child(
                self.toggle_row("tg-osc", "mpv on-screen controller", "mpv's own seek bar with chapter marks over the video", cfg.mpv_osc, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.mpv_osc = !c.mpv_osc, false, cx))),
            )
            .child(
                self.toggle_row("tg-auto", "Autoplay next episode", "10 s countdown at the end of an episode", cfg.autoplay_next, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.autoplay_next = !c.autoplay_next, false, cx))),
            )
            .child(
                self.toggle_row("tg-fs", "Separate window starts fullscreen", "Only for the separate-window mode", cfg.window_fullscreen, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.window_fullscreen = !c.window_fullscreen, false, cx))),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().flex_1().child(self.field("Preferred audio languages", "mpv --alang, comma separated", &self.alang_input, p)))
                    .child(div().flex_1().child(self.field("Preferred subtitle languages", "mpv --slang, comma separated", &self.slang_input, p))),
            )
            .child(self.field("mpv executable", "Leave as mpv to use the one on PATH.", &self.mpv_input, p));

        // Services
        let trakt_state = match (self.trakt.configured, self.trakt.logged_in) {
            (false, _) => "Not set up: create an API app at trakt.tv/oauth/applications with redirect URI https://alteian.github.io/aurivon/oauth/trakt.html and paste its client id (no secret).".to_string(),
            (true, false) if self.trakt_waiting => "Browser opened. Approve Aurivon, then return here.".to_string(),
            (true, false) => "Client id saved. Click Connect — a browser window opens for Trakt PKCE login.".to_string(),
            (true, true) => format!("Connected{}. Playback is scrobbled; Sync marks watched both ways.", self.trakt.username.clone().map(|u| format!(" as {u}")).unwrap_or_default()),
        };
        let mut trakt_actions = div().flex().flex_wrap().gap_2();
        if self.trakt.logged_in {
            trakt_actions = trakt_actions
                .child(self.button("trakt-sync", "Sync now", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.trakt_sync(cx))))
                .child(self.button("trakt-out", "Disconnect", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.trakt_disconnect(cx))));
        } else {
            trakt_actions = trakt_actions.child(
                self.button("trakt-in", if self.trakt_waiting { "Waiting for browser…" } else { "Save & connect" }, true, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.trakt_connect(cx))),
            );
        }
        let services = self
            .card(p)
            .child(self.section_title("Services", p))
            .child(self.field("TMDB API key", "Posters, details, seasons/episodes, trending. Free at themoviedb.org/settings/api.", &self.tmdb_input, p))
            .child(
                div().flex().gap_2().child(
                    self.button("match-all", if self.matching { "Matching…" } else { "Re-match library with TMDB" }, false, p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.match_library(true, cx))),
                ),
            )
            .child(self.field("Real-Debrid API key", "Optional. Cached torrents play instantly over HTTPS.", &self.rd_input, p))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().flex_1().child(self.field("TorBox API key", "Optional second debrid", &self.disc.inputs.torbox, p)))
                    .child(div().flex_1().child(self.field("AllDebrid API key", "Optional third debrid", &self.disc.inputs.alldebrid, p))),
            )
            .child(div().h(px(4.0)))
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("Trakt"))
            .child(div().text_xs().text_color(rgb(p.muted)).child(trakt_state))
            .child(self.field("Client id", "Public client id from your Trakt API app (no secret)", &self.trakt_id_input, p))
            .child(trakt_actions)
            .when(self.trakt.logged_in, |d| {
                d.child(
                    div().flex().gap_2().child(
                        self.button("trakt-wl-sync", "Sync watchlist", false, p)
                            .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.sync_watchlist_only(cx))),
                    ),
                )
            })
            .child(
                self.toggle_row("tg-scrobble", "Scrobble to Trakt", "Send start/pause/stop while watching", cfg.trakt_scrobble, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.trakt_scrobble = !c.trakt_scrobble, false, cx))),
            )
            .child(
                div().flex().child(
                    self.button("save-settings", "Save settings", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.save_settings(cx))),
                ),
            );

        let status = self
            .card(p)
            .child(self.section_title("Status", p))
            .child(status_line(
                p,
                self.mpv_version.is_some(),
                match &self.mpv_version {
                    Some(v) => format!("Player: {v} ({})", if self.embedded() { "embedded in the app" } else { "separate window" }),
                    None => "Player: mpv not found. Install it: sudo apt install mpv".to_string(),
                },
            ))
            .child(status_line(p, self.svc.has_tmdb(), if self.svc.has_tmdb() { "TMDB: configured" } else { "TMDB: not configured" }.into()))
            .when(self.trakt.configured, |d| {
                d.child(status_line(p, self.trakt.logged_in, if self.trakt.logged_in { "Trakt: connected" } else { "Trakt: not logged in" }.into()))
            })
            .child(status_line(
                p,
                self.svc.has_real_debrid(),
                if self.svc.has_real_debrid() { "Real-Debrid: configured" } else { "Real-Debrid: not configured (P2P only)" }.into(),
            ))
            .child(div().text_xs().text_color(rgb(p.faint)).child(
                "Keys: Ctrl+K or / search · Space/K play · ←/→ 10 s (Shift 60 s) · ↑/↓ volume · ,/. frame · [/] speed · PgUp/PgDn next/prev chapter · I skip intro · A audio · J subtitles · O OpenSubtitles · T panel tab · N next episode · F fullscreen (player) · F11 fullscreen (anywhere) · M mute · S stop · Enter resume · Ctrl+1–7 pages · Ctrl+L home layout · Ctrl+I details",
            ));

        div()
            .flex()
            .flex_col()
            .gap_6()
            .max_w(px(900.0))
            .child(self.card(p).child(self.section_title("Libraries", p)).child(libs).child(self.add_library_row(p, cx)))
            .child(self.card(p).child(self.section_title("Home layout", p)).child(layouts))
            .child(player)
            .child(services)
            .child(self.more_services(p, cx))
            .child(self.stream_filter_card(p, cx))
            .child(self.cache_card(p, cx))
            .child(self.home_rows_card(p, cx))
            .child(self.addons_card(p, cx))
            .child(self.playback_extras_card(p, cx))
            .child(self.updates_card(p, cx))
            .child(status)
            .into_any_element()
    }

    fn home_rows_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.svc.config();
        let rows = [
            ("continue", "Continue watching"),
            ("up_next", "Up next"),
            ("watchlist", "Watchlist"),
            ("trakt_playback", "Continue on Trakt"),
            ("trending", "Trending"),
            ("library", "Library grid"),
        ];
        let mut body = div().flex().flex_wrap().gap_2();
        for (key, label) in rows {
            let key = key.to_string();
            let on = cfg.home_row_visible(&key);
            body = body.child(
                self.pill(SharedString::from(format!("home-row-{key}")), label.to_string(), on, p).on_click(cx.listener(move |this, _: &ClickEvent, _w, cx| {
                    let key = key.clone();
                    this.update_config(
                        move |c| {
                            if c.home_rows.iter().any(|r| r == &key) {
                                c.home_rows.retain(|r| r != &key);
                            } else {
                                c.home_rows.push(key);
                            }
                        },
                        false,
                        cx,
                    );
                })),
            );
        }
        self.card(p)
            .child(self.section_title("Home rows", p))
            .child(div().text_sm().text_color(rgb(p.muted)).child("Choose which rows appear on Home."))
            .child(body)
    }

    fn playback_extras_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.svc.config();
        self.card(p)
            .child(self.section_title("Playback extras", p))
            .child(
                self.toggle_row(
                    "tg-presearch",
                    "Pre-search next episode",
                    "Look up streams for the next episode while one is playing",
                    cfg.next_episode_presearch,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.update_config(|c| c.next_episode_presearch = !c.next_episode_presearch, false, cx)
                })),
            )
            .child(
                self.toggle_row(
                    "tg-introdb",
                    "TheIntroDB skip intro",
                    "When chapters are missing, use community intro/credits timestamps",
                    cfg.introdb_skip,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.introdb_skip = !c.introdb_skip, false, cx))),
            )
    }

    fn addons_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let addons = self.svc.list_addons();
        let mut list = div().flex().flex_col().gap_2();
        for a in &addons {
            let id = a.id.clone();
            let enabled = a.enabled;
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_sm().child(format!("{} ({})", a.name, a.id)))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                self.pill(
                                    SharedString::from(format!("addon-en-{id}")),
                                    SharedString::from(if enabled { "On" } else { "Off" }),
                                    enabled,
                                    p,
                                )
                                .on_click({
                                    let id = id.clone();
                                    cx.listener(move |this, _: &ClickEvent, _w, cx| {
                                        let _ = this.svc.set_addon_enabled(&id, !enabled);
                                        cx.notify();
                                    })
                                }),
                            )
                            .child(self.button(SharedString::from(format!("addon-rm-{id}")), "Remove", false, p).on_click({
                                let id = id.clone();
                                cx.listener(move |this, _: &ClickEvent, _w, cx| {
                                    let _ = this.svc.remove_addon(&id);
                                    this.status = Some("Addon removed".into());
                                    cx.notify();
                                })
                            })),
                    ),
            );
        }
        self.card(p)
            .child(self.section_title("Stremio addons", p))
            .child(div().text_sm().text_color(rgb(p.muted)).child("Install community addons by manifest URL. Streams are queried alongside Torrentio."))
            .child(list)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().child(self.field("Addon URL", "https://addon.example/manifest.json", &self.disc.inputs.addon_url, p)))
                    .child(self.button("addon-install", "Install", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.install_addon(cx)))),
            )
    }

    /// Region, OpenSubtitles, Simkl, AniList
    fn more_services(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.svc.config();
        let i = &self.disc.inputs;
        let simkl = &self.disc.simkl;
        let anilist = &self.disc.anilist;
        let simkl_state = match (&simkl.username, simkl.logged_in, simkl.configured) {
            (Some(u), true, _) => format!("Connected as {u}"),
            (None, true, _) => "Connected".into(),
            (_, false, true) => "Not connected".into(),
            _ => "Add a client id (simkl.com/settings/developer) to connect".into(),
        };
        let anilist_state = match (&anilist.username, anilist.logged_in) {
            (Some(u), true) => format!("Connected as {u}"),
            (None, true) => "Connected".into(),
            _ if cfg.anilist_client_id.is_some() => "Not connected: open the login page, approve, then paste the token".into(),
            _ => "Add a client id (anilist.co/settings/developer, redirect https://anilist.co/api/v2/oauth/pin)".into(),
        };
        self.card(p)
            .child(self.section_title("More services", p))
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().w(px(160.0)).child(self.field("Region", "Release dates & streaming", &i.region, p)))
                    .child(div().flex_1().child(self.field("OpenSubtitles API key", "opensubtitles.com/consumers · searches by file hash + TMDB id", &i.os_key, p))),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(div().flex_1().child(self.field("OpenSubtitles username", "Optional, raises the daily download limit", &i.os_user, p)))
                    .child(div().flex_1().child(self.field("OpenSubtitles password", "Optional", &i.os_pass, p))),
            )
            .child(
                self.toggle_row(
                    "tg-subauto",
                    "Fetch subtitles automatically",
                    "When a file has no track in your first subtitle language (Player → subtitle languages, default Czech then English)",
                    cfg.subtitles_auto,
                    p,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.subtitles_auto = !c.subtitles_auto, false, cx))),
            )
            .child(div().h(px(4.0)))
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("Simkl"))
            .child(div().text_xs().text_color(rgb(if simkl.logged_in { p.good } else { p.muted })).child(simkl_state))
            .child(self.field("Simkl client id", "Scrobbling, history and plan-to-watch", &i.simkl_id, p))
            .when_some(self.disc.simkl_pin.clone(), |d, pin| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .p_4()
                        .rounded_xl()
                        .bg(rgb(p.accent_soft))
                        .child(div().text_3xl().font_weight(FontWeight::BOLD).child(pin.user_code.clone()))
                        .child(div().text_sm().child(format!("Open {} and enter this code. Waiting…", pin.verification_url))),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .when(!simkl.logged_in, |d| {
                        d.child(self.button("simkl-connect", "Connect Simkl", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.simkl_connect(cx))))
                    })
                    .when(simkl.logged_in, |d| {
                        d.child(self.button("simkl-out", "Disconnect", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                            this.svc.simkl_logout();
                            this.refresh_social(cx);
                        })))
                    }),
            )
            .child(
                self.toggle_row("tg-simkl", "Scrobble to Simkl", "Start / pause / stop while watching", cfg.simkl_scrobble, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.simkl_scrobble = !c.simkl_scrobble, false, cx))),
            )
            .child(div().h(px(4.0)))
            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("AniList"))
            .child(div().text_xs().text_color(rgb(if anilist.logged_in { p.good } else { p.muted })).child(anilist_state))
            .child(self.field("AniList client id", "Anime progress (Japanese animation is detected automatically)", &i.anilist_id, p))
            .when(!anilist.logged_in, |d| {
                d.child(
                    div()
                        .flex()
                        .items_end()
                        .gap_2()
                        .child(self.button("al-open", "Open AniList login", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.anilist_open_auth(cx))))
                        .child(div().flex_1().child(Input::new(&i.anilist_token).w_full()))
                        .child(self.button("al-save", "Save token", false, p).on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.anilist_save_token(window, cx)))),
                )
            })
            .when(anilist.logged_in, |d| {
                d.child(div().flex().child(self.button("al-out", "Disconnect", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                    this.svc.anilist_logout();
                    this.refresh_social(cx);
                }))))
            })
            .child(
                self.toggle_row("tg-anilist", "Sync anime to AniList", "Finished episodes update your progress; watchlist adds go to Planning", cfg.anilist_sync, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.anilist_sync = !c.anilist_sync, false, cx))),
            )
            .child(div().flex().child(
                self.button("save-services", "Save services", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.save_service_settings(cx))),
            ))
    }
}
