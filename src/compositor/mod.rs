//! Compositor adapter. RadiAll is Hyprland-first but runs on any Wayland
//! compositor; everything compositor-specific lives behind this trait so the
//! rest of the app is generic (and a Win32 adapter can slot in later).
//!
//! Capabilities mirror the old Compositor.qml singleton:
//!   - window listing / focus / close / fullscreen -> Hyprland IPC or
//!     wlr-foreign-toplevel-management
//!   - float + send-keys are Hyprland-only          -> guarded by can_float / can_send_keys
//!   - global keybinds are Hyprland-only            -> guarded by can_manage_keybinds
//!     (elsewhere, users bind their compositor key to `radiall --apps`, which
//!      reaches the running daemon over the unix socket — see ipc.rs)

mod hyprland;
mod wlr;

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
    fn can_manage_keybinds(&self) -> bool {
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

/// No-op adapter for environments with no window-listing protocol (e.g. GNOME
/// Wayland). The apps ring still works fully; windows/actions rings degrade.
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
    fn watch(&mut self, _tx: Sender<CompositorEvent>) {}
    fn activate(&mut self, _id: &WindowId) {}
    fn close_window(&mut self, _id: &WindowId) {}
    fn fullscreen(&mut self, _id: &WindowId) {}
}

/// Pick the best adapter for the current session.
pub fn detect() -> Box<dyn Compositor> {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        match hyprland::HyprlandCompositor::connect() {
            Ok(c) => return Box::new(c),
            Err(e) => log::warn!("hyprland adapter failed ({e}), trying wlr"),
        }
    }
    match wlr::WlrCompositor::connect() {
        Ok(c) => Box::new(c),
        Err(e) => {
            log::warn!("no window-listing protocol available ({e}); windows/actions rings degrade");
            Box::new(NullCompositor)
        }
    }
}
