import Quickshell
import QtQuick
import QtQuick.Effects
import "../services"

// Per-app action menu, shaped to match the wheel layout. Shown when an icon is
// long-pressed (Launcher.actionApp set); actions come from the app's .desktop
// Actions plus window actions (Close/Float/Fullscreen) — see Launcher.actionsFor().
// RadialMenu centres it on the wheel (radial) or the pressed item (bar/half):
//   radial → a full ring of buttons around the wheel
//   half   → a half ring opening the wheel's way
//   bar    → a straight action bar beside the pressed icon
Item {
    id: arc
    readonly property var app: Launcher.actionApp
    readonly property var actions: app ? Launcher.actionsFor(app) : []
    readonly property int n: actions.length
    property string hoveredLabel: ""

    // ---- layout awareness (mirrors Wheel) ----
    readonly property var st: Launcher.settings
    readonly property int layoutMode: st.layout === "bar" ? 1 : (st.layout === "half" ? 2 : 0)
    readonly property bool isBar: layoutMode === 1
    readonly property bool isHalf: layoutMode === 2
    readonly property string pos: st.position || "center"
    readonly property bool barVertical: isBar && (pos === "left" || pos === "right")

    // ---- metrics ----
    readonly property real gap: Skin.s(10)
    readonly property real btnR: Skin.s(23)
    // ring (radial + half)
    readonly property real mainOuterR: Skin.s(st.ringRadius) + Skin.s(st.iconSize) * 0.96
    readonly property real arcRadius: mainOuterR + gap + btnR
    readonly property real bandInner: mainOuterR + Skin.s(3)
    readonly property real bandOuter: arcRadius + btnR + Skin.s(8)
    // bar strip
    readonly property real iconBox: Skin.s(st.iconSize)
    readonly property real pitch: 2 * btnR + Skin.s(10)
    readonly property real barOffset: iconBox / 2 + gap + btnR      // item centre → button line
    readonly property real barThick: 2 * btnR + Skin.s(12)
    readonly property real barLen: Math.max(1, n) * pitch + Skin.s(4)
    // half opening direction (rad): bottom→up, top→down, left→right, right→left
    readonly property real halfBase: pos === "top" ? Math.PI / 2
                                   : pos === "left" ? 0
                                   : pos === "right" ? Math.PI
                                   : -Math.PI / 2

    implicitWidth: bandOuter * 2
    implicitHeight: bandOuter * 2
    readonly property real cx: width / 2
    readonly property real cy: height / 2

    // bar strip centre (arc-local): pushed off the pressed item toward the opening
    readonly property real barCX: cx + (barVertical ? (pos === "left" ? barOffset : -barOffset) : 0)
    readonly property real barCY: cy + (barVertical ? 0 : (pos === "top" ? barOffset : -barOffset))

    enabled: app !== null
    visible: app !== null || opacity > 0.01
    opacity: app !== null ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
    scale: app !== null ? 1 : 0.9
    transformOrigin: Item.Center
    Behavior on scale { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }

    // centre of button `i` in arc-local coords
    function btnPos(i) {
        if (isBar) {
            var t = (i - (n - 1) / 2) * pitch
            return barVertical ? Qt.point(barCX, barCY + t) : Qt.point(barCX + t, barCY)
        }
        var ang
        if (isHalf) {
            var span = Math.min(150, 30 * Math.max(1, n)) * Math.PI / 180
            ang = halfBase - span / 2 + (n <= 1 ? span / 2 : i * span / (n - 1))
        } else {                                   // radial: spread the full circle
            ang = -Math.PI / 2 + i * 2 * Math.PI / Math.max(1, n)
        }
        return Qt.point(cx + arcRadius * Math.cos(ang), cy + arcRadius * Math.sin(ang))
    }

    // click on empty space (not a button) closes the arc, back to the wheel
    MouseArea { anchors.fill: parent; onClicked: Launcher.actionApp = null }

    // ---- band for ring layouts: full annulus (radial) or half annulus (half) ----
    Canvas {
        id: band
        anchors.fill: parent
        visible: !arc.isBar
        onPaint: {
            var ctx = getContext("2d"); ctx.reset()
            if (arc.isBar) return
            var c = width / 2
            ctx.beginPath()
            if (arc.isHalf) {
                var s0 = arc.halfBase - Math.PI / 2, s1 = arc.halfBase + Math.PI / 2
                ctx.arc(c, c, arc.bandOuter, s0, s1)
                ctx.arc(c, c, arc.bandInner, s1, s0, true)
                ctx.closePath()
            } else {
                ctx.arc(c, c, arc.bandOuter, 0, 2 * Math.PI, false)
                ctx.moveTo(c + arc.bandInner, c)
                ctx.arc(c, c, arc.bandInner, 0, 2 * Math.PI, true)
            }
            ctx.fillStyle = Skin.arcBg; ctx.fill()
            ctx.lineWidth = 1; ctx.strokeStyle = Skin.arcStroke; ctx.stroke()
        }
        Component.onCompleted: requestPaint()
        onWidthChanged: requestPaint()
        Connections { target: arc; function onLayoutModeChanged() { band.requestPaint() } }
        Connections { target: Launcher; function onSettingsChanged() { band.requestPaint() } }
    }

    // ---- band for bar layout: a rounded strip beside the pressed icon ----
    Rectangle {
        visible: arc.isBar
        width: arc.barVertical ? arc.barThick : arc.barLen
        height: arc.barVertical ? arc.barLen : arc.barThick
        x: arc.barCX - width / 2
        y: arc.barCY - height / 2
        radius: Math.min(width, height) / 2
        color: Skin.arcBg
        border.width: 1; border.color: Skin.arcStroke
    }

    // ---- action buttons ----
    Repeater {
        model: arc.actions
        delegate: Item {
            id: b
            required property int index
            required property var modelData
            readonly property point p: arc.btnPos(index)
            width: arc.btnR * 2; height: arc.btnR * 2
            x: p.x - width / 2
            y: p.y - height / 2

            Rectangle {
                anchors.fill: parent; radius: width / 2
                color: ma.containsMouse ? ((Launcher.actionApp && Launcher.actionApp.accent) ? Launcher.actionApp.accent : (Skin.arcBtnHover.a > 0 ? Skin.arcBtnHover : Skin.accent)) : Skin.arcBtn
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

    // ---- label pill (app name, or the hovered action) ----
    Rectangle {
        id: pill
        readonly property point p: {
            if (arc.isBar) {
                if (arc.barVertical)
                    return Qt.point(arc.barCX, arc.barCY - arc.barLen / 2 - arc.gap - height / 2)
                var ly = arc.pos === "top" ? arc.barCY + arc.barThick / 2 + arc.gap + height / 2
                                           : arc.barCY - arc.barThick / 2 - arc.gap - height / 2
                return Qt.point(arc.barCX, ly)
            }
            var bAng = arc.isHalf ? arc.halfBase : -Math.PI / 2
            var r = arc.arcRadius + arc.btnR + arc.gap + height / 2
            return Qt.point(arc.cx + r * Math.cos(bAng), arc.cy + r * Math.sin(bAng))
        }
        x: p.x - width / 2
        y: p.y - height / 2
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
