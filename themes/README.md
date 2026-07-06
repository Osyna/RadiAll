# Themes

A theme is one JSON file in this folder. To make one:

1. `cp default.json mytheme.json`
2. Edit the values you care about. **You only need the keys you want to change** —
   anything you omit falls back to the built-in default (see `default.json` for the
   full list). `nord.json` is a 7-line example.
3. Activate it in **Launcher settings → Look → Theme** (every file in this folder shows
   up there automatically), or set `"theme": "mytheme"` in
   `~/.config/quickshell/radiall/launcher-settings.json`.

Edits to the **active** theme file apply **live** — no restart, no reload. Great for
tweaking colors and watching the ring update as you save.

## The ring (`bg` + `accent`)

`bg` (the ring's band) and `accent` (the highlight) are optional. If a theme **sets**
them, that theme fully owns the ring. If it **omits** them (like `default.json`), the
live **Look → Background / Accent** pickers drive the ring instead. The bundled
`radiall` / `paper` / `light` / `catppuccin` / `nord` themes set both so they look coherent.
(`radiall` is the brand theme — paper + radish-red, cell-shaded.)

## Colors

Colors are hex strings. Two forms:

- `"#RRGGBB"` — opaque, e.g. `"#0a84ff"`.
- `"#AARRGGBB"` — **alpha first**, e.g. `"#85161618"` is ~52% opaque. Most surface
  colors (`glassBg`, `pillBg`, `fg`, …) are translucent so the desktop shows through.

## Keys

| Key | What it colors |
|-----|----------------|
| `scale` | Global size multiplier (number, not a color). |
| `bg` | Ring band (optional — see above). |
| `accent` | Highlight / selection color (optional — see above). |
| `glassBg` / `glassHover` | Ring / bar glass background + hover. |
| `pillBg` / `pillHover` | Pill buttons. |
| `btnHover` / `btnActive` | Generic button states. |
| `fg` / `fgStrong` / `fgDim` | Text (normal / emphasized / muted). |
| `yellow` / `red` / `green` | Gauge tiers + status. |
| `sep` | Divider lines. |
| `edge` | Ring outline for a cell-shaded/cartoon border. Omit (or transparent) for the subtle auto edge; give a solid colour like `"#0c183c"` for a bold border. |
| `edgeWidth` | Outline thickness in px (default `3`). Only matters when `edge` is set. |
| `panelBg` | Settings / overlay panels. |
| `labelPillBg` | Wheel & arc name pills. |
| `font` / `fontDisplay` / `iconFont` / `monoFont` | Font families. |
