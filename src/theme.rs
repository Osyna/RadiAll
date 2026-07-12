//! The launcher's skin — every visual value the ring/settings read lives
//! here, overridable by a JSON theme file in the user's themes dir.
//! A theme only needs the keys it wants to change; anything omitted falls
//! back to the built-in defaults (which ARE the "default" theme).
//!
//! Color strings use the QML formats the old themes shipped with:
//! "#RGB", "#RRGGBB", "#AARRGGBB" (alpha FIRST, QML-style).

use serde_json::Value;
use std::path::PathBuf;

/// RGBA color, straightforward to convert to slint::Color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse QML color strings: #RGB, #RRGGBB, #AARRGGBB.
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        let v = u32::from_str_radix(hex, 16).ok()?;
        match hex.len() {
            3 => {
                let (r, g, b) = ((v >> 8 & 0xf) as u8, (v >> 4 & 0xf) as u8, (v & 0xf) as u8);
                Some(Self::rgb(r * 17, g * 17, b * 17))
            }
            6 => Some(Self::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
            8 => Some(Self::rgba(
                (v >> 16) as u8,
                (v >> 8) as u8,
                v as u8,
                (v >> 24) as u8,
            )),
            _ => None,
        }
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// QML-style hex: "#rrggbb", or "#aarrggbb" when translucent.
    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.a, self.r, self.g, self.b)
        }
    }
}

macro_rules! skin {
    ($( $field:ident : $ty:ty = $default:expr => $key:literal ),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct Skin {
            $(pub $field: $ty),+
        }

        impl Default for Skin {
            fn default() -> Self {
                Self { $($field: $default),+ }
            }
        }

        impl Skin {
            fn apply(&mut self, json: &Value) {
                $(FromTheme::apply(&mut self.$field, &json[$key]);)+
            }

            /// Serialize as a theme JSON (used by "Save as theme…").
            /// Auto (None) keys are omitted so they keep deriving.
            pub fn to_theme_json(&self) -> Value {
                let mut map = serde_json::Map::new();
                $(ToTheme::put(&self.$field, $key, &mut map);)+
                Value::Object(map)
            }
        }
    };
}

// Built-in fallback: the original palette from Skin.qml plus the extended
// wheel/arc surfaces. Field order and names track the theme JSON keys 1:1.
// `Option<Rgba>` keys are AUTO when absent: derived from bg/accent/fg at
// use-time (see ui.rs adaptive resolution); a theme pins them explicitly.
skin! {
    scale:        f32 = 1.1                            => "scale",
    bg:           Rgba = Rgba::rgb(0x0b, 0x0b, 0x0d)   => "bg",
    glass_bg:     Rgba = Rgba::rgba(22, 22, 24, 133)   => "glassBg",
    glass_hover:  Rgba = Rgba::rgba(255, 255, 255, 23) => "glassHover",
    pill_bg:      Rgba = Rgba::rgba(255, 255, 255, 38) => "pillBg",
    pill_hover:   Rgba = Rgba::rgba(255, 255, 255, 64) => "pillHover",
    btn_hover:    Rgba = Rgba::rgba(255, 255, 255, 23) => "btnHover",
    btn_active:   Rgba = Rgba::rgba(255, 255, 255, 36) => "btnActive",
    fg:           Rgba = Rgba::rgba(255, 255, 255, 224) => "fg",
    fg_strong:    Rgba = Rgba::rgba(255, 255, 255, 242) => "fgStrong",
    fg_dim:       Rgba = Rgba::rgba(255, 255, 255, 115) => "fgDim",
    accent:       Rgba = Rgba::rgb(0x0a, 0x84, 0xff)   => "accent",
    yellow:       Rgba = Rgba::rgb(0xff, 0xd6, 0x0a)   => "yellow",
    red:          Rgba = Rgba::rgb(0xff, 0x45, 0x3a)   => "red",
    green:        Rgba = Rgba::rgb(0x30, 0xd1, 0x58)   => "green",
    sep:          Rgba = Rgba::rgba(255, 255, 255, 31) => "sep",
    edge:         Rgba = Rgba::rgba(0, 0, 0, 0)        => "edge",
    edge_width:   f32 = 3.0                            => "edgeWidth",
    panel_bg:     Rgba = Rgba::rgba(24, 24, 26, 252)   => "panelBg",
    label_pill_bg: Rgba = Rgba::rgba(26, 32, 46, 245)  => "labelPillBg",
    // ---- extended surfaces (all previously hard-coded) ----
    backdrop:     Rgba = Rgba::rgb(0, 0, 0)            => "backdrop",
    arc_bg:       Rgba = Rgba::rgba(20, 20, 24, 247)   => "arcBg",
    arc_stroke:   Rgba = Rgba::rgba(255, 255, 255, 20) => "arcStroke",
    settings_btn: Rgba = Rgba::rgb(255, 255, 255)      => "settingsBtn",
    // ---- auto keys (None = derived; explicit value pins them) ----
    sector:       Option<Rgba> = None => "sector",       // wedge; auto = accent 4% toward white
    on_band:      Option<Rgba> = None => "onBand",       // glyph/monogram ink; auto by band luminance
    dot:          Option<Rgba> = None => "dot",          // window dots; auto by band luminance
    label_fg:     Option<Rgba> = None => "labelFg",      // pill text; auto = fgStrong
    arc_btn:      Option<Rgba> = None => "arcBtn",       // idle arc button; auto = fg @ 10%
    arc_btn_hover: Option<Rgba> = None => "arcBtnHover", // auto = accent
    seg_bg:       Option<Rgba> = None => "segBg",        // inactive sections; auto = settings picker or invisible
    font:         String = "SF Pro Text".into()        => "font",
    font_display: String = "SF Pro Display".into()     => "fontDisplay",
    icon_font:    String = "JetBrainsMono Nerd Font".into() => "iconFont",
    mono_font:    String = "JetBrainsMono Nerd Font".into() => "monoFont",
}

trait FromTheme {
    fn apply(slot: &mut Self, v: &Value);
}

impl FromTheme for f32 {
    fn apply(slot: &mut Self, v: &Value) {
        if let Some(n) = v.as_f64() {
            *slot = n as f32;
        }
    }
}

impl FromTheme for String {
    fn apply(slot: &mut Self, v: &Value) {
        if let Some(s) = v.as_str() {
            *slot = s.to_owned();
        }
    }
}

impl FromTheme for Rgba {
    fn apply(slot: &mut Self, v: &Value) {
        if let Some(c) = v.as_str().and_then(Rgba::parse) {
            *slot = c;
        }
    }
}

impl FromTheme for Option<Rgba> {
    fn apply(slot: &mut Self, v: &Value) {
        // Absent key (serde_json indexing yields Null): keep inherited value.
        // Empty string resets an inherited override back to auto.
        if let Some(s) = v.as_str() {
            if s.is_empty() {
                *slot = None;
            } else if let Some(c) = Rgba::parse(s) {
                *slot = Some(c);
            }
        }
    }
}

trait ToTheme {
    fn put(&self, key: &str, map: &mut serde_json::Map<String, Value>);
}

impl ToTheme for f32 {
    fn put(&self, key: &str, map: &mut serde_json::Map<String, Value>) {
        // f32 -> f64 directly would leak representation noise (1.100000023…)
        let rounded = (*self as f64 * 10_000.0).round() / 10_000.0;
        map.insert(key.into(), serde_json::json!(rounded));
    }
}

impl ToTheme for String {
    fn put(&self, key: &str, map: &mut serde_json::Map<String, Value>) {
        map.insert(key.into(), Value::String(self.clone()));
    }
}

impl ToTheme for Rgba {
    fn put(&self, key: &str, map: &mut serde_json::Map<String, Value>) {
        map.insert(key.into(), Value::String(self.to_hex()));
    }
}

impl ToTheme for Option<Rgba> {
    fn put(&self, key: &str, map: &mut serde_json::Map<String, Value>) {
        if let Some(c) = self {
            map.insert(key.into(), Value::String(c.to_hex()));
        }
    }
}

/// Themes shipped/installed for the user: ~/.config/radiall/themes/<name>.json
pub fn themes_dir() -> PathBuf {
    crate::config::config_dir().join("themes")
}

/// Names of every available theme file (sorted, without extension).
pub fn available() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(themes_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension()? == "json").then(|| p.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    if !names.iter().any(|n| n == "default") {
        names.push("default".into());
    }
    names.sort();
    names
}

impl Skin {
    /// Load theme `name`, overlaying its keys onto the built-in defaults.
    /// `settings_bg` / `settings_accent` / `settings_seg_bg` are the live
    /// Look-tab picker values: they win over the defaults but LOSE to an
    /// explicit theme key — same precedence as the old Skin.qml.
    ///
    /// A theme may set `"extends": "<parent>"`: the parent's keys apply
    /// first (recursively, depth-capped), then the child's overrides.
    pub fn load(
        name: &str,
        settings_bg: Option<&str>,
        settings_accent: Option<&str>,
        settings_seg_bg: Option<&str>,
    ) -> Self {
        let mut skin = Self::default();
        if let Some(c) = settings_bg.and_then(Rgba::parse) {
            skin.bg = c;
        }
        if let Some(c) = settings_accent.and_then(Rgba::parse) {
            skin.accent = c;
        }
        if let Some(c) = settings_seg_bg.and_then(Rgba::parse) {
            skin.seg_bg = Some(c);
        }
        skin.apply_file(name, 0);
        skin
    }

    fn apply_file(&mut self, name: &str, depth: u8) {
        if depth > 8 {
            log::warn!("Theme: extends chain too deep / cyclic at {name}");
            return;
        }
        let path = themes_dir().join(format!("{name}.json"));
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(json) => {
                    if let Some(parent) = json["extends"].as_str() {
                        self.apply_file(parent, depth + 1);
                    }
                    self.apply(&json);
                }
                Err(e) => log::warn!("Theme: bad {name} — {e}"),
            },
            Err(_) => {} // unknown theme -> built-in defaults
        }
    }

    /// Scale helper: Skin.s(px) from QML.
    pub fn s(&self, px: f32) -> f32 {
        (px * self.scale).round()
    }
}

/// File-safe theme name: lowercase, alphanumerics, dashes.
fn sanitize_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

/// Write `skin` to the user themes dir as `<name>.json` ("Save as theme…").
/// Returns the sanitized theme name actually used.
pub fn save_theme(name: &str, skin: &Skin) -> Result<String, String> {
    let clean = sanitize_name(name);
    if clean.is_empty() {
        return Err("theme name is empty".into());
    }
    let dir = themes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(&skin.to_theme_json()).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{clean}.json")), text).map_err(|e| e.to_string())?;
    Ok(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qml_color_formats() {
        assert_eq!(Rgba::parse("#0b0b0d"), Some(Rgba::rgb(0x0b, 0x0b, 0x0d)));
        // QML 8-digit = AARRGGBB, alpha first
        assert_eq!(
            Rgba::parse("#85161618"),
            Some(Rgba::rgba(0x16, 0x16, 0x18, 0x85))
        );
        assert_eq!(Rgba::parse("#fff"), Some(Rgba::rgb(255, 255, 255)));
        assert_eq!(Rgba::parse("bogus"), None);
    }

    #[test]
    fn theme_overlay_beats_settings_beats_default() {
        // no theme file, settings accent wins over default
        let s = Skin::load("__nonexistent__", None, Some("#ff0000"), None);
        assert_eq!(s.accent, Rgba::rgb(255, 0, 0));
        // untouched keys keep defaults
        assert_eq!(s.scale, 1.1);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgba::rgb(0xe4, 0x48, 0x54).to_hex(), "#e44854");
        assert_eq!(Rgba::rgba(20, 20, 24, 247).to_hex(), "#f7141418");
        let c = Rgba::parse("#f7141418").unwrap();
        assert_eq!(Rgba::parse(&c.to_hex()), Some(c));
    }

    #[test]
    fn auto_keys_apply_pin_and_reset() {
        let mut s = Skin::default();
        assert_eq!(s.sector, None);
        s.apply(&serde_json::json!({ "sector": "#123456" }));
        assert_eq!(s.sector, Some(Rgba::rgb(0x12, 0x34, 0x56)));
        // absent key keeps the inherited pin
        s.apply(&serde_json::json!({ "accent": "#ffffff" }));
        assert_eq!(s.sector, Some(Rgba::rgb(0x12, 0x34, 0x56)));
        // empty string resets to auto
        s.apply(&serde_json::json!({ "sector": "" }));
        assert_eq!(s.sector, None);
    }

    #[test]
    fn theme_json_writer_omits_autos() {
        let mut s = Skin::default();
        s.dot = Some(Rgba::rgb(1, 2, 3));
        let json = s.to_theme_json();
        assert_eq!(json["dot"], "#010203");
        assert!(json.get("sector").is_none()); // auto stays absent
        assert_eq!(json["bg"], "#0b0b0d");
        assert_eq!(json["scale"], 1.1);
        // writer output parses back to the same skin
        let mut back = Skin::default();
        back.apply(&json);
        assert_eq!(back, s);
    }

    #[test]
    fn extends_applies_parent_first() {
        // build a tiny theme chain in a temp themes dir via env override is
        // not available; exercise the merge order directly instead.
        let mut s = Skin::default();
        let parent = serde_json::json!({ "bg": "#111111", "accent": "#222222" });
        let child = serde_json::json!({ "accent": "#333333" });
        s.apply(&parent);
        s.apply(&child);
        assert_eq!(s.bg, Rgba::rgb(0x11, 0x11, 0x11)); // from parent
        assert_eq!(s.accent, Rgba::rgb(0x33, 0x33, 0x33)); // child override
    }

    #[test]
    fn theme_names_sanitize() {
        assert_eq!(sanitize_name("  My Cool Theme! "), "my-cool-theme");
        assert_eq!(sanitize_name("nord"), "nord");
        assert_eq!(sanitize_name("---"), "");
        assert_eq!(sanitize_name("émoji ok"), "émoji-ok");
    }
}
