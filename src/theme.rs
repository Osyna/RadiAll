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
        }
    };
}

// Built-in fallback: the original palette from Skin.qml. Field order and
// names track the theme JSON keys 1:1.
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
    /// `settings_bg` / `settings_accent` are the live Look-tab picker values:
    /// they win over the defaults but LOSE to an explicit theme key —
    /// same precedence as the old Skin.qml.
    pub fn load(name: &str, settings_bg: Option<&str>, settings_accent: Option<&str>) -> Self {
        let mut skin = Self::default();
        if let Some(c) = settings_bg.and_then(Rgba::parse) {
            skin.bg = c;
        }
        if let Some(c) = settings_accent.and_then(Rgba::parse) {
            skin.accent = c;
        }
        let path = themes_dir().join(format!("{name}.json"));
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(json) => skin.apply(&json),
                Err(e) => log::warn!("Theme: bad {name} — {e}"),
            },
            Err(_) => {} // unknown theme -> built-in defaults
        }
        skin
    }

    /// Scale helper: Skin.s(px) from QML.
    pub fn s(&self, px: f32) -> f32 {
        (px * self.scale).round()
    }

    /// Adaptive tint: a wash of the text color at alpha `a` (0..1).
    pub fn tint(&self, a: f32) -> Rgba {
        self.fg.with_alpha((a * 255.0) as u8)
    }
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
        let s = Skin::load("__nonexistent__", None, Some("#ff0000"));
        assert_eq!(s.accent, Rgba::rgb(255, 0, 0));
        // untouched keys keep defaults
        assert_eq!(s.scale, 1.1);
    }
}
