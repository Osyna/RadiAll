//! UI glue: the wheel's interaction state machine (hover, click, long-press,
//! scroll, follow-outside, Esc), geometry/hit-testing, and pushing models
//! into the Slint windows. Ports Wheel.qml/RadialMenu.qml behavior 1:1;
//! rendering itself is declarative in ui/*.slint.

use crate::apps;
use crate::config::{self, AppEntry, CustomAction};
use crate::ring::{ActionTemplate, Core, FocusedApp, Mode, RingEntry};
use crate::shortcuts;
use crate::theme::{Rgba, Skin as SkinData};
use crate::{
    ActionRow, AppRow, ArcItem, IconPick, InstalledRow, RingItem, RingWindow, SettingsWindow, Skin,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;
use std::time::Duration;

const CENTER_TIMER_MS: u64 = 2000; // hole hover -> settings button
const LONG_PRESS_MS: u64 = 400; // press-and-hold -> action arc
const CLOSE_FADE_MS: u64 = 230; // window stays mapped through fade-out
const ACTIVATE_DELAY_MS: u64 = 120; // unmap-refocus settle before dispatch
const TITLE_TRUNCATE: usize = 42;

// ------------------------------------------------------------- geometry

#[derive(Debug, Clone, Copy, Default)]
pub struct Geometry {
    pub outer_r: f32,
    pub inner_r: f32,
    pub ring_r: f32,
    pub icon_box: f32,
    // action arc (spec §5)
    pub arc_radius: f32,
    pub arc_btn_r: f32,
    pub arc_band_inner: f32,
    pub arc_band_outer: f32,
}

impl Geometry {
    pub fn compute(settings: &crate::config::Settings, skin: &SkinData, ui_scale: f32) -> Self {
        let s = |px: f32| skin.s(px);
        let ring_r = s(settings.ring_radius) * ui_scale;
        let icon_box = s(settings.icon_size) * ui_scale;
        let outer_r = ring_r + icon_box * 0.96;
        let inner_r = (s(64.0) * ui_scale).max(ring_r - icon_box * 0.90);
        let gap = s(10.0);
        let btn_r = s(23.0);
        let arc_radius = outer_r + gap + btn_r;
        Self {
            outer_r,
            inner_r,
            ring_r,
            icon_box,
            arc_radius,
            arc_btn_r: btn_r,
            arc_band_inner: outer_r + s(3.0),
            arc_band_outer: arc_radius + btn_r + s(8.0),
        }
    }

    fn dist(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.outer_r;
        let dy = y - self.outer_r;
        (dx * dx + dy * dy).sqrt()
    }

    pub fn in_hole(&self, x: f32, y: f32) -> bool {
        self.dist(x, y) < self.inner_r
    }

    /// Nearest-center slice hit test (spec §3). -1 = hole / outside.
    pub fn slice_at(&self, x: f32, y: f32, count: usize, follow_outside: bool) -> i32 {
        if count == 0 {
            return -1;
        }
        let d = self.dist(x, y);
        if d < self.inner_r {
            return -1;
        }
        if d > self.outer_r && !follow_outside {
            return -1;
        }
        let a = (y - self.outer_r).atan2(x - self.outer_r);
        let seg = TAU / count as f32;
        let idx = ((a + PI / 2.0) / seg).round() as i64;
        idx.rem_euclid(count as i64) as i32
    }
}

/// Shortest-path rotation delta (spec §2 accumulator).
pub fn rotation_delta(current: f32, active_index: usize, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let target = active_index as f32 * (360.0 / count as f32);
    let cur_norm = current.rem_euclid(360.0);
    ((target - cur_norm).rem_euclid(360.0) + 540.0).rem_euclid(360.0) - 180.0
}

/// Perceived luminance 0..1 of the band color (drives ink-vs-white text).
fn band_lum(c: Rgba) -> f32 {
    (0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32) / 255.0
}

fn to_color(c: Rgba) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn truncate_title(t: &str) -> String {
    if t.chars().count() > TITLE_TRUNCATE {
        let mut s: String = t.chars().take(TITLE_TRUNCATE).collect();
        s.push('…');
        s
    } else {
        t.to_owned()
    }
}

// ----------------------------------------------------------- ui state

/// Which press is in flight (for long-press vs click resolution).
#[derive(Default)]
struct PressState {
    down_slice: i32,
    arc_opened_by_press: bool,
}

pub struct Ui {
    pub core: RefCell<Core>,
    // Window components are created fresh on every open and dropped after
    // the close fade: the winit backend only maps a native window whose
    // component was created in the same event-loop turn as its first show().
    ring_win: RefCell<Option<RingWindow>>,
    settings_win: RefCell<Option<SettingsWindow>>,
    mode: RefCell<Mode>,
    open: RefCell<bool>,
    ring: RefCell<Vec<RingEntry>>,
    geo: RefCell<Geometry>,
    skin: RefCell<SkinData>,
    /// Skin values derived from settings + band luminance, pushed on create.
    adaptive: RefCell<Adaptive>,
    /// Long-press arc: source entry + its filtered actions.
    arc: RefCell<Option<(RingEntry, Vec<ActionTemplate>)>>,
    press: RefCell<PressState>,
    center_timer: slint::Timer,
    long_press_timer: slint::Timer,
    hide_timer: slint::Timer,
    settings_save: slint::Timer,
    apps_save: slint::Timer,
    /// Live theme reload: polls the active theme file's mtime (v1 parity
    /// with Quickshell's FileView watchChanges).
    theme_watch: slint::Timer,
    theme_mtime: RefCell<Option<std::time::SystemTime>>,
    /// Keeps the tray component alive for the daemon's lifetime.
    tray: RefCell<Option<crate::Tray>>,
}

/// Rust-computed colors the Skin global can't derive itself.
#[derive(Debug, Clone, Copy)]
struct Adaptive {
    on_band: Rgba,
    dot: Rgba,
    rim: Rgba,
    rim_width: f32,
    sector: Rgba,
    label_fg: Rgba,
    arc_btn: Rgba,
    arc_btn_hover: Rgba,
}

impl Default for Adaptive {
    fn default() -> Self {
        Self {
            on_band: Rgba::rgb(0x19, 0x1a, 0x2e),
            dot: Rgba::rgba(0, 0, 0, 140),
            rim: Rgba::rgba(0, 0, 0, 26),
            rim_width: 1.0,
            sector: Rgba::rgb(0xe8, 0x55, 0x5f),
            label_fg: Rgba::rgba(255, 255, 255, 242),
            arc_btn: Rgba::rgba(255, 255, 255, 26),
            arc_btn_hover: Rgba::rgb(0xe4, 0x48, 0x54),
        }
    }
}

/// The wedge's auto color: 4% toward white, opaque (v1 sectorC).
fn mix_toward_white(c: Rgba) -> Rgba {
    let mix = |v: u8| (v as f32 * 0.96 + 255.0 * 0.04).round() as u8;
    Rgba::rgb(mix(c.r), mix(c.g), mix(c.b))
}

impl Ui {
    pub fn new(core: Core) -> Result<Rc<Self>, slint::PlatformError> {
        let ui = Rc::new(Self {
            core: RefCell::new(core),
            ring_win: RefCell::new(None),
            settings_win: RefCell::new(None),
            mode: RefCell::new(Mode::Apps),
            open: RefCell::new(false),
            ring: RefCell::new(Vec::new()),
            geo: RefCell::new(Geometry::default()),
            skin: RefCell::new(SkinData::default()),
            adaptive: RefCell::new(Adaptive::default()),
            arc: RefCell::new(None),
            press: RefCell::new(PressState::default()),
            center_timer: slint::Timer::default(),
            long_press_timer: slint::Timer::default(),
            hide_timer: slint::Timer::default(),
            settings_save: slint::Timer::default(),
            apps_save: slint::Timer::default(),
            theme_watch: slint::Timer::default(),
            theme_mtime: RefCell::new(None),
            tray: RefCell::new(None),
        });
        ui.sync_all();
        *ui.theme_mtime.borrow_mut() = ui.active_theme_mtime();
        let this = Rc::downgrade(&ui);
        ui.theme_watch.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(1000),
            move || {
                let Some(ui) = this.upgrade() else { return };
                let now = ui.active_theme_mtime();
                if now != *ui.theme_mtime.borrow() {
                    *ui.theme_mtime.borrow_mut() = now;
                    log::debug!("theme file changed, reloading");
                    ui.sync_all();
                }
            },
        );
        Ok(ui)
    }

    fn active_theme_mtime(&self) -> Option<std::time::SystemTime> {
        let name = self.core.borrow().settings.theme.clone();
        let path = crate::theme::themes_dir().join(format!("{name}.json"));
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    pub fn keep_tray(&self, tray: crate::Tray) {
        *self.tray.borrow_mut() = Some(tray);
    }

    /// Live handle to the ring window, if one is mapped.
    fn ring_handle(&self) -> Option<RingWindow> {
        self.ring_win.borrow().as_ref().map(|w| w.clone_strong())
    }

    /// Live handle to the settings window, if open.
    fn settings_handle(&self) -> Option<SettingsWindow> {
        self.settings_win.borrow().as_ref().map(|w| w.clone_strong())
    }

    // ------------------------------------------------------ skin sync

    pub fn sync_all(self: &Rc<Self>) {
        self.sync_skin();
        self.sync_geometry();
        // the latched wedge color is pushed on hover; re-derive it so a
        // live theme reload recolors the visible wedge too
        if let Some(win) = self.ring_handle() {
            let idx = win.get_active_index().max(0) as usize;
            self.set_active_index(idx);
        }
        self.refresh_editor_models();
        self.refresh_preview();
    }

    fn sync_skin(self: &Rc<Self>) {
        let core = self.core.borrow();
        let s = &core.settings;
        let skin = SkinData::load(
            &s.theme,
            Some(&s.bg),
            Some(&s.accent),
            if s.seg_bg.is_empty() { None } else { Some(&s.seg_bg) },
        );
        let light = band_lum(skin.bg) > 0.5;
        let cell_edge = skin.edge.a > 0;
        let (rim, rim_w) = if cell_edge {
            (skin.edge, skin.s(skin.edge_width))
        } else {
            let a = if light { 0.10 } else { 0.14 };
            let base = if light { Rgba::rgb(0, 0, 0) } else { Rgba::rgb(255, 255, 255) };
            (base.with_alpha((a * s.wheel_opacity * 255.0) as u8), 1.0)
        };
        let adaptive = Adaptive {
            on_band: skin.on_band.unwrap_or(if light {
                Rgba::rgb(0x19, 0x1a, 0x2e)
            } else {
                Rgba::rgb(255, 255, 255)
            }),
            dot: skin.dot.unwrap_or(if light {
                Rgba::rgba(0, 0, 0, 140)
            } else {
                Rgba::rgba(255, 255, 255, 217)
            }),
            rim,
            rim_width: rim_w,
            sector: skin.sector.unwrap_or_else(|| mix_toward_white(skin.accent)),
            label_fg: skin.label_fg.unwrap_or(skin.fg_strong),
            arc_btn: skin.arc_btn.unwrap_or(skin.fg.with_alpha(26)),
            arc_btn_hover: skin.arc_btn_hover.unwrap_or(skin.accent),
        };
        drop(core);
        *self.skin.borrow_mut() = skin;
        *self.adaptive.borrow_mut() = adaptive;
        if let Some(w) = self.ring_handle() {
            self.push_skin(&w.global::<Skin>());
        }
        if let Some(w) = self.settings_handle() {
            self.push_skin(&w.global::<Skin>());
        }
    }

    /// Push the stored skin + adaptive values into one window's Skin global.
    fn push_skin(&self, g: &Skin<'_>) {
        let skin = self.skin.borrow();
        let a = *self.adaptive.borrow();
        let core = self.core.borrow();
        g.set_scale(skin.scale);
        g.set_bg(to_color(skin.bg));
        g.set_accent(to_color(skin.accent));
        g.set_glass_bg(to_color(skin.glass_bg));
        g.set_glass_hover(to_color(skin.glass_hover));
        g.set_pill_bg(to_color(skin.pill_bg));
        g.set_pill_hover(to_color(skin.pill_hover));
        g.set_btn_hover(to_color(skin.btn_hover));
        g.set_btn_active(to_color(skin.btn_active));
        g.set_fg(to_color(skin.fg));
        g.set_fg_strong(to_color(skin.fg_strong));
        g.set_fg_dim(to_color(skin.fg_dim));
        g.set_yellow(to_color(skin.yellow));
        g.set_red(to_color(skin.red));
        g.set_green(to_color(skin.green));
        g.set_sep(to_color(skin.sep));
        g.set_edge(to_color(skin.edge));
        g.set_edge_width(skin.edge_width);
        g.set_panel_bg(to_color(skin.panel_bg));
        g.set_label_pill_bg(to_color(skin.label_pill_bg));
        g.set_font(skin.font.clone().into());
        g.set_font_display(skin.font_display.clone().into());
        g.set_icon_font(skin.icon_font.clone().into());
        g.set_mono_font(skin.mono_font.clone().into());
        g.set_wheel_opacity(core.settings.wheel_opacity);
        g.set_show_labels(core.settings.show_labels);
        g.set_on_band(to_color(a.on_band));
        g.set_dot(to_color(a.dot));
        g.set_rim(to_color(a.rim));
        g.set_rim_width(a.rim_width);
        g.set_sector(to_color(a.sector));
        g.set_label_fg(to_color(a.label_fg));
        g.set_arc_btn(to_color(a.arc_btn));
        g.set_arc_btn_hover(to_color(a.arc_btn_hover));
        g.set_backdrop(to_color(skin.backdrop));
        g.set_arc_bg(to_color(skin.arc_bg));
        g.set_arc_stroke(to_color(skin.arc_stroke));
        g.set_settings_btn(to_color(skin.settings_btn));
        // section geometry + inactive-section fill (design-scaled px; the
        // wheel applies its own ui-scale / pop factors)
        // "bg" sentinel: sections take the band's exact pixel color, so only
        // the gaps/inset reveal what's behind (theme pin still wins).
        let seg_bg = skin.seg_bg.unwrap_or_else(|| {
            if core.settings.seg_bg == "bg" {
                skin.bg
                    .with_alpha((core.settings.wheel_opacity * 255.0).round() as u8)
            } else {
                Rgba::rgba(0, 0, 0, 0)
            }
        });
        g.set_seg_bg(to_color(seg_bg));
        g.set_sector_radius(skin.s(core.settings.sector_radius));
        g.set_seg_radius(skin.s(core.settings.seg_radius));
        g.set_section_inset(skin.s(core.settings.section_inset));
        g.set_seg_gap(skin.s(core.settings.seg_gap));
    }

    fn sync_geometry(self: &Rc<Self>) {
        let geo = Geometry::compute(&self.core.borrow().settings, &self.skin.borrow(), 1.0);
        *self.geo.borrow_mut() = geo;
        if let Some(w) = self.ring_handle() {
            self.push_geometry(&w);
        }
        if let Some(w) = self.settings_handle() {
            self.push_preview_geometry(&w);
        }
    }

    fn push_geometry(&self, w: &RingWindow) {
        let geo = *self.geo.borrow();
        w.set_outer_r(geo.outer_r);
        w.set_inner_r(geo.inner_r);
        w.set_ring_r(geo.ring_r);
        w.set_icon_box(geo.icon_box);
        w.set_arc_radius(geo.arc_radius);
        w.set_arc_btn_r(geo.arc_btn_r);
        w.set_arc_band_inner(geo.arc_band_inner);
        w.set_arc_band_outer(geo.arc_band_outer);
        w.set_dim(self.core.borrow().settings.dim);
    }

    fn push_preview_geometry(&self, w: &SettingsWindow) {
        let pv = Geometry::compute(&self.core.borrow().settings, &self.skin.borrow(), 0.5);
        w.set_pv_outer_r(pv.outer_r);
        w.set_pv_inner_r(pv.inner_r);
        w.set_pv_ring_r(pv.ring_r);
        w.set_pv_icon_box(pv.icon_box);
    }

    // --------------------------------------------------- model building

    fn load_image(icon: &str, px: u32) -> (slint::Image, bool) {
        match apps::load_icon_pixels(icon, px) {
            Some(buf) => (slint::Image::from_rgba8(buf), true),
            None => (slint::Image::default(), false),
        }
    }

    fn ring_item(&self, e: &RingEntry, px: u32) -> RingItem {
        let core = self.core.borrow();
        let ws = core.windows_for(e);
        let win_count = ws.len();
        let sel = core.selected_window_index(e);
        let (icon, has_icon) = Self::load_image(&e.icon, px);
        let is_action = e.is_action();
        let tint = if is_action {
            let c = Rgba::parse(&e.color).unwrap_or_else(|| {
                if band_lum(self.skin.borrow().bg) > 0.5 {
                    Rgba::rgb(0x19, 0x1a, 0x2e)
                } else {
                    Rgba::rgb(255, 255, 255)
                }
            });
            to_color(c)
        } else {
            slint::Color::from_argb_u8(0, 0, 0, 0)
        };
        RingItem {
            label: e.name.clone().into(),
            icon,
            has_icon,
            // Apps/windows without a resolvable icon fall back to a monogram
            // letter (always renderable); nerd glyphs are kept for action
            // items only, where the pictogram carries meaning.
            glyph: if is_action { e.glyph.clone().into() } else { SharedString::default() },
            monogram: e
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default()
                .into(),
            dot_count: win_count.min(6) as i32,
            sel_dot: sel as i32,
            is_action,
            tint,
            wedge: if is_action {
                slint::Color::from_argb_u8(0, 0, 0, 0)
            } else {
                Rgba::parse(&e.color)
                    .map(to_color)
                    .unwrap_or(slint::Color::from_argb_u8(0, 0, 0, 0))
            },
        }
    }

    fn rebuild_ring(self: &Rc<Self>) {
        let Some(win) = self.ring_handle() else { return };
        let mode = *self.mode.borrow();
        let entries = self.core.borrow().ring_model(mode);
        let px = self.geo.borrow().icon_box.max(32.0) as u32;
        let items: Vec<RingItem> = entries.iter().map(|e| self.ring_item(e, px)).collect();
        win.set_items(ModelRc::new(VecModel::from(items)));
        win.set_actions_mode(mode == Mode::Actions);
        // actions mode: focused app icon in the hole
        if mode == Mode::Actions {
            if let Some(app) = self.core.borrow().actions_target.clone() {
                let (img, ok) = Self::load_image(&app.icon, 96);
                win.set_focus_icon(img);
                win.set_show_focus_icon(ok);
            }
        } else {
            win.set_show_focus_icon(false);
        }
        log::trace!(
            "ring model ({mode:?}): {:?}",
            entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()
        );
        *self.ring.borrow_mut() = entries;
        // initial selection: first running app (apps ring), else 0
        let initial = match mode {
            Mode::Apps => self.core.borrow().first_running_index(),
            _ => 0,
        };
        self.set_active_index(initial);
        win.set_hovered_index(-1);
        self.update_center_label(-1);
    }

    fn set_active_index(self: &Rc<Self>, idx: usize) {
        let Some(win) = self.ring_handle() else { return };
        let count = self.ring.borrow().len();
        let cur = win.get_sector_rotation();
        let delta = rotation_delta(cur, idx, count);
        win.set_sector_rotation(cur + delta);
        win.set_active_index(idx as i32);
        // wedge takes the app's own accent when it has one
        let wedge = self
            .ring
            .borrow()
            .get(idx)
            .filter(|e| !e.is_action())
            .and_then(|e| Rgba::parse(&e.color))
            .unwrap_or(self.adaptive.borrow().sector);
        win.set_wedge_color(to_color(wedge));
    }

    fn update_center_label(self: &Rc<Self>, hovered: i32) {
        let Some(win) = self.ring_handle() else { return };
        let ring = self.ring.borrow();
        let core = self.core.borrow();
        let idx = if hovered >= 0 {
            hovered as usize
        } else {
            win.get_active_index().max(0) as usize
        };
        let label = match ring.get(idx) {
            Some(e) => {
                let ws = core.windows_for(e);
                if ws.len() > 1 {
                    let sel = core.selected_window_index(e).max(0) as usize;
                    let title = ws.get(sel).map(|w| w.title.as_str()).unwrap_or("");
                    format!(
                        "{}  ·  {}   {}/{}",
                        e.name,
                        truncate_title(title),
                        sel + 1,
                        ws.len()
                    )
                } else {
                    e.name.clone()
                }
            }
            None => String::new(),
        };
        drop(ring);
        drop(core);
        win.set_center_label(label.into());
    }

    // ------------------------------------------------------ open/close

    /// Create + wire a fresh ring window (per-open lifecycle, see struct doc).
    fn create_ring_window(self: &Rc<Self>) -> Result<RingWindow, slint::PlatformError> {
        let win = RingWindow::new()?;
        self.push_skin(&win.global::<Skin>());
        self.push_geometry(&win);
        Self::wire_ring(self, &win);
        *self.ring_win.borrow_mut() = Some(win.clone_strong());
        Ok(win)
    }

    pub fn open_ring(self: &Rc<Self>, mode: Mode) {
        self.hide_timer.stop();
        *self.mode.borrow_mut() = mode;
        *self.arc.borrow_mut() = None;
        let win = match self.ring_handle() {
            Some(w) => w,
            None => match self.create_ring_window() {
                Ok(w) => w,
                Err(e) => {
                    log::error!("ring window creation failed: {e}");
                    return;
                }
            },
        };
        win.set_arc_open(false);
        // fresh compositor snapshot (adapters also push, but be exact on open)
        {
            let mut core = self.core.borrow_mut();
            let ws = core.comp.windows();
            core.windows = ws;
            let active = core.comp.active_window();
            core.active = active;
            // freeze the actions target NOW — the overlay steals focus next
            core.actions_target = core.focused_app();
        }
        self.rebuild_ring();
        *self.open.borrow_mut() = true;
        win.set_open(true);
        // Overlay via compositor rules (float + 100% size) when available;
        // real fullscreen makes compositors black out the windows behind.
        if !self.core.borrow().overlay_ready {
            win.window().set_fullscreen(true);
        }
        if let Err(e) = win.show() {
            log::warn!("ring window show failed: {e}");
        }
    }

    pub fn close(self: &Rc<Self>) {
        log::debug!("close() called, open={}", *self.open.borrow());
        if !*self.open.borrow() {
            return;
        }
        *self.open.borrow_mut() = false;
        *self.arc.borrow_mut() = None;
        self.center_timer.stop();
        self.long_press_timer.stop();
        let Some(win) = self.ring_handle() else { return };
        win.set_arc_open(false);
        win.set_open(false);
        win.set_show_settings_btn(false);
        // keep the window mapped through the fade, then unmap AND drop the
        // component: the next open creates a fresh one (see struct doc)
        let this = Rc::downgrade(self);
        self.hide_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(CLOSE_FADE_MS),
            move || {
                if let Some(ui) = this.upgrade() {
                    if let Some(w) = ui.ring_win.borrow_mut().take() {
                        w.hide().ok();
                    }
                }
            },
        );
    }

    /// Close WITHOUT the fade: unmap + drop the window immediately.
    /// Used before dispatching window activations/actions — the compositor
    /// re-focuses the previously-focused window when our overlay unmaps, so
    /// the unmap must land BEFORE our focuswindow/action dispatch or it
    /// stomps the activation (xdg toplevels, unlike the old layer-shell).
    pub fn close_now(self: &Rc<Self>) {
        if !*self.open.borrow() {
            return;
        }
        *self.open.borrow_mut() = false;
        *self.arc.borrow_mut() = None;
        self.center_timer.stop();
        self.long_press_timer.stop();
        self.hide_timer.stop();
        if let Some(w) = self.ring_win.borrow_mut().take() {
            w.hide().ok();
        }
    }

    pub fn toggle(self: &Rc<Self>, mode: Mode) {
        if *self.open.borrow() && *self.mode.borrow() == mode {
            self.close();
        } else {
            self.open_ring(mode);
        }
    }

    pub fn open_settings(self: &Rc<Self>) {
        self.close();
        let win = match self.settings_handle() {
            Some(w) => w,
            None => {
                let win = match SettingsWindow::new() {
                    Ok(w) => w,
                    Err(e) => {
                        log::error!("settings window creation failed: {e}");
                        return;
                    }
                };
                self.push_skin(&win.global::<Skin>());
                self.push_preview_geometry(&win);
                Self::wire_editor(self, &win);
                *self.settings_win.borrow_mut() = Some(win.clone_strong());
                win
            }
        };
        self.refresh_editor_models();
        self.refresh_preview();
        win.show().ok();
    }

    // ----------------------------------------------- compositor events

    pub fn on_windows_changed(self: &Rc<Self>, windows: Vec<crate::compositor::WindowInfo>) {
        self.core.borrow_mut().windows = windows;
        if *self.open.borrow() {
            match *self.mode.borrow() {
                Mode::Windows | Mode::Actions => self.rebuild_ring(),
                Mode::Apps => {
                    // dots only; rebuild is cheap enough
                    self.rebuild_ring()
                }
            }
        }
    }

    pub fn on_active_changed(self: &Rc<Self>, active: Option<crate::compositor::WindowInfo>) {
        self.core.borrow_mut().active = active;
        if *self.open.borrow() && *self.mode.borrow() == Mode::Actions {
            self.rebuild_ring();
        }
    }

    // ------------------------------------------------- input handlers

    fn wheel_local(self: &Rc<Self>, win: &RingWindow, x: f32, y: f32) -> (f32, f32) {
        // the wheel is centered in the fullscreen window
        let size = win.window().size();
        let sf = win.window().scale_factor();
        let (w, h) = (size.width as f32 / sf, size.height as f32 / sf);
        let geo = self.geo.borrow();
        (
            x - (w / 2.0 - geo.outer_r),
            y - (h / 2.0 - geo.outer_r),
        )
    }

    fn on_moved(self: &Rc<Self>, x: f32, y: f32) {
        log::trace!("pointer moved {x},{y}");
        if self.arc.borrow().is_some() {
            return; // arc handles its own hover via its buttons
        }
        let Some(win) = self.ring_handle() else { return };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();
        let count = self.ring.borrow().len();
        let follow = self.core.borrow().settings.follow_outside;
        if geo.in_hole(lx, ly) {
            win.set_hovered_index(-1);
            self.update_center_label(-1);
            if !win.get_show_settings_btn() && !self.center_timer.running() {
                let this = Rc::downgrade(self);
                self.center_timer.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(CENTER_TIMER_MS),
                    move || {
                        if let Some(ui) = this.upgrade() {
                            if let Some(w) = ui.ring_handle() {
                                w.set_show_settings_btn(true);
                            }
                        }
                    },
                );
            }
            return;
        }
        self.center_timer.stop();
        win.set_show_settings_btn(false);
        let s = geo.slice_at(lx, ly, count, follow);
        log::trace!("hover: local {lx},{ly} -> slice {s}");
        win.set_hovered_index(s);
        if s >= 0 {
            self.set_active_index(s as usize);
            // moving off the pressed slice cancels the pending long-press
            if self.press.borrow().down_slice != s {
                self.long_press_timer.stop();
            }
        }
        self.update_center_label(s);
    }

    fn on_down(self: &Rc<Self>, x: f32, y: f32, _right: bool) {
        log::debug!("on_down {x},{y}");
        let Some(win) = self.ring_handle() else { return };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();
        let count = self.ring.borrow().len();
        let follow = self.core.borrow().settings.follow_outside;
        let s = geo.slice_at(lx, ly, count, follow);
        let mut press = self.press.borrow_mut();
        press.down_slice = s;
        press.arc_opened_by_press = false;
        drop(press);
        // long-press opens the per-app arc (apps mode, valid slice only)
        if *self.mode.borrow() == Mode::Apps && s >= 0 && self.arc.borrow().is_none() {
            let this = Rc::downgrade(self);
            self.long_press_timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_millis(LONG_PRESS_MS),
                move || {
                    if let Some(ui) = this.upgrade() {
                        ui.open_arc(s as usize);
                        ui.press.borrow_mut().arc_opened_by_press = true;
                    }
                },
            );
        }
    }

    fn on_up(self: &Rc<Self>, x: f32, y: f32, right: bool) {
        log::debug!("on_up {x},{y} right={right}");
        self.long_press_timer.stop();
        if self.press.borrow().arc_opened_by_press {
            // QML suppresses the click that follows a pressAndHold
            self.press.borrow_mut().arc_opened_by_press = false;
            return;
        }
        let Some(win) = self.ring_handle() else { return };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();

        // arc open: empty-arc click returns to the wheel; outside closes all
        if self.arc.borrow().is_some() {
            let d = geo.dist(lx, ly);
            log::debug!("on_up with arc open: d={d} band_outer={}", geo.arc_band_outer);
            if d <= geo.arc_band_outer {
                *self.arc.borrow_mut() = None;
                win.set_arc_open(false);
            } else {
                self.close();
            }
            return;
        }

        let count = self.ring.borrow().len();
        let follow = self.core.borrow().settings.follow_outside;
        if geo.in_hole(lx, ly) {
            if win.get_show_settings_btn() {
                self.open_settings();
            } else {
                self.close();
            }
            return;
        }
        let s = geo.slice_at(lx, ly, count, follow);
        log::debug!("click up: local {lx},{ly} -> slice {s}");
        if s < 0 {
            self.close();
            return;
        }
        let entry = self.ring.borrow().get(s as usize).cloned();
        if let Some(e) = entry {
            // Unmap first, act second: the compositor refocuses the previous
            // window when the overlay unmaps; dispatching after that settles
            // (~1 frame) keeps our activation from being stomped.
            self.close_now();
            let this = Rc::downgrade(self);
            slint::Timer::single_shot(Duration::from_millis(ACTIVATE_DELAY_MS), move || {
                if let Some(ui) = this.upgrade() {
                    ui.core.borrow_mut().activate_entry(&e, right);
                }
            });
        } else {
            self.close();
        }
    }

    fn on_scroll(self: &Rc<Self>, x: f32, y: f32, delta_y: f32) {
        if *self.mode.borrow() == Mode::Actions {
            return;
        }
        let Some(win) = self.ring_handle() else { return };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();
        let count = self.ring.borrow().len();
        let follow = self.core.borrow().settings.follow_outside;
        let s = geo.slice_at(lx, ly, count, follow);
        if s < 0 {
            return;
        }
        let entry = self.ring.borrow().get(s as usize).cloned();
        if let Some(e) = entry {
            let dir = if delta_y > 0.0 { 1 } else { -1 };
            self.core.borrow_mut().cycle_window(&e, dir);
            // refresh dots + label for the changed selection
            let px = self.geo.borrow().icon_box.max(32.0) as u32;
            let item = self.ring_item(&e, px);
            if let Some(model) = win
                .get_items()
                .as_any()
                .downcast_ref::<VecModel<RingItem>>()
            {
                model.set_row_data(s as usize, item);
            }
            self.update_center_label(s);
        }
    }

    fn on_esc(self: &Rc<Self>) {
        log::debug!("on_esc");
        if self.arc.borrow().is_some() {
            *self.arc.borrow_mut() = None;
            if let Some(win) = self.ring_handle() {
                win.set_arc_open(false);
            }
            return;
        }
        self.close();
    }

    // ------------------------------------------------------ action arc

    fn open_arc(self: &Rc<Self>, slice: usize) {
        log::debug!("open_arc slice {slice}");
        let entry = match self.ring.borrow().get(slice).cloned() {
            Some(e) => e,
            None => return,
        };
        let core = self.core.borrow();
        let app_cfg = entry.app_index.and_then(|i| core.apps.get(i));
        let focused = FocusedApp {
            icon: entry.icon.clone(),
            wm_class: entry.wm_class.clone(),
            window: None,
            custom_actions: app_cfg.map(|a| a.custom_actions.clone()).unwrap_or_default(),
            action_ids: app_cfg.and_then(|a| a.action_ids.clone()),
        };
        let exec0 = app_cfg
            .and_then(|a| a.exec.first().cloned())
            .unwrap_or_default();
        let mut templates = core.action_templates(&focused, &exec0);
        // apply the same filter as actions_for, but with launch fallback list
        let has_window = !core.windows_for_class(&focused.wm_class).is_empty();
        templates.retain(|t| {
            let custom = matches!(t.kind, crate::ring::ActionKind::Keys(_));
            if !custom {
                if let Some(ids) = &focused.action_ids {
                    if !ids.contains(&t.id) {
                        return false;
                    }
                }
            }
            !(matches!(
                t.kind,
                crate::ring::ActionKind::Close
                    | crate::ring::ActionKind::Float
                    | crate::ring::ActionKind::Fullscreen
            ) && !has_window)
        });
        if templates.is_empty() {
            templates.push(ActionTemplate {
                id: "launch".into(),
                label: "Open".into(),
                glyph: crate::ring::G_OPEN.into(),
                icon: String::new(),
                color: String::new(),
                kind: crate::ring::ActionKind::Launch,
            });
        }
        drop(core);

        let items: Vec<ArcItem> = templates
            .iter()
            .map(|t| {
                let (icon, has_icon) = Self::load_image(&t.icon, 48);
                let tint = Rgba::parse(&t.color).unwrap_or(Rgba::rgb(255, 255, 255));
                ArcItem {
                    label: t.label.clone().into(),
                    icon,
                    has_icon,
                    glyph: t.glyph.clone().into(),
                    tint: to_color(tint),
                }
            })
            .collect();
        let Some(win) = self.ring_handle() else { return };
        win.set_arc_items(ModelRc::new(VecModel::from(items)));
        win.set_arc_label(entry.name.clone().into());
        win.set_arc_open(true);
        *self.arc.borrow_mut() = Some((entry, templates));
    }

    fn on_arc_clicked(self: &Rc<Self>, idx: i32) {
        let arc = self.arc.borrow().clone();
        if let Some((entry, templates)) = arc {
            if let Some(t) = templates.get(idx as usize) {
                self.close_now();
                let win = self.core.borrow().window_for(&entry);
                let this = Rc::downgrade(self);
                let entry = entry.clone();
                let action = t.clone();
                slint::Timer::single_shot(Duration::from_millis(ACTIVATE_DELAY_MS), move || {
                    if let Some(ui) = this.upgrade() {
                        ui.core.borrow_mut().run_action_on(
                            &entry.wm_class,
                            win,
                            entry.app_index,
                            &action,
                        );
                    }
                });
            }
        }
    }

    // ---------------------------------------------------------- wiring

    fn wire_ring(ui: &Rc<Self>, w: &RingWindow) {
        let this = Rc::downgrade(ui);
        w.on_pointer_moved(move |x, y| {
            if let Some(ui) = this.upgrade() {
                ui.on_moved(x, y);
            }
        });
        let this = Rc::downgrade(ui);
        w.on_pointer_down(move |x, y, right| {
            if let Some(ui) = this.upgrade() {
                ui.on_down(x, y, right);
            }
        });
        let this = Rc::downgrade(ui);
        w.on_pointer_up(move |x, y, right| {
            if let Some(ui) = this.upgrade() {
                ui.on_up(x, y, right);
            }
        });
        let this = Rc::downgrade(ui);
        w.on_scrolled(move |x, y, d| {
            if let Some(ui) = this.upgrade() {
                ui.on_scroll(x, y, d);
            }
        });
        let this = Rc::downgrade(ui);
        w.on_esc_pressed(move || {
            if let Some(ui) = this.upgrade() {
                ui.on_esc();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_arc_clicked(move |i| {
            if let Some(ui) = this.upgrade() {
                ui.on_arc_clicked(i);
            }
        });
        // logo for the center settings button
        let logo = apps::load_icon_pixels_from_bytes(include_bytes!("../launcher/RadiAll.png"), 128);
        if let Some(buf) = logo {
            w.set_logo(slint::Image::from_rgba8(buf));
        }
    }

    // ------------------------------------------------------- editor

    fn refresh_preview(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else { return };
        let core = self.core.borrow();
        let entries = core.ring_model(Mode::Apps);
        let px = 40;
        drop(core);
        let items: Vec<RingItem> = entries.iter().map(|e| self.ring_item(e, px)).collect();
        let first = self.core.borrow().first_running_index();
        let label = entries
            .get(first)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        sw.set_pv_items(ModelRc::new(VecModel::from(items)));
        sw.set_pv_active_index(first as i32);
        sw.set_pv_label(label.into());
    }

    /// Installed-app picker rows, filtered by `query` (case-insensitive
    /// substring). Rows carry their ORIGINAL index so add-installed(idx)
    /// stays valid regardless of filtering.
    fn installed_rows(core: &Core, query: &str) -> Vec<InstalledRow> {
        let q = query.trim().to_lowercase();
        core.index
            .installed()
            .iter()
            .enumerate()
            .filter(|(_, d)| q.is_empty() || d.name.to_lowercase().contains(&q))
            .map(|(i, d)| {
                let (icon, has_icon) = Self::load_image(&d.icon, 32);
                InstalledRow {
                    idx: i as i32,
                    name: d.name.clone().into(),
                    icon,
                    has_icon,
                }
            })
            .collect()
    }

    /// Icon-library picker rows for `query`, capped (search-as-you-type).
    fn icon_pick_rows(core: &Core, query: &str) -> Vec<IconPick> {
        core.icons
            .search(query, 200)
            .into_iter()
            .map(|e| {
                let path = e.path.to_string_lossy().to_string();
                let (icon, _) = Self::load_image(&path, 24);
                IconPick {
                    name: e.name.clone().into(),
                    set: e.set.clone().into(),
                    path: path.into(),
                    icon,
                }
            })
            .collect()
    }

    fn refresh_editor_models(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else { return };
        let core = self.core.borrow();
        let apps_rows: Vec<AppRow> = core
            .apps
            .iter()
            .map(|a| {
                let (icon, has_icon) = Self::load_image(&a.icon, 32);
                AppRow {
                    name: a.name.clone().into(),
                    icon_name: a.icon.clone().into(),
                    exec: a.exec.join(" ").into(),
                    wm_class: a.wm_class.clone().into(),
                    color: a.color.clone().into(),
                    icon,
                    has_icon,
                }
            })
            .collect();
        let installed_rows = Self::installed_rows(&core, "");
        let s = &core.settings;
        let sw = &sw;
        sw.set_apps(ModelRc::new(VecModel::from(apps_rows)));
        sw.set_installed(ModelRc::new(VecModel::from(installed_rows)));
        sw.set_icon_lib_count(core.icons.len() as i32);
        sw.set_themes(ModelRc::new(VecModel::from(
            crate::theme::available()
                .into_iter()
                .map(SharedString::from)
                .collect::<Vec<_>>(),
        )));
        sw.set_current_theme(s.theme.clone().into());
        sw.set_accent_hex(s.accent.clone().into());
        sw.set_bg_hex(s.bg.clone().into());
        sw.set_icon_size(s.icon_size);
        sw.set_ring_radius(s.ring_radius);
        sw.set_dim(s.dim);
        sw.set_wheel_opacity(s.wheel_opacity);
        sw.set_show_labels(s.show_labels);
        sw.set_follow_outside(s.follow_outside);
        sw.set_sector_radius(s.sector_radius);
        sw.set_seg_radius(s.seg_radius);
        sw.set_section_inset(s.section_inset);
        sw.set_seg_gap(s.seg_gap);
        sw.set_seg_bg_hex(s.seg_bg.clone().into());
        sw.set_shortcuts_enabled(s.shortcuts_enabled);
        sw.set_persist_binds(s.persist_binds);
        sw.set_sc_apps(s.shortcuts.apps.clone().into());
        sw.set_sc_windows(s.shortcuts.windows.clone().into());
        sw.set_sc_actions(s.shortcuts.actions.clone().into());
        sw.set_can_manage_keybinds(core.comp.can_manage_keybinds());
        sw.set_can_send_keys(core.comp.can_send_keys());
        drop(core);
        self.refresh_selected_actions();
    }

    fn refresh_selected_actions(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else { return };
        let sel = sw.get_selected_app();
        let core = self.core.borrow();
        let rows: Vec<ActionRow> = match core.apps.get(sel.max(0) as usize) {
            Some(app) if sel >= 0 => {
                let focused = FocusedApp {
                    icon: app.icon.clone(),
                    wm_class: app.wm_class.clone(),
                    window: None,
                    custom_actions: app.custom_actions.clone(),
                    action_ids: app.action_ids.clone(),
                };
                let exec0 = app.exec.first().cloned().unwrap_or_default();
                core.action_templates(&focused, &exec0)
                    .into_iter()
                    .map(|t| {
                        let custom = matches!(t.kind, crate::ring::ActionKind::Keys(_));
                        let enabled = custom
                            || app
                                .action_ids
                                .as_ref()
                                .map(|ids| ids.contains(&t.id))
                                .unwrap_or(true);
                        let shortcut = match &t.kind {
                            crate::ring::ActionKind::Keys(s) => s.clone(),
                            _ => String::new(),
                        };
                        ActionRow {
                            id: t.id.clone().into(),
                            label: t.label.clone().into(),
                            glyph: t.glyph.clone().into(),
                            enabled,
                            is_custom: custom,
                            shortcut: shortcut.into(),
                            icon_name: t.icon.clone().into(),
                            color: t.color.clone().into(),
                        }
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        drop(core);
        sw.set_app_actions(ModelRc::new(VecModel::from(rows)));
    }

    fn save_settings_debounced(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.settings_save.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(250),
            move || {
                if let Some(ui) = this.upgrade() {
                    config::save_settings(&ui.core.borrow().settings);
                }
            },
        );
    }

    fn save_apps_debounced(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.apps_save.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(400),
            move || {
                if let Some(ui) = this.upgrade() {
                    config::save_apps(&ui.core.borrow().apps);
                }
            },
        );
    }

    fn save_apps_now(self: &Rc<Self>) {
        self.apps_save.stop();
        config::save_apps(&self.core.borrow().apps);
    }

    /// Selected-app index in the editor (-1 when none / editor closed).
    fn selected_app(&self) -> i32 {
        self.settings_handle()
            .map(|w| w.get_selected_app())
            .unwrap_or(-1)
    }

    fn set_selected_app(&self, i: i32) {
        if let Some(w) = self.settings_handle() {
            w.set_selected_app(i);
        }
    }

    fn wire_editor(ui: &Rc<Self>, w: &SettingsWindow) {

        // picker filters run in Rust: re-push the models on every keystroke
        let this = Rc::downgrade(ui);
        w.on_pick_query(move |q| {
            if let Some(ui) = this.upgrade() {
                let rows = Self::installed_rows(&ui.core.borrow(), &q);
                if let Some(sw) = ui.settings_handle() {
                    sw.set_installed(ModelRc::new(VecModel::from(rows)));
                }
            }
        });
        let this = Rc::downgrade(ui);
        w.on_icon_query(move |q| {
            if let Some(ui) = this.upgrade() {
                let rows = Self::icon_pick_rows(&ui.core.borrow(), &q);
                if let Some(sw) = ui.settings_handle() {
                    sw.set_icon_picks(ModelRc::new(VecModel::from(rows)));
                }
            }
        });

        // color picker: seed HSV from the target's current hex, then route
        // live picks back through the same paths the swatches use
        let this = Rc::downgrade(ui);
        w.on_open_picker(move |target, current| {
            if let Some(ui) = this.upgrade() {
                let Some(sw) = ui.settings_handle() else { return };
                let seed = Rgba::parse(&current).unwrap_or(Rgba::rgb(0xe4, 0x48, 0x54));
                let (h, s, v, a) = seed.to_hsv();
                sw.set_picker_h(h);
                sw.set_picker_s(s);
                sw.set_picker_v(v);
                sw.set_picker_a(a);
                sw.set_picker_for(target);
            }
        });
        let this = Rc::downgrade(ui);
        w.on_color_picked(move |target, h, s, v, a| {
            if let Some(ui) = this.upgrade() {
                let Some(sw) = ui.settings_handle() else { return };
                let hex: SharedString = Rgba::from_hsv(h, s, v, a).to_hex().into();
                if let Some(idx) = target.strip_prefix("app:") {
                    if let Ok(i) = idx.parse::<i32>() {
                        sw.invoke_set_app_field(i, "color".into(), hex);
                    }
                } else if let Some(id) = target.strip_prefix("act:") {
                    sw.invoke_set_custom_field(id.into(), "color".into(), hex);
                } else {
                    sw.invoke_set_setting(target, hex);
                }
            }
        });

        let this = Rc::downgrade(ui);
        w.on_select_app(move |i| {
            if let Some(ui) = this.upgrade() {
                ui.set_selected_app(i);
                ui.refresh_selected_actions();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_add_app(move || {
            if let Some(ui) = this.upgrade() {
                ui.core.borrow_mut().apps.push(AppEntry {
                    name: "New App".into(),
                    icon: "application-x-executable".into(),
                    exec: vec![String::new()],
                    ..Default::default()
                });
                ui.save_apps_now();
                ui.refresh_editor_models();
                ui.refresh_preview();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_add_installed(move |i| {
            if let Some(ui) = this.upgrade() {
                let entry = ui.core.borrow().index.installed().get(i as usize).cloned();
                if let Some(d) = entry {
                    ui.core.borrow_mut().apps.push(AppEntry {
                        name: d.name.clone(),
                        icon: if d.icon.is_empty() {
                            "application-x-executable".into()
                        } else {
                            d.icon.clone()
                        },
                        exec: if d.exec.is_empty() {
                            vec![d.id.clone()]
                        } else {
                            d.exec.clone()
                        },
                        wm_class: if d.startup_wm_class.is_empty() {
                            d.id.clone()
                        } else {
                            d.startup_wm_class.clone()
                        },
                        ..Default::default()
                    });
                    ui.save_apps_now();
                    ui.refresh_editor_models();
                    ui.refresh_preview();
                }
            }
        });
        let this = Rc::downgrade(ui);
        w.on_remove_app(move |i| {
            if let Some(ui) = this.upgrade() {
                let mut core = ui.core.borrow_mut();
                if (i as usize) < core.apps.len() {
                    core.apps.remove(i as usize);
                }
                drop(core);
                ui.set_selected_app(-1);
                ui.save_apps_now();
                ui.refresh_editor_models();
                ui.refresh_preview();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_move_app(move |i, dir| {
            if let Some(ui) = this.upgrade() {
                let mut core = ui.core.borrow_mut();
                let j = i + dir;
                if i >= 0 && j >= 0 && (i as usize) < core.apps.len() && (j as usize) < core.apps.len()
                {
                    core.apps.swap(i as usize, j as usize);
                }
                drop(core);
                ui.set_selected_app(i + dir);
                ui.save_apps_now();
                ui.refresh_editor_models();
                ui.refresh_preview();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_set_app_field(move |i, field, value| {
            if let Some(ui) = this.upgrade() {
                {
                    let mut core = ui.core.borrow_mut();
                    if let Some(a) = core.apps.get_mut(i as usize) {
                        let v = value.to_string();
                        match field.as_str() {
                            "name" => a.name = v,
                            "icon" => a.icon = v,
                            "exec" => {
                                a.exec = v.split_whitespace().map(str::to_owned).collect()
                            }
                            "wmClass" => a.wm_class = v,
                            "color" => a.color = v,
                            _ => {}
                        }
                    }
                }
                ui.save_apps_debounced();
                ui.refresh_preview();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_toggle_action(move |id, on| {
            if let Some(ui) = this.upgrade() {
                let sel = ui.selected_app();
                {
                    let mut core = ui.core.borrow_mut();
                    // materialize the full id list on first toggle (spec §6)
                    let all_ids: Vec<String> = {
                        let app = match core.apps.get(sel.max(0) as usize) {
                            Some(a) if sel >= 0 => a.clone(),
                            _ => return,
                        };
                        let focused = FocusedApp {
                            wm_class: app.wm_class.clone(),
                            custom_actions: app.custom_actions.clone(),
                            action_ids: None,
                            ..Default::default()
                        };
                        let exec0 = app.exec.first().cloned().unwrap_or_default();
                        core.action_templates(&focused, &exec0)
                            .into_iter()
                            .filter(|t| !matches!(t.kind, crate::ring::ActionKind::Keys(_)))
                            .map(|t| t.id)
                            .collect()
                    };
                    if let Some(app) = core.apps.get_mut(sel as usize) {
                        let ids = app.action_ids.get_or_insert_with(|| all_ids.clone());
                        let id = id.to_string();
                        if on {
                            if !ids.contains(&id) {
                                ids.push(id);
                            }
                        } else {
                            ids.retain(|x| x != &id);
                        }
                    }
                }
                ui.save_apps_now();
                ui.refresh_selected_actions();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_add_custom_action(move || {
            if let Some(ui) = this.upgrade() {
                let sel = ui.selected_app();
                if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel.max(0) as usize) {
                    if sel >= 0 {
                        app.custom_actions.push(CustomAction::default());
                    }
                }
                ui.save_apps_now();
                ui.refresh_selected_actions();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_remove_custom_action(move |id| {
            if let Some(ui) = this.upgrade() {
                let sel = ui.selected_app();
                if let Some(k) = id.strip_prefix("c:").and_then(|k| k.parse::<usize>().ok()) {
                    if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel.max(0) as usize) {
                        if sel >= 0 && k < app.custom_actions.len() {
                            app.custom_actions.remove(k);
                        }
                    }
                }
                ui.save_apps_now();
                ui.refresh_selected_actions();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_set_custom_field(move |id, field, value| {
            if let Some(ui) = this.upgrade() {
                let sel = ui.selected_app();
                if let Some(k) = id.strip_prefix("c:").and_then(|k| k.parse::<usize>().ok()) {
                    if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel.max(0) as usize) {
                        if sel >= 0 {
                            if let Some(c) = app.custom_actions.get_mut(k) {
                                let v = value.to_string();
                                match field.as_str() {
                                    "label" => c.label = v,
                                    "shortcut" => c.shortcut = v,
                                    "icon" => c.icon = v,
                                    "color" => c.color = v,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                ui.save_apps_debounced();
                ui.refresh_selected_actions();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_set_setting(move |key, value| {
            if let Some(ui) = this.upgrade() {
                let v = value.to_string();
                {
                    let mut core = ui.core.borrow_mut();
                    let s = &mut core.settings;
                    match key.as_str() {
                        "accent" => s.accent = v,
                        "bg" => s.bg = v,
                        "iconSize" => s.icon_size = v.parse().unwrap_or(s.icon_size),
                        "ringRadius" => s.ring_radius = v.parse().unwrap_or(s.ring_radius),
                        "dim" => s.dim = v.parse().unwrap_or(s.dim),
                        "wheelOpacity" => s.wheel_opacity = v.parse().unwrap_or(s.wheel_opacity),
                        "sectorRadius" => s.sector_radius = v.parse().unwrap_or(s.sector_radius),
                        "segRadius" => s.seg_radius = v.parse().unwrap_or(s.seg_radius),
                        "sectionInset" => s.section_inset = v.parse().unwrap_or(s.section_inset),
                        "segGap" => s.seg_gap = v.parse().unwrap_or(s.seg_gap),
                        "segBg" => s.seg_bg = v,
                        "showLabels" => s.show_labels = v == "true",
                        "followOutside" => s.follow_outside = v == "true",
                        "theme" => s.theme = v,
                        "shortcutsEnabled" => s.shortcuts_enabled = v == "true",
                        "persistBinds" => s.persist_binds = v == "true",
                        _ => log::warn!("set-setting: unknown key {key}"),
                    }
                }
                ui.save_settings_debounced();
                if matches!(key.as_str(), "shortcutsEnabled" | "persistBinds") {
                    let core = ui.core.borrow();
                    shortcuts::apply_shortcuts(&core.settings, core.comp.can_manage_keybinds());
                }
                ui.sync_skin();
                ui.sync_geometry();
                ui.refresh_editor_models();
                ui.refresh_preview();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_set_shortcut(move |mode, combo| {
            if let Some(ui) = this.upgrade() {
                let old;
                {
                    let mut core = ui.core.borrow_mut();
                    let slot = match mode.as_str() {
                        "windows" => &mut core.settings.shortcuts.windows,
                        "actions" => &mut core.settings.shortcuts.actions,
                        _ => &mut core.settings.shortcuts.apps,
                    };
                    old = slot.clone();
                    *slot = combo.to_string();
                }
                ui.save_settings_debounced();
                let core = ui.core.borrow();
                shortcuts::update_shortcut(
                    &core.settings,
                    core.comp.can_manage_keybinds(),
                    &mode,
                    &old,
                );
                drop(core);
                ui.refresh_editor_models();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_save_theme(move |name| {
            if let Some(ui) = this.upgrade() {
                // Snapshot the current effective skin (incl. live bg/accent)
                let result = crate::theme::save_theme(&name, &ui.skin.borrow());
                match result {
                    Ok(clean) => {
                        ui.core.borrow_mut().settings.theme = clean;
                        config::save_settings(&ui.core.borrow().settings);
                        ui.sync_all();
                    }
                    Err(e) => log::warn!("save theme: {e}"),
                }
            }
        });

        let this = Rc::downgrade(ui);
        w.on_reset_settings(move || {
            if let Some(ui) = this.upgrade() {
                ui.core.borrow_mut().settings = crate::config::Settings::default();
                config::save_settings(&ui.core.borrow().settings);
                let core = ui.core.borrow();
                shortcuts::apply_shortcuts(&core.settings, core.comp.can_manage_keybinds());
                drop(core);
                ui.sync_all();
            }
        });
        let this = Rc::downgrade(ui);
        w.on_close_requested(move || {
            if let Some(ui) = this.upgrade() {
                ui.settings_save.stop();
                config::save_settings(&ui.core.borrow().settings);
                ui.save_apps_now();
                if let Some(w) = ui.settings_win.borrow_mut().take() {
                    w.hide().ok();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    fn default_geo() -> Geometry {
        Geometry::compute(&Settings::default(), &SkinData::default(), 1.0)
    }

    #[test]
    fn geometry_matches_spec_defaults() {
        let g = default_geo();
        // spec: ringR 165, iconBox 59 (rounded s()), outerR 221.64, innerR 111.9(ish)
        assert_eq!(g.ring_r, 165.0);
        assert_eq!(g.icon_box, 59.0);
        assert!((g.outer_r - 221.64).abs() < 0.01, "outer {}", g.outer_r);
        assert!((g.inner_r - 111.9).abs() < 0.01, "inner {}", g.inner_r);
    }

    #[test]
    fn hit_testing_nearest_center() {
        let g = default_geo();
        let cx = g.outer_r;
        let cy = g.outer_r;
        // straight up on the ring = slice 0
        assert_eq!(g.slice_at(cx, cy - g.ring_r, 8, false), 0);
        // straight right = quarter turn clockwise = slice 2 of 8
        assert_eq!(g.slice_at(cx + g.ring_r, cy, 8, false), 2);
        // straight down = slice 4
        assert_eq!(g.slice_at(cx, cy + g.ring_r, 8, false), 4);
        // nearest-center rounding: 20° past up with 8 slices (45° each) still slice 0
        let a = -PI / 2.0 + 20f32.to_radians();
        assert_eq!(
            g.slice_at(cx + g.ring_r * a.cos(), cy + g.ring_r * a.sin(), 8, false),
            0
        );
        // 25° -> rounds to slice 1
        let a = -PI / 2.0 + 25f32.to_radians();
        assert_eq!(
            g.slice_at(cx + g.ring_r * a.cos(), cy + g.ring_r * a.sin(), 8, false),
            1
        );
        // hole and outside
        assert_eq!(g.slice_at(cx, cy, 8, false), -1);
        assert_eq!(g.slice_at(cx, cy - g.outer_r - 50.0, 8, false), -1);
        // follow-outside accepts beyond the rim
        assert_eq!(g.slice_at(cx, cy - g.outer_r - 50.0, 8, true), 0);
        // empty ring never panics
        assert_eq!(g.slice_at(cx, cy - g.ring_r, 0, false), -1);
    }

    #[test]
    fn rotation_takes_shortest_path() {
        // 8 slices, at slice 0 (0deg), going to slice 7 should go -45, not +315
        let d = rotation_delta(0.0, 7, 8);
        assert_eq!(d, -45.0);
        let d = rotation_delta(0.0, 1, 8);
        assert_eq!(d, 45.0);
        // accumulated rotation normalizes correctly
        let d = rotation_delta(720.0, 1, 8);
        assert_eq!(d, 45.0);
        // 180° tie resolves to -180 (matches the QML formula's JS % semantics)
        let d = rotation_delta(0.0, 4, 8);
        assert_eq!(d, -180.0);
    }

    #[test]
    fn title_truncation() {
        let long = "x".repeat(60);
        let t = truncate_title(&long);
        assert_eq!(t.chars().count(), 43); // 42 + ellipsis
        assert_eq!(truncate_title("short"), "short");
    }
}
