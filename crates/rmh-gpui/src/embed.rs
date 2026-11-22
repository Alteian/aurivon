//! Embedded video surface that mpv draws into (`mpv --wid`).
//!
//! - Linux: X11 / XWayland child window
//! - Windows: Win32 child HWND
//! - Other: unavailable (mpv opens its own window)

use gpui_kit::Window;

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
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
        pub fn new(gpui_window: &Window) -> Option<Self> {
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

        pub fn xid(&self) -> i64 {
            self.window as i64
        }

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
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::cell::Cell;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use tracing::{info, warn};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CreateSolidBrush, UpdateWindow, HBRUSH};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, MoveWindow, RegisterClassExW, SetWindowPos,
        ShowWindow, CS_HREDRAW, CS_OWNDC, CS_VREDRAW, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        SW_HIDE, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW, WS_CHILD,
        WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
    };

    const CLASS: &str = "AurivonVideoHost";

    unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_PAINT => {
                let _ = windows::Win32::Graphics::Gdi::ValidateRect(hwnd, None);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn ensure_class() -> bool {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        static mut OK: bool = false;
        ONCE.call_once(|| {
            let class: Vec<u16> = CLASS.encode_utf16().chain(std::iter::once(0)).collect();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_OWNDC | CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hbrBackground: HBRUSH(CreateSolidBrush(windows::Win32::Foundation::COLORREF(0)).0),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            unsafe {
                OK = RegisterClassExW(&wc).0 != 0;
                if !OK {
                    warn!("RegisterClassExW for video host failed");
                }
            }
        });
        unsafe { OK }
    }

    pub struct VideoHost {
        hwnd: HWND,
        mapped: Cell<bool>,
        geometry: Cell<(i32, i32, u32, u32)>,
    }

    impl VideoHost {
        pub fn new(gpui_window: &Window) -> Option<Self> {
            let parent = match HasWindowHandle::window_handle(gpui_window).ok()?.as_raw() {
                RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut _),
                other => {
                    info!("Embedded video needs a Win32 HWND; GPUI is on {other:?}");
                    return None;
                }
            };
            if !ensure_class() {
                return None;
            }
            let class: Vec<u16> = CLASS.encode_utf16().chain(std::iter::once(0)).collect();
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    PCWSTR(class.as_ptr()),
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                    0,
                    0,
                    16,
                    16,
                    parent,
                    None,
                    None,
                    None,
                )
            }
            .ok()?;
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                let _ = UpdateWindow(hwnd);
            }
            info!("Embedded video HWND {hwnd:?} in parent {parent:?}");
            Some(Self { hwnd, mapped: Cell::new(true), geometry: Cell::new((0, 0, 0, 0)) })
        }

        /// HWND value for `mpv --wid=` (decimal pointer)
        pub fn xid(&self) -> i64 {
            self.hwnd.0 as isize as i64
        }

        pub fn place(&self, x: f32, y: f32, w: f32, h: f32) {
            let g = (x.round() as i32, y.round() as i32, w.round().max(1.0) as u32, h.round().max(1.0) as u32);
            if g != self.geometry.get() {
                self.geometry.set(g);
                unsafe {
                    let _ = MoveWindow(self.hwnd, g.0, g.1, g.2 as i32, g.3 as i32, true);
                    let _ = SetWindowPos(
                        self.hwnd,
                        HWND_TOP,
                        g.0,
                        g.1,
                        g.2 as i32,
                        g.3 as i32,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
            }
            if !self.mapped.get() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                }
                self.mapped.set(true);
            }
        }

        pub fn hide(&self) {
            if self.mapped.get() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
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
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    use super::*;

    pub struct VideoHost;

    impl VideoHost {
        pub fn new(_gpui_window: &Window) -> Option<Self> {
            None
        }

        pub fn xid(&self) -> i64 {
            0
        }

        pub fn place(&self, _x: f32, _y: f32, _w: f32, _h: f32) {}

        pub fn hide(&self) {}

        #[allow(dead_code)]
        pub fn is_visible(&self) -> bool {
            false
        }
    }
}

pub use platform::VideoHost;
