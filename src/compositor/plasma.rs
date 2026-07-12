//! KDE Plasma Wayland adapter via org_kde_plasma_window_management.
//!
//! KWin exposes this in every Plasma Wayland session; nothing else ships it,
//! so connect() fails elsewhere and detect() falls through (wlr is tried
//! first, which KWin does not implement).
//!
//! Protocol shape differs from wlr-foreign-toplevel in two ways that matter:
//!   - the manager announces windows by uint id / uuid string; the client
//!     creates the org_kde_plasma_window proxy itself via get_window[_by_uuid]
//!   - there is no per-batch `done`; `initial_state` fires once after the
//!     first property burst, later changes arrive as bare property events.
//!     We therefore publish on `initial_state`, then per-event afterwards.
//!
//! Threading mirrors wlr.rs: trait methods run on the UI thread, one
//! background thread owns all wayland dispatch (sole socket reader), ops are
//! forwarded over a channel and issued through the thread-safe proxies. Both
//! loops block — no polling. Trait methods only lock a mutex.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window::{self, OrgKdePlasmaWindow},
    org_kde_plasma_window_management::{self, OrgKdePlasmaWindowManagement, State},
};

use super::{Compositor, CompositorEvent, WindowId, WindowInfo};

/// State bits from org_kde_plasma_window_management.state (type-checked
/// against the crate's generated enum; the protocol sends them as a bitfield).
const STATE_ACTIVE: u32 = State::Active as u32;
const STATE_FULLSCREEN: u32 = State::Fullscreen as u32;
const STATE_SKIPTASKBAR: u32 = State::Skiptaskbar as u32;

/// `initial_state` exists since version 4; `destroy` too.
const VERSION_INITIAL_STATE: u32 = 4;

/// Decoded state_changed bitfield — only the bits RadiAll acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StateFlags {
    active: bool,
    fullscreen: bool,
    skiptaskbar: bool,
}

fn decode_state(flags: u32) -> StateFlags {
    StateFlags {
        active: flags & STATE_ACTIVE != 0,
        fullscreen: flags & STATE_FULLSCREEN != 0,
        skiptaskbar: flags & STATE_SKIPTASKBAR != 0,
    }
}

/// Operations forwarded from the UI thread to the wayland side.
enum Op {
    Activate(WindowId),
    Close(WindowId),
    Fullscreen(WindowId),
}

/// Per-window record, updated by org_kde_plasma_window events.
struct Window {
    handle: OrgKdePlasmaWindow,
    app_id: String,
    title: String,
    state: StateFlags,
    /// initial_state seen (or proxy predates it): publish on every change.
    /// Before that, the initial property burst stays silent and the
    /// initial_state publish covers it in one shot.
    initialized: bool,
}

/// State shared between the UI thread and the wayland threads.
struct Shared {
    windows: HashMap<WindowId, Window>,
    /// Compositor announcement order, so listings are stable.
    order: Vec<WindowId>,
    watch_tx: Option<Sender<CompositorEvent>>,
    /// Last published active id, to emit Active only on focus change.
    last_active: Option<WindowId>,
}

impl Shared {
    fn new() -> Self {
        Shared {
            windows: HashMap::new(),
            order: Vec::new(),
            watch_tx: None,
            last_active: None,
        }
    }

    /// Published view: windows with an empty app_id are phantoms and
    /// skiptaskbar windows (panels, plasmashell popups) are noise — skip.
    fn snapshot(&self) -> (Vec<WindowInfo>, Option<WindowInfo>) {
        let mut list = Vec::with_capacity(self.windows.len());
        let mut active = None;
        for id in &self.order {
            let Some(w) = self.windows.get(id) else { continue };
            if w.app_id.is_empty() || w.state.skiptaskbar {
                continue;
            }
            let info = WindowInfo {
                id: id.clone(),
                app_id: w.app_id.clone(),
                title: w.title.clone(),
                focused: w.state.active,
            };
            if w.state.active {
                active = Some(info.clone());
            }
            list.push(info);
        }
        (list, active)
    }

    /// Push events after a change. Windows always; Active only when focus
    /// actually moved.
    fn publish(&mut self) {
        let (list, active) = self.snapshot();
        let active_id = active.as_ref().map(|w| w.id.clone());
        if let Some(tx) = &self.watch_tx {
            if tx.send(CompositorEvent::Windows(list)).is_err() {
                // Receiver gone; stop pushing.
                self.watch_tx = None;
            } else if active_id != self.last_active {
                let _ = tx.send(CompositorEvent::Active(active));
            }
        }
        self.last_active = active_id;
    }

    /// Publish only once the window's initial burst completed — property
    /// events before initial_state would spam one snapshot per attribute.
    fn publish_if_initialized(&mut self, id: &WindowId) {
        if self.windows.get(id).is_some_and(|w| w.initialized) {
            self.publish();
        }
    }
}

/// Poison-proof lock: an event-thread panic must not take the UI thread down.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    match shared.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub struct PlasmaCompositor {
    ops: Sender<Op>,
    shared: Arc<Mutex<Shared>>,
}

impl PlasmaCompositor {
    pub fn connect() -> Result<Self, String> {
        let conn = Connection::connect_to_env()
            .map_err(|e| format!("wayland connect failed: {e}"))?;
        let (globals, mut queue) = registry_queue_init::<DispatchState>(&conn)
            .map_err(|e| format!("wayland registry init failed: {e}"))?;
        let qh = queue.handle();

        // v13+ announces windows via window_with_uuid; older KWins send the
        // deprecated uint-id event. Both are handled, so any version works.
        let _manager: OrgKdePlasmaWindowManagement = globals
            .bind(&qh, 1..=18, ())
            .map_err(|e| format!("org_kde_plasma_window_management unavailable: {e}"))?;

        let shared = Arc::new(Mutex::new(Shared::new()));
        let (op_tx, op_rx) = channel::<Op>();

        // Dispatch thread: sole reader of the wayland socket. Started here so
        // windows() is live before watch() is ever called.
        {
            let mut state = DispatchState { shared: Arc::clone(&shared) };
            std::thread::Builder::new()
                .name("plasma-events".into())
                .spawn(move || loop {
                    if let Err(e) = queue.blocking_dispatch(&mut state) {
                        log::warn!("plasma: wayland dispatch failed, adapter degraded: {e}");
                        return;
                    }
                })
                .map_err(|e| format!("failed to spawn plasma event thread: {e}"))?;
        }

        // Op executor: blocks on the channel, issues requests, flushes.
        {
            let shared = Arc::clone(&shared);
            let conn = conn.clone();
            std::thread::Builder::new()
                .name("plasma-ops".into())
                .spawn(move || {
                    while let Ok(op) = op_rx.recv() {
                        {
                            let s = lock(&shared);
                            match op {
                                Op::Activate(id) => match s.windows.get(&id) {
                                    Some(w) => w.handle.set_state(STATE_ACTIVE, STATE_ACTIVE),
                                    None => log::warn!("plasma: activate: unknown window {id}"),
                                },
                                Op::Close(id) => match s.windows.get(&id) {
                                    Some(w) => w.handle.close(),
                                    None => log::warn!("plasma: close: unknown window {id}"),
                                },
                                Op::Fullscreen(id) => match s.windows.get(&id) {
                                    Some(w) if w.state.fullscreen => {
                                        w.handle.set_state(STATE_FULLSCREEN, 0)
                                    }
                                    Some(w) => w.handle.set_state(STATE_FULLSCREEN, STATE_FULLSCREEN),
                                    None => log::warn!("plasma: fullscreen: unknown window {id}"),
                                },
                            }
                        }
                        if let Err(e) = conn.flush() {
                            log::warn!("plasma: wayland flush failed, ops disabled: {e}");
                            return;
                        }
                    }
                })
                .map_err(|e| format!("failed to spawn plasma op thread: {e}"))?;
        }

        Ok(PlasmaCompositor { ops: op_tx, shared })
    }

    fn send(&self, op: Op) {
        if self.ops.send(op).is_err() {
            log::warn!("plasma: op thread gone; window action dropped");
        }
    }
}

impl Compositor for PlasmaCompositor {
    fn backend(&self) -> &'static str {
        "plasma"
    }
    fn windows(&mut self) -> Vec<WindowInfo> {
        lock(&self.shared).snapshot().0
    }
    fn active_window(&mut self) -> Option<WindowInfo> {
        lock(&self.shared).snapshot().1
    }
    fn watch(&mut self, tx: Sender<CompositorEvent>) {
        let mut s = lock(&self.shared);
        // Sync the consumer immediately with the current state.
        let (list, active) = s.snapshot();
        let _ = tx.send(CompositorEvent::Windows(list));
        let _ = tx.send(CompositorEvent::Active(active));
        s.watch_tx = Some(tx);
    }
    fn activate(&mut self, id: &WindowId) {
        self.send(Op::Activate(id.clone()));
    }
    fn close_window(&mut self, id: &WindowId) {
        self.send(Op::Close(id.clone()));
    }
    fn fullscreen(&mut self, id: &WindowId) {
        self.send(Op::Fullscreen(id.clone()));
    }
}

// ---- wayland dispatch ----

struct DispatchState {
    shared: Arc<Mutex<Shared>>,
}

impl DispatchState {
    /// Register a newly announced window under `id` (the WindowId key, also
    /// stored as the proxy's user data so per-window events find the record).
    fn add_window(&mut self, id: WindowId, handle: OrgKdePlasmaWindow) {
        let mut s = lock(&self.shared);
        // Pre-initial_state versions never confirm the burst; publish per-event.
        let initialized = handle.version() < VERSION_INITIAL_STATE;
        s.order.push(id.clone());
        s.windows.insert(
            id,
            Window {
                handle,
                app_id: String::new(),
                title: String::new(),
                state: decode_state(0),
                initialized,
            },
        );
        // Properties + `initial_state` follow; publish happens there.
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for DispatchState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdePlasmaWindowManagement, ()> for DispatchState {
    fn event(
        state: &mut Self,
        manager: &OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        // KWin sends exactly one mapped-event per window, picked by bound
        // version: window_with_uuid on v13+, the deprecated uint id below.
        match event {
            org_kde_plasma_window_management::Event::WindowWithUuid { id, uuid } => {
                // The uuid is the stable identifier; the uint id is deprecated
                // and may be meaningless on new KWins.
                let key: WindowId = if uuid.is_empty() { id.to_string() } else { uuid.clone() };
                let handle = manager.get_window_by_uuid(uuid, qh, key.clone());
                state.add_window(key, handle);
            }
            org_kde_plasma_window_management::Event::Window { id } => {
                // Pre-v13 path: the uint id is the only identifier there is.
                let key: WindowId = id.to_string();
                let handle = manager.get_window(id, qh, key.clone());
                state.add_window(key, handle);
            }
            // show_desktop_changed / stacking order: not surfaced by the trait.
            _ => {}
        }
    }
}

impl Dispatch<OrgKdePlasmaWindow, WindowId> for DispatchState {
    fn event(
        state: &mut Self,
        window: &OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        id: &WindowId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut s = lock(&state.shared);
        match event {
            org_kde_plasma_window::Event::TitleChanged { title } => {
                if let Some(w) = s.windows.get_mut(id) {
                    w.title = title;
                }
                s.publish_if_initialized(id);
            }
            org_kde_plasma_window::Event::AppIdChanged { app_id } => {
                if let Some(w) = s.windows.get_mut(id) {
                    w.app_id = app_id;
                }
                s.publish_if_initialized(id);
            }
            org_kde_plasma_window::Event::StateChanged { flags } => {
                if let Some(w) = s.windows.get_mut(id) {
                    w.state = decode_state(flags);
                }
                s.publish_if_initialized(id);
            }
            org_kde_plasma_window::Event::InitialState => {
                if let Some(w) = s.windows.get_mut(id) {
                    w.initialized = true;
                }
                s.publish();
            }
            org_kde_plasma_window::Event::Unmapped => {
                // May arrive before initial_state for already-dead windows.
                s.windows.remove(id);
                s.order.retain(|o| o != id);
                if window.version() >= VERSION_INITIAL_STATE {
                    window.destroy();
                }
                s.publish();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_state_reads_the_documented_bits() {
        // Bits per org_kde_plasma_window_management.state.
        assert_eq!(
            decode_state(0x1),
            StateFlags { active: true, fullscreen: false, skiptaskbar: false }
        );
        assert_eq!(
            decode_state(0x8),
            StateFlags { active: false, fullscreen: true, skiptaskbar: false }
        );
        assert_eq!(
            decode_state(0x1000),
            StateFlags { active: false, fullscreen: false, skiptaskbar: true }
        );
        // Unrelated bits (minimized|maximized|closeable) don't leak through.
        assert_eq!(
            decode_state(0x2 | 0x4 | 0x100),
            StateFlags { active: false, fullscreen: false, skiptaskbar: false }
        );
        // Combined mask decodes all three at once.
        assert_eq!(
            decode_state(0x1 | 0x8 | 0x1000),
            StateFlags { active: true, fullscreen: true, skiptaskbar: true }
        );
    }

    /// On any non-KDE Wayland session the global is absent and connect() must
    /// fail cleanly (that is what lets detect() fall through). Skips silently
    /// without a Wayland session or under a real KWin.
    #[test]
    fn connect_fails_cleanly_without_kwin() {
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            eprintln!("skipped: WAYLAND_DISPLAY unset");
            return;
        }
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if desktop.to_lowercase().contains("kde") {
            eprintln!("skipped: running under KDE");
            return;
        }
        match PlasmaCompositor::connect() {
            Err(e) => println!("plasma connect refused as expected: {e}"),
            Ok(_) => panic!("connect succeeded on a non-KDE compositor"),
        }
    }
}
