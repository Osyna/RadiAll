//! wlr-foreign-toplevel-management adapter (generic Wayland).
//!
//! Works on any compositor exposing zwlr_foreign_toplevel_manager_v1
//! (Hyprland, Sway, river, labwc, ...). GNOME does not — connect() fails and
//! detect() falls back to NullCompositor.
//!
//! Threading: trait methods run on the UI thread, wayland events flow on a
//! queue. All wayland *dispatch* (socket reads) happens on one background
//! thread spawned in connect(). Operations are sent over a channel and issued
//! as requests through the thread-safe proxies (only the dispatch thread ever
//! reads the socket; request submission + flush is safe from any thread).
//! Both loops block — no polling. Trait methods only lock a mutex.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, MutexGuard};

use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat::WlSeat};
use wayland_client::{event_created_child, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use super::{Compositor, CompositorEvent, WindowId, WindowInfo};

/// Operations forwarded from the UI thread to the wayland side.
enum Op {
    Activate(WindowId),
    Close(WindowId),
    Fullscreen(WindowId),
}

/// Per-toplevel record, updated by handle events, committed on `done`.
struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    app_id: String,
    title: String,
    activated: bool,
    fullscreen: bool,
}

/// State shared between the UI thread and the wayland threads.
struct Shared {
    toplevels: HashMap<WindowId, Toplevel>,
    /// Compositor announcement order, so listings are stable.
    order: Vec<WindowId>,
    watch_tx: Option<Sender<CompositorEvent>>,
    /// Last published active id, to emit Active only on focus change.
    last_active: Option<WindowId>,
}

impl Shared {
    fn new() -> Self {
        Shared {
            toplevels: HashMap::new(),
            order: Vec::new(),
            watch_tx: None,
            last_active: None,
        }
    }

    /// Published view: toplevels with an empty app_id are phantoms — skip.
    fn snapshot(&self) -> (Vec<WindowInfo>, Option<WindowInfo>) {
        let mut list = Vec::with_capacity(self.toplevels.len());
        let mut active = None;
        for id in &self.order {
            let Some(t) = self.toplevels.get(id) else { continue };
            if t.app_id.is_empty() {
                continue;
            }
            let info = WindowInfo {
                id: id.clone(),
                app_id: t.app_id.clone(),
                title: t.title.clone(),
                focused: t.activated,
            };
            if t.activated {
                active = Some(info.clone());
            }
            list.push(info);
        }
        (list, active)
    }

    /// Push events after a `done` batch (or a close). Windows always; Active
    /// only when focus actually moved.
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
}

/// Poison-proof lock: an event-thread panic must not take the UI thread down.
fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    match shared.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn proxy_id(handle: &ZwlrForeignToplevelHandleV1) -> WindowId {
    handle.id().protocol_id().to_string()
}

pub struct WlrCompositor {
    ops: Sender<Op>,
    shared: Arc<Mutex<Shared>>,
}

impl WlrCompositor {
    pub fn connect() -> Result<Self, String> {
        let conn = Connection::connect_to_env()
            .map_err(|e| format!("wayland connect failed: {e}"))?;
        let (globals, mut queue) = registry_queue_init::<DispatchState>(&conn)
            .map_err(|e| format!("wayland registry init failed: {e}"))?;
        let qh = queue.handle();

        let _manager: ZwlrForeignToplevelManagerV1 = globals
            .bind(&qh, 1..=3, ())
            .map_err(|e| format!("zwlr_foreign_toplevel_manager_v1 unavailable: {e}"))?;
        let seat: WlSeat = globals
            .bind(&qh, 1..=4, ())
            .map_err(|e| format!("wl_seat unavailable: {e}"))?;

        let shared = Arc::new(Mutex::new(Shared::new()));
        let (op_tx, op_rx) = channel::<Op>();

        // Dispatch thread: sole reader of the wayland socket. Started here so
        // windows() is live before watch() is ever called.
        {
            let mut state = DispatchState { shared: Arc::clone(&shared) };
            std::thread::Builder::new()
                .name("wlr-events".into())
                .spawn(move || loop {
                    if let Err(e) = queue.blocking_dispatch(&mut state) {
                        log::warn!("wlr: wayland dispatch failed, adapter degraded: {e}");
                        return;
                    }
                })
                .map_err(|e| format!("failed to spawn wlr event thread: {e}"))?;
        }

        // Op executor: blocks on the channel, issues requests, flushes.
        {
            let shared = Arc::clone(&shared);
            let conn = conn.clone();
            std::thread::Builder::new()
                .name("wlr-ops".into())
                .spawn(move || {
                    while let Ok(op) = op_rx.recv() {
                        {
                            let s = lock(&shared);
                            match op {
                                Op::Activate(id) => match s.toplevels.get(&id) {
                                    Some(t) => t.handle.activate(&seat),
                                    None => log::warn!("wlr: activate: unknown window {id}"),
                                },
                                Op::Close(id) => match s.toplevels.get(&id) {
                                    Some(t) => t.handle.close(),
                                    None => log::warn!("wlr: close: unknown window {id}"),
                                },
                                Op::Fullscreen(id) => match s.toplevels.get(&id) {
                                    Some(t) if t.fullscreen => t.handle.unset_fullscreen(),
                                    Some(t) => t.handle.set_fullscreen(None),
                                    None => log::warn!("wlr: fullscreen: unknown window {id}"),
                                },
                            }
                        }
                        if let Err(e) = conn.flush() {
                            log::warn!("wlr: wayland flush failed, ops disabled: {e}");
                            return;
                        }
                    }
                })
                .map_err(|e| format!("failed to spawn wlr op thread: {e}"))?;
        }

        Ok(WlrCompositor { ops: op_tx, shared })
    }

    fn send(&self, op: Op) {
        if self.ops.send(op).is_err() {
            log::warn!("wlr: op thread gone; window action dropped");
        }
    }
}

impl Compositor for WlrCompositor {
    fn backend(&self) -> &'static str {
        "wlr"
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

wayland_client::delegate_noop!(DispatchState: ignore WlSeat);

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for DispatchState {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                let id = proxy_id(&toplevel);
                let mut s = lock(&state.shared);
                s.order.push(id.clone());
                s.toplevels.insert(
                    id,
                    Toplevel {
                        handle: toplevel,
                        app_id: String::new(),
                        title: String::new(),
                        activated: false,
                        fullscreen: false,
                    },
                );
                // Properties + `done` follow; publish happens there.
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                log::warn!("wlr: compositor finished the toplevel manager; list frozen");
            }
            _ => {}
        }
    }

    event_created_child!(DispatchState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for DispatchState {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy_id(handle);
        let mut s = lock(&state.shared);
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(t) = s.toplevels.get_mut(&id) {
                    t.title = title;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(t) = s.toplevels.get_mut(&id) {
                    t.app_id = app_id;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: raw } => {
                // Array of native-endian u32; per protocol enum:
                // 0=maximized 1=minimized 2=activated 3=fullscreen.
                let mut activated = false;
                let mut fullscreen = false;
                for chunk in raw.chunks_exact(4) {
                    match u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) {
                        2 => activated = true,
                        3 => fullscreen = true,
                        _ => {}
                    }
                }
                if let Some(t) = s.toplevels.get_mut(&id) {
                    t.activated = activated;
                    t.fullscreen = fullscreen;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                s.publish();
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                s.toplevels.remove(&id);
                s.order.retain(|o| o != &id);
                handle.destroy();
                s.publish();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live smoke test against the running compositor (Hyprland also
    /// implements zwlr-foreign-toplevel-management). Skips silently when no
    /// Wayland session is available.
    #[test]
    fn smoke_lists_real_windows() {
        if std::env::var("WAYLAND_DISPLAY").is_err() {
            eprintln!("skipped: WAYLAND_DISPLAY unset");
            return;
        }
        let mut c = match WlrCompositor::connect() {
            Ok(c) => c,
            Err(e) => panic!("connect failed on a live wayland session: {e}"),
        };
        std::thread::sleep(std::time::Duration::from_millis(300));
        let ws = c.windows();
        println!("wlr smoke: {} window(s)", ws.len());
        for w in &ws {
            println!(
                "  [{}] app_id={:?} title={:?} focused={}",
                w.id, w.app_id, w.title, w.focused
            );
        }
        println!("wlr smoke: active = {:?}", c.active_window());
        assert!(!ws.is_empty(), "expected at least one toplevel on a live session");
    }
}
