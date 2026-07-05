pragma Singleton
import Quickshell
import QtQuick

Singleton {
    // ---- global scale (AGS used $scale: 1.1) ----
    readonly property real scale: 1.1
    function s(px) { return Math.round(px * scale) }

    // ---- palette (from style.scss) ----
    readonly property color glassBg:      Qt.rgba(22/255, 22/255, 24/255, 0.52)
    readonly property color glassHover:   Qt.rgba(1, 1, 1, 0.09)
    readonly property color pillBg:       Qt.rgba(1, 1, 1, 0.15)
    readonly property color pillHover:    Qt.rgba(1, 1, 1, 0.25)
    readonly property color btnHover:     Qt.rgba(1, 1, 1, 0.09)
    readonly property color btnActive:    Qt.rgba(1, 1, 1, 0.14)

    readonly property color fg:       Qt.rgba(1, 1, 1, 0.88)
    readonly property color fgStrong: Qt.rgba(1, 1, 1, 0.95)
    readonly property color fgDim:    Qt.rgba(1, 1, 1, 0.45)

    readonly property color accent: "#0a84ff"
    readonly property color yellow: "#ffd60a"
    readonly property color red:    "#ff453a"
    readonly property color green:  "#30d158"

    readonly property color sep: Qt.rgba(1, 1, 1, 0.12)

    // ---- launcher surfaces ----
    readonly property color panelBg:     Qt.rgba(24/255, 24/255, 26/255, 0.99)  // settings/overlay panels
    readonly property color labelPillBg: Qt.rgba(26/255, 32/255, 46/255, 0.96)  // wheel/arc name pill

    // ---- fonts ----
    readonly property string font:        "SF Pro Text"
    readonly property string fontDisplay: "SF Pro Display"
    readonly property string iconFont:    "JetBrainsMono Nerd Font"
    readonly property string monoFont:    "JetBrainsMono Nerd Font"

    // ---- dimensions ----
    readonly property int barHeight: s(32)   // ~35
    readonly property int barMargin: s(6)    // ~7 top/left/right
    readonly property int barRadius: s(14)
    readonly property int pillRadius: s(12)

    // tier color helper for gauges (ratio 0..1)
    function tier(ratio) {
        if (ratio > 0.85) return red
        if (ratio > 0.60) return yellow
        return green
    }
}
