//! GPUI front end for RustMediaHub.
//!
//! Built on `gpui-kit` exactly like Kalyreon's desktop app: the window hosts a
//! single [`view::MediaHubView`]; all data/IO lives in `rmh-app` and playback
//! in `rmh-player`, so this crate is purely presentation.

mod theme;
mod view;

use gpui_kit::assets::Assets;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::*;
use rmh_app::MediaService;

pub use view::{MediaHubView, Page};

/// Open the main window and run the GPUI event loop (blocks until quit).
pub fn run() -> anyhow_like::Result {
    let svc = MediaService::new().map_err(|e| format!("Failed to start services: {e:#}"))?;

    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions { title: Some("RustMediaHub".into()), ..Default::default() }),
            window_bounds: Some(WindowBounds::centered(size(px(1360.0), px(860.0)), cx)),
            window_min_size: Some(size(px(960.0), px(640.0))),
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

mod anyhow_like {
    pub type Result = std::result::Result<(), String>;
}
