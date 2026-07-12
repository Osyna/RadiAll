//! XGrabKey global-shortcut provider (any X11 window manager).
//!
//! Grabs each combo on the root window and calls `fire` with the mode on
//! KeyPress. Grabs fire regardless of focus, so this covers every plain X11
//! WM with zero configuration. Not used under Wayland: grabs through
//! XWayland only fire while an X11 client is focused.
//!
//! Threading: one dedicated RustConnection shared by two background threads.
//! The event thread blocks in wait_for_event() and only reads the shared
//! grab map; the control thread owns the grabs (ungrab-all + regrab, driven
//! by apply() messages) — RustConnection allows requests from any thread
//! while another waits for events. Both loops block, no polling.
//!
//! Lock keys: X delivers a grab only for the exact modifier state, so every
//! combo is grabbed four times — base, +Lock (Caps), +Mod2 (Num), +both.
//! GrabKey conflicts (BadAccess: someone else owns the combo) warn and skip
//! that combo; everything else keeps working.

use crate::config::Settings;
use crate::shortcuts::{combo_for, parse, valid, Combo, FireFn, MODES};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask, Window};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

/// Modifier bits a combo can ask for; KeyPress state is masked down to these
/// before lookup so Caps/Num lock (and pointer buttons) don't break matches.
const COMBO_MODS: u16 = 0x01 | 0x04 | 0x08 | 0x40; // Shift | Control | Mod1 | Mod4

/// Combo -> X modifier mask: ctrl->Control, shift->Shift, alt->Mod1,
/// super->Mod4 (the conventional mappings; good enough without XKB).
fn mod_mask(c: &Combo) -> u16 {
    let mut mask = 0u16;
    for m in &c.mods {
        mask |= match m.as_str() {
            "ctrl" | "control" => u16::from(ModMask::CONTROL),
            "shift" => u16::from(ModMask::SHIFT),
            "alt" => u16::from(ModMask::M1),
            "super" | "meta" => u16::from(ModMask::M4),
            other => {
                log::warn!("x11: unknown modifier {other:?} ignored");
                0
            }
        };
    }
    mask
}

/// Key name -> keysym. ASCII printables are their own keysyms (lowercased:
/// grabs are per-keycode); F-keys and a few navigation names on top.
fn keysym_for(key: &str) -> Option<u32> {
    let lower = key.to_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_graphic() {
            return Some(c as u32);
        }
    }
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0xffbe + n - 1); // XK_F1 ..= XK_F24
        }
    }
    match lower.as_str() {
        "space" => Some(0x20),
        "return" | "enter" => Some(0xff0d),
        "tab" => Some(0xff09),
        "escape" | "esc" => Some(0xff1b),
        _ => None,
    }
}

/// Keysym -> first keycode producing it, via the server keyboard mapping.
/// Fetched per regrab: layouts change and regrabs are rare.
fn keycode_for(conn: &RustConnection, keysym: u32) -> Option<u8> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(min) + 1;
    let reply = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
    let per = usize::from(reply.keysyms_per_keycode).max(1);
    reply
        .keysyms
        .chunks(per)
        .position(|syms| syms.contains(&keysym))
        .map(|i| min + i as u8)
}

/// (mode, combo) pairs to grab; empty when the master toggle is off.
fn combos(settings: &Settings) -> Vec<(&'static str, Combo)> {
    if !settings.shortcuts_enabled {
        return Vec::new();
    }
    MODES
        .iter()
        .filter(|mode| valid(combo_for(settings, mode)))
        .map(|mode| (*mode, parse(combo_for(settings, mode))))
        .collect()
}

type GrabMap = HashMap<(u8, u16), &'static str>;

/// Ungrab everything we hold, then grab `binds` from scratch and publish the
/// (keycode, base mask) -> mode map for the event thread.
fn regrab(conn: &RustConnection, root: Window, grabs: &Mutex<GrabMap>, binds: &[(&'static str, Combo)]) {
    let mut map = GrabMap::new();
    // AnyKey + AnyModifier: drops only OUR grabs on the root window.
    if let Err(e) = conn.ungrab_key(0u8, root, ModMask::ANY) {
        log::warn!("x11: ungrab failed, shortcuts inert: {e}");
        return;
    }
    for (mode, combo) in binds {
        let Some(keysym) = keysym_for(&combo.key) else {
            log::warn!("x11: no keysym for key {:?}; combo skipped", combo.key);
            continue;
        };
        let Some(keycode) = keycode_for(conn, keysym) else {
            log::warn!("x11: key {:?} not on this keyboard; combo skipped", combo.key);
            continue;
        };
        let base = mod_mask(combo);
        let lock = u16::from(ModMask::LOCK);
        let num = u16::from(ModMask::M2);
        let mut ok = true;
        for extra in [0, lock, num, lock | num] {
            let grabbed = conn
                .grab_key(
                    false,
                    root,
                    ModMask::from(base | extra),
                    keycode,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                )
                .map_err(|e| e.to_string())
                .and_then(|c| c.check().map_err(|e| e.to_string()));
            if let Err(e) = grabbed {
                // Typically BadAccess: another client owns this combo.
                log::warn!("x11: can't grab {mode} shortcut (taken?): {e}");
                ok = false;
                break;
            }
        }
        if ok {
            map.insert((keycode, base), mode);
        }
    }
    if let Err(e) = conn.flush() {
        log::warn!("x11: flush failed: {e}");
    }
    *grabs.lock().unwrap_or_else(|p| p.into_inner()) = map;
}

pub struct X11Provider {
    apply_tx: Sender<Vec<(&'static str, Combo)>>,
}

impl X11Provider {
    /// Connect to the X server and spawn the grab + event threads. None only
    /// when the display is unreachable; grab conflicts later just warn.
    pub fn start(settings: &Settings, fire: FireFn) -> Option<X11Provider> {
        let (conn, screen) = match x11rb::connect(None) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("x11: can't connect to display: {e}");
                return None;
            }
        };
        let root = conn.setup().roots[screen].root;
        let conn = Arc::new(conn);
        let grabs: Arc<Mutex<GrabMap>> = Arc::default();
        let (apply_tx, apply_rx) = channel::<Vec<(&'static str, Combo)>>();

        // Control: initial grab, then one regrab per apply() message.
        {
            let conn = Arc::clone(&conn);
            let grabs = Arc::clone(&grabs);
            let initial = combos(settings);
            let spawned = std::thread::Builder::new()
                .name("x11-shortcuts".into())
                .spawn(move || {
                    regrab(&conn, root, &grabs, &initial);
                    while let Ok(binds) = apply_rx.recv() {
                        regrab(&conn, root, &grabs, &binds);
                    }
                });
            if let Err(e) = spawned {
                log::warn!("x11: failed to spawn grab thread: {e}");
                return None;
            }
        }

        // Events: sole reader of the socket; fires the callback on KeyPress.
        {
            let grabs = Arc::clone(&grabs);
            let spawned = std::thread::Builder::new()
                .name("x11-keys".into())
                .spawn(move || loop {
                    match conn.wait_for_event() {
                        Ok(Event::KeyPress(e)) => {
                            let mask = u16::from(e.state) & COMBO_MODS;
                            let mode = grabs
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .get(&(e.detail, mask))
                                .copied();
                            if let Some(mode) = mode {
                                fire(mode);
                            }
                        }
                        Ok(_) => {} // KeyRelease etc.
                        Err(e) => {
                            log::warn!("x11: event loop failed, shortcuts inert: {e}");
                            return;
                        }
                    }
                });
            if let Err(e) = spawned {
                log::warn!("x11: failed to spawn event thread: {e}");
                return None;
            }
        }

        Some(X11Provider { apply_tx })
    }

    /// Re-sync: ungrab everything and regrab the combos now in settings.
    pub fn apply(&self, settings: &Settings) {
        if self.apply_tx.send(combos(settings)).is_err() {
            log::warn!("x11: grab thread gone; shortcut apply dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_mask_mapping() {
        assert_eq!(mod_mask(&parse("ctrl+a")), u16::from(ModMask::CONTROL));
        assert_eq!(mod_mask(&parse("shift+a")), u16::from(ModMask::SHIFT));
        assert_eq!(mod_mask(&parse("alt+a")), u16::from(ModMask::M1));
        assert_eq!(mod_mask(&parse("super+a")), u16::from(ModMask::M4));
        assert_eq!(mod_mask(&parse("meta+a")), u16::from(ModMask::M4));
        assert_eq!(
            mod_mask(&parse("ctrl+shift+r")),
            u16::from(ModMask::CONTROL) | u16::from(ModMask::SHIFT)
        );
        assert_eq!(mod_mask(&parse("hyper+a")), 0); // unknown ignored
        assert_eq!(mod_mask(&parse("f11")), 0);
    }

    #[test]
    fn keysyms_cover_latin_digits_and_named_keys() {
        assert_eq!(keysym_for("a"), Some('a' as u32));
        assert_eq!(keysym_for("A"), Some('a' as u32)); // case-folded
        assert_eq!(keysym_for("7"), Some('7' as u32));
        assert_eq!(keysym_for("f1"), Some(0xffbe));
        assert_eq!(keysym_for("F12"), Some(0xffc9));
        assert_eq!(keysym_for("space"), Some(0x20));
        assert_eq!(keysym_for("return"), Some(0xff0d));
        assert_eq!(keysym_for(""), None);
        assert_eq!(keysym_for("f99"), None);
    }

    /// Live probe against the X server in DISPLAY (XWayland on a Wayland
    /// dev box). Run manually:
    ///   RUST_LOG=warn cargo test shortcuts_x11 -- --ignored --nocapture
    /// Exercises connect + keysym->keycode resolution + the grab/regrab
    /// round-trip; grab conflicts surface as warn lines. Grabs die with the
    /// test process. Firing needs a focused X11 client, so it isn't asserted.
    #[test]
    #[ignore = "grabs keys on the real display"]
    fn live_grab_roundtrip() {
        let _ = env_logger::try_init(); // make regrab warns visible
        let settings = Settings::default(); // super+a / super+w / super+d
        let provider = X11Provider::start(&settings, Arc::new(|mode| println!("fired {mode}")))
            .expect("X display unreachable");
        std::thread::sleep(std::time::Duration::from_millis(500));
        provider.apply(&settings); // ungrab-all + regrab
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
