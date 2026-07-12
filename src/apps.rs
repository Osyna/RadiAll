//! Desktop-entry index: scanning, Exec parsing, icon resolution, launching.
//!
//! Ports the Quickshell `DesktopEntries` surface used by services/Launcher.qml
//! (spec-launcher.md §2, §4, §5): installed-apps picker source, `findEntry`,
//! `iconForClass`, `iconSource`, and `Quickshell.execDetached`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use freedesktop_desktop_entry as fde;
use parking_lot::Mutex;
use slint::{Rgba8Pixel, SharedPixelBuffer};

use crate::config::AppEntry;

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
    /// Scan all XDG data dirs for desktop entries (precedence order), skipping
    /// NoDisplay/Hidden entries, deduping by desktop id (first wins).
    pub fn scan() -> Self {
        let locales = fde::get_languages_from_env();
        let mut seen: HashSet<String> = HashSet::new();
        let mut installed: Vec<DesktopApp> = Vec::new();

        for path in fde::Iter::new(fde::default_paths()) {
            let entry = match fde::DesktopEntry::from_path(&path, Some(&locales)) {
                Ok(e) => e,
                Err(_) => continue, // not a .desktop file or unreadable
            };
            let id = entry.id().to_owned();
            // First occurrence wins in XDG precedence order — even a hidden or
            // NoDisplay entry shadows lower-priority files with the same id.
            if !seen.insert(id.clone()) {
                continue;
            }
            if entry.no_display() || entry.hidden() {
                continue;
            }
            if entry.type_().is_some_and(|t| t != "Application") {
                continue;
            }

            let exec = match entry.exec() {
                Some(raw) => {
                    let argv = tokenize_exec(raw);
                    if argv.is_empty() {
                        vec![id.clone()]
                    } else {
                        argv
                    }
                }
                None => vec![id.clone()],
            };

            let mut actions = Vec::new();
            if let Some(names) = entry.actions() {
                for action in names {
                    if action.is_empty() {
                        continue;
                    }
                    let Some(raw) = entry.action_exec(action) else {
                        continue;
                    };
                    let command = tokenize_exec(raw);
                    if command.is_empty() {
                        continue;
                    }
                    let name = entry
                        .action_name(action, &locales)
                        .map(|n| n.into_owned())
                        .unwrap_or_else(|| action.to_owned());
                    actions.push(DesktopAction { name, command });
                }
            }

            installed.push(DesktopApp {
                name: entry
                    .name(&locales)
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|| id.clone()),
                icon: entry.icon().unwrap_or_default().to_owned(),
                exec,
                startup_wm_class: entry.startup_wm_class().unwrap_or_default().to_owned(),
                actions,
                id,
            });
        }

        installed.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        Self { installed }
    }

    /// Visible apps sorted case-insensitively by name (picker source).
    pub fn installed(&self) -> &[DesktopApp] {
        &self.installed
    }

    /// findEntry port: match StartupWMClass, then desktop id, then exec basename.
    /// Phase 1 (skipped when `wm_class` is empty): first entry whose
    /// StartupWMClass or id equals `wm_class` case-insensitively.
    /// Phase 2 (skipped when `exec0` is empty): first entry whose exec[0]
    /// basename equals `exec0`'s basename case-insensitively.
    pub fn find_for(&self, wm_class: &str, exec0: &str) -> Option<&DesktopApp> {
        let want = wm_class.to_lowercase();
        if !want.is_empty() {
            if let Some(e) = self.installed.iter().find(|e| {
                e.startup_wm_class.to_lowercase() == want || e.id.to_lowercase() == want
            }) {
                return Some(e);
            }
        }
        let want_exec = basename(exec0).to_lowercase();
        if !want_exec.is_empty() {
            if let Some(e) = self
                .installed
                .iter()
                .find(|e| {
                    e.exec
                        .first()
                        .is_some_and(|a| basename(a).to_lowercase() == want_exec)
                })
            {
                return Some(e);
            }
        }
        None
    }

    /// iconForClass port (spec §5): configured app -> entry icon -> heuristic
    /// -> cls itself. Returns an icon NAME (or abs path); "" for empty cls.
    pub fn icon_for_class(&self, cls: &str, configured: &[AppEntry]) -> String {
        let c = cls.to_lowercase();
        if c.is_empty() {
            return String::new();
        }
        // (a) configured app with matching wmClass -> its icon verbatim.
        if let Some(app) = configured.iter().find(|a| a.wm_class.to_lowercase() == c) {
            return app.icon.clone();
        }
        // (b) desktop entry via StartupWMClass / id match.
        if let Some(e) = self.installed.iter().find(|e| {
            e.startup_wm_class.to_lowercase() == c || e.id.to_lowercase() == c
        }) {
            return if e.icon.is_empty() {
                cls.to_owned()
            } else {
                e.icon.clone()
            };
        }
        // (c) Best-effort port of Quickshell's DesktopEntries.heuristicLookup:
        // fuzzy match for class != id apps (e.g. window "orca" vs entry
        // "stably-orca"). We accept an entry whose id contains the class (or
        // vice versa) or whose name equals the class, case-insensitively.
        if let Some(e) = self.installed.iter().find(|e| {
            let id = e.id.to_lowercase();
            id.contains(&c) || c.contains(&id) || e.name.to_lowercase() == c
        }) {
            if !e.icon.is_empty() {
                return e.icon.clone();
            }
        }
        // (d) Last guess: the class itself as an icon name.
        cls.to_owned()
    }
}

/// Basename of a path-ish string ("" stays "").
fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// Tokenize a Desktop Entry Exec= line per the freedesktop spec:
/// 1. general string unescape (\s \n \t \r \\),
/// 2. argument splitting honoring double quotes with backslash escapes
///    (\" \\ \$ \`) inside quoted sections,
/// 3. field codes %f %F %u %U %d %D %n %N %i %c %k %v %m dropped entirely
///    (standalone tokens removed, embedded occurrences stripped); %% -> "%".
fn tokenize_exec(exec: &str) -> Vec<String> {
    // Stage 1: general string-value unescape.
    let mut unescaped = String::with_capacity(exec.len());
    let mut chars = exec.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('s') => unescaped.push(' '),
                Some('n') => unescaped.push('\n'),
                Some('t') => unescaped.push('\t'),
                Some('r') => unescaped.push('\r'),
                Some('\\') => unescaped.push('\\'),
                Some(other) => {
                    // Unknown escape: keep both chars (lenient).
                    unescaped.push('\\');
                    unescaped.push(other);
                }
                None => unescaped.push('\\'),
            }
        } else {
            unescaped.push(ch);
        }
    }

    // Stage 2: whitespace splitting with double-quote sections.
    let mut raw_tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut have_token = false;
    let mut in_quotes = false;
    let mut chars = unescaped.chars();
    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' => in_quotes = false,
                '\\' => {
                    // Inside quotes: \" \\ \$ \` (lenient: escape any next char).
                    match chars.next() {
                        Some(esc) => cur.push(esc),
                        None => cur.push('\\'),
                    }
                }
                _ => cur.push(ch),
            }
        } else {
            match ch {
                '"' => {
                    in_quotes = true;
                    have_token = true;
                }
                c if c.is_whitespace() => {
                    if have_token {
                        raw_tokens.push(std::mem::take(&mut cur));
                        have_token = false;
                    }
                }
                _ => {
                    cur.push(ch);
                    have_token = true;
                }
            }
        }
    }
    if have_token {
        raw_tokens.push(cur);
    }

    // Stage 3: field-code stripping.
    let mut out = Vec::with_capacity(raw_tokens.len());
    for tok in raw_tokens {
        let had_percent = tok.contains('%');
        let mut clean = String::with_capacity(tok.len());
        let mut chars = tok.chars();
        while let Some(ch) = chars.next() {
            if ch != '%' {
                clean.push(ch);
                continue;
            }
            match chars.next() {
                Some('%') => clean.push('%'),
                Some(
                    'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'i' | 'c' | 'k' | 'v' | 'm',
                ) => {} // dropped, never expanded
                Some(other) => {
                    // Unknown/deprecated code: keep verbatim (lenient).
                    clean.push('%');
                    clean.push(other);
                }
                None => clean.push('%'),
            }
        }
        // A token that consisted only of field codes vanishes entirely.
        if clean.is_empty() && had_percent {
            continue;
        }
        out.push(clean);
    }
    out
}

/// GTK default theme, resolved once (freedesktop-icons falls back to hicolor
/// and /usr/share/pixmaps internally).
static GTK_THEME: LazyLock<Option<String>> = LazyLock::new(freedesktop_icons::default_theme_gtk);

/// Sizes preference for theme lookups.
const ICON_SIZES: [u16; 5] = [64, 128, 48, 256, 32];

fn theme_lookup(name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let build = |name: &str, size: u16| {
        let b = freedesktop_icons::lookup(name).with_size(size).with_scale(1);
        match GTK_THEME.as_deref() {
            Some(theme) => b.with_theme(theme).with_cache().find(),
            None => b.with_cache().find(),
        }
    };
    for size in ICON_SIZES {
        if let Some(p) = build(name, size) {
            return Some(p);
        }
    }
    // "scalable" pass: prefer an SVG when no fixed size matched.
    if let Some(p) = {
        let b = freedesktop_icons::lookup(name)
            .with_size(64)
            .with_scale(1)
            .force_svg();
        match GTK_THEME.as_deref() {
            Some(theme) => b.with_theme(theme).with_cache().find(),
            None => b.with_cache().find(),
        }
    } {
        return Some(p);
    }
    // Common quirk: Icon= names carrying a file extension ("foo.png").
    if let Some(stem) = name
        .strip_suffix(".png")
        .or_else(|| name.strip_suffix(".svg"))
        .or_else(|| name.strip_suffix(".xpm"))
    {
        return theme_lookup(stem);
    }
    None
}

/// iconSource port: abs path passthrough (must exist), else icon-theme lookup,
/// else the application-x-executable fallback; None = caller draws a monogram.
pub fn icon_path(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    if icon.starts_with('/') {
        let p = Path::new(icon);
        if p.exists() {
            return Some(p.to_path_buf());
        }
        // Dead absolute path: fall through to the generic fallback.
    } else if let Some(p) = theme_lookup(icon) {
        return Some(p);
    }
    theme_lookup("application-x-executable")
}

/// Icon pixel cache: scans repeat and decoding (especially SVG) is costly.
static ICON_CACHE: LazyLock<Mutex<HashMap<(String, u32), Option<SharedPixelBuffer<Rgba8Pixel>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Decode an icon (PNG/JPEG or SVG) to RGBA pixels at ~`px` size. Send-safe.
/// Aspect ratio preserved: the long edge is at most `px`.
pub fn load_icon_pixels(icon: &str, px: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if icon.is_empty() || px == 0 {
        return None;
    }
    let key = (icon.to_owned(), px);
    if let Some(cached) = ICON_CACHE.lock().get(&key) {
        return cached.clone();
    }
    let decoded = icon_path(icon).and_then(|path| decode_at(&path, px));
    ICON_CACHE.lock().insert(key, decoded.clone());
    decoded
}

fn decode_at(path: &Path, px: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext == "svg" || ext == "svgz" {
        render_svg(path, px)
    } else {
        decode_raster(path, px)
    }
}

fn render_svg(path: &Path, px: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let data = std::fs::read(path)
        .map_err(|e| log::warn!("icon read failed {}: {e}", path.display()))
        .ok()?;
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(&data, &opt)
        .map_err(|e| log::warn!("svg parse failed {}: {e}", path.display()))
        .ok()?;
    let size = tree.size();
    let long = size.width().max(size.height());
    if long <= 0.0 {
        return None;
    }
    let scale = px as f32 / long;
    let w = ((size.width() * scale).round() as u32).max(1);
    let h = ((size.height() * scale).round() as u32).max(1);
    // Transparent background by construction: Pixmap::new zero-fills.
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    for (dst, src) in buf.make_mut_slice().iter_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = Rgba8Pixel::new(c.red(), c.green(), c.blue(), c.alpha());
    }
    Some(buf)
}

fn decode_raster(path: &Path, px: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = image::open(path)
        .map_err(|e| log::warn!("icon decode failed {}: {e}", path.display()))
        .ok()?;
    let mut rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let long = w.max(h);
    if long > px {
        let scale = px as f32 / long as f32;
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Nearest);
    }
    let (w, h) = rgba.dimensions();
    Some(SharedPixelBuffer::clone_from_slice(rgba.as_raw(), w, h))
}

/// Decode an in-memory PNG (e.g. the bundled logo) to RGBA at <= `px`.
pub fn load_icon_pixels_from_bytes(bytes: &[u8], px: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| log::warn!("bundled image decode failed: {e}"))
        .ok()?;
    let mut rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    let long = w.max(h);
    if long > px && px > 0 {
        let scale = px as f32 / long as f32;
        let nw = ((w as f32 * scale).round() as u32).max(1);
        let nh = ((h as f32 * scale).round() as u32).max(1);
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    let (w, h) = rgba.dimensions();
    Some(SharedPixelBuffer::clone_from_slice(rgba.as_raw(), w, h))
}

/// Detached spawn of an argv vector: no shell, survives daemon exit.
/// Port of `Quickshell.execDetached` (spec §4): stdio to null, own process
/// group so the child outlives us and never receives our signals.
pub fn launch(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else {
        log::warn!("launch: empty argv");
        return;
    };
    if program.is_empty() {
        log::warn!("launch: empty program name");
        return;
    }
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let result = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    match result {
        Ok(child) => log::info!("launched {program} (pid {})", child.id()),
        Err(e) => log::error!("launch {program} failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, icon: &str, exec: &[&str], wm: &str) -> DesktopApp {
        DesktopApp {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
            exec: exec.iter().map(|s| s.to_string()).collect(),
            startup_wm_class: wm.into(),
            actions: Vec::new(),
        }
    }

    fn cfg(name: &str, icon: &str, wm: &str) -> AppEntry {
        AppEntry {
            name: name.into(),
            icon: icon.into(),
            wm_class: wm.into(),
            ..Default::default()
        }
    }

    // ---- tokenize_exec -------------------------------------------------

    #[test]
    fn tokenize_plain() {
        assert_eq!(tokenize_exec("firefox --new-window"), vec!["firefox", "--new-window"]);
        assert_eq!(tokenize_exec("   spaced   out  "), vec!["spaced", "out"]);
        assert!(tokenize_exec("").is_empty());
    }

    #[test]
    fn tokenize_quotes() {
        assert_eq!(
            tokenize_exec(r#""/opt/my app/run" --flag"#),
            vec!["/opt/my app/run", "--flag"]
        );
        // Adjacent quoted + bare parts form one token.
        assert_eq!(tokenize_exec(r#"pre"mid"post"#), vec!["premidpost"]);
        // Empty quoted argument survives (it is a real token).
        assert_eq!(tokenize_exec(r#"cmd """#), vec!["cmd", ""]);
    }

    #[test]
    fn tokenize_two_stage_escapes() {
        // File value: sh -c "echo \\"hi\\""  → stage 1 turns \\ into \,
        // stage 2 sees \" inside quotes → literal quote.
        assert_eq!(
            tokenize_exec(r#"sh -c "echo \\"hi\\"""#),
            vec!["sh", "-c", r#"echo "hi""#]
        );
        // \\$ inside quotes → literal $.
        assert_eq!(tokenize_exec(r#"sh -c "print \\$HOME""#), vec!["sh", "-c", "print $HOME"]);
        // General escape \s → space; unescape precedes splitting, so an
        // unquoted \s separates tokens. Quoting keeps the space literal.
        assert_eq!(tokenize_exec(r"a\sb"), vec!["a", "b"]);
        assert_eq!(tokenize_exec("\"a\\sb\""), vec!["a b"]);
    }

    #[test]
    fn tokenize_field_codes() {
        // Standalone codes vanish entirely.
        assert_eq!(tokenize_exec("vlc %U"), vec!["vlc"]);
        assert_eq!(tokenize_exec("app %F %i %c %k"), vec!["app"]);
        // Embedded codes are stripped in place.
        assert_eq!(tokenize_exec("foo --file=%f --x"), vec!["foo", "--file=", "--x"]);
        // %% is a literal percent.
        assert_eq!(tokenize_exec("printf %%s"), vec!["printf", "%s"]);
        // Mixed: %ivlc%c mid-token strips both codes.
        assert_eq!(tokenize_exec("run %ivlc%c"), vec!["run", "vlc"]);
        // All field codes named by the spec.
        assert_eq!(
            tokenize_exec("x %f %F %u %U %d %D %n %N %i %c %k %v %m"),
            vec!["x"]
        );
    }

    // ---- find_for ------------------------------------------------------

    #[test]
    fn find_for_precedence() {
        let idx = AppIndex {
            installed: vec![
                app("org.gnome.Maps", "Maps", "maps", &["gnome-maps"], ""),
                app("firefox", "Firefox", "firefox", &["/usr/lib/firefox/firefox"], "Firefox"),
                app("code", "VS Code", "vscode", &["/usr/bin/code", "--ozone"], "Code"),
            ],
        };
        // StartupWMClass match, case-insensitive.
        assert_eq!(idx.find_for("FIREFOX", "").unwrap().id, "firefox");
        // Desktop-id match.
        assert_eq!(idx.find_for("org.gnome.maps", "").unwrap().id, "org.gnome.Maps");
        // wm_class phase wins over a would-be exec match on an earlier entry.
        assert_eq!(idx.find_for("code", "gnome-maps").unwrap().id, "code");
        // Exec-basename phase (basenames compared).
        assert_eq!(idx.find_for("nope", "/opt/other/FIREFOX").unwrap().id, "firefox");
        // Empty inputs skip their phase.
        assert_eq!(idx.find_for("", "code").unwrap().id, "code");
        assert!(idx.find_for("", "").is_none());
        assert!(idx.find_for("ghost", "ghost").is_none());
    }

    // ---- icon_for_class -------------------------------------------------

    #[test]
    fn icon_for_class_chain() {
        let idx = AppIndex {
            installed: vec![
                app("stably-orca", "Orca Browser", "orca-icon", &["orca"], ""),
                app("kitty", "kitty", "kitty-icon", &["kitty"], "Kitty"),
                app("noicon", "Bare", "", &["bare"], "bareclass"),
            ],
        };
        let configured = vec![cfg("Term", "term-icon", "Kitty")];
        // (a) configured app wins over the desktop entry.
        assert_eq!(idx.icon_for_class("kitty", &configured), "term-icon");
        // (b) entry via StartupWMClass, icon non-empty.
        assert_eq!(idx.icon_for_class("kitty", &[]), "kitty-icon");
        // (b) entry matched but icon empty → cls itself.
        assert_eq!(idx.icon_for_class("bareclass", &[]), "bareclass");
        // (c) heuristic: class "orca" ⊂ id "stably-orca".
        assert_eq!(idx.icon_for_class("orca", &[]), "orca-icon");
        // (d) nothing matches → cls verbatim (case preserved).
        assert_eq!(idx.icon_for_class("Mystery", &[]), "Mystery");
        // Empty class → "".
        assert_eq!(idx.icon_for_class("", &[]), "");
    }

    // ---- live scan (guarded) ---------------------------------------------

    #[test]
    fn live_scan_and_icons() {
        let idx = AppIndex::scan();
        if idx.installed().is_empty() {
            eprintln!("live_scan_and_icons: no desktop entries on this machine, skipping");
            return;
        }
        // Sorted case-insensitively.
        for pair in idx.installed().windows(2) {
            assert!(pair[0].name.to_lowercase() <= pair[1].name.to_lowercase());
        }
        // Every entry has a non-empty argv.
        for e in idx.installed() {
            assert!(!e.exec.is_empty(), "entry {} has empty argv", e.id);
        }
        eprintln!("installed() count: {}", idx.installed().len());
        let mut shown = 0;
        for e in idx.installed() {
            if e.icon.is_empty() {
                continue;
            }
            if let Some(p) = icon_path(&e.icon) {
                eprintln!("  {} [{}] icon {} -> {}", e.name, e.id, e.icon, p.display());
                let pix = load_icon_pixels(&e.icon, 64);
                if let Some(buf) = &pix {
                    assert!(buf.width().max(buf.height()) <= 64 + 1);
                }
                shown += 1;
                if shown == 3 {
                    break;
                }
            }
        }
        assert!(shown > 0, "no icon resolved for any installed entry");
    }
}
