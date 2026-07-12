//! Persisted state: settings.json + apps.json in ~/.config/radiall/.
//! Schemas are 1:1 with the Quickshell version (launcher-settings.json /
//! apps.json); on first run we migrate from the old install path.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("radiall")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}
fn apps_path() -> PathBuf {
    config_dir().join("apps.json")
}

/// Old Quickshell-era config dir, for one-time migration.
fn legacy_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("quickshell/radiall")
}

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Shortcuts {
    pub apps: String,
    pub windows: String,
    pub actions: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            apps: "super+a".into(),
            windows: "super+w".into(),
            actions: "super+d".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Hover-sector / selected-icon color ("radish red").
    pub accent: String,
    /// Donut band color.
    pub bg: String,
    pub icon_size: f32,
    pub ring_radius: f32,
    /// Opacity of the full-screen black backdrop while open.
    pub dim: f32,
    pub wheel_opacity: f32,
    /// Corner radius of the ACTIVE section (the accent wedge), design px.
    pub sector_radius: f32,
    /// Corner radius of INACTIVE sections (only visible with seg_bg), design px.
    pub seg_radius: f32,
    /// Radial padding between the band edges and the sections; 0 = flush.
    pub section_inset: f32,
    /// Angular padding between sections, in px of arc.
    pub seg_gap: f32,
    /// Inactive-section fill; "" = invisible (band shows through).
    pub seg_bg: String,
    /// Center hole minimum radius, design px (the hole grows with ring size).
    pub hole_size: f32,
    pub show_labels: bool,
    /// Window-count dots under app icons.
    pub show_dots: bool,
    /// Live window thumbnail while scrolling a multi-window app.
    /// Parsed for compat; the standalone build shows a title preview instead.
    pub thumbnails: bool,
    /// Accent sector tracks the cursor across the whole screen.
    pub follow_outside: bool,
    /// Persisted since v1 but read by nothing (kept for file compat).
    pub hold_ms: f32,
    /// Write binds to ~/.config/hypr/launcher-binds.conf vs live-only.
    pub persist_binds: bool,
    /// Master on/off for RadiAll-managed ring keybinds (Hyprland).
    pub shortcuts_enabled: bool,
    pub shortcuts: Shortcuts,
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            accent: "#e44854".into(),
            bg: "#c9d8ef".into(),
            icon_size: 54.0,
            ring_radius: 150.0,
            dim: 0.28,
            wheel_opacity: 0.96,
            sector_radius: 6.0,
            seg_radius: 6.0,
            section_inset: 3.0,
            seg_gap: 0.0,
            seg_bg: String::new(),
            hole_size: 64.0,
            show_labels: true,
            show_dots: true,
            thumbnails: false,
            follow_outside: false,
            hold_ms: 450.0,
            persist_binds: true,
            shortcuts_enabled: true,
            shortcuts: Shortcuts::default(),
            theme: "default".into(),
        }
    }
}

// ------------------------------------------------------------------- apps

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CustomAction {
    pub label: String,
    /// Key combo, lowercase, '+'-joined; last segment = key.
    pub shortcut: String,
    pub icon: String,
    pub color: String,
}

impl Default for CustomAction {
    fn default() -> Self {
        Self {
            label: "New action".into(),
            shortcut: "ctrl+r".into(),
            icon: String::new(),
            color: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppEntry {
    pub name: String,
    /// Icon-theme name or absolute file path.
    pub icon: String,
    /// argv vector, launched verbatim (no shell, field codes pre-stripped).
    pub exec: Vec<String>,
    /// Window-class key for ALL window matching (case-insensitive).
    pub wm_class: String,
    /// Optional per-app accent: hovered wedge + selected dot ("" = theme).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub color: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub custom_actions: Vec<CustomAction>,
    /// Whitelist of enabled action-template ids; None = all enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_ids: Option<Vec<String>>,
}

fn default_apps() -> Vec<AppEntry> {
    let mk = |name: &str, icon: &str, exec: &str, wm: &str| AppEntry {
        name: name.into(),
        icon: icon.into(),
        exec: vec![exec.into()],
        wm_class: wm.into(),
        ..Default::default()
    };
    vec![
        mk("Firefox", "firefox", "firefox", "firefox"),
        mk("Terminal", "utilities-terminal", "kitty", "kitty"),
        mk("Files", "system-file-manager", "nautilus", "nautilus"),
        mk("Editor", "text-editor", "code", "Code"),
    ]
}

// -------------------------------------------------------------------- io

fn read_json<T: for<'a> Deserialize<'a>>(path: &PathBuf) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            log::warn!("config: bad {} — {e}", path.display());
            None
        }
    }
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    match serde_json::to_string_pretty(value) {
        Ok(text) => {
            if let Err(e) = std::fs::write(path, text) {
                log::warn!("config: can't write {} — {e}", path.display());
            }
        }
        Err(e) => log::warn!("config: serialize failed — {e}"),
    }
}

/// One-time migration from the Quickshell install: copy old settings/apps/themes
/// into ~/.config/radiall/ when the new dir doesn't exist yet.
fn migrate_legacy() {
    let new = config_dir();
    if new.exists() {
        return;
    }
    let old = legacy_dir();
    if !old.exists() {
        return;
    }
    std::fs::create_dir_all(&new).ok();
    for (from, to) in [
        ("launcher-settings.json", "settings.json"),
        ("apps.json", "apps.json"),
    ] {
        if std::fs::copy(old.join(from), new.join(to)).is_ok() {
            log::info!("config: migrated {from} from the Quickshell install");
        }
    }
    if let Ok(entries) = std::fs::read_dir(old.join("themes")) {
        let themes = new.join("themes");
        std::fs::create_dir_all(&themes).ok();
        for e in entries.flatten() {
            std::fs::copy(e.path(), themes.join(e.file_name())).ok();
        }
    }
}

/// Seed the user themes dir with the bundled themes (never overwrites).
pub fn seed_themes(bundled: &[(&str, &str)]) {
    let dir = crate::theme::themes_dir();
    std::fs::create_dir_all(&dir).ok();
    for (name, body) in bundled {
        let path = dir.join(format!("{name}.json"));
        if !path.exists() {
            std::fs::write(path, body).ok();
        }
    }
}

pub fn load_settings() -> Settings {
    migrate_legacy();
    match read_json::<Settings>(&settings_path()) {
        Some(s) => s,
        None => {
            let s = Settings::default();
            write_json(&settings_path(), &s); // seed on first run, like v1
            s
        }
    }
}

pub fn save_settings(s: &Settings) {
    write_json(&settings_path(), s);
}

pub fn load_apps() -> Vec<AppEntry> {
    let loaded: Option<Vec<AppEntry>> = read_json(&apps_path());
    match loaded {
        // accept only a non-empty array, like v1
        Some(a) if !a.is_empty() => a,
        _ => {
            let a = default_apps();
            write_json(&apps_path(), &a);
            a
        }
    }
}

pub fn save_apps(apps: &[AppEntry]) {
    write_json(&apps_path(), &apps);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_and_partial_parse() {
        // partial file -> missing keys fall back to defaults (shallow merge)
        let s: Settings = serde_json::from_str(r##"{"accent":"#123456","shortcuts":{"apps":"ctrl+space"}}"##).unwrap();
        assert_eq!(s.accent, "#123456");
        assert_eq!(s.shortcuts.apps, "ctrl+space");
        assert_eq!(s.shortcuts.windows, "super+w"); // nested default preserved
        assert_eq!(s.ring_radius, 150.0);

        // camelCase on disk
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(text.contains("\"ringRadius\""));
        assert!(text.contains("\"followOutside\""));
    }

    #[test]
    fn app_entry_matches_v1_schema() {
        let json = r#"{"name":"Firefox","icon":"firefox","exec":["firefox"],"wmClass":"firefox",
                       "customActions":[{"label":"Reload","shortcut":"ctrl+r","icon":"","color":""}],
                       "actionIds":["w:close"]}"#;
        let a: AppEntry = serde_json::from_str(json).unwrap();
        assert_eq!(a.wm_class, "firefox");
        assert_eq!(a.custom_actions.len(), 1);
        assert_eq!(a.action_ids.as_deref(), Some(&["w:close".to_string()][..]));
    }
}
