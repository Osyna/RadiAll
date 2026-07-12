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

/// Wire a Slint callback to the `Ui`: holds a `Weak`, upgrades per call, and
/// silently no-ops once the Ui is torn down. `|ui, args…|` rebinds the
/// upgraded `Rc` for the body — the single pattern every callback shares.
macro_rules! wire {
    ($w:ident.$setter:ident, $ui:ident, |$this:ident $(, $arg:ident)*| $body:block) => {{
        let weak = Rc::downgrade($ui);
        $w.$setter(move |$($arg),*| {
            if let Some($this) = weak.upgrade() $body
        });
    }};
}

const CENTER_TIMER_MS: u64 = 2000; // hole hover -> settings button
const LONG_PRESS_MS: u64 = 400; // press-and-hold -> action arc
const CLOSE_FADE_MS: u64 = 230; // window stays mapped through fade-out
const ACTIVATE_DELAY_MS: u64 = 120; // unmap-refocus settle before dispatch
// (window-title cleaning lives in ring.rs beside the model that uses it)

// ------------------------------------------------------------- geometry

/// Menu shape. Radial and Half share all angular math (Half is a radial
/// wheel whose center sits ON a screen edge — the hidden hemisphere is
/// simply clipped); Bar is linear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LayoutKind {
    #[default]
    Radial,
    Bar,
    Half,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LayoutPos {
    #[default]
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

pub fn parse_layout(layout: &str, pos: &str) -> (LayoutKind, LayoutPos) {
    let kind = match layout {
        "bar" => LayoutKind::Bar,
        "half" => LayoutKind::Half,
        _ => LayoutKind::Radial,
    };
    let mut pos = match pos {
        "left" => LayoutPos::Left,
        "right" => LayoutPos::Right,
        "top" => LayoutPos::Top,
        "bottom" => LayoutPos::Bottom,
        _ => LayoutPos::Center,
    };
    // a half ring needs an edge to sit on
    if kind == LayoutKind::Half && pos == LayoutPos::Center {
        pos = LayoutPos::Bottom;
    }
    (kind, pos)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Geometry {
    pub kind: LayoutKind,
    pub pos: LayoutPos,
    pub outer_r: f32,
    pub inner_r: f32,
    pub ring_r: f32,
    pub icon_box: f32,
    /// Angular window (radial: full turn from the top; half: the visible
    /// hemisphere). a0 = start edge of slice 0, radians, screen coords.
    pub a0: f32,
    pub span: f32,
    // bar layout (lengths depend on the item count, computed per rebuild)
    pub bar_pitch: f32,
    pub bar_thick: f32,
    pub bar_pad: f32,
    // action arc (spec §5)
    pub arc_radius: f32,
    pub arc_btn_r: f32,
    pub arc_band_inner: f32,
    pub arc_band_outer: f32,
}

impl Geometry {
    pub fn compute(settings: &crate::config::Settings, skin: &SkinData, ui_scale: f32) -> Self {
        let s = |px: f32| skin.s(px);
        let (kind, pos) = parse_layout(&settings.layout, &settings.layout_pos);
        let ring_r = s(settings.ring_radius) * ui_scale;
        let icon_box = s(settings.icon_size) * ui_scale;
        let outer_r = ring_r + icon_box * 0.96;
        let inner_r = (s(settings.hole_size) * ui_scale).max(ring_r - icon_box * 0.90);
        let gap = s(10.0);
        let btn_r = s(23.0);
        let arc_radius = outer_r + gap + btn_r;
        // angular window: radial = full turn, slice 0 centered at the top;
        // half = the hemisphere facing away from the anchored edge
        let (a0, span) = match kind {
            LayoutKind::Half => (
                match pos {
                    LayoutPos::Top => 0.0,           // spreads downward
                    LayoutPos::Left => -PI / 2.0,    // spreads rightward
                    LayoutPos::Right => PI / 2.0,    // spreads leftward
                    _ => PI,                         // bottom: spreads upward
                },
                PI,
            ),
            _ => (0.0, TAU), // a0 set per-count in slice_at/angle_of (top-centered)
        };
        Self {
            kind,
            pos,
            outer_r,
            inner_r,
            ring_r,
            icon_box,
            a0,
            span,
            bar_pitch: icon_box + s(18.0),
            bar_thick: icon_box + s(26.0),
            bar_pad: s(16.0),
            arc_radius,
            arc_btn_r: btn_r,
            arc_band_inner: outer_r + s(3.0),
            arc_band_outer: arc_radius + btn_r + s(8.0),
        }
    }

    /// True when the bar runs vertically (anchored to a side edge).
    pub fn bar_vertical(&self) -> bool {
        matches!(self.pos, LayoutPos::Left | LayoutPos::Right)
    }

    /// Bar length along its axis for `count` items.
    pub fn bar_len(&self, count: usize) -> f32 {
        count as f32 * self.bar_pitch + 2.0 * self.bar_pad
    }

    /// The menu's bounding box inside a `w`×`h` window. Radial: centered
    /// square. Half: 2r square whose CENTER sits on the anchored edge's
    /// midpoint. Bar: rect at the chosen position (margin s-scaled via pad).
    pub fn menu_origin(&self, w: f32, h: f32, count: usize) -> (f32, f32) {
        let m = self.bar_pad * 1.5; // edge margin for bars
        match self.kind {
            LayoutKind::Radial => (w / 2.0 - self.outer_r, h / 2.0 - self.outer_r),
            LayoutKind::Half => match self.pos {
                LayoutPos::Top => (w / 2.0 - self.outer_r, -self.outer_r),
                LayoutPos::Left => (-self.outer_r, h / 2.0 - self.outer_r),
                LayoutPos::Right => (w - self.outer_r, h / 2.0 - self.outer_r),
                _ => (w / 2.0 - self.outer_r, h - self.outer_r),
            },
            LayoutKind::Bar => {
                let len = self.bar_len(count);
                let (bw, bh) = if self.bar_vertical() {
                    (self.bar_thick, len)
                } else {
                    (len, self.bar_thick)
                };
                match self.pos {
                    LayoutPos::Left => (m, h / 2.0 - bh / 2.0),
                    LayoutPos::Right => (w - m - bw, h / 2.0 - bh / 2.0),
                    LayoutPos::Top => (w / 2.0 - bw / 2.0, m),
                    LayoutPos::Bottom => (w / 2.0 - bw / 2.0, h - m - bh),
                    LayoutPos::Center => (w / 2.0 - bw / 2.0, h / 2.0 - bh / 2.0),
                }
            }
        }
    }

    fn dist(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.outer_r;
        let dy = y - self.outer_r;
        (dx * dx + dy * dy).sqrt()
    }

    /// The settings disc lives in the radial hole only.
    pub fn in_hole(&self, x: f32, y: f32) -> bool {
        match self.kind {
            LayoutKind::Bar => false,
            _ => self.dist(x, y) < self.inner_r,
        }
    }

    /// Nearest-center slice hit test (spec §3), local coords relative to
    /// menu_origin. -1 = hole / outside.
    pub fn slice_at(&self, x: f32, y: f32, count: usize, follow_outside: bool) -> i32 {
        if count == 0 {
            return -1;
        }
        if self.kind == LayoutKind::Bar {
            let len = self.bar_len(count);
            let (along, across, thick) = if self.bar_vertical() {
                (y, x - self.bar_thick / 2.0, self.bar_thick)
            } else {
                (x, y - self.bar_thick / 2.0, self.bar_thick)
            };
            if !follow_outside && (across.abs() > thick / 2.0 || !(0.0..=len).contains(&along)) {
                return -1;
            }
            let idx = ((along - self.bar_pad) / self.bar_pitch).floor() as i64;
            return idx.clamp(0, count as i64 - 1) as i32;
        }
        let d = self.dist(x, y);
        if d < self.inner_r {
            return -1;
        }
        if d > self.outer_r && !follow_outside {
            return -1;
        }
        let a = (y - self.outer_r).atan2(x - self.outer_r);
        let seg = self.span / count as f32;
        if self.kind == LayoutKind::Half {
            // slices tile [a0, a0+span]; no wrap — clamp to the span ends
            let rel = (a - self.a0).rem_euclid(TAU);
            if rel > self.span && !follow_outside {
                return -1;
            }
            let rel = if rel > self.span {
                // outside the hemisphere with follow-outside: nearest end.
                // Distance past the span end = rel - span; distance to the
                // start (wrapping the rest of the circle) = TAU - rel.
                if rel - self.span < TAU - rel { self.span } else { 0.0 }
            } else {
                rel
            };
            return ((rel / seg - 0.5).round() as i64).clamp(0, count as i64 - 1) as i32;
        }
        // radial: slice 0 centered at the top, indices clockwise
        let idx = ((a + PI / 2.0) / seg).round() as i64;
        idx.rem_euclid(count as i64) as i32
    }

    /// Center angle of slice `i` (radians) — mirrors the slint positioning.
    pub fn angle_of(&self, i: usize, count: usize) -> f32 {
        let seg = self.span / count.max(1) as f32;
        match self.kind {
            LayoutKind::Half => self.a0 + (i as f32 + 0.5) * seg,
            _ => -PI / 2.0 + i as f32 * seg,
        }
    }

    /// Item center in menu-local coords — for arc placement off-radial.
    pub fn item_center(&self, i: usize, count: usize) -> (f32, f32) {
        if self.kind == LayoutKind::Bar {
            let along = self.bar_pad + (i as f32 + 0.5) * self.bar_pitch;
            return if self.bar_vertical() {
                (self.bar_thick / 2.0, along)
            } else {
                (along, self.bar_thick / 2.0)
            };
        }
        let a = self.angle_of(i, count);
        (
            self.outer_r + self.ring_r * a.cos(),
            self.outer_r + self.ring_r * a.sin(),
        )
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
        self.settings_win
            .borrow()
            .as_ref()
            .map(|w| w.clone_strong())
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
            if s.seg_bg.is_empty() {
                None
            } else {
                Some(&s.seg_bg)
            },
        );
        let light = band_lum(skin.bg) > 0.5;
        let cell_edge = skin.edge.a > 0;
        let (rim, rim_w) = if cell_edge {
            (skin.edge, skin.s(skin.edge_width))
        } else {
            let a = if light { 0.10 } else { 0.14 };
            let base = if light {
                Rgba::rgb(0, 0, 0)
            } else {
                Rgba::rgb(255, 255, 255)
            };
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
        g.set_show_dots(core.settings.show_dots);
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

    fn layout_ints(geo: &Geometry) -> (i32, i32) {
        let kind = match geo.kind {
            LayoutKind::Radial => 0,
            LayoutKind::Bar => 1,
            LayoutKind::Half => 2,
        };
        let pos = match geo.pos {
            LayoutPos::Center => 0,
            LayoutPos::Left => 1,
            LayoutPos::Right => 2,
            LayoutPos::Top => 3,
            LayoutPos::Bottom => 4,
        };
        (kind, pos)
    }

    fn push_geometry(&self, w: &RingWindow) {
        let geo = *self.geo.borrow();
        w.set_outer_r(geo.outer_r);
        w.set_inner_r(geo.inner_r);
        w.set_ring_r(geo.ring_r);
        w.set_icon_box(geo.icon_box);
        let (kind, pos) = Self::layout_ints(&geo);
        w.set_menu_layout(kind);
        w.set_menu_pos(pos);
        w.set_a0_deg(geo.a0.to_degrees());
        w.set_bar_pitch(geo.bar_pitch);
        w.set_bar_thick(geo.bar_thick);
        w.set_bar_pad(geo.bar_pad);
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
        let (kind, pos) = Self::layout_ints(&pv);
        w.set_pv_layout(kind);
        w.set_pv_pos(pos);
        w.set_pv_a0_deg(pv.a0.to_degrees());
        w.set_pv_bar_pitch(pv.bar_pitch);
        w.set_pv_bar_thick(pv.bar_thick);
        w.set_pv_bar_pad(pv.bar_pad);
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
            glyph: if is_action {
                e.glyph.clone().into()
            } else {
                SharedString::default()
            },
            monogram: e
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default()
                .into(),
            // window dots + selection are app/window concepts; actions all
            // inherit the app's wm_class and would show identical noise
            dot_count: if is_action {
                0
            } else {
                win_count.min(6) as i32
            },
            sel_dot: if is_action { -1 } else { sel as i32 },
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
        let Some(win) = self.ring_handle() else {
            return;
        };
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
        let Some(win) = self.ring_handle() else {
            return;
        };
        let count = self.ring.borrow().len();
        let cur = win.get_sector_rotation();
        // radial wraps (shortest path around the circle); half rings span a
        // fixed 180° window, so the wedge tracks the slot directly
        let geo = *self.geo.borrow();
        let delta = if geo.kind == LayoutKind::Half {
            let seg = geo.span.to_degrees() / count.max(1) as f32;
            idx as f32 * seg - cur
        } else {
            rotation_delta(cur, idx, count)
        };
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
        let Some(win) = self.ring_handle() else {
            return;
        };
        let ring = self.ring.borrow();
        let core = self.core.borrow();
        let idx = if hovered >= 0 {
            hovered as usize
        } else {
            win.get_active_index().max(0) as usize
        };
        let label = match ring.get(idx) {
            Some(e) => {
                // actions share the app's wm_class: the "· title i/N" window
                // suffix belongs to app/window slices only
                let ws = if e.is_action() {
                    Vec::new()
                } else {
                    core.windows_for(e)
                };
                if ws.len() > 1 {
                    let sel = core.selected_window_index(e).max(0) as usize;
                    let title = ws.get(sel).map(|w| w.title.as_str()).unwrap_or("");
                    format!(
                        "{}  ·  {}   {}/{}",
                        e.name,
                        crate::ring::clean_title(title, &e.name),
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
        // A reopen during the close fade is a ring SWITCH, not a fresh open:
        // the overlay is still mapped and still holds focus, so a "fresh"
        // snapshot here would capture our own dying window (empty class) as
        // the actions target -> empty actions ring on the next switch.
        let was_open = *self.open.borrow() || self.hide_timer.running();
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
            // Freeze the actions target on a FRESH open only — the overlay
            // steals focus once mapped, so re-snapshotting while switching
            // rings in-session would capture our own window (empty target).
            if !was_open {
                core.actions_target = core.focused_app();
            }
        }
        self.rebuild_ring();
        *self.open.borrow_mut() = true;
        // Pre-size to the focused output BEFORE mapping: the first committed
        // frame is otherwise the preferred-size window at the compositor's
        // default spot, flashing a mis-placed wheel until the float/resize
        // window rules land.
        let output = self.core.borrow_mut().comp.output_size();
        if let Some((w, h)) = output {
            win.window().set_size(slint::PhysicalSize::new(w, h));
        }
        // Overlay via compositor rules (float + 100% size) when available;
        // real fullscreen makes compositors black out the windows behind.
        if !self.core.borrow().overlay_ready {
            win.window().set_fullscreen(true);
        }
        if let Err(e) = win.show() {
            log::warn!("ring window show failed: {e}");
        }
        // Start the open animation AFTER the map: set before show(), the
        // first frame would render at full opacity instead of fading in.
        win.set_open(true);
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
        let Some(win) = self.ring_handle() else {
            return;
        };
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
        // The 24k-glyph icon index is picker-only: scan it on first settings
        // open instead of at daemon startup (saves ~4 MB and a 24k-file walk
        // for daemons that never open the editor).
        {
            let mut core = self.core.borrow_mut();
            if core.icons.is_none() {
                core.icons = Some(crate::icons::IconLib::scan());
            }
        }
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
        // map window coords into the menu box (position depends on layout)
        let size = win.window().size();
        let sf = win.window().scale_factor();
        let (w, h) = (size.width as f32 / sf, size.height as f32 / sf);
        let geo = self.geo.borrow();
        let count = self.ring.borrow().len();
        let (ox, oy) = geo.menu_origin(w, h, count);
        (x - ox, y - oy)
    }

    fn on_moved(self: &Rc<Self>, x: f32, y: f32) {
        log::trace!("pointer moved {x},{y}");
        if self.arc.borrow().is_some() {
            return; // arc handles its own hover via its buttons
        }
        let Some(win) = self.ring_handle() else {
            return;
        };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();
        let count = self.ring.borrow().len();
        let follow = self.core.borrow().settings.follow_outside;
        if geo.in_hole(lx, ly) {
            win.set_hovered_index(-1);
            self.update_center_label(-1);
            if geo.kind == LayoutKind::Radial
                && !win.get_show_settings_btn()
                && !self.center_timer.running()
            {
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
        let Some(win) = self.ring_handle() else {
            return;
        };
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
        let Some(win) = self.ring_handle() else {
            return;
        };
        let (lx, ly) = self.wheel_local(&win, x, y);
        let geo = *self.geo.borrow();

        // arc open: empty-arc click returns to the wheel; outside closes all
        if self.arc.borrow().is_some() {
            let d = geo.dist(lx, ly);
            log::debug!(
                "on_up with arc open: d={d} band_outer={}",
                geo.arc_band_outer
            );
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
        let Some(win) = self.ring_handle() else {
            return;
        };
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
            custom_actions: app_cfg
                .map(|a| a.custom_actions.clone())
                .unwrap_or_default(),
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
            has_window
                || !matches!(
                    t.kind,
                    crate::ring::ActionKind::Close
                        | crate::ring::ActionKind::Float
                        | crate::ring::ActionKind::Fullscreen
                )
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
        let Some(win) = self.ring_handle() else {
            return;
        };
        // anchor: radial arcs share the wheel center; bar/half arcs pop
        // around the pressed item itself
        let geo = *self.geo.borrow();
        let size = win.window().size();
        let sf = win.window().scale_factor();
        let (w, h) = (size.width as f32 / sf, size.height as f32 / sf);
        let count = self.ring.borrow().len();
        let (ox, oy) = geo.menu_origin(w, h, count);
        let (cx, cy) = if geo.kind == LayoutKind::Radial {
            (w / 2.0, h / 2.0)
        } else {
            let (ix, iy) = geo.item_center(slice, count);
            (ox + ix, oy + iy)
        };
        win.set_arc_cx(cx);
        win.set_arc_cy(cy);
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
        wire!(w.on_pointer_moved, ui, |ui, x, y| {
            ui.on_moved(x, y);
        });
        wire!(w.on_pointer_down, ui, |ui, x, y, right| {
            ui.on_down(x, y, right);
        });
        wire!(w.on_pointer_up, ui, |ui, x, y, right| {
            ui.on_up(x, y, right);
        });
        wire!(w.on_scrolled, ui, |ui, x, y, d| {
            ui.on_scroll(x, y, d);
        });
        wire!(w.on_esc_pressed, ui, |ui| {
            ui.on_esc();
        });
        wire!(w.on_arc_clicked, ui, |ui, i| {
            ui.on_arc_clicked(i);
        });
        // logo for the center settings button
        let logo =
            apps::load_icon_pixels_from_bytes(include_bytes!("../launcher/RadiAll.png"), 128);
        if let Some(buf) = logo {
            w.set_logo(slint::Image::from_rgba8(buf));
        }
    }

    // ------------------------------------------------------- editor

    fn refresh_preview(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else {
            return;
        };
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

    /// Icon picker rows for `query`: installed-app THEME icons first (their
    /// value = the portable icon NAME), then the glyph SVG library (value =
    /// absolute path). Capped for search-as-you-type.
    fn icon_pick_rows(core: &Core, query: &str) -> Vec<IconPick> {
        let q = query.trim().to_lowercase();
        let mut rows: Vec<IconPick> = core
            .index
            .installed()
            .iter()
            .filter(|d| !d.icon.is_empty())
            .filter(|d| {
                q.is_empty()
                    || d.name.to_lowercase().contains(&q)
                    || d.icon.to_lowercase().contains(&q)
            })
            .take(40)
            .filter_map(|d| {
                let (icon, has) = Self::load_image(&d.icon, 24);
                has.then(|| IconPick {
                    name: d.name.clone().into(),
                    set: "apps".into(),
                    path: d.icon.clone().into(), // theme icon NAME, portable
                    icon,
                })
            })
            .collect();
        let lib = core.icons.as_ref();
        rows.extend(lib.into_iter().flat_map(|l| l.search(query, 200)).map(|e| {
            let path = e.path.to_string_lossy().to_string();
            let (icon, _) = Self::load_image(&path, 24);
            IconPick {
                name: e.name.clone().into(),
                set: e.set.clone().into(),
                path: path.into(),
                icon,
            }
        }));
        rows.truncate(220);
        rows
    }

    fn refresh_editor_models(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else {
            return;
        };
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
        sw.set_icon_lib_count(core.icons.as_ref().map_or(0, |l| l.len()) as i32);
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
        sw.set_hole_size(s.hole_size);
        sw.set_show_dots(s.show_dots);
        sw.set_menu_layout_name(s.layout.clone().into());
        sw.set_menu_pos_name(s.layout_pos.clone().into());
        sw.set_shortcuts_enabled(s.shortcuts_enabled);
        sw.set_persist_binds(s.persist_binds);
        sw.set_sc_apps(s.shortcuts.apps.clone().into());
        sw.set_sc_windows(s.shortcuts.windows.clone().into());
        sw.set_sc_actions(s.shortcuts.actions.clone().into());
        sw.set_keybind_backend(
            core.shortcuts
                .as_ref()
                .map_or("none", |s| s.backend_name())
                .into(),
        );
        sw.set_can_send_keys(core.comp.can_send_keys());
        drop(core);
        self.refresh_selected_actions();
    }

    fn refresh_selected_actions(self: &Rc<Self>) {
        let Some(sw) = self.settings_handle() else {
            return;
        };
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
        // global defaults panel (shown when no app is selected; per-app
        // lists exclude g: rows — they're edited in one place only)
        let default_rows: Vec<ActionRow> = core
            .settings
            .default_actions
            .iter()
            .enumerate()
            .map(|(k, c)| ActionRow {
                id: format!("g:{k}").into(),
                label: c.label.clone().into(),
                glyph: crate::ring::G_KEY.into(),
                enabled: true,
                is_custom: true,
                shortcut: c.shortcut.clone().into(),
                icon_name: c.icon.clone().into(),
                color: c.color.clone().into(),
            })
            .collect();
        drop(core);
        sw.set_app_actions(ModelRc::new(VecModel::from(
            rows.into_iter()
                .filter(|r| !r.id.starts_with("g:"))
                .collect::<Vec<_>>(),
        )));
        sw.set_default_actions(ModelRc::new(VecModel::from(default_rows)));
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
        wire!(w.on_pick_query, ui, |ui, q| {
            let rows = Self::installed_rows(&ui.core.borrow(), &q);
            if let Some(sw) = ui.settings_handle() {
                sw.set_installed(ModelRc::new(VecModel::from(rows)));
            }
        });
        wire!(w.on_icon_query, ui, |ui, q| {
            let rows = Self::icon_pick_rows(&ui.core.borrow(), &q);
            if let Some(sw) = ui.settings_handle() {
                sw.set_icon_picks(ModelRc::new(VecModel::from(rows)));
            }
        });

        // color picker: seed HSV from the target's current hex, then route
        // live picks back through the same paths the swatches use
        wire!(w.on_open_picker, ui, |ui, target, current| {
            let Some(sw) = ui.settings_handle() else {
                return;
            };
            let seed = Rgba::parse(&current).unwrap_or(Rgba::rgb(0xe4, 0x48, 0x54));
            let (h, s, v, a) = seed.to_hsv();
            sw.set_picker_h(h);
            sw.set_picker_s(s);
            sw.set_picker_v(v);
            sw.set_picker_a(a);
            sw.set_picker_for(target);
        });
        wire!(w.on_color_picked, ui, |ui, target, h, s, v, a| {
            let Some(sw) = ui.settings_handle() else {
                return;
            };
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
        });

        // icon picker selection: same target scheme as the color picker
        wire!(w.on_icon_picked, ui, |ui, target, value| {
            let Some(sw) = ui.settings_handle() else {
                return;
            };
            if let Some(idx) = target.strip_prefix("app:") {
                if let Ok(i) = idx.parse::<i32>() {
                    sw.invoke_set_app_field(i, "icon".into(), value);
                }
            } else if let Some(id) = target.strip_prefix("act:") {
                sw.invoke_set_custom_field(id.into(), "icon".into(), value);
            }
            // the icon on the app list refreshes with the models
            ui.refresh_editor_models();
        });

        wire!(w.on_select_app, ui, |ui, i| {
            ui.set_selected_app(i);
            ui.refresh_selected_actions();
        });
        wire!(w.on_add_app, ui, |ui| {
            ui.core.borrow_mut().apps.push(AppEntry {
                name: "New App".into(),
                icon: "application-x-executable".into(),
                exec: vec![String::new()],
                ..Default::default()
            });
            ui.save_apps_now();
            ui.refresh_editor_models();
            ui.refresh_preview();
        });
        wire!(w.on_add_installed, ui, |ui, i| {
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
        });
        wire!(w.on_remove_app, ui, |ui, i| {
            let mut core = ui.core.borrow_mut();
            if (i as usize) < core.apps.len() {
                core.apps.remove(i as usize);
            }
            drop(core);
            ui.set_selected_app(-1);
            ui.save_apps_now();
            ui.refresh_editor_models();
            ui.refresh_preview();
        });
        wire!(w.on_move_app, ui, |ui, i, dir| {
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
        });
        wire!(w.on_reorder_app, ui, |ui, from, to| {
            {
                let mut core = ui.core.borrow_mut();
                let n = core.apps.len();
                let (from, to) = (from as usize, to as usize);
                if from < n && to < n && from != to {
                    let app = core.apps.remove(from);
                    core.apps.insert(to, app);
                }
            }
            ui.set_selected_app(to);
            ui.save_apps_now();
            ui.refresh_editor_models();
            ui.refresh_preview();
        });
        wire!(w.on_set_app_field, ui, |ui, i, field, value| {
            {
                let mut core = ui.core.borrow_mut();
                if let Some(a) = core.apps.get_mut(i as usize) {
                    let v = value.to_string();
                    match field.as_str() {
                        "name" => a.name = v,
                        "icon" => a.icon = v,
                        "exec" => a.exec = v.split_whitespace().map(str::to_owned).collect(),
                        "wmClass" => a.wm_class = v,
                        "color" => a.color = v,
                        _ => {}
                    }
                }
            }
            ui.save_apps_debounced();
            ui.refresh_preview();
        });
        wire!(w.on_toggle_action, ui, |ui, id, on| {
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
        });
        wire!(w.on_add_custom_action, ui, |ui| {
            let sel = ui.selected_app();
            if sel >= 0 {
                if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel as usize) {
                    app.custom_actions.push(CustomAction::default());
                }
                ui.save_apps_now();
            } else {
                // no selection = the global defaults panel
                ui.core
                    .borrow_mut()
                    .settings
                    .default_actions
                    .push(CustomAction::default());
                config::save_settings(&ui.core.borrow().settings);
            }
            ui.refresh_selected_actions();
        });
        wire!(w.on_remove_custom_action, ui, |ui, id| {
            if let Some(k) = id.strip_prefix("g:").and_then(|k| k.parse::<usize>().ok()) {
                {
                    let mut core = ui.core.borrow_mut();
                    if k < core.settings.default_actions.len() {
                        core.settings.default_actions.remove(k);
                    }
                }
                config::save_settings(&ui.core.borrow().settings);
            } else if let Some(k) = id.strip_prefix("c:").and_then(|k| k.parse::<usize>().ok()) {
                let sel = ui.selected_app();
                if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel.max(0) as usize) {
                    if sel >= 0 && k < app.custom_actions.len() {
                        app.custom_actions.remove(k);
                    }
                }
                ui.save_apps_now();
            }
            ui.refresh_selected_actions();
        });
        wire!(w.on_set_custom_field, ui, |ui, id, field, value| {
            let v = value.to_string();
            let apply = |c: &mut CustomAction| match field.as_str() {
                "label" => c.label = v.clone(),
                "shortcut" => c.shortcut = v.clone(),
                "icon" => c.icon = v.clone(),
                "color" => c.color = v.clone(),
                _ => {}
            };
            if let Some(k) = id.strip_prefix("g:").and_then(|k| k.parse::<usize>().ok()) {
                if let Some(c) = ui.core.borrow_mut().settings.default_actions.get_mut(k) {
                    apply(c);
                }
                ui.save_settings_debounced();
            } else if let Some(k) = id.strip_prefix("c:").and_then(|k| k.parse::<usize>().ok()) {
                let sel = ui.selected_app();
                if let Some(app) = ui.core.borrow_mut().apps.get_mut(sel.max(0) as usize) {
                    if sel >= 0 {
                        if let Some(c) = app.custom_actions.get_mut(k) {
                            apply(c);
                        }
                    }
                }
                ui.save_apps_debounced();
            }
            ui.refresh_selected_actions();
        });
        wire!(w.on_set_setting, ui, |ui, key, value| {
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
                    "holeSize" => s.hole_size = v.parse().unwrap_or(s.hole_size),
                    "showLabels" => s.show_labels = v == "true",
                    "showDots" => s.show_dots = v == "true",
                    "followOutside" => s.follow_outside = v == "true",
                    "layout" => s.layout = v,
                    "layoutPos" => s.layout_pos = v,
                    "theme" => s.theme = v,
                    "shortcutsEnabled" => s.shortcuts_enabled = v == "true",
                    "persistBinds" => s.persist_binds = v == "true",
                    _ => log::warn!("set-setting: unknown key {key}"),
                }
            }
            ui.save_settings_debounced();
            if matches!(key.as_str(), "shortcutsEnabled" | "persistBinds") {
                let core = ui.core.borrow();
                if let Some(sc) = &core.shortcuts {
                    sc.apply(&core.settings);
                }
            }
            ui.sync_skin();
            ui.sync_geometry();
            ui.refresh_editor_models();
            ui.refresh_preview();
        });
        wire!(w.on_set_shortcut, ui, |ui, mode, combo| {
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
            match &core.shortcuts {
                // hyprctl: precise unbind of the old combo, rebind the new
                Some(sc) if sc.kind() == shortcuts::ProviderKind::Hyprctl => {
                    shortcuts::update_shortcut(&core.settings, true, &mode, &old);
                }
                // portal / X11: providers re-sync the full set
                Some(sc) => sc.apply(&core.settings),
                None => {}
            }
            drop(core);
            ui.refresh_editor_models();
        });
        wire!(w.on_save_theme, ui, |ui, name| {
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
        });

        wire!(w.on_reset_settings, ui, |ui| {
            ui.core.borrow_mut().settings = crate::config::Settings::default();
            config::save_settings(&ui.core.borrow().settings);
            let core = ui.core.borrow();
            if let Some(sc) = &core.shortcuts {
                sc.apply(&core.settings);
            }
            drop(core);
            ui.sync_all();
        });
        wire!(w.on_close_requested, ui, |ui| {
            ui.settings_save.stop();
            config::save_settings(&ui.core.borrow().settings);
            ui.save_apps_now();
            if let Some(w) = ui.settings_win.borrow_mut().take() {
                w.hide().ok();
            }
            // Release the 24k-glyph index with the editor; reopening
            // re-scans (~80 ms) — cheaper than 4 MB parked forever.
            ui.core.borrow_mut().icons = None;
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

    fn geo_for(layout: &str, pos: &str) -> Geometry {
        let mut s = Settings::default();
        s.layout = layout.into();
        s.layout_pos = pos.into();
        Geometry::compute(&s, &SkinData::default(), 1.0)
    }

    #[test]
    fn layout_parsing_and_validation() {
        assert_eq!(parse_layout("bar", "left"), (LayoutKind::Bar, LayoutPos::Left));
        // half rings can't float mid-screen
        assert_eq!(parse_layout("half", "center"), (LayoutKind::Half, LayoutPos::Bottom));
        // unknown values degrade to the defaults
        assert_eq!(parse_layout("blob", "wat"), (LayoutKind::Radial, LayoutPos::Center));
    }

    #[test]
    fn bar_hit_testing() {
        let g = geo_for("bar", "center");
        let mid = g.bar_thick / 2.0;
        // slot centers along the bar
        for i in 0..4 {
            let x = g.bar_pad + (i as f32 + 0.5) * g.bar_pitch;
            assert_eq!(g.slice_at(x, mid, 4, false), i as i32, "slot {i}");
        }
        // outside the thickness -> miss; follow-outside -> nearest slot
        let x0 = g.bar_pad + 0.5 * g.bar_pitch;
        assert_eq!(g.slice_at(x0, -g.bar_thick, 4, false), -1);
        assert_eq!(g.slice_at(x0, -g.bar_thick, 4, true), 0);
        // beyond the end clamps with follow-outside
        assert_eq!(g.slice_at(g.bar_len(4) + 100.0, mid, 4, true), 3);
        // vertical bars project on y
        let v = geo_for("bar", "left");
        assert!(v.bar_vertical());
        let y2 = v.bar_pad + 2.5 * v.bar_pitch;
        assert_eq!(v.slice_at(v.bar_thick / 2.0, y2, 4, false), 2);
        // bars have no hole
        assert!(!v.in_hole(v.bar_thick / 2.0, y2));
    }

    #[test]
    fn half_ring_hit_testing() {
        // bottom-anchored: span [180°, 360°], slice centers at 202.5°...
        let g = geo_for("half", "bottom");
        let c = g.outer_r;
        for i in 0..4 {
            let a = g.angle_of(i, 4);
            let (x, y) = (c + g.ring_r * a.cos(), c + g.ring_r * a.sin());
            assert_eq!(g.slice_at(x, y, 4, false), i as i32, "slice {i}");
        }
        // the hidden hemisphere (below the anchor edge) is a miss...
        assert_eq!(g.slice_at(c, c + g.ring_r, 4, false), -1);
        // ...unless follow-outside clamps to the nearest span end
        let end = g.slice_at(c + g.ring_r * 0.1, c + g.ring_r, 4, true);
        assert_eq!(end, 3); // just past the right end of the span
        // hole still rejects
        assert_eq!(g.slice_at(c, c - g.inner_r * 0.5, 4, false), -1);
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
}
