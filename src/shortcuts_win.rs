//! RegisterHotKey global-shortcut provider (Windows).
//!
//! Windows has no compositor to bind keys in, so the daemon registers the ring
//! hotkeys itself. `RegisterHotKey(NULL, id, …)` binds a system-wide hotkey to
//! the *calling thread*: `WM_HOTKEY` is then posted to that thread's message
//! queue. So one dedicated thread registers the combos and blocks in a
//! `GetMessage` loop, firing the mode callback on each `WM_HOTKEY`.
//!
//! Threading: hotkeys are thread-affine (registered from and delivered to the
//! same thread), so `apply()` never touches them directly — it stores the new
//! combos and posts `WM_APP_REAPPLY`, and the owning thread does the actual
//! unregister + re-register. Combos Windows reserves (e.g. Win+A/W/D) fail to
//! register; that just warns and skips, the rest keep working.

use crate::config::Settings;
use crate::shortcuts::{combo_for, parse, valid, Combo, FireFn, MODES};
use parking_lot::Mutex;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW, TranslateMessage, MSG,
};

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
const MOD_NOREPEAT: u32 = 0x4000;
const WM_HOTKEY: u32 = 0x0312;
const WM_USER: u32 = 0x0400;
const WM_APP_REAPPLY: u32 = 0x8001; // WM_APP + 1
const WM_APP_QUIT: u32 = 0x8002; // WM_APP + 2
const PM_NOREMOVE: u32 = 0x0000;

/// Owns the background hotkey thread. `apply` and `Drop` steer it by posting
/// thread messages to `tid`.
pub struct WinProvider {
    tid: u32,
    combos: Arc<Mutex<Vec<(&'static str, Combo)>>>,
}

impl WinProvider {
    pub fn start(settings: &Settings, fire: FireFn) -> Option<WinProvider> {
        let combos = Arc::new(Mutex::new(combos_of(settings)));
        let combos_thread = Arc::clone(&combos);
        let (tx_tid, rx_tid) = channel::<u32>();

        let spawned = std::thread::Builder::new()
            .name("win-hotkeys".into())
            .spawn(move || {
                let tid = unsafe { GetCurrentThreadId() };
                // Force a message queue to exist before anyone PostThreadMessages us.
                let mut msg: MSG = unsafe { std::mem::zeroed() };
                unsafe {
                    PeekMessageW(&mut msg, std::ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
                    register_all(&combos_thread.lock());
                }
                // Only now can apply()/drop() reach us.
                let _ = tx_tid.send(tid);

                loop {
                    let got = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
                    if got <= 0 {
                        break; // WM_QUIT (0) or error (-1)
                    }
                    match msg.message {
                        WM_HOTKEY => {
                            if let Some(mode) = mode_for_id(msg.wParam as i32) {
                                fire(mode);
                            }
                        }
                        WM_APP_REAPPLY => unsafe {
                            unregister_all();
                            register_all(&combos_thread.lock());
                        },
                        WM_APP_QUIT => {
                            unsafe { unregister_all() };
                            break;
                        }
                        _ => unsafe {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        },
                    }
                }
            });

        if let Err(e) = spawned {
            log::warn!("win: failed to spawn hotkey thread: {e}");
            return None;
        }
        match rx_tid.recv_timeout(Duration::from_secs(2)) {
            Ok(tid) => Some(WinProvider { tid, combos }),
            Err(_) => {
                log::warn!("win: hotkey thread did not start");
                None
            }
        }
    }

    /// Re-sync: store the new combos and let the owning thread re-register.
    pub fn apply(&self, settings: &Settings) {
        *self.combos.lock() = combos_of(settings);
        unsafe { PostThreadMessageW(self.tid, WM_APP_REAPPLY, 0, 0) };
    }
}

impl Drop for WinProvider {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.tid, WM_APP_QUIT, 0, 0) };
    }
}

fn combos_of(settings: &Settings) -> Vec<(&'static str, Combo)> {
    MODES
        .iter()
        .filter_map(|&mode| {
            let s = combo_for(settings, mode);
            valid(s).then(|| (mode, parse(s)))
        })
        .collect()
}

/// Stable hotkey id per mode (WM_HOTKEY wParam).
fn id_for_mode(mode: &str) -> i32 {
    match mode {
        "apps" => 1,
        "windows" => 2,
        "actions" => 3,
        _ => 0,
    }
}

fn mode_for_id(id: i32) -> Option<&'static str> {
    match id {
        1 => Some("apps"),
        2 => Some("windows"),
        3 => Some("actions"),
        _ => None,
    }
}

/// Combo modifiers -> RegisterHotKey fsModifiers (+ MOD_NOREPEAT so a held
/// key doesn't machine-gun the ring open/closed).
fn mods_for(c: &Combo) -> u32 {
    let mut m = MOD_NOREPEAT;
    for name in &c.mods {
        m |= match name.as_str() {
            "ctrl" | "control" => MOD_CONTROL,
            "alt" => MOD_ALT,
            "shift" => MOD_SHIFT,
            "super" | "meta" => MOD_WIN,
            other => {
                log::warn!("win: unknown modifier {other:?} ignored");
                0
            }
        };
    }
    m
}

/// Key name -> Win32 virtual-key code. Letters/digits are their ASCII-uppercase
/// value (== the VK code); F-keys and a few named keys on top.
fn vk_for(key: &str) -> Option<u32> {
    let lower = key.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase() as u32);
        }
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1); // VK_F1 ..= VK_F24
        }
    }
    match lower.as_str() {
        "space" => Some(0x20),
        "return" | "enter" => Some(0x0D),
        "tab" => Some(0x09),
        "escape" | "esc" => Some(0x1B),
        _ => None,
    }
}

/// Register every combo on the current thread. MUST run on the hotkey thread.
unsafe fn register_all(combos: &[(&'static str, Combo)]) {
    for (mode, combo) in combos {
        let id = id_for_mode(mode);
        if id == 0 {
            continue;
        }
        let Some(vk) = vk_for(&combo.key) else {
            log::warn!("win: no virtual-key for {:?}", combo.key);
            continue;
        };
        if RegisterHotKey(std::ptr::null_mut(), id, mods_for(combo), vk) == 0 {
            log::warn!(
                "win: RegisterHotKey failed for {mode} ({combo:?}); the combo may be reserved by Windows"
            );
        }
    }
}

/// Unregister all three ids. MUST run on the hotkey thread.
unsafe fn unregister_all() {
    for id in 1..=3 {
        UnregisterHotKey(std::ptr::null_mut(), id);
    }
}
