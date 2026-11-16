use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, Window};

/// Find a window ID by its name (title)
/// 
/// Note: This only works if connected to an X11 server (or XWayland).
/// If running on native Wayland, this will likely return None.
pub fn find_window_by_title(title: &str) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        find_window_by_title_linux(title)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn find_window_by_title_linux(title: &str) -> Option<u64> {
    // Connect to X server
    // If we are on pure Wayland without XWayland or permissions, this might fail.
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = &conn.setup().roots[screen_num];

    // Get the _NET_CLIENT_LIST atom to find all managed windows
    let net_client_list = conn.intern_atom(false, b"_NET_CLIENT_LIST").ok()?.reply().ok()?.atom;
    
    // Get the _NET_WM_NAME atom to check names
    let net_wm_name = conn.intern_atom(false, b"_NET_WM_NAME").ok()?.reply().ok()?.atom;
    let wm_name = AtomEnum::WM_NAME;

    // Get list of windows
    let reply = conn.get_property(
        false, 
        screen.root, 
        net_client_list, 
        AtomEnum::WINDOW, 
        0, 
        1024
    ).ok()?.reply().ok()?;

    if reply.type_ != u32::from(AtomEnum::WINDOW) {
        return None;
    }

    // Iterate over windows (32-bit values)
    for window_id in reply.value32()? {
        // Check window name
        if let Some(name) = get_window_name(&conn, window_id, net_wm_name).or_else(|| get_window_name(&conn, window_id, wm_name.into())) {
            // Check approximate match
            if name.contains(title) {
                return Some(window_id as u64);
            }
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn get_window_name(conn: &impl Connection, window: Window, atom: x11rb::protocol::xproto::Atom) -> Option<String> {
    let reply = conn.get_property(false, window, atom, AtomEnum::ANY, 0, 1024).ok()?.reply().ok()?;
    
    // Attempt to stringify
    String::from_utf8(reply.value).ok()
}
