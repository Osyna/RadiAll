//! Desktop-entry index: scanning, Exec parsing, icon resolution, launching.
//! STUB — implementation pending.

use crate::config::AppEntry;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct DesktopAction {
    /// Display label (the Action group's Name=).
    pub name: String,
    /// Tokenized Exec argv, field codes stripped.
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DesktopApp {
    /// Desktop-file id (basename without .desktop).
    pub id: String,
    pub name: String,
    pub icon: String,
    /// Tokenized Exec argv, field codes stripped; falls back to [id].
    pub exec: Vec<String>,
    /// StartupWMClass= or "".
    pub startup_wm_class: String,
    pub actions: Vec<DesktopAction>,
}

#[derive(Debug, Default)]
pub struct AppIndex {
    installed: Vec<DesktopApp>,
}

impl AppIndex {
    pub fn scan() -> Self {
        Self::default()
    }
    /// Visible apps sorted case-insensitively by name (picker source).
    pub fn installed(&self) -> &[DesktopApp] {
        &self.installed
    }
    /// findEntry port: match StartupWMClass, then desktop id, then exec basename.
    pub fn find_for(&self, _wm_class: &str, _exec0: &str) -> Option<&DesktopApp> {
        None
    }
    /// iconForClass port: configured app -> entry icon -> heuristic -> cls itself.
    pub fn icon_for_class(&self, cls: &str, _configured: &[AppEntry]) -> String {
        cls.to_owned()
    }
}

/// iconSource port: abs path passthrough, else icon-theme lookup,
/// else the application-x-executable fallback; None = caller draws a monogram.
pub fn icon_path(_icon: &str) -> Option<PathBuf> {
    None
}

/// Decode an icon (PNG or SVG) to RGBA pixels at ~`px` size. Send-safe.
pub fn load_icon_pixels(
    _icon: &str,
    _px: u32,
) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    None
}

/// Detached spawn of an argv vector: no shell, survives daemon exit.
pub fn launch(_argv: &[String]) {}
