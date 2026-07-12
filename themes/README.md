# RadiAll themes

A theme is one JSON file: `~/.config/radiall/themes/<name>.json`. Pick it in
Settings → Look → Theme, or set `"theme": "<name>"` in
`~/.config/radiall/settings.json`. **Edits to the active theme file apply
live** — no restart, no reload command.

Three ways to make one:

1. **Settings → Look → "Save as theme"** — snapshots every color of your
   current look into a new file. Hand-edit from there.
2. Copy a bundled theme and change what you want. A theme only needs the keys
   it wants to change; everything omitted falls back.
3. Start from another theme with inheritance:

```json
{
  "extends": "gruvbox",
  "accent": "#b8bb26",
  "sector": ""
}
```

`extends` applies the parent first (chains allowed), then your overrides.
Setting an auto key to `""` resets a parent's pin back to automatic.

## Color format

QML-style hex, alpha FIRST: `#rgb`, `#rrggbb`, or `#AArrggbb`
(`"#8000ff00"` = 50 % translucent green).

## Keys

### Core palette

| Key | Default | What it paints |
|---|---|---|
| `scale` | `1.1` | Global UI scale (every `s(px)` dimension) |
| `bg` | settings picker | The donut band |
| `accent` | settings picker | Hover wedge (via auto mix), selected dots, editor highlights |
| `fg` / `fgStrong` / `fgDim` | white-ish | Text tiers everywhere |
| `yellow` / `red` / `green` | apple-ish | Status colors (gauges, warnings) |
| `sep` | white 12 % | Separators in the editor |

### Wheel & overlay

| Key | Default | What it paints |
|---|---|---|
| `backdrop` | `#000000` | Full-screen dim layer behind the ring (alpha comes from the Dim slider) |
| `sector` | *auto* | Wedge fill. Auto = `accent` nudged 4 % toward white |
| `onBand` | *auto* | Glyph/monogram ink on the band. Auto = dark ink on light bands, white on dark |
| `dot` | *auto* | Unselected window dots. Auto like `onBand` |
| `labelFg` | *auto* | Center label pill text. Auto = `fgStrong` |
| `labelPillBg` | dark navy | Center label pill fill |
| `edge` | transparent | Ring outline. Transparent = subtle auto edge; a solid color turns on **cell-shading** (bold cartoon border, see `matrix`) |
| `edgeWidth` | `3` | Outline width in cell-shading mode |
| `segBg` | *auto* | Inactive-section fill. Auto = the Look-tab picker, default invisible (band shows through) |
| `settingsBtn` | white | The center settings disc (behind the radish) |

### Sections (geometry)

The wheel's sections are fully parametric, but geometry lives in
**settings** (like ring/icon size), tweakable live under Settings → Look →
Sections: **Active radius** (wedge corner rounding), **Inactive radius**,
**Edge padding** (radial inset from the band edges — 0 = flush), and
**Section gap** (angular padding between sections). Give inactive sections
a fill (`segBg` or the Inactive-fill picker) plus a gap for a segmented
wheel; set Wheel opacity to 0 % for a sections-only look. Cell-shading
themes (`edge` set) keep their sharp-cornered style.

### Action arc

| Key | Default | What it paints |
|---|---|---|
| `arcBg` | `#f7141418` | The half-ring band behind action buttons |
| `arcStroke` | white 8 % | Its outline |
| `arcBtn` | *auto* | Idle button fill. Auto = `fg` at 10 % |
| `arcBtnHover` | *auto* | Hovered button fill. Auto = `accent` |

### Editor / panels

| Key | Default | What it paints |
|---|---|---|
| `panelBg` | near-black | Settings window background |
| `glassBg` `glassHover` `pillBg` `pillHover` `btnHover` `btnActive` | white tints | Editor control chrome |

### Fonts

`font`, `fontDisplay`, `iconFont`, `monoFont` — family names; missing fonts
fall back to system defaults. `iconFont` renders the action glyphs (a Nerd
Font looks best).

*Auto* keys derive from `bg`/`accent`/`fg` at runtime, so a theme that only
sets `bg` + `accent` already looks coherent. Pin them when you want full
control; set them to `""` to un-pin.

## Per-app accents

Not a theme key, but part of the same look: each app in Settings → Apps can
get its own **Accent** — the hover wedge and selected window dot take that
color when the app has one.

## Bundled themes

`default`, `radiall` (brand: cream band, radish red, bold navy edge),
`catppuccin` (mocha), `nord`, `light`, `paper`, `dracula`, `gruvbox`,
`tokyo-night`, `rose-pine`, and `matrix` (green phosphor cell-shade —
a demo of `edge` + pinned autos).

Bundled themes are seeded into `~/.config/radiall/themes/` on first run and
never overwritten — they're yours to edit.
