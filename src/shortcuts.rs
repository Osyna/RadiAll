//! Shortcut-string handling ("ctrl+shift+r") and Hyprland keybind management.
//!
//! Standalone deviation from v1: binds run `exec, radiall --<mode>` instead of
//! the hyprland-global-shortcuts protocol (`global, launcher:<mode>`), so the
//! same CLI path serves every compositor. Overwriting the old binds file
//! migrates v1 installs automatically.

use crate::config::Settings;
use std::path::PathBuf;
use std::process::Command;

const MOD_NAMES: [&str; 6] = ["ctrl", "control", "alt", "shift", "super", "meta"];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Combo {
    /// Lowercase modifiers in stored order.
    pub mods: Vec<String>,
    /// Key, case preserved.
    pub key: String,
}

/// "ctrl+shift+r" -> mods ["ctrl","shift"], key "r".
pub fn parse(s: &str) -> Combo {
    let mut parts: Vec<&str> = s.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    match parts.pop() {
        Some(key) => Combo {
            mods: parts.iter().map(|m| m.to_lowercase()).collect(),
            key: key.to_owned(),
        },
        None => Combo::default(),
    }
}

pub fn valid(s: &str) -> bool {
    let c = parse(s);
    !c.key.is_empty() && !MOD_NAMES.contains(&c.key.to_lowercase().as_str())
}

/// Hyprland MOD syntax: "CTRL SHIFT".
pub fn hypr_mods(c: &Combo) -> String {
    c.mods
        .iter()
        .map(|m| m.to_uppercase())
        .collect::<Vec<_>>()
        .join(" ")
}


// ------------------------------------------------------- hyprland binds

fn hypr_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/hypr")
}

pub fn binds_file() -> PathBuf {
    hypr_dir().join("launcher-binds.conf")
}

const MODES: [&str; 3] = ["apps", "windows", "actions"];

fn combo_for<'s>(settings: &'s Settings, mode: &str) -> &'s str {
    match mode {
        "windows" => &settings.shortcuts.windows,
        "actions" => &settings.shortcuts.actions,
        _ => &settings.shortcuts.apps,
    }
}

fn bind_line(mode: &str, combo: &Combo) -> String {
    format!(
        "bind = {}, {}, exec, radiall --{}\n",
        hypr_mods(combo),
        combo.key,
        mode
    )
}

pub fn write_binds_file(settings: &Settings) {
    let mut out =
        String::from("# Managed by RadiAll settings. Sourced by your hyprland config.\n");
    for mode in MODES {
        let s = combo_for(settings, mode);
        if valid(s) {
            out.push_str(&bind_line(mode, &parse(s)));
        }
    }
    std::fs::create_dir_all(hypr_dir()).ok();
    if let Err(e) = std::fs::write(binds_file(), out) {
        log::warn!("binds: can't write {} — {e}", binds_file().display());
    }
}

pub fn clear_binds_file() {
    std::fs::create_dir_all(hypr_dir()).ok();
    std::fs::write(
        binds_file(),
        "# RadiAll: keybinds applied at runtime; Hyprland config left untouched.\n",
    )
    .ok();
}

fn hyprctl(args: &[&str]) {
    match Command::new("hyprctl").args(args).output() {
        Ok(o) if !o.status.success() => {
            log::warn!("hyprctl {args:?}: {}", String::from_utf8_lossy(&o.stderr))
        }
        Err(e) => log::warn!("hyprctl {args:?}: {e}"),
        _ => {}
    }
}

fn unbind_live(combo: &Combo) {
    hyprctl(&[
        "keyword",
        "unbind",
        &format!("{},{}", hypr_mods(combo), combo.key),
    ]);
}

fn bind_live(mode: &str, combo: &Combo) {
    hyprctl(&[
        "keyword",
        "bind",
        &format!(
            "{},{},exec,radiall --{}",
            hypr_mods(combo),
            combo.key,
            mode
        ),
    ]);
}

/// Full reconcile, port of applyShortcuts(): no-op off Hyprland;
/// disabled -> unbind everything + stub file; enabled -> file per persistBinds
/// + live binds either way.
pub fn apply_shortcuts(settings: &Settings, can_manage: bool) {
    if !can_manage {
        return;
    }
    if !settings.shortcuts_enabled {
        for mode in MODES {
            let s = combo_for(settings, mode);
            if valid(s) {
                unbind_live(&parse(s));
            }
        }
        clear_binds_file();
        return;
    }
    if settings.persist_binds {
        write_binds_file(settings);
    } else {
        clear_binds_file();
    }
    for mode in MODES {
        let s = combo_for(settings, mode);
        if valid(s) {
            let c = parse(s);
            unbind_live(&c);
            bind_live(mode, &c);
        }
    }
}

/// One-shortcut update, port of setShortcut(): caller already stored the new
/// value in settings; `old` is the previous combo string.
pub fn update_shortcut(settings: &Settings, can_manage: bool, mode: &str, old: &str) {
    if !can_manage || !settings.shortcuts_enabled {
        return;
    }
    if settings.persist_binds {
        write_binds_file(settings);
    }
    let new = combo_for(settings, mode);
    if old != new && valid(old) {
        unbind_live(&parse(old));
    }
    if valid(new) {
        let c = parse(new);
        unbind_live(&c);
        bind_live(mode, &c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_mods() {
        let c = parse("ctrl+shift+r");
        assert_eq!(c.mods, ["ctrl", "shift"]);
        assert_eq!(c.key, "r");
        assert_eq!(hypr_mods(&c), "CTRL SHIFT");
        assert_eq!(parse("").key, "");
        assert_eq!(parse("f11").mods.len(), 0);
    }

    #[test]
    fn validity_rejects_lone_modifiers() {
        assert!(valid("super+a"));
        assert!(valid("f5"));
        assert!(!valid("ctrl+shift"));
        assert!(!valid(""));
        assert!(!valid("super"));
    }


    #[test]
    fn bind_lines_use_exec_cli() {
        let line = bind_line("apps", &parse("super+a"));
        assert_eq!(line, "bind = SUPER, a, exec, radiall --apps\n");
    }
}
