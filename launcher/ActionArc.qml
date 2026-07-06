import Quickshell
import QtQuick
import QtQuick.Effects
import "../services"

// Per-app action menu: a half-circle of buttons above the main wheel, shown when
// an icon is long-pressed (Launcher.actionApp set). Actions come from the app's
// .desktop Actions plus Hyprland window actions (Close/Float/Fullscreen) — see
// Launcher.actionsFor(). Centre it on the same point as the main wheel.
Item {
    id: arc
    readonly property var app: Launcher.actionApp
    readonly property var actions: app ? Launcher.actionsFor(app) : []
    property string hoveredLabel: ""

    readonly property real mainOuterR: Skin.s(Launcher.settings.ringRadius) + Skin.s(Launcher.settings.iconSize) * 0.96
    readonly property real gap: Skin.s(10)
    readonly property real btnR: Skin.s(23)
    readonly property real arcRadius: mainOuterR + gap + btnR
    readonly property real bandInner: mainOuterR + Skin.s(3)
    readonly property real bandOuter: arcRadius + btnR + Skin.s(8)

    implicitWidth: bandOuter * 2
    implicitHeight: bandOuter * 2
    readonly property real cx: width / 2
    readonly property real cy: height / 2

    enabled: app !== null
    visible: app !== null || opacity > 0.01
    opacity: app !== null ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
    scale: app !== null ? 1 : 0.9
    transformOrigin: Item.Center
    Behavior on scale { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }

    // click on empty space (not a button) closes the arc, back to the wheel
    MouseArea { anchors.fill: parent; onClicked: Launcher.actionApp = null }

    // half-circle band (top)
    Canvas {
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d"); ctx.reset()
            var c = width / 2
            ctx.beginPath()
            ctx.arc(c, c, arc.bandOuter, Math.PI, 2 * Math.PI)
            ctx.arc(c, c, arc.bandInner, 2 * Math.PI, Math.PI, true)
            ctx.closePath()
            ctx.fillStyle = "rgba(20,20,24,0.97)"; ctx.fill()
            ctx.lineWidth = 1; ctx.strokeStyle = "rgba(255,255,255,0.08)"; ctx.stroke()
        }
        onWidthChanged: requestPaint()
    }

    // action buttons spread along the top arc
    Repeater {
        model: arc.actions
        delegate: Item {
            id: b
            required property int index
            required property var modelData
            readonly property int n: arc.actions.length
            readonly property real span: Math.min(150, 30 * Math.max(1, n))
            readonly property real deg: 180 + (180 - span) / 2 + (n <= 1 ? span / 2 : index * span / (n - 1))
            readonly property real rad: deg * Math.PI / 180
            width: arc.btnR * 2; height: arc.btnR * 2
            x: arc.cx + arc.arcRadius * Math.cos(rad) - width / 2
            y: arc.cy + arc.arcRadius * Math.sin(rad) - height / 2

            Rectangle {
                anchors.fill: parent; radius: width / 2
                color: ma.containsMouse ? Skin.accent : Skin.tint(0.10)
                Behavior on color { ColorAnimation { duration: 120 } }
                scale: ma.containsMouse ? 1.12 : 1
                Behavior on scale { NumberAnimation { duration: 130; easing.type: Easing.OutBack } }
                Image {
                    anchors.centerIn: parent
                    width: arc.btnR; height: arc.btnR
                    sourceSize.width: width; sourceSize.height: width
                    visible: !!b.modelData.icon
                    source: b.modelData.icon ? Launcher.iconSource(b.modelData.icon) : ""
                    smooth: true
                    // symbolic icons ship a fixed light fill — recolour to the chosen tint
                    layer.enabled: !!b.modelData.icon
                    layer.effect: MultiEffect {
                        colorization: 1.0
                        colorizationColor: b.modelData.color ? b.modelData.color : "white"
                    }
                }
                Text {
                    anchors.centerIn: parent
                    visible: !b.modelData.icon
                    text: b.modelData.glyph || ""
                    font.family: Skin.iconFont; font.pixelSize: Skin.s(18)
                    color: b.modelData.color ? b.modelData.color : "white"
                    renderType: Text.NativeRendering
                }
            }
            MouseArea {
                id: ma
                anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                onEntered: arc.hoveredLabel = b.modelData.label
                onExited: if (arc.hoveredLabel === b.modelData.label) arc.hoveredLabel = ""
                onClicked: Launcher.runAction(arc.app, b.modelData)
            }
        }
    }

    // label pill above the arc (app name, or the hovered action)
    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        y: arc.cy - arc.arcRadius - arc.btnR - Skin.s(8) - height
        implicitWidth: lbl.implicitWidth + Skin.s(22); implicitHeight: lbl.implicitHeight + Skin.s(10)
        radius: Skin.s(9)
        color: Skin.labelPillBg
        Text {
            id: lbl
            anchors.centerIn: parent
            text: arc.hoveredLabel !== "" ? arc.hoveredLabel : (arc.app ? arc.app.name : "")
            color: "white"; font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium
            renderType: Text.NativeRendering
        }
    }
}
