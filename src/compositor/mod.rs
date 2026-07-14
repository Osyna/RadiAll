//! Compositor adapter. RadiAll is Hyprland-first but runs on any Linux
//! session, on Windows (Win32), and on macOS; everything compositor-specific
//! lives behind this trait so the rest of the app stays generic.
//!
//! Linux coverage:
//! - Hyprland: IPC socket (full control incl. float/send-keys)
//! - wlroots compositors: wlr-foreign-toplevel-management (sway, river,
//!   Wayfire, labwc, niri, ...)
//! - KDE Plasma Wayland: org_kde_plasma_window_management
//! - any X11 WM: EWMH (KDE-X11, XFCE, Cinnamon, MATE, i3, GNOME-X11, ...)
//! - GNOME Wayland: no window-listing protocol exists; the apps ring works
//!   fully, windows/actions rings degrade
//! - Windows: Win32 — EnumWindows + SetForegroundWindow/PostMessage
//! - macOS: no window adapter yet; apps ring works fully, windows/actions
//!   rings degrade (a CGWindowList/AX adapter can slot in behind this trait)
//!
//! Capabilities mirror the old Compositor.qml singleton:
//!   - window listing / focus / close / fullscreen -> per-adapter
//!   - float + send-keys are Hyprland-only          -> guarded by can_float / can_send_keys
//!   - global keybinds live in shortcuts.rs providers (hyprctl / XDG portal /
//!     X11 grabs); users can always bind `radiall --apps` manually instead

#[cfg(target_os = "linux")]
mod hyprland;
#[cfg(target_os = "linux")]
mod plasma;
#[cfg(target_os = "linux")]
mod wlr;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(windows)]
mod windows;

use std::sync::mpsc::Sender;

/// Opaque per-window identifier, stable while the window lives.
/// Hyprland: the `address` hex string. wlr: the toplevel handle's protocol id.
pub type WindowId = String;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WindowInfo {
    pub id: WindowId,
    /// Wayland app_id (== X11/ipc "class"); used to match desktop entries.
    pub app_id: String,
    pub title: String,
    pub focused: bool,
}

/// Pushed by the adapter's event thread whenever the window set changes.
#[derive(Debug, Clone)]
pub enum CompositorEvent {
    /// Full refreshed window list (order = compositor order).
    Windows(Vec<WindowInfo>),
    /// Currently focused window changed (None = nothing focused).
    Active(Option<WindowInfo>),
    /// The compositor reloaded its config (Hyprland): keyword-applied binds
    /// and window rules were wiped and have been / must be re-applied.
    /// (constructed only by the Hyprland adapter)
    #[cfg(target_os = "linux")]
    ConfigReloaded,
}

pub trait Compositor: Send {
    fn backend(&self) -> &'static str;

    fn can_float(&self) -> bool {
        false
    }
    fn can_send_keys(&self) -> bool {
        false
    }

    /// Configure the compositor so the ring window behaves as a true overlay
    /// (floating, screen-sized, undecorated, above other windows) WITHOUT
    /// xdg-fullscreen — real fullscreen makes compositors skip rendering the
    /// windows behind it, breaking the transparent backdrop. Returns true when
    /// the compositor is set up; false -> caller falls back to fullscreen.
    fn setup_overlay(&mut self) -> bool {
        false
    }

    /// Physical pixel size of the focused output, when the compositor can
    /// tell us. Used to pre-size the overlay window BEFORE it maps, so the
    /// first committed frame is already screen-sized (no flash of a small
    /// window while the float/resize rules land).
    fn output_size(&mut self) -> Option<(u32, u32)> {
        None
    }

    // ---- queries (synchronous snapshot; adapters may also push events) ----
    fn windows(&mut self) -> Vec<WindowInfo>;
    fn active_window(&mut self) -> Option<WindowInfo>;

    /// Start a background watcher that pushes CompositorEvents on `tx`.
    /// Called once at daemon startup; implementations spawn their own thread.
    fn watch(&mut self, tx: Sender<CompositorEvent>);

    // ---- operations ----
    fn activate(&mut self, id: &WindowId);
    fn close_window(&mut self, id: &WindowId);
    fn fullscreen(&mut self, id: &WindowId);
    /// Hyprland only — guard callers with can_float().
    fn toggle_float(&mut self, _id: &WindowId) {}
    /// Hyprland only — guard callers with can_send_keys().
    /// `mods` is a Hyprland-style modmask string ("SUPER SHIFT"), `key` a key name.
    fn send_keys(&mut self, _id: &WindowId, _mods: &str, _key: &str) {}
}

/// Fallback adapter for environments with no window-listing protocol — GNOME
/// Wayland, and macOS until a CGWindowList/AX adapter lands. The apps ring
/// works fully; the windows/actions rings stay empty.
struct NullCompositor;

impl Compositor for NullCompositor {
    fn backend(&self) -> &'static str {
        "none"
    }
    fn windows(&mut self) -> Vec<WindowInfo> {
        Vec::new()
    }
    fn active_window(&mut self) -> Option<WindowInfo> {
        None
    }
    fn watch(&mut self, tx: Sender<CompositorEvent>) {
        // No adapter here: report an empty, unfocused window set once so the
        // windows ring initializes cleanly on apps-only targets (macOS, GNOME
        // Wayland) instead of leaving stale state.
        let _ = tx.send(CompositorEvent::Windows(Vec::new()));
        let _ = tx.send(CompositorEvent::Active(None));
    }
    fn activate(&mut self, _id: &WindowId) {}
    fn close_window(&mut self, _id: &WindowId) {}
    fn fullscreen(&mut self, _id: &WindowId) {}
}

/// Pick the best adapter for the current session.
///
/// Windows: the Win32 adapter (EnumWindows + SetForegroundWindow/PostMessage).
/// Wayland: hyprland (env-detected) -> wlr-foreign-toplevel -> plasma.
/// No Wayland but X11: EWMH. An XWayland DISPLAY under a Wayland session is
/// deliberately NOT used as a fallback — it would list only XWayland windows
/// and silently miss every native client, which is worse than degrading.
pub fn detect() -> Box<dyn Compositor> {
    #[cfg(windows)]
    {
        match windows::WindowsCompositor::connect() {
            Ok(c) => return Box::new(c),
            Err(e) => log::warn!("win32 window adapter failed ({e})"),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let x11 = std::env::var_os("DISPLAY").is_some();

        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            match hyprland::HyprlandCompositor::connect() {
                Ok(c) => return Box::new(c),
                Err(e) => log::warn!("hyprland adapter failed ({e}), trying wlr"),
            }
        }
        if wayland {
            match wlr::WlrCompositor::connect() {
                Ok(c) => return Box::new(c),
                Err(e) => log::debug!("wlr-foreign-toplevel unavailable ({e}), trying plasma"),
            }
            match plasma::PlasmaCompositor::connect() {
                Ok(c) => return Box::new(c),
                Err(e) => log::debug!("plasma window-management unavailable ({e})"),
            }
        } else if x11 {
            match x11::X11Compositor::connect() {
                Ok(c) => return Box::new(c),
                Err(e) => log::warn!("x11 adapter failed ({e})"),
            }
        }
    }
    log::warn!("no window-listing protocol available; windows/actions rings degrade");
    Box::new(NullCompositor)
}
