//! Embedded video: an X11 child window inside the GPUI window that mpv draws
//! into (`mpv --wid`). Works on X11 and on Wayland desktops through XWayland
//! (the app switches GPUI to its X11 backend when embedding is enabled).
//!
//! The child is a native window stacked above GPUI's surface, so nothing GPUI
//! paints can overlap the video. The player page therefore keeps its menus
//! and controls *beside/below* the video area.

use std::cell::Cell;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tracing::{info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConfigureWindowAux, ConnectionExt, CreateWindowAux, EventMask, StackMode, WindowClass,
};
use x11rb::rust_connection::RustConnection;

pub struct VideoHost {
    conn: RustConnection,
    window: u32,
    mapped: Cell<bool>,
    geometry: Cell<(i32, i32, u32, u32)>,
}

impl VideoHost {
    /// Create the child window inside the GPUI window (X11 only)
    pub fn new(gpui_window: &gpui_kit::Window) -> Option<Self> {
        let parent = match HasWindowHandle::window_handle(gpui_window).ok()?.as_raw() {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            other => {
                info!("Embedded video needs X11/XWayland; GPUI is on {other:?}");
                return None;
            }
        };
        let (conn, _) = x11rb::connect(None).map_err(|e| warn!("X11 connect failed: {e}")).ok()?;
        let window = conn.generate_id().ok()?;
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            parent,
            0,
            0,
            16,
            16,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().background_pixel(0).event_mask(EventMask::STRUCTURE_NOTIFY),
        )
        .ok()?;
        conn.flush().ok()?;
        info!("Embedded video surface 0x{window:x} in GPUI window 0x{parent:x}");
        Some(Self { conn, window, mapped: Cell::new(false), geometry: Cell::new((0, 0, 0, 0)) })
    }

    /// X window id for `mpv --wid`
    pub fn xid(&self) -> i64 {
        self.window as i64
    }

    /// Position in *device* pixels relative to the GPUI window, and show
    pub fn place(&self, x: f32, y: f32, w: f32, h: f32) {
        let g = (x.round() as i32, y.round() as i32, w.round().max(1.0) as u32, h.round().max(1.0) as u32);
        if g != self.geometry.get() {
            self.geometry.set(g);
            let _ = self.conn.configure_window(
                self.window,
                &ConfigureWindowAux::new().x(g.0).y(g.1).width(g.2).height(g.3).stack_mode(StackMode::ABOVE),
            );
        }
        if !self.mapped.get() {
            let _ = self.conn.map_window(self.window);
            self.mapped.set(true);
        }
        let _ = self.conn.flush();
    }

    pub fn hide(&self) {
        if self.mapped.get() {
            let _ = self.conn.unmap_window(self.window);
            let _ = self.conn.flush();
            self.mapped.set(false);
        }
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.mapped.get()
    }
}

impl Drop for VideoHost {
    fn drop(&mut self) {
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
    }
}
