pragma Singleton
import Quickshell
import Quickshell.Io
import QtQuick
import "."

// The launcher's skin — every visual value the ring/settings read lives here, and
// every one can be overridden by a JSON theme file in themes/. Deliberately NOT named
// "Theme": if you drop this launcher into a shell that already has its own `Theme`
// singleton (a bar, panels…), `Skin` stays separate so theming the ring never touches
// the host shell's look.
//
// Make a theme:  copy themes/default.json → themes/mytheme.json, change some values,
//                then set "theme": "mytheme" in launcher-settings.json.
// A theme only needs the keys it wants to change; anything omitted falls back to the
// built-in defaults below. Edits to the active file apply live (no restart).
Singleton {
    id: root

    // active theme name — set launcher-settings.json "theme" to switch.
    readonly property string name: Launcher.settings.theme || "default"

    // built-in fallback: the original palette. This IS the "default" theme.
    readonly property var d: ({
        scale: 1.1,

        bg:           "#0b0b0d",   // wheel band; falls back to the Look→Background picker
        glassBg:      Qt.rgba(22/255, 22/255, 24/255, 0.52),
        glassHover:   Qt.rgba(1, 1, 1, 0.09),
        pillBg:       Qt.rgba(1, 1, 1, 0.15),
        pillHover:    Qt.rgba(1, 1, 1, 0.25),
        btnHover:     Qt.rgba(1, 1, 1, 0.09),
        btnActive:    Qt.rgba(1, 1, 1, 0.14),

        fg:       Qt.rgba(1, 1, 1, 0.88),
        fgStrong: Qt.rgba(1, 1, 1, 0.95),
        fgDim:    Qt.rgba(1, 1, 1, 0.45),

        accent: "#0a84ff",
        yellow: "#ffd60a",
        red:    "#ff453a",
        green:  "#30d158",

        sep: Qt.rgba(1, 1, 1, 0.12),

        // ring outline (cell-shading). Transparent = the old subtle auto edge; give a
        // theme a solid colour + width for a bold cartoon border around the wheel.
        edge:      "#00000000",
        edgeWidth: 3,

        panelBg:     Qt.rgba(24/255, 24/255, 26/255, 0.99),
        labelPillBg: Qt.rgba(26/255, 32/255, 46/255, 0.96),

        font:        "SF Pro Text",
        fontDisplay: "SF Pro Display",
        iconFont:    "JetBrainsMono Nerd Font",
        monoFont:    "JetBrainsMono Nerd Font",

        // ring/arc surfaces — themes may override; transparent = auto-derive
        segBg:       "#00000000",   // inactive-section fill (off)
        sector:      "#00000000",   // hovered-wedge fill (auto = accent mix)
        onBand:      "#00000000",   // icon/glyph ink on the band (auto by band luma)
        dot:         "#00000000",   // open-window dot (auto by band luma)
        labelFg:     "#ffffffff",   // centre pill text
        rim:         "#00000000",   // ring outline colour (auto = edge/border)
        rimWidth:    -1,            // ring outline width px (-1 = auto)
        backdrop:    "#000000",     // dim backdrop behind the ring
        settingsBtn: "#ffffffff",   // settings disc
        arcBg:       Qt.rgba(20/255, 20/255, 24/255, 0.97),   // action-arc band
        arcStroke:   Qt.rgba(1, 1, 1, 0.08),                  // action-arc band edge
        arcBtn:      Qt.rgba(1, 1, 1, 0.10),                  // action-arc button idle
        arcBtnHover: "#00000000"    // action-arc button hover (auto = accent)
    })

    property var t: ({})                                   // resolved overrides (after `extends` merge)
    function v(k) { return t[k] !== undefined ? t[k] : d[k] }   // override-or-default

    // Resolve a theme's `extends:` chain synchronously (parent keys first, child
    // overrides on top). XHR does a blocking local-file read; depth-capped.
    function loadThemeFile(nm) {
        try {
            var xhr = new XMLHttpRequest()
            xhr.open("GET", "file://" + Quickshell.shellDir + "/themes/" + nm + ".json", false)
            xhr.send()
            if (xhr.status === 200 || xhr.status === 0) return JSON.parse(xhr.responseText)
        } catch (e) { console.log("Skin: extends load failed", nm, e) }
        return null
    }
    function resolveExtends(obj, depth) {
        if (!obj || obj.extends === undefined || depth > 8) return obj
        var parent = loadThemeFile(obj.extends)
        if (!parent) return obj
        var merged = Object.assign({}, resolveExtends(parent, depth + 1), obj)
        delete merged.extends
        return merged
    }

    FileView {
        path: Quickshell.shellDir + "/themes/" + root.name + ".json"
        watchChanges: true
        onFileChanged: reload()
        onLoaded: { try { root.t = root.resolveExtends(JSON.parse(text()), 0) } catch (e) { console.log("Theme: bad", root.name, "—", e); root.t = ({}) } }
        onLoadFailed: (err) => { root.t = ({}) }            // unknown theme → built-in defaults
    }

    // ---- global scale ----
    readonly property real scale: v("scale")
    function s(px) { return Math.round(px * scale) }

    // ---- palette ----
    readonly property color glassBg:    v("glassBg")
    readonly property color glassHover: v("glassHover")
    readonly property color pillBg:     v("pillBg")
    readonly property color pillHover:  v("pillHover")
    readonly property color btnHover:   v("btnHover")
    readonly property color btnActive:  v("btnActive")

    readonly property color fg:       v("fg")
    readonly property color fgStrong: v("fgStrong")
    readonly property color fgDim:    v("fgDim")

    // bg + accent also power the ring (Wheel). A theme may set them to fully own the
    // ring's look; if it omits them, the live Look→Background/Accent pickers win.
    readonly property color bg:     t.bg     !== undefined ? t.bg     : (Launcher.settings.bg     || d.bg)
    readonly property color accent: t.accent !== undefined ? t.accent : (Launcher.settings.accent || d.accent)
    readonly property color yellow: v("yellow")
    readonly property color red:    v("red")
    readonly property color green:  v("green")

    readonly property color sep: v("sep")

    // ring outline: bold cell-shading when a theme sets `edge` (else transparent → subtle auto edge)
    readonly property color edge:      t.edge !== undefined ? t.edge : (Launcher.settings.border || d.edge)
    readonly property real  edgeWidth: v("edgeWidth")

    // inactive-section fill: theme key > Look→Inactive-fill picker > off
    readonly property color segBg: t.segBg !== undefined ? t.segBg : (Launcher.settings.segBg || d.segBg)
    // optional ring/arc overrides (the wheel auto-derives when transparent)
    readonly property color sector:      v("sector")
    readonly property color onBand:      v("onBand")
    readonly property color dot:         v("dot")
    readonly property color labelFg:     v("labelFg")
    readonly property color rim:         v("rim")
    readonly property real  rimWidth:    v("rimWidth")
    readonly property color backdrop:    v("backdrop")
    readonly property color settingsBtn: v("settingsBtn")
    readonly property color arcBg:       v("arcBg")
    readonly property color arcStroke:   v("arcStroke")
    readonly property color arcBtn:      v("arcBtn")
    readonly property color arcBtnHover: v("arcBtnHover")

    readonly property color panelBg:     v("panelBg")
    readonly property color labelPillBg: v("labelPillBg")

    // ---- fonts ----
    readonly property string font:        v("font")
    readonly property string fontDisplay: v("fontDisplay")
    readonly property string iconFont:    v("iconFont")
    readonly property string monoFont:    v("monoFont")

    // ---- dimensions (derived from scale) ----
    readonly property int barHeight: s(32)
    readonly property int barMargin: s(6)
    readonly property int barRadius: s(14)
    readonly property int pillRadius: s(12)

    // adaptive tint: a wash of the text color at alpha `a`. White-ish in dark themes,
    // ink in light themes — so control fills/borders stay visible on any panel.
    function tint(a) { return Qt.rgba(fg.r, fg.g, fg.b, a) }

    // tier color helper for gauges (ratio 0..1)
    function tier(ratio) {
        if (ratio > 0.85) return red
        if (ratio > 0.60) return yellow
        return green
    }
}
