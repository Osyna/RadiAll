//! X11 / EWMH adapter (any X11 window manager: KDE-X11, XFCE, Cinnamon, MATE,
//! i3, GNOME-X11, ...).
//!
//! Pure EWMH over x11rb: window list from _NET_CLIENT_LIST, focus from
//! _NET_ACTIVE_WINDOW, actions as wmctrl-style ClientMessages to the root.
//! connect() refuses non-EWMH servers (_NET_SUPPORTING_WM_CHECK missing or
//! stale) so detect() can fall through to NullCompositor.
//!
//! Threading: trait methods run on the UI thread over the primary connection
//! (RustConnection is thread-safe, but nothing else touches it). watch()
//! spawns one background thread with its OWN connection that selects
//! PropertyChange on the root and re-reads the list/focus on every
//! _NET_CLIENT_LIST / _NET_ACTIVE_WINDOW flip. Atoms are server-global, so
//! the interned set is shared with the watcher.

use std::sync::mpsc::Sender;

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux,
    ConnectionExt, EventMask, StackMode, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use super::{Compositor, CompositorEvent, WindowId, WindowInfo};

x11rb::atom_manager! {
    /// All EWMH atoms we speak, interned in one round-trip at connect().
    Atoms:
    AtomsCookie {
        _NET_SUPPORTING_WM_CHECK,
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_WM_NAME,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_STATE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_FULLSCREEN,
        _NET_CLOSE_WINDOW,
        UTF8_STRING,
    }
}

pub struct X11Compositor {
    conn: RustConnection,
    screen: usize,
    root: Window,
    atoms: Atoms,
}

impl X11Compositor {
    pub fn connect() -> Result<Self, String> {
        let (conn, screen) =
            RustConnection::connect(None).map_err(|e| format!("x11 connect failed: {e}"))?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)
            .map_err(|e| format!("x11 atom intern failed: {e}"))?
            .reply()
            .map_err(|e| format!("x11 atom intern failed: {e}"))?;

        // EWMH liveness check: root names a check window that names itself.
        // A bare X server (or a dead WM leaving a stale property) fails here
        // and detect() falls through.
        let check = get_window_prop(&conn, root, atoms._NET_SUPPORTING_WM_CHECK)
            .ok_or("no EWMH window manager (_NET_SUPPORTING_WM_CHECK unset)")?;
        match get_window_prop(&conn, check, atoms._NET_SUPPORTING_WM_CHECK) {
            Some(w) if w == check => {}
            _ => return Err("stale _NET_SUPPORTING_WM_CHECK (WM gone?)".into()),
        }

        Ok(X11Compositor { conn, screen, root, atoms })
    }

    /// wmctrl-style request: format-32 ClientMessage to the root with the
    /// substructure masks so the WM (not the client) receives it.
    fn client_message(&self, win: Window, type_: Atom, data: [u32; 5]) {
        let event = ClientMessageEvent::new(32, win, type_, data);
        let sent = self
            .conn
            .send_event(
                false,
                self.root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .and_then(|_| self.conn.flush());
        if let Err(e) = sent {
            log::warn!("x11: ClientMessage send failed: {e}");
        }
    }
}

impl Compositor for X11Compositor {
    fn backend(&self) -> &'static str {
        "x11"
    }

    fn windows(&mut self) -> Vec<WindowInfo> {
        fetch_windows(&self.conn, &self.atoms, self.root)
    }

    fn active_window(&mut self) -> Option<WindowInfo> {
        fetch_active(&self.conn, &self.atoms, self.root)
    }

    fn watch(&mut self, tx: Sender<CompositorEvent>) {
        let atoms = self.atoms;
        let spawned = std::thread::Builder::new().name("x11-events".into()).spawn(move || {
            // Own connection: the watcher blocks in wait_for_event() forever,
            // which must not contend with UI-thread requests.
            let (conn, screen) = match RustConnection::connect(None) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("x11: watch connect failed, no live updates: {e}");
                    return;
                }
            };
            let root = conn.setup().roots[screen].root;
            let select = conn
                .change_window_attributes(
                    root,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                )
                .and_then(|_| conn.flush());
            if let Err(e) = select {
                log::warn!("x11: PropertyChange select failed, no live updates: {e}");
                return;
            }

            // Initial burst: sync the consumer with the current state.
            let _ = tx.send(CompositorEvent::Windows(fetch_windows(&conn, &atoms, root)));
            let _ = tx.send(CompositorEvent::Active(fetch_active(&conn, &atoms, root)));

            loop {
                let event = match conn.wait_for_event() {
                    Ok(ev) => ev,
                    Err(e) => {
                        log::warn!("x11: connection lost, live updates stopped: {e}");
                        return;
                    }
                };
                let prop = match event {
                    Event::PropertyNotify(ev) => ev.atom,
                    Event::Error(e) => {
                        log::debug!("x11: watch got X error: {e:?}");
                        continue;
                    }
                    _ => continue,
                };
                // Focus flips also change the `focused` flags inside the
                // list, so both properties refresh both event kinds.
                if prop != atoms._NET_CLIENT_LIST && prop != atoms._NET_ACTIVE_WINDOW {
                    continue;
                }
                let windows = fetch_windows(&conn, &atoms, root);
                let active = fetch_active(&conn, &atoms, root);
                if tx.send(CompositorEvent::Windows(windows)).is_err()
                    || tx.send(CompositorEvent::Active(active)).is_err()
                {
                    return; // consumer gone
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("x11: failed to spawn event thread: {e}");
        }
    }

    fn activate(&mut self, id: &WindowId) {
        let Some(win) = parse_window_id(id) else {
            log::warn!("x11: activate: bad window id {id}");
            return;
        };
        // data: source-indication 2 (pager/direct user action), timestamp
        // CurrentTime, requestor's currently-active window (0 = unknown).
        self.client_message(win, self.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]);
        // Fallback for WMs that honour restack but ignore activation from
        // "stale" timestamps: raise the window ourselves.
        let raised = self
            .conn
            .configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))
            .and_then(|_| self.conn.flush());
        if let Err(e) = raised {
            log::warn!("x11: raise failed: {e}");
        }
    }

    fn close_window(&mut self, id: &WindowId) {
        let Some(win) = parse_window_id(id) else {
            log::warn!("x11: close: bad window id {id}");
            return;
        };
        // data: timestamp 0, source-indication 2.
        self.client_message(win, self.atoms._NET_CLOSE_WINDOW, [0, 2, 0, 0, 0]);
    }

    fn fullscreen(&mut self, id: &WindowId) {
        let Some(win) = parse_window_id(id) else {
            log::warn!("x11: fullscreen: bad window id {id}");
            return;
        };
        // data: action 2 (_NET_WM_STATE_TOGGLE), property to flip, second
        // property (none), source-indication 2.
        self.client_message(
            win,
            self.atoms._NET_WM_STATE,
            [2, self.atoms._NET_WM_STATE_FULLSCREEN, 0, 2, 0],
        );
    }

    fn output_size(&mut self) -> Option<(u32, u32)> {
        let screen = &self.conn.setup().roots[self.screen];
        Some((u32::from(screen.width_in_pixels), u32::from(screen.height_in_pixels)))
    }
}

// ---- EWMH property plumbing (free fns: shared by the UI thread and the
// watcher, which each own a connection) ----

fn format_window_id(win: Window) -> WindowId {
    win.to_string()
}

fn parse_window_id(id: &WindowId) -> Option<Window> {
    id.parse::<Window>().ok()
}

/// First CARDINAL/WINDOW value of a 32-bit property, if set and non-zero.
fn get_window_prop(conn: &RustConnection, win: Window, atom: Atom) -> Option<Window> {
    let reply = conn
        .get_property(false, win, atom, AtomEnum::ANY, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let win = reply.value32()?.next();
    match win {
        Some(w) if w != 0 => Some(w),
        _ => None,
    }
}

/// All atoms of an ATOM[] property (empty vec = unset property).
fn get_atoms_prop(conn: &RustConnection, win: Window, atom: Atom) -> Vec<Atom> {
    conn.get_property(false, win, atom, AtomEnum::ATOM, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|it| it.collect()))
        .unwrap_or_default()
}

/// A text property as lossy UTF-8 (None = unset/empty).
fn get_text_prop(conn: &RustConnection, win: Window, atom: Atom, type_: Atom) -> Option<String> {
    let reply = conn
        .get_property(false, win, atom, type_, 0, 4096)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&reply.value).into_owned())
}

/// WM_CLASS raw bytes are two NUL-terminated strings, `instance\0class\0`;
/// the class half is what matches desktop entries (== Wayland app_id).
fn wm_class_from_bytes(raw: &[u8]) -> Option<String> {
    let mut parts = raw.split(|&b| b == 0);
    let _instance = parts.next()?;
    let class = parts.next()?;
    if class.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(class).into_owned())
}

/// Build a WindowInfo for one client, or None when the window should be
/// skipped (non-NORMAL type, skip-taskbar, no class, or already destroyed —
/// _NET_CLIENT_LIST can be momentarily stale, so failed reads just skip).
fn window_info(conn: &RustConnection, atoms: &Atoms, win: Window, active: Window) -> Option<WindowInfo> {
    // Missing _NET_WM_WINDOW_TYPE counts as NORMAL (EWMH default).
    let types = get_atoms_prop(conn, win, atoms._NET_WM_WINDOW_TYPE);
    if !types.is_empty() && !types.contains(&atoms._NET_WM_WINDOW_TYPE_NORMAL) {
        return None;
    }
    if get_atoms_prop(conn, win, atoms._NET_WM_STATE).contains(&atoms._NET_WM_STATE_SKIP_TASKBAR) {
        return None;
    }

    let class_raw = conn
        .get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    let app_id = wm_class_from_bytes(&class_raw.value)?;

    let title = get_text_prop(conn, win, atoms._NET_WM_NAME, atoms.UTF8_STRING)
        .or_else(|| get_text_prop(conn, win, AtomEnum::WM_NAME.into(), AtomEnum::ANY.into()))
        .unwrap_or_default();

    Some(WindowInfo { id: format_window_id(win), app_id, title, focused: win == active })
}

fn fetch_windows(conn: &RustConnection, atoms: &Atoms, root: Window) -> Vec<WindowInfo> {
    let clients: Vec<Window> = conn
        .get_property(false, root, atoms._NET_CLIENT_LIST, AtomEnum::WINDOW, 0, 4096)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|it| it.collect()))
        .unwrap_or_default();
    let active = get_window_prop(conn, root, atoms._NET_ACTIVE_WINDOW).unwrap_or(0);
    clients
        .into_iter()
        .filter_map(|win| window_info(conn, atoms, win, active))
        .collect()
}

fn fetch_active(conn: &RustConnection, atoms: &Atoms, root: Window) -> Option<WindowInfo> {
    let active = get_window_prop(conn, root, atoms._NET_ACTIVE_WINDOW)?;
    window_info(conn, atoms, active, active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_id_round_trips() {
        for win in [1u32, 0x0060_0007, u32::MAX] {
            let id = format_window_id(win);
            assert_eq!(parse_window_id(&id), Some(win));
        }
        assert_eq!(parse_window_id(&"not-a-window".to_string()), None);
        assert_eq!(parse_window_id(&"".to_string()), None);
        // Hyprland-style hex addresses must NOT silently misparse.
        assert_eq!(parse_window_id(&"0x60a0007".to_string()), None);
    }

    #[test]
    fn wm_class_takes_res_class() {
        assert_eq!(wm_class_from_bytes(b"chromium\0Chromium\0"), Some("Chromium".into()));
        // Some clients omit the trailing NUL.
        assert_eq!(wm_class_from_bytes(b"foot\0footclient"), Some("footclient".into()));
        assert_eq!(wm_class_from_bytes(b""), None);
        assert_eq!(wm_class_from_bytes(b"instance-only"), None);
        assert_eq!(wm_class_from_bytes(b"instance\0\0"), None);
    }

    /// Live probe against $DISPLAY (XWayland on the dev box). Run manually:
    /// `cargo test -p radiall x11 -- --ignored --nocapture`. Either outcome is
    /// valid: an EWMH root lists X11 clients; a bare XWayland root (no WM
    /// check) makes connect() Err, which is exactly what detect() relies on.
    #[test]
    #[ignore]
    fn live_probe() {
        match X11Compositor::connect() {
            Ok(mut c) => {
                println!("connected; active = {:?}", c.active_window());
                for w in c.windows() {
                    println!("{} [{}] {} focused={}", w.id, w.app_id, w.title, w.focused);
                }
            }
            Err(e) => println!("connect() -> Err({e}) — correct on a WM-less root"),
        }
    }
}
