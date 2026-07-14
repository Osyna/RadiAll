import Quickshell
import Quickshell.Wayland
import QtQuick
import "../services"

// Full-screen overlay radial app launcher. Toggled by SUPER+A. Press-and-hold on
// an empty zone opens the settings (with a live wheel preview beside it). Shown
// only on the output the ring was opened on; the wheel anchors per layout/position
// (radial centred/edged, half ring flush to an edge, bar as a dock strip).
PanelWindow {
    id: root
    property var modelData
    screen: modelData
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore   // span the full screen, under the bar too
    WlrLayershell.layer: WlrLayershell.Overlay
    WlrLayershell.namespace: "quickshell-launcher"

    // Only the output the ring was opened on shows it (mirrors the standalone's
    // active-output behaviour). Unknown monitor (non-Hyprland) → every output.
    readonly property bool onActiveOutput: Launcher.openMonitor === ""
                                           || (modelData && modelData.name === Launcher.openMonitor)

    WlrLayershell.keyboardFocus: (Launcher.visible && onActiveOutput) ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    anchors { top: true; bottom: true; left: true; right: true }
    visible: (Launcher.visible || wheelWrap.opacity > 0.01) && onActiveOutput

    // ---- wheel placement (centre point on screen) per layout + position ----
    readonly property real ww: wheel.implicitWidth
    readonly property real wh: wheel.implicitHeight
    readonly property string lay: Launcher.settings.layout || "radial"
    readonly property string ppos: Launcher.settings.position || "center"
    readonly property int edgeMargin: Skin.s(40)
    readonly property real wcx: {
        if (lay === "half") return ppos === "left" ? 0 : (ppos === "right" ? width : width / 2)
        if (ppos === "left")  return edgeMargin + ww / 2
        if (ppos === "right") return width - (edgeMargin + ww / 2)
        return width / 2
    }
    readonly property real wcy: {
        if (lay === "half") return ppos === "top" ? 0 : ((ppos === "left" || ppos === "right") ? height / 2 : height)
        if (ppos === "top")    return edgeMargin + wh / 2
        if (ppos === "bottom") return height - (edgeMargin + wh / 2)
        return height / 2
    }

    // background: click outside the wheel = dismiss (or commit while editing).
    // Settings are opened from the centre button (hover the hole ~2s) — see Wheel.qml.
    MouseArea {
        id: bg
        anchors.fill: parent
        readonly property bool follow: Launcher.settings.followOutside && Launcher.visible && !Launcher.editing
        hoverEnabled: follow
        onPositionChanged: (m) => {
            if (!follow) return
            var p = mapToItem(wheel, m.x, m.y)
            wheel.updateHover(p.x, p.y)
        }
        onClicked: (m) => {
            if (Launcher.editing) { Launcher.commit(); return }
            if (follow && Launcher.actionApp === null) {
                var p = mapToItem(wheel, m.x, m.y)
                wheel.handleClick(p.x, p.y, m.button)
            } else {
                Launcher.close()
            }
        }
    }
    Item {
        id: keyCatcher
        anchors.fill: parent
        focus: Launcher.visible
        // Esc always fully dismisses (ring or settings); settings are saved live.
        Keys.onEscapePressed: Launcher.close()
        // grab focus the instant the overlay opens, so Esc works immediately
        Connections {
            target: Launcher
            function onVisibleChanged() { if (Launcher.visible) keyCatcher.forceActiveFocus() }
        }
    }

    // dim backdrop
    Rectangle {
        anchors.fill: parent
        color: Skin.backdrop
        opacity: Launcher.visible ? Launcher.settings.dim : 0
        Behavior on opacity { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
    }

    // ---- settings: live preview + editor (centred; press-and-hold to open) ----
    Loader {
        anchors.centerIn: parent
        active: Launcher.editing
        visible: active
        sourceComponent: Row {
            spacing: Skin.s(36)
            Rectangle {
                width: Skin.s(330); height: Skin.s(560)
                radius: Skin.s(20)
                color: Skin.panelBg
                border.width: 1; border.color: Skin.tint(0.10)
                clip: true
                MouseArea { anchors.fill: parent }
                Text {
                    x: Skin.s(18); y: Skin.s(16)
                    text: "Preview"; color: Skin.fgDim
                    font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering
                }
                Wheel {
                    anchors.centerIn: parent
                    uiScale: 0.5
                    enabled: false
                    readonly property int sample: Launcher.firstRunningIndex()
                    forceSlice: sample
                    forceLabel: Launcher.ringModel.length ? Launcher.ringModel[sample].name : ""
                }
            }
            LauncherEditor {}
        }
    }

    // ---- main clickable wheel (anchored per layout/position) ----
    Item {
        id: wheelWrap
        x: root.wcx - width / 2
        y: root.wcy - height / 2
        width: wheel.implicitWidth; height: wheel.implicitHeight
        visible: !Launcher.editing
        opacity: Launcher.visible ? 1 : 0
        scale: Launcher.visible ? 1 : 0.9
        Behavior on opacity { NumberAnimation { duration: 170; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }

        Wheel { id: wheel; anchors.centerIn: parent }
    }

    // per-app action arc (long-press an icon). Radial → halo on the wheel centre;
    // bar/half → pops over the pressed item itself (matches the standalone).
    ActionArc {
        readonly property real ctrX: (Launcher.actionApp === null || Launcher.actionSlice < 0 || wheel.layoutMode === 0)
            ? wheelWrap.x + wheelWrap.width / 2
            : wheelWrap.x + wheel.itemCenter(Launcher.actionSlice).x
        readonly property real ctrY: (Launcher.actionApp === null || Launcher.actionSlice < 0 || wheel.layoutMode === 0)
            ? wheelWrap.y + wheelWrap.height / 2
            : wheelWrap.y + wheel.itemCenter(Launcher.actionSlice).y
        x: ctrX - width / 2
        y: ctrY - height / 2
    }
}
