import Quickshell
import Quickshell.Wayland
import QtQuick
import "../services"

// Full-screen overlay radial app launcher. Toggled by SUPER+A. Press-and-hold on
// an empty zone opens the settings (with a live wheel preview beside it).
PanelWindow {
    id: root
    property var modelData
    screen: modelData
    color: "transparent"
    exclusionMode: ExclusionMode.Ignore   // span the full screen, under the bar too
    WlrLayershell.layer: WlrLayershell.Overlay
    WlrLayershell.namespace: "quickshell-launcher"
    WlrLayershell.keyboardFocus: Launcher.visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    anchors { top: true; bottom: true; left: true; right: true }
    visible: Launcher.visible || wheelWrap.opacity > 0.01

    // background: click outside the wheel = dismiss (or commit while editing).
    // Settings are opened from the centre button (hover the hole ~2s) — see Wheel.qml.
    MouseArea {
        id: bg
        anchors.fill: parent
        // "Follow outside" mode: track the cursor over the whole screen so the accent
        // sector points at it even off the ring, and a click activates the aimed slice.
        // (Dismiss with Esc or by clicking the centre hole.) Otherwise a click dismisses.
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
        anchors.fill: parent
        focus: Launcher.visible
        // Esc: leave settings (back to wheel) while editing, else dismiss the launcher.
        // The shortcut recorder consumes Esc itself while capturing, so it won't bubble here.
        Keys.onEscapePressed: Launcher.editing ? Launcher.commit() : Launcher.close()
    }

    // dim backdrop
    Rectangle {
        anchors.fill: parent
        color: "black"
        opacity: Launcher.visible ? Launcher.settings.dim : 0
        Behavior on opacity { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
    }

    // ---- settings: live preview + editor (press-and-hold to open) ----
    Loader {
        anchors.centerIn: parent
        active: Launcher.editing
        visible: active
        sourceComponent: Row {
            spacing: Theme.s(36)

            // live preview pane
            Rectangle {
                width: Theme.s(330); height: Theme.s(560)
                radius: Theme.s(20)
                color: Theme.panelBg
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.10)
                clip: true
                MouseArea { anchors.fill: parent }   // swallow stray clicks
                Text {
                    x: Theme.s(18); y: Theme.s(16)
                    text: "Preview"; color: Theme.fgDim
                    font.family: Theme.font; font.pixelSize: Theme.s(13); renderType: Text.NativeRendering
                }
                Wheel {
                    anchors.centerIn: parent
                    uiScale: 0.5
                    enabled: false   // preview: non-interactive (disables child MouseAreas)
                    // highlight a running app so the live thumbnail shows in the preview too
                    readonly property int sample: Launcher.firstRunningIndex()
                    forceSlice: sample
                    forceLabel: Launcher.ringModel.length ? Launcher.ringModel[sample].name : ""
                }
            }

            LauncherEditor {}
        }
    }

    // ---- main clickable wheel ----
    Item {
        id: wheelWrap
        anchors.centerIn: parent
        width: wheel.implicitWidth; height: wheel.implicitHeight
        visible: !Launcher.editing          // fully hidden while editing (no phantom icon clicks)
        opacity: Launcher.visible ? 1 : 0
        scale: Launcher.visible ? 1 : 0.9
        Behavior on opacity { NumberAnimation { duration: 170; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }

        Wheel { id: wheel; anchors.centerIn: parent }
    }

    // per-app action arc (long-press an icon) — on top of the wheel
    ActionArc { anchors.centerIn: parent }
}
