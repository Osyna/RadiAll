//! Shortcut-string handling ("ctrl+shift+r") and global-shortcut providers.
//!
//! Combo parsing plus the Hyprland keybind path live here; the portal and
//! X11 providers live in `shortcuts_portal` / `shortcuts_x11`. `Shortcuts`
//! wraps whichever provider `detect_provider()` picked behind one handle.
//!
//! Standalone deviation from v1: Hyprland binds run `exec, radiall --<mode>`
//! instead of the hyprland-global-shortcuts protocol (`global,
//! launcher:<mode>`), so the same CLI path serves every compositor.
//! Overwriting the old binds file migrates v1 installs automatically.

use crate::config::Settings;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

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

pub(crate) const MODES: [&str; 3] = ["apps", "windows", "actions"];

pub(crate) fn combo_for<'s>(settings: &'s Settings, mode: &str) -> &'s str {
    match mode {
        "windows" => &settings.shortcuts.windows,
        "actions" => &settings.shortcuts.actions,
        _ => &settings.shortcuts.apps,
    }
}

/// The command binds should exec. Prefer the running daemon's own binary so
/// binds work even when RadiAll runs from a build tree and isn't on PATH;
/// fall back to a bare `radiall` (PATH lookup) if current_exe is unreadable.
fn exec_cmd() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .filter(|p| !p.contains(' ')) // hyprland exec args are not quoted
        .unwrap_or_else(|| "radiall".into())
}

fn bind_line(mode: &str, combo: &Combo) -> String {
    format!(
        "bind = {}, {}, exec, {} --{}\n",
        hypr_mods(combo),
        combo.key,
        exec_cmd(),
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

#[cfg(target_os = "linux")]
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

/// Remove any bind on this combo. Hyprland stores "a" and "A" as distinct
/// binds yet a keypress FIRES both — a user's own `SUPER, A` config bind
/// plus our runtime `SUPER, a` would toggle the ring twice (open + instant
/// close). Unbind every case variant so exactly one bind survives.
fn unbind_live(combo: &Combo) {
    let mods = hypr_mods(combo);
    let mut variants = vec![combo.key.clone()];
    let (lo, up) = (combo.key.to_lowercase(), combo.key.to_uppercase());
    if lo != combo.key {
        variants.push(lo);
    }
    if up != combo.key && up.chars().count() == combo.key.chars().count() {
        variants.push(up);
    }
    for key in variants {
        hyprctl(&["keyword", "unbind", &format!("{mods},{key}")]);
    }
}

fn bind_live(mode: &str, combo: &Combo) {
    hyprctl(&[
        "keyword",
        "bind",
        &format!(
            "{},{},exec,{} --{}",
            hypr_mods(combo),
            combo.key,
            exec_cmd(),
            mode
        ),
    ]);
}

/// Full reconcile, port of applyShortcuts(): no-op off Hyprland;
/// disabled -> unbind everything + stub file; enabled -> file per persistBinds
/// + live binds either way.
#[cfg(target_os = "linux")]
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

// ------------------------------------------------------- provider layer

/// Callback the non-Hyprland providers invoke when a shortcut fires.
/// Receives "apps" | "windows" | "actions".
pub(crate) type FireFn = Arc<dyn Fn(&'static str) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Hyprland: hyprctl-managed binds that exec the CLI (today's path).
    Hyprctl,
    /// XDG desktop portal org.freedesktop.portal.GlobalShortcuts.
    Portal,
    /// Plain X11 XGrabKey on the root window.
    X11,
    /// Windows: RegisterHotKey global hotkeys (in-process message loop).
    Win,
    /// No provider; the user binds `radiall --<mode>` in their WM config.
    None,
}

/// RADIALL_SHORTCUTS=hyprctl|portal|x11|none -> kind; anything else None.
fn provider_override(raw: &str) -> Option<ProviderKind> {
    match raw.trim().to_lowercase().as_str() {
        "hyprctl" | "hyprland" => Some(ProviderKind::Hyprctl),
        "portal" => Some(ProviderKind::Portal),
        "x11" => Some(ProviderKind::X11),
        "win" | "windows" => Some(ProviderKind::Win),
        "none" | "off" => Some(ProviderKind::None),
        _ => None,
    }
}

/// Pick the shortcut provider for this session. RADIALL_SHORTCUTS wins
/// unconditionally; otherwise: Hyprland -> hyprctl binds, other Wayland ->
/// portal if the session bus offers GlobalShortcuts, bare X11 -> XGrabKey.
/// Wayland without the portal yields None — X11 grabs through XWayland
/// would only fire while an X11 client is focused, which reads as broken.
pub fn detect_provider() -> ProviderKind {
    if let Ok(raw) = std::env::var("RADIALL_SHORTCUTS") {
        match provider_override(&raw) {
            Some(kind) => {
                log::info!("shortcuts: RADIALL_SHORTCUTS={raw} forces {kind:?}");
                return kind;
            }
            None => log::warn!(
                "shortcuts: RADIALL_SHORTCUTS={raw:?} not one of hyprctl|portal|x11|none; autodetecting"
            ),
        }
    }
    #[cfg(windows)]
    {
        ProviderKind::Win
    }
    #[cfg(target_os = "linux")]
    {
        let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.is_empty());
        if set("HYPRLAND_INSTANCE_SIGNATURE") {
            ProviderKind::Hyprctl
        } else if set("WAYLAND_DISPLAY") {
            if crate::shortcuts_portal::available() {
                ProviderKind::Portal
            } else {
                ProviderKind::None
            }
        } else if set("DISPLAY") {
            ProviderKind::X11
        } else {
            ProviderKind::None
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        ProviderKind::None
    }
}

/// The running provider. Hyprctl keeps no state (binds live in the
/// compositor); portal/X11 hold a handle to their background thread.
enum Provider {
    #[cfg(target_os = "linux")]
    Hyprctl,
    #[cfg(target_os = "linux")]
    Portal(crate::shortcuts_portal::PortalProvider),
    #[cfg(target_os = "linux")]
    X11(crate::shortcuts_x11::X11Provider),
    #[cfg(windows)]
    Win(crate::shortcuts_win::WinProvider),
    Inert,
}

/// Handle owning the active global-shortcut provider.
pub struct Shortcuts {
    kind: ProviderKind,
    provider: Provider,
}

impl Shortcuts {
    /// Start `kind` and sync it to `settings`. `fire` is called (from a
    /// provider thread) with the mode of a triggered shortcut; the Hyprctl
    /// provider never fires — its binds exec the CLI directly. A provider
    /// that fails to start degrades to `ProviderKind::None` with a warning;
    /// the daemon keeps running either way.
    pub fn start(
        kind: ProviderKind,
        settings: &Settings,
        fire: impl Fn(&'static str) + Send + Sync + 'static,
    ) -> Shortcuts {
        let fire: FireFn = Arc::new(fire);
        #[cfg(target_os = "linux")]
        let (kind, provider) = match kind {
            ProviderKind::Hyprctl => {
                apply_shortcuts(settings, true);
                (ProviderKind::Hyprctl, Provider::Hyprctl)
            }
            ProviderKind::Portal => match crate::shortcuts_portal::PortalProvider::start(settings, fire) {
                Some(p) => (ProviderKind::Portal, Provider::Portal(p)),
                None => (ProviderKind::None, Provider::Inert),
            },
            ProviderKind::X11 => match crate::shortcuts_x11::X11Provider::start(settings, fire) {
                Some(p) => (ProviderKind::X11, Provider::X11(p)),
                None => (ProviderKind::None, Provider::Inert),
            },
            // The Windows kind can't be selected on Unix; degrade cleanly.
            ProviderKind::Win | ProviderKind::None => (ProviderKind::None, Provider::Inert),
        };
        #[cfg(windows)]
        let (kind, provider) = match kind {
            ProviderKind::Win => match crate::shortcuts_win::WinProvider::start(settings, fire) {
                Some(p) => (ProviderKind::Win, Provider::Win(p)),
                None => (ProviderKind::None, Provider::Inert),
            },
            // Unix-only kinds can't be selected on Windows; degrade cleanly.
            _ => (ProviderKind::None, Provider::Inert),
        };
        #[cfg(not(any(windows, target_os = "linux")))]
        let (kind, provider) = {
            let _ = (kind, settings, fire); // no in-process hotkey provider here
            (ProviderKind::None, Provider::Inert)
        };
        let s = Shortcuts { kind, provider };
        log::info!("shortcuts: provider {}", s.backend_name());
        s
    }

    /// Re-sync bindings after a settings change (combos edited, master
    /// toggle flipped).
    pub fn apply(&self, settings: &Settings) {
        // off Linux/Windows the only provider is Inert; settings goes unused
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = settings;
        match &self.provider {
            #[cfg(target_os = "linux")]
            Provider::Hyprctl => apply_shortcuts(settings, true),
            #[cfg(target_os = "linux")]
            Provider::Portal(p) => p.apply(settings),
            #[cfg(target_os = "linux")]
            Provider::X11(p) => p.apply(settings),
            #[cfg(windows)]
            Provider::Win(p) => p.apply(settings),
            Provider::Inert => {}
        }
    }

    pub fn kind(&self) -> ProviderKind {
        self.kind
    }

    pub fn backend_name(&self) -> &'static str {
        match self.kind {
            ProviderKind::Hyprctl => "hyprland",
            ProviderKind::Portal => "portal",
            ProviderKind::X11 => "x11",
            ProviderKind::Win => "win",
            ProviderKind::None => "none",
        }
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
        assert!(line.starts_with("bind = SUPER, a, exec, "), "{line}");
        assert!(line.ends_with(" --apps\n"), "{line}");
        // the exec target is the running binary (or bare radiall fallback)
        assert!(line.contains("radiall"), "{line}");
    }

    #[test]
    fn provider_override_parsing() {
        assert_eq!(provider_override("hyprctl"), Some(ProviderKind::Hyprctl));
        assert_eq!(provider_override("hyprland"), Some(ProviderKind::Hyprctl));
        assert_eq!(provider_override(" Portal "), Some(ProviderKind::Portal));
        assert_eq!(provider_override("X11"), Some(ProviderKind::X11));
        assert_eq!(provider_override("none"), Some(ProviderKind::None));
        assert_eq!(provider_override("off"), Some(ProviderKind::None));
        assert_eq!(provider_override("kglobalaccel"), None);
        assert_eq!(provider_override(""), None);
    }
}
