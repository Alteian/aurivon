//! GPUI front end for RustMediaHub (same gpui-kit stack as Kalyreon).

mod details;
mod embed;
mod home;
mod player;
mod settings;
mod theme;
mod ui;
mod view;

use gpui_kit::assets::Assets;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::*;
use rmh_app::{AppConfig, MediaService, VideoMode};

pub use view::{MediaHubView, Page};

/// Embedded video needs an X11 window. On Wayland desktops (COSMIC, GNOME…)
/// run GPUI through XWayland instead, unless the user opted out.
fn choose_backend() {
    let cfg = AppConfig::load().unwrap_or_default();
    let want_embed = cfg.video_mode == VideoMode::Embedded && std::env::var_os("RMH_FORCE_WAYLAND").is_none();
    if want_embed && std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
        tracing::info!("Embedded video: using XWayland for the app window");
        // SAFETY: called at startup before any other thread exists
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
}

/// Open the main window and run the GPUI event loop (blocks until quit).
pub fn run() -> Result<(), String> {
    choose_backend();
    let svc = MediaService::new().map_err(|e| format!("Failed to start services: {e:#}"))?;

    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("RustMediaHub".into()), ..Default::default() }),
            window_bounds: Some(WindowBounds::centered(size(px(1440.0), px(900.0)), cx)),
            window_min_size: Some(size(px(1024.0), px(680.0))),
            app_id: Some("org.rustmediahub.desktop".to_string()),
            ..Default::default()
        };
        let svc = svc.clone();
        gpui_kit::open_window(options, cx, move |window, cx| cx.new(|cx| MediaHubView::new(svc, window, cx)))
            .expect("failed to open RustMediaHub window");
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
    Ok(())
}
