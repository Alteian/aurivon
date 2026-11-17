//! In-app updates: periodic GitHub release check, "Update available" banner
//! with the changelog, verified install and restart.

use std::time::{Duration, Instant};

use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rmh_app::update::{self, ReleaseInfo, UpdateCheck};

use crate::theme::Palette;
use crate::view::MediaHubView;

#[derive(Default)]
pub struct UpdateUi {
    pub checking: bool,
    pub available: Option<ReleaseInfo>,
    pub installing: bool,
    /// Version installed and waiting for a restart
    pub installed: Option<String>,
    pub error: Option<String>,
    /// Result of the last manual check, for Settings
    pub message: Option<String>,
    pub dismissed: bool,
    pub show_notes: bool,
    pub last_check: Option<Instant>,
}

/// How often to look for a release (`RMH_UPDATE_INTERVAL_SECS` overrides, for testing)
fn interval() -> Duration {
    let secs = std::env::var("RMH_UPDATE_INTERVAL_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(6 * 3600);
    Duration::from_secs(secs.max(60))
}

impl MediaHubView {
    /// Check shortly after startup, then periodically, while auto-update is on
    pub(crate) fn start_update_loop(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(4)).await;
            loop {
                let ok = this
                    .update(cx, |this, cx| {
                        if this.svc.config().auto_update && this.upd.installed.is_none() {
                            this.check_updates(false, cx);
                        }
                    })
                    .is_ok();
                if !ok {
                    break;
                }
                cx.background_executor().timer(interval()).await;
            }
        })
        .detach();
    }

    pub(crate) fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.upd.checking || self.upd.installing {
            return;
        }
        self.upd.checking = true;
        if manual {
            self.upd.message = Some("Checking…".into());
        }
        cx.notify();
        let fut = self.svc.check_update();
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let u = &mut this.upd;
                u.checking = false;
                u.last_check = Some(Instant::now());
                match res {
                    Ok(UpdateCheck::Available(rel)) => {
                        tracing::info!("Update available: {} (running {})", rel.version, update::CURRENT_VERSION);
                        u.message = Some(format!("Version {} is available", rel.version));
                        // a newer release than the one dismissed shows the banner again
                        if manual || u.available.as_ref().map(|a| a.version != rel.version).unwrap_or(true) {
                            u.dismissed = false;
                        }
                        u.available = Some(rel);
                        u.error = None;
                    }
                    Ok(UpdateCheck::UpToDate { latest }) => {
                        u.available = None;
                        u.error = None;
                        u.message = Some(match latest {
                            Some(l) => format!("Up to date (latest release {l})"),
                            None => "Up to date (no releases published yet)".into(),
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Update check failed: {e}");
                        u.message = Some(format!("Couldn't check for updates: {e}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(rel) = self.upd.available.clone() else { return };
        if self.upd.installing {
            return;
        }
        self.upd.installing = true;
        self.upd.error = None;
        cx.notify();
        let fut = self.svc.install_update(rel.clone());
        cx.spawn(async move |this, cx| {
            let res = fut.await;
            this.update(cx, |this, cx| {
                let u = &mut this.upd;
                u.installing = false;
                match res {
                    Ok(_) => {
                        u.installed = Some(rel.version.to_string());
                        u.dismissed = false;
                        u.message = Some(format!("Version {} installed. Restart to use it.", rel.version));
                    }
                    Err(e) => u.error = Some(e),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn restart_for_update(&mut self, cx: &mut Context<Self>) {
        let _ = self.player.stop();
        match update::spawn_restart() {
            Ok(()) => cx.quit(),
            Err(e) => {
                self.upd.error = Some(format!("Restart failed: {e:#}. Start RustMediaHub again from the launcher."));
                cx.notify();
            }
        }
    }

    /// Banner under the top bar while an update is available / installed
    pub(crate) fn update_banner(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let u = &self.upd;
        if u.dismissed {
            return None;
        }
        let rel = u.available.as_ref()?;
        let installed = u.installed.is_some();
        let title = if installed {
            format!("RustMediaHub {} is installed", rel.version)
        } else if u.installing {
            format!("Installing {}…", rel.version)
        } else {
            format!("Update available: {}", rel.version)
        };
        let sub = if installed {
            "Restart to start using it. Your library and settings stay as they are.".to_string()
        } else {
            format!("You have {}{}", update::CURRENT_VERSION, if rel.prerelease { " · this is a pre-release" } else { "" })
        };
        let notes = rel.notes.trim().to_string();
        let mut actions = div().flex().items_center().gap_2().flex_none();
        if !notes.is_empty() {
            actions = actions.child(
                self.button("upd-notes", if u.show_notes { "Hide changes" } else { "What's new" }, false, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                        this.upd.show_notes = !this.upd.show_notes;
                        cx.notify();
                    })),
            );
        }
        if installed {
            actions = actions.child(
                self.button("upd-restart", "Restart now", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.restart_for_update(cx))),
            );
        } else if !u.installing {
            actions = actions.child(
                self.button("upd-install", "Install update", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.install_update(cx))),
            );
        }
        actions = actions.child(
            self.button("upd-later", "Later", false, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| {
                this.upd.dismissed = true;
                cx.notify();
            })),
        );
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .mx_8()
            .mt_4()
            .p_4()
            .rounded_xl()
            .border_1()
            .border_color(rgb(p.accent))
            .bg(rgb(p.accent_soft))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().size(px(10.0)).flex_none().rounded_full().bg(rgb(if installed { p.good } else { p.accent })))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(title))
                            .child(div().text_sm().text_color(rgb(p.muted)).child(sub)),
                    )
                    .child(actions),
            );
        if let Some(err) = &u.error {
            body = body.child(div().text_sm().text_color(rgb(p.danger)).child(err.clone()));
        }
        if u.show_notes && !notes.is_empty() {
            let mut list = div().flex().flex_col().gap_1().text_sm().pl_6();
            for line in notes.lines().filter(|l| !l.trim().is_empty()).take(24) {
                let l = line.trim();
                let (text, heading) = match l.strip_prefix("#") {
                    Some(h) => (h.trim_start_matches('#').trim().to_string(), true),
                    None => (l.trim_start_matches(['-', '*']).trim().to_string(), false),
                };
                list = list.child(if heading {
                    div().pt_1().font_weight(FontWeight::SEMIBOLD).child(text)
                } else {
                    div().text_color(rgb(p.muted)).child(format!("•  {text}"))
                });
            }
            body = body.child(list);
        }
        Some(body.into_any_element())
    }

    /// Settings card: version, auto-update toggles, manual check
    pub(crate) fn updates_card(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.svc.config();
        let installed = update::installed_root().is_some();
        let u = &self.upd;
        let mut buttons = div().flex().items_center().gap_2().child(
            self.button("upd-check", if u.checking { "Checking…" } else { "Check for updates" }, false, p)
                .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.check_updates(true, cx))),
        );
        if u.available.is_some() && u.installed.is_none() && installed {
            buttons = buttons.child(
                self.button("upd-install2", if u.installing { "Installing…" } else { "Install update" }, true, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.install_update(cx))),
            );
        }
        if u.installed.is_some() {
            buttons = buttons.child(
                self.button("upd-restart2", "Restart now", true, p).on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.restart_for_update(cx))),
            );
        }
        self.card(p)
            .child(self.section_title("Updates", p))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(p.muted))
                    .child(format!(
                        "RustMediaHub {} · {}",
                        update::CURRENT_VERSION,
                        if installed { "installed in ~/.local" } else { "development build (updates install only into an installed copy)" }
                    )),
            )
            .child(
                self.toggle_row_pub("tg-autoupdate", "Check for updates automatically", "On startup and every 6 hours; you choose when to install", cfg.auto_update, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.auto_update = !c.auto_update, false, cx))),
            )
            .child(
                self.toggle_row_pub("tg-prerelease", "Include pre-releases", "Offer beta versions like 0.3.0-beta.1", cfg.update_prereleases, p)
                    .on_click(cx.listener(|this, _: &ClickEvent, _w, cx| this.update_config(|c| c.update_prereleases = !c.update_prereleases, false, cx))),
            )
            .child(buttons)
            .when_some(u.message.clone(), |d, m| {
                let ago = u.last_check.map(|t| t.elapsed().as_secs() / 60).map(|m| match m {
                    0 => " · checked just now".to_string(),
                    1..=59 => format!(" · checked {m} min ago"),
                    _ => format!(" · checked {} h ago", m / 60),
                });
                d.child(div().text_sm().text_color(rgb(p.muted)).child(format!("{m}{}", ago.unwrap_or_default())))
            })
            .when_some(u.error.clone(), |d, e| d.child(div().text_sm().text_color(rgb(p.danger)).child(e)))
    }
}
