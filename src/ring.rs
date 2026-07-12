//! Ring domain model: what's on the wheel per mode, window matching,
//! the actions system, and activation dispatch. Direct port of the
//! Launcher.qml singleton's logic (see spec §3/§4/§6).

use crate::apps::{self, AppIndex};
use crate::compositor::{Compositor, WindowId, WindowInfo};
use crate::config::{AppEntry, CustomAction, Settings};
use crate::shortcuts;
use std::collections::HashMap;

// Nerd-font PUA glyphs, exact v1 codepoints.
pub const G_APP: &str = "\u{f2d0}"; // generic app window
pub const G_KEY: &str = "\u{f084}"; // custom key-combo action
pub const G_PLUS: &str = "\u{f067}"; // desktop action
pub const G_CLOSE: &str = "\u{f00d}";
pub const G_FLOAT: &str = "\u{f2d2}";
pub const G_FULL: &str = "\u{f065}";
pub const G_OPEN: &str = "\u{f35d}";

const TITLE_MAX: usize = 160; // safety cap only — display overflow marquees

/// Window titles usually repeat the app: "Page — Mozilla Firefox",
/// "docs - Thunar". Ring slices already carry the app's icon (and app-mode
/// labels lead with the app name), so strip trailing
/// "<sep> <something containing the app name>" segments. Display length is
/// the wheel's problem (marquee) — only a safety cap here.
pub fn clean_title(t: &str, app: &str) -> String {
    let mut s = t.trim();
    let app_lc = app.to_lowercase();
    for _ in 0..2 {
        let cut = [" — ", " – ", " - ", " | "]
            .iter()
            .filter_map(|sep| s.rfind(sep).map(|i| (i, sep.len())))
            .max_by_key(|&(i, _)| i);
        match cut {
            Some((i, sep_len))
                if !app_lc.is_empty()
                    && s[i + sep_len..].to_lowercase().contains(&app_lc)
                    && !s[..i].trim().is_empty() =>
            {
                s = s[..i].trim_end();
            }
            _ => break,
        }
    }
    if s.chars().count() > TITLE_MAX {
        let mut out: String = s.chars().take(TITLE_MAX).collect();
        out.push('…');
        out
    } else {
        s.to_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Apps,
    Windows,
    Actions,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    /// Desktop-file action: run its argv.
    Run(Vec<String>),
    Close,
    Float,
    Fullscreen,
    /// Send a key combo to the window (Hyprland), else launch.
    Keys(String),
    /// Fallback "Open" when no actions apply.
    Launch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActionTemplate {
    pub id: String,
    pub label: String,
    pub glyph: String,
    pub icon: String,
    pub color: String,
    pub kind: ActionKind,
}

/// One slice on the wheel.
#[derive(Debug, Clone, Default)]
pub struct RingEntry {
    pub name: String,
    /// Unresolved icon (theme name or abs path); "" = glyph/monogram.
    pub icon: String,
    pub glyph: String,
    pub color: String,
    pub wm_class: String,
    /// Windows-ring entries and the focused app carry a concrete window.
    pub window: Option<WindowId>,
    pub is_window: bool,
    pub action: Option<ActionTemplate>,
    /// Apps-ring entries: index into the configured apps array.
    pub app_index: Option<usize>,
}

impl RingEntry {
    pub fn is_action(&self) -> bool {
        self.action.is_some()
    }
}

/// The focused window merged with its configured app (spec §3 focusedApp).
#[derive(Debug, Clone, Default)]
pub struct FocusedApp {
    pub icon: String,
    pub wm_class: String,
    pub window: Option<WindowId>,
    pub custom_actions: Vec<CustomAction>,
    pub action_ids: Option<Vec<String>>,
}

/// Everything the rings need to answer questions. Owned by the UI thread;
/// compositor snapshots are pushed in from the adapter's watch thread.
pub struct Core {
    pub settings: Settings,
    pub apps: Vec<AppEntry>,
    pub index: AppIndex,
    /// Glyph SVG library for action icons. None until the settings editor
    /// first needs it (lazy: 24k-file scan + ~4 MB index, picker-only).
    pub icons: Option<crate::icons::IconLib>,
    pub comp: Box<dyn Compositor>,
    pub windows: Vec<WindowInfo>,
    pub active: Option<WindowInfo>,
    /// Transient: lowercase wmClass -> chosen window index. Not persisted.
    pub win_sel: HashMap<String, usize>,
    /// True when the compositor was configured (window rules) so the ring
    /// window is a real overlay; false -> the UI falls back to fullscreen.
    pub overlay_ready: bool,
    /// The focused app SNAPSHOTTED when the actions ring opens: the overlay
    /// itself takes keyboard focus right after, so building the ring from
    /// the live active window would immediately wipe it. Frozen per open.
    pub actions_target: Option<FocusedApp>,
    /// Global-shortcut provider handle (hyprctl / portal / X11 grabs).
    /// None until the daemon wires it in main.rs — and in unit tests.
    pub shortcuts: Option<crate::shortcuts::Shortcuts>,
}

impl Core {
    pub fn new(settings: Settings, apps: Vec<AppEntry>, index: AppIndex, comp: Box<dyn Compositor>) -> Self {
        Self {
            settings,
            apps,
            index,
            icons: None,
            comp,
            windows: Vec::new(),
            active: None,
            win_sel: HashMap::new(),
            overlay_ready: false,
            actions_target: None,
            shortcuts: None,
        }
    }

    // ------------------------------------------------- window matching

    pub fn windows_for_class(&self, wm_class: &str) -> Vec<&WindowInfo> {
        let want = wm_class.to_lowercase();
        if want.is_empty() {
            return Vec::new();
        }
        self.windows
            .iter()
            .filter(|w| w.app_id.to_lowercase() == want)
            .collect()
    }

    pub fn windows_for(&self, e: &RingEntry) -> Vec<&WindowInfo> {
        if e.is_window {
            return match &e.window {
                Some(id) => self.windows.iter().filter(|w| &w.id == id).collect(),
                None => Vec::new(),
            };
        }
        self.windows_for_class(&e.wm_class)
    }

    pub fn selected_window_index(&self, e: &RingEntry) -> isize {
        let ws = self.windows_for(e);
        if ws.is_empty() {
            return -1;
        }
        let idx = *self
            .win_sel
            .get(&e.wm_class.to_lowercase())
            .unwrap_or(&0);
        if idx >= ws.len() {
            0
        } else {
            idx as isize
        }
    }

    pub fn window_for(&self, e: &RingEntry) -> Option<WindowId> {
        if e.is_window {
            return e.window.clone();
        }
        let idx = self.selected_window_index(e);
        if idx < 0 {
            return None;
        }
        self.windows_for(e).get(idx as usize).map(|w| w.id.clone())
    }

    /// Scroll on a multi-window slice: cycle the chosen window (wraps).
    pub fn cycle_window(&mut self, e: &RingEntry, dir: isize) {
        let n = self.windows_for(e).len() as isize;
        if n <= 1 {
            return;
        }
        let key = e.wm_class.to_lowercase();
        let cur = *self.win_sel.get(&key).unwrap_or(&0) as isize;
        let next = (cur + dir).rem_euclid(n) as usize;
        self.win_sel.insert(key, next);
    }

    // ---------------------------------------------------- ring models

    pub fn ring_model(&self, mode: Mode) -> Vec<RingEntry> {
        match mode {
            Mode::Apps => self
                .apps
                .iter()
                .enumerate()
                .map(|(i, a)| RingEntry {
                    name: a.name.clone(),
                    icon: a.icon.clone(),
                    glyph: G_APP.into(),
                    wm_class: a.wm_class.clone(),
                    color: a.color.clone(),
                    app_index: Some(i),
                    ..Default::default()
                })
                .collect(),
            Mode::Windows => {
                let mut ring: Vec<RingEntry> = self
                    .windows
                    .iter()
                    .map(|w| {
                        let cls = &w.app_id;
                        // slice label: the window title, minus the redundant
                        // "— App Name" tail (the icon already says which app)
                        let app_name = self
                            .apps
                            .iter()
                            .find(|a| a.wm_class.eq_ignore_ascii_case(cls))
                            .map(|a| a.name.as_str())
                            .unwrap_or(cls);
                        RingEntry {
                            name: if w.title.is_empty() {
                                cls.clone()
                            } else {
                                clean_title(&w.title, app_name)
                            },
                            icon: self.index.icon_for_class(cls, &self.apps),
                            glyph: G_APP.into(),
                            wm_class: cls.clone(),
                            window: Some(w.id.clone()),
                            is_window: true,
                            ..Default::default()
                        }
                    })
                    .collect();
                // group same-app windows adjacently
                ring.sort_by(|a, b| {
                    (a.wm_class.to_lowercase(), a.name.to_lowercase())
                        .cmp(&(b.wm_class.to_lowercase(), b.name.to_lowercase()))
                });
                ring
            }
            Mode::Actions => match self.actions_target.clone() {
                None => Vec::new(),
                Some(app) => self
                    .actions_for(&app)
                    .into_iter()
                    .map(|t| RingEntry {
                        name: t.label.clone(),
                        icon: t.icon.clone(),
                        glyph: t.glyph.clone(),
                        color: t.color.clone(),
                        wm_class: app.wm_class.clone(),
                        window: app.window.clone(),
                        action: Some(t),
                        ..Default::default()
                    })
                    .collect(),
            },
        }
    }

    /// Focused window merged with its configured app (icon/custom actions).
    pub fn focused_app(&self) -> Option<FocusedApp> {
        let win = self.active.as_ref()?;
        // A class-less window can't be matched to apps or action templates —
        // and our own overlay reports an empty app_id, so this also stops a
        // mid-close focus snapshot from targeting the launcher itself.
        if win.app_id.is_empty() {
            return None;
        }
        let cls = &win.app_id;
        let cfg = self
            .apps
            .iter()
            .find(|a| a.wm_class.to_lowercase() == cls.to_lowercase());
        Some(FocusedApp {
            icon: match cfg {
                Some(c) if !c.icon.is_empty() => c.icon.clone(),
                _ => self.index.icon_for_class(cls, &self.apps),
            },
            wm_class: cls.clone(),
            window: Some(win.id.clone()),
            custom_actions: cfg.map(|c| c.custom_actions.clone()).unwrap_or_default(),
            action_ids: cfg.and_then(|c| c.action_ids.clone()),
        })
    }

    /// First apps-ring index with >= 1 open window, else 0 (initial selection).
    pub fn first_running_index(&self) -> usize {
        self.apps
            .iter()
            .position(|a| !self.windows_for_class(&a.wm_class).is_empty())
            .unwrap_or(0)
    }

    // -------------------------------------------------------- actions

    /// Full template list for an app (spec §6 order, stable ids).
    pub fn action_templates(&self, app: &FocusedApp, exec0: &str) -> Vec<ActionTemplate> {
        let mut out = Vec::new();
        if let Some(entry) = self.index.find_for(&app.wm_class, exec0) {
            for a in &entry.actions {
                out.push(ActionTemplate {
                    id: format!("d:{}", a.name),
                    label: a.name.clone(),
                    glyph: G_PLUS.into(),
                    icon: String::new(),
                    color: String::new(),
                    kind: ActionKind::Run(a.command.clone()),
                });
            }
        }
        out.push(ActionTemplate {
            id: "w:close".into(),
            label: "Close".into(),
            glyph: G_CLOSE.into(),
            icon: String::new(),
            color: String::new(),
            kind: ActionKind::Close,
        });
        if self.comp.can_float() {
            out.push(ActionTemplate {
                id: "w:float".into(),
                label: "Float".into(),
                glyph: G_FLOAT.into(),
                icon: String::new(),
                color: String::new(),
                kind: ActionKind::Float,
            });
        }
        out.push(ActionTemplate {
            id: "w:full".into(),
            label: "Fullscreen".into(),
            glyph: G_FULL.into(),
            icon: String::new(),
            color: String::new(),
            kind: ActionKind::Fullscreen,
        });
        for (k, c) in app.custom_actions.iter().enumerate() {
            out.push(ActionTemplate {
                id: format!("c:{k}"),
                label: c.label.clone(),
                glyph: G_KEY.into(),
                icon: c.icon.clone(),
                color: c.color.clone(),
                kind: ActionKind::Keys(c.shortcut.clone()),
            });
        }
        out
    }

    /// Filtered actions (spec §6 actionsFor): actionIds whitelist for
    /// non-custom, window ops need a running window, empty -> [Open].
    pub fn actions_for(&self, app: &FocusedApp) -> Vec<ActionTemplate> {
        let has_window = !self.windows_for_class(&app.wm_class).is_empty() || app.window.is_some();
        let mut out: Vec<ActionTemplate> = self
            .action_templates(app, "")
            .into_iter()
            .filter(|t| {
                let custom = matches!(t.kind, ActionKind::Keys(_));
                if !custom {
                    if let Some(ids) = &app.action_ids {
                        if !ids.contains(&t.id) {
                            return false;
                        }
                    }
                }
                if matches!(t.kind, ActionKind::Close | ActionKind::Float | ActionKind::Fullscreen)
                    && !has_window
                {
                    return false;
                }
                true
            })
            .collect();
        if out.is_empty() {
            out.push(ActionTemplate {
                id: "launch".into(),
                label: "Open".into(),
                glyph: G_OPEN.into(),
                icon: String::new(),
                color: String::new(),
                kind: ActionKind::Launch,
            });
        }
        out
    }

    // ------------------------------------------------------- dispatch

    fn launch_entry(&self, e: &RingEntry) {
        if let Some(i) = e.app_index {
            if let Some(a) = self.apps.get(i) {
                apps::launch(&a.exec);
            }
        }
    }

    /// Left/right-click activation (spec §4 activateItem/activate).
    /// Returns true when the launcher should close.
    pub fn activate_entry(&mut self, e: &RingEntry, right_button: bool) -> bool {
        if let Some(action) = e.action.clone() {
            self.run_action_on(&e.wm_class, e.window.clone(), e.app_index, &action);
            return true;
        }
        if right_button && !e.is_window {
            self.launch_entry(e);
            return true;
        }
        match self.window_for(e) {
            Some(id) => {
                log::debug!("activate: focusing {} ({id})", e.name);
                self.comp.activate(&id);
            }
            None => {
                log::debug!("activate: no window for {}, launching", e.name);
                self.launch_entry(e);
            }
        }
        true
    }

    /// Run one action against the app's chosen window (spec §6 runAction).
    pub fn run_action_on(
        &mut self,
        wm_class: &str,
        window: Option<WindowId>,
        app_index: Option<usize>,
        a: &ActionTemplate,
    ) {
        let probe = RingEntry {
            wm_class: wm_class.to_owned(),
            window: window.clone(),
            is_window: window.is_some(),
            app_index,
            ..Default::default()
        };
        let launch_fallback = |core: &Self| {
            if let Some(i) = app_index {
                if let Some(app) = core.apps.get(i) {
                    apps::launch(&app.exec);
                }
            }
        };
        match &a.kind {
            ActionKind::Run(cmd) => apps::launch(cmd),
            ActionKind::Close => {
                if let Some(id) = self.window_for(&probe) {
                    self.comp.close_window(&id);
                }
            }
            ActionKind::Float => {
                if let Some(id) = self.window_for(&probe) {
                    self.comp.toggle_float(&id);
                }
            }
            ActionKind::Fullscreen => {
                if let Some(id) = self.window_for(&probe) {
                    self.comp.fullscreen(&id);
                }
            }
            ActionKind::Keys(combo) => {
                match self.window_for(&probe) {
                    Some(id) if self.comp.can_send_keys() => {
                        let c = shortcuts::parse(combo);
                        self.comp.send_keys(&id, &shortcuts::hypr_mods(&c), &c.key);
                    }
                    _ => launch_fallback(self), // not running or non-Hyprland
                }
            }
            ActionKind::Launch => launch_fallback(self),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::CompositorEvent;
    use std::sync::mpsc::Sender;

    struct FakeComp;
    impl Compositor for FakeComp {
        fn backend(&self) -> &'static str {
            "fake"
        }
        fn windows(&mut self) -> Vec<WindowInfo> {
            Vec::new()
        }
        fn active_window(&mut self) -> Option<WindowInfo> {
            None
        }
        fn watch(&mut self, _tx: Sender<CompositorEvent>) {}
        fn activate(&mut self, _id: &WindowId) {}
        fn close_window(&mut self, _id: &WindowId) {}
        fn fullscreen(&mut self, _id: &WindowId) {}
    }

    fn core_with(windows: Vec<WindowInfo>) -> Core {
        let apps = vec![
            AppEntry {
                name: "Firefox".into(),
                icon: "firefox".into(),
                exec: vec!["firefox".into()],
                wm_class: "firefox".into(),
                ..Default::default()
            },
            AppEntry {
                name: "Kitty".into(),
                icon: "kitty".into(),
                exec: vec!["kitty".into()],
                wm_class: "kitty".into(),
                ..Default::default()
            },
        ];
        let mut core = Core::new(Settings::default(), apps, AppIndex::default(), Box::new(FakeComp));
        core.windows = windows;
        core
    }

    fn win(id: &str, cls: &str, title: &str, focused: bool) -> WindowInfo {
        WindowInfo {
            id: id.into(),
            app_id: cls.into(),
            title: title.into(),
            focused,
        }
    }

    #[test]
    fn window_matching_is_case_insensitive_and_exact() {
        let core = core_with(vec![
            win("1", "Firefox", "a", false),
            win("2", "firefox", "b", false),
            win("3", "firefox-dev", "c", false),
        ]);
        let e = RingEntry {
            wm_class: "firefox".into(),
            ..Default::default()
        };
        assert_eq!(core.windows_for(&e).len(), 2); // exact lc match, not substring
    }

    #[test]
    fn cycle_wraps_and_negative() {
        let mut core = core_with(vec![
            win("1", "firefox", "a", false),
            win("2", "firefox", "b", false),
            win("3", "firefox", "c", false),
        ]);
        let e = RingEntry {
            wm_class: "firefox".into(),
            ..Default::default()
        };
        core.cycle_window(&e, -1); // 0 - 1 wraps to 2
        assert_eq!(core.selected_window_index(&e), 2);
        core.cycle_window(&e, 1);
        assert_eq!(core.selected_window_index(&e), 0);
    }

    #[test]
    fn windows_ring_groups_by_class() {
        let core = core_with(vec![
            win("1", "kitty", "z-term", false),
            win("2", "firefox", "b", false),
            win("3", "kitty", "a-term", false),
        ]);
        let ring = core.ring_model(Mode::Windows);
        let classes: Vec<&str> = ring.iter().map(|e| e.wm_class.as_str()).collect();
        assert_eq!(classes, ["firefox", "kitty", "kitty"]);
        assert_eq!(ring[1].name, "a-term"); // sorted by title within class
    }

    #[test]
    fn actions_filtering_and_open_fallback() {
        let mut core = core_with(vec![win("1", "firefox", "page", true)]);
        core.active = Some(win("1", "firefox", "page", true));
        let app = core.focused_app().unwrap();
        let acts = core.actions_for(&app);
        // FakeComp: no float capability -> close + fullscreen only
        let ids: Vec<&str> = acts.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["w:close", "w:full"]);

        // whitelist filters non-custom actions
        let mut app2 = app.clone();
        app2.action_ids = Some(vec!["w:close".into()]);
        let ids2: Vec<String> = core.actions_for(&app2).iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids2, ["w:close"]);

        // nothing left -> Open fallback
        core.windows.clear();
        core.active = None;
        let mut app3 = app.clone();
        app3.window = None;
        app3.action_ids = Some(vec![]);
        let acts3 = core.actions_for(&app3);
        assert_eq!(acts3.len(), 1);
        assert!(matches!(acts3[0].kind, ActionKind::Launch));
    }

    #[test]
    fn actions_ring_survives_overlay_focus_steal() {
        let mut core = core_with(vec![win("1", "firefox", "page", true)]);
        core.active = Some(win("1", "firefox", "page", true));
        // open_ring snapshots the target...
        core.actions_target = core.focused_app();
        assert!(!core.ring_model(Mode::Actions).is_empty());
        // ...then the overlay steals focus (active becomes None/our window):
        // the ring must keep building from the frozen snapshot
        core.active = None;
        let ring = core.ring_model(Mode::Actions);
        assert!(!ring.is_empty(), "actions ring wiped by focus change");
        assert!(ring.iter().any(|e| e.name == "Close"));
        // no snapshot (nothing focused at open) -> empty ring
        core.actions_target = None;
        assert!(core.ring_model(Mode::Actions).is_empty());
    }

    #[test]
    fn classless_window_is_never_a_focus_target() {
        // Our own overlay reports an empty app_id; snapshotting it mid-close
        // must yield None (keep/clear semantics live in open_ring), never a
        // dead FocusedApp that builds an empty actions ring.
        let mut core = core_with(vec![]);
        core.active = Some(win("9", "", "RadiAll", true));
        assert!(core.focused_app().is_none());
    }

    #[test]
    fn title_cleaning() {
        // strips the app-name suffix browsers append
        assert_eq!(
            clean_title("Qwandra · Cheap flights — Mozilla Firefox", "Firefox"),
            "Qwandra · Cheap flights"
        );
        assert_eq!(clean_title("docs - Thunar", "Files"), "docs - Thunar"); // no app match -> kept
        assert_eq!(clean_title("docs - Thunar", "Thunar"), "docs");
        // two-stage suffixes ("Page — Firefox — Nightly" style) strip twice
        assert_eq!(
            clean_title("Page — Mozilla Firefox — Firefox Nightly", "Firefox"),
            "Page"
        );
        // stripping is greedy but bounded (2 passes): any app-mentioning
        // tail segment goes — the slice label leads with the app name anyway
        assert_eq!(
            clean_title("a - b | chromium thing - Chromium", "Chromium"),
            "a - b"
        );
        // never strip down to nothing
        assert_eq!(clean_title("Mozilla Firefox", "Firefox"), "Mozilla Firefox");
        // safety cap still applies
        let long = "x".repeat(200);
        assert_eq!(clean_title(&long, "App").chars().count(), 161); // 160 + ellipsis
        assert_eq!(clean_title("short", "App"), "short");
    }

    #[test]
    fn windows_ring_names_are_cleaned_titles() {
        // "firefox" is a configured app named "Firefox": its window slice
        // label drops the "— Mozilla Firefox" tail. Unpinned classes clean
        // against the class itself.
        let mut core = core_with(vec![win("1", "firefox", "Page — Mozilla Firefox", true)]);
        core.windows = vec![win("1", "firefox", "Page — Mozilla Firefox", true)];
        let ring = core.ring_model(Mode::Windows);
        assert_eq!(ring[0].name, "Page");
    }

    #[test]
    fn first_running_index_falls_back_to_zero() {
        let core = core_with(vec![win("1", "kitty", "t", false)]);
        assert_eq!(core.first_running_index(), 1); // kitty is apps[1]
        let empty = core_with(vec![]);
        assert_eq!(empty.first_running_index(), 0);
    }
}
