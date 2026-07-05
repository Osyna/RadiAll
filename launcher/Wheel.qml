import Quickshell
import Quickshell.Hyprland
import Quickshell.Wayland
import QtQuick
import QtQuick.Shapes
import "../services"

// The radial wheel: band with a transparent hole, app icons on the ring, an accent
// pie-sector that follows the cursor's ANGLE (whole-slice zones, gapless), a centre
// name pill, and a settings button that appears after hovering the hole for 2s.
// Reused by RadialMenu (interactive) and the settings preview (enabled:false).
Item {
    id: wheel
    property real uiScale: 1.0
    property string forceLabel: ""   // preview: pill text
    property int forceSlice: -1      // preview: force a highlighted slice

    readonly property var st: Launcher.settings
    readonly property var ring: Launcher.ringModel
    readonly property int count: ring.length
    readonly property real ringR:  Theme.s(st.ringRadius) * uiScale
    readonly property real iconBox: Theme.s(st.iconSize) * uiScale
    readonly property real outerR: ringR + iconBox * 0.96
    readonly property real innerR: Math.max(Theme.s(64) * uiScale, ringR - iconBox * 0.90)

    property int hoveredIndex: -1
    readonly property int effIndex: hoveredIndex >= 0 ? hoveredIndex : forceSlice
    property int activeIndex: 0
    onEffIndexChanged: if (effIndex >= 0) activeIndex = effIndex
    // label: app name on hover; for a multi-window app also show the selected
    // window's title + "(i/N)" so scrolling previews which window will be targeted.
    readonly property string shownLabel: {
        if (effIndex < 0 || effIndex >= count) return forceLabel
        void Launcher.winSel; void Hyprland.toplevels.values
        var app = ring[effIndex]
        var n = Launcher.windowsFor(app).length
        if (n <= 1) return app.name
        var t = Launcher.windowTitle(Launcher.windowFor(app))
        if (t.length > 42) t = t.slice(0, 42) + "…"
        return app.name + (t ? "  ·  " + t : "") + "   " + (Launcher.selectedWindowIndex(app) + 1) + "/" + n
    }
    property bool showSettingsBtn: false

    // live thumbnail source: the selected window of the hovered app (settings-gated).
    // Only in the interactive wheel (uiScale 1), never the tiny settings preview.
    readonly property var thumbSource: {
        void Launcher.winSel; void Hyprland.toplevels.values
        if (!st.thumbnails || effIndex < 0 || effIndex >= count) return null
        var w = Launcher.windowFor(ring[effIndex])
        return w ? w.wayland : null
    }
    readonly property bool thumbActive: thumbSource !== null

    // accumulated sector rotation, always shortest way (no full-circle roll on wrap)
    property real sectorRotation: 0
    onActiveIndexChanged: {
        if (count <= 0) return
        var target = activeIndex * (360 / count)
        var curNorm = ((sectorRotation % 360) + 360) % 360
        var delta = ((target - curNorm) % 360 + 540) % 360 - 180
        sectorRotation += delta
    }

    // palette
    function mixw(c, t) { return Qt.rgba(c.r * (1 - t) + t, c.g * (1 - t) + t, c.b * (1 - t) + t, 1) }
    readonly property color accentC: st.accent
    readonly property color bandC:   st.bg
    readonly property color sectorC: mixw(accentC, 0.04)
    readonly property real  bandLum: 0.299 * bandC.r + 0.587 * bandC.g + 0.114 * bandC.b
    readonly property color edgeC:   bandLum > 0.5 ? Qt.rgba(0, 0, 0, 1) : Qt.rgba(1, 1, 1, 1)
    readonly property real  edgeA:   bandLum > 0.5 ? 0.10 : 0.14

    implicitWidth: outerR * 2
    implicitHeight: outerR * 2

    // geometry
    readonly property real cx: width / 2
    readonly property real cy: height / 2
    readonly property real sw: Theme.s(12) * uiScale
    readonly property real segAng: count > 0 ? 2 * Math.PI / count : 0
    readonly property real ro: outerR - sw / 2 - 3 * uiScale
    readonly property real ri: innerR + sw / 2 + 3 * uiScale
    // FULL slice, no angular gap — zones tile the circle equally
    readonly property real a0: -Math.PI / 2 - segAng / 2
    readonly property real a1: -Math.PI / 2 + segAng / 2

    // which slice a point falls in (-1 = hole or outside ring)
    function sliceAt(x, y) {
        var dx = x - cx, dy = y - cy
        var d = Math.sqrt(dx * dx + dy * dy)
        if (d < innerR) return -1                       // the hole is never a slice
        if (d > outerR && !st.followOutside) return -1  // outside the ring, unless following the cursor
        var a = Math.atan2(dy, dx)
        return ((Math.round((a + Math.PI / 2) / segAng) % count) + count) % count
    }
    function inHole(x, y) {
        var dx = x - cx, dy = y - cy
        return Math.sqrt(dx * dx + dy * dy) < innerR
    }

    function updateHover(x, y) {
        if (inHole(x, y)) {
            hoveredIndex = -1
            if (!showSettingsBtn && !centerTimer.running) centerTimer.start()
        } else {
            centerTimer.stop()
            showSettingsBtn = false
            hoveredIndex = sliceAt(x, y)
        }
    }
    function handleClick(x, y, button) {
        if (inHole(x, y)) {
            if (showSettingsBtn) Launcher.editing = true
            else Launcher.close()
            return
        }
        var s = sliceAt(x, y)
        if (s < 0) { Launcher.close(); return }   // corner outside the ring
        Launcher.activateItem(ring[s], button)    // apps/windows: focus/launch; actions: run
    }

    Timer { id: centerTimer; interval: 2000; onTriggered: wheel.showSettingsBtn = true }

    // band with a transparent hole
    Canvas {
        id: donut
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d"); ctx.reset()
            var c = width / 2, op = wheel.st.wheelOpacity
            function css(col, a) { return "rgba(" + Math.round(col.r * 255) + "," + Math.round(col.g * 255) + "," + Math.round(col.b * 255) + "," + a + ")" }
            ctx.beginPath(); ctx.arc(c, c, wheel.outerR, 0, 2 * Math.PI); ctx.closePath()
            ctx.fillStyle = css(wheel.bandC, op); ctx.fill()
            ctx.globalCompositeOperation = "destination-out"
            ctx.beginPath(); ctx.arc(c, c, wheel.innerR, 0, 2 * Math.PI); ctx.closePath(); ctx.fill()
            ctx.globalCompositeOperation = "source-over"
            ctx.lineWidth = 1; ctx.strokeStyle = css(wheel.edgeC, wheel.edgeA * op)
            ctx.beginPath(); ctx.arc(c, c, wheel.outerR - 0.5, 0, 2 * Math.PI); ctx.stroke()
            ctx.beginPath(); ctx.arc(c, c, wheel.innerR + 0.5, 0, 2 * Math.PI); ctx.stroke()
        }
        Connections { target: Launcher; function onSettingsChanged() { donut.requestPaint() } }
        onWidthChanged: requestPaint()
    }

    // accent pie sector — fixed top wedge rotated to the active slice (MSAA, no seam)
    Shape {
        id: sector
        anchors.fill: parent
        antialiasing: true
        // supersample: render at 2× into the layer then downscale smoothly (SSAA).
        // reliable AA even where MSAA FBOs aren't honoured; no CurveRenderer seam.
        layer.enabled: true
        layer.smooth: true
        layer.samples: 8
        layer.textureSize: Qt.size(width * 2, height * 2)
        visible: wheel.effIndex >= 0 && wheel.count > 0
        opacity: visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 110; easing.type: Easing.OutCubic } }
        transformOrigin: Item.Center
        rotation: wheel.sectorRotation
        Behavior on rotation { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
        ShapePath {
            fillColor: wheel.sectorC
            strokeColor: wheel.sectorC
            strokeWidth: wheel.sw
            joinStyle: ShapePath.RoundJoin
            capStyle: ShapePath.RoundCap
            startX: wheel.cx + wheel.ro * Math.cos(wheel.a0)
            startY: wheel.cy + wheel.ro * Math.sin(wheel.a0)
            PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: wheel.ro; radiusY: wheel.ro
                           startAngle: wheel.a0 * 180 / Math.PI; sweepAngle: (wheel.a1 - wheel.a0) * 180 / Math.PI }
            PathLine { x: wheel.cx + wheel.ri * Math.cos(wheel.a1); y: wheel.cy + wheel.ri * Math.sin(wheel.a1) }
            PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: wheel.ri; radiusY: wheel.ri
                           startAngle: wheel.a1 * 180 / Math.PI; sweepAngle: -(wheel.a1 - wheel.a0) * 180 / Math.PI }
            PathLine { x: wheel.cx + wheel.ro * Math.cos(wheel.a0); y: wheel.cy + wheel.ro * Math.sin(wheel.a0) }
        }
    }

    // centre name pill (hidden when the settings button is up). In actions mode
    // it sits in the lower half of the hole so the app icon (upper half) is clear.
    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: Launcher.mode === "actions"
                                      ? wheel.innerR * 0.46 * wheel.uiScale : 0
        z: 3
        implicitWidth: label.implicitWidth + Theme.s(24) * wheel.uiScale
        implicitHeight: label.implicitHeight + Theme.s(12) * wheel.uiScale
        radius: Theme.s(9) * wheel.uiScale
        color: Theme.labelPillBg
        visible: wheel.st.showLabels && !wheel.showSettingsBtn && !wheel.thumbActive
        opacity: wheel.shownLabel !== "" ? 1 : 0
        scale: wheel.shownLabel !== "" ? 1 : 0.85
        Behavior on opacity { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 160; easing.type: Easing.OutBack } }
        Text {
            id: label
            anchors.centerIn: parent
            text: wheel.shownLabel
            color: "white"
            font.family: Theme.font; font.pixelSize: Theme.s(14) * wheel.uiScale; font.weight: Font.Medium
            renderType: Text.NativeRendering
        }
    }

    // "actions" mode: the focused window's app icon, in the upper half of the hole
    // (the action label pill sits in the lower half, so they don't overlap).
    Image {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: -wheel.innerR * 0.24 * wheel.uiScale
        readonly property var fa: Launcher.focusedApp
        visible: Launcher.mode === "actions" && !wheel.showSettingsBtn && fa !== null
        width: wheel.innerR * 0.74 * wheel.uiScale; height: width
        sourceSize.width: width; sourceSize.height: width
        source: fa ? Launcher.iconSource(fa.icon) : ""
        smooth: true
    }

    // live window thumbnail of the selected window, centred in the hole (settings-gated)
    Loader {
        anchors.centerIn: parent
        active: wheel.thumbActive
        visible: active && !wheel.showSettingsBtn
        sourceComponent: Item {
            id: thumb
            // whole preview (frame + caption) fits inside the hole circle: a square of
            // side avail centred in the hole has its corners at 0.92·innerR < innerR.
            readonly property real avail: wheel.innerR * 1.3
            readonly property real m: Theme.s(4) * wheel.uiScale     // frame margin
            readonly property real gap: Theme.s(6) * wheel.uiScale
            readonly property real maxW: avail - 2 * m
            readonly property real maxH: avail - cap.implicitHeight - gap - 2 * m
            implicitWidth: avail
            implicitHeight: view.height + 2 * m + gap + cap.implicitHeight
            Rectangle {   // frame behind the capture
                anchors.fill: view; anchors.margins: -thumb.m
                color: Qt.rgba(8/255, 8/255, 10/255, 0.94)
                radius: Theme.s(10) * wheel.uiScale
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
            }
            ScreencopyView {
                id: view
                captureSource: wheel.thumbSource
                live: true; paintCursor: false
                readonly property real a: (sourceSize.width > 0 && sourceSize.height > 0)
                                          ? sourceSize.width / sourceSize.height : 1.6
                width:  Math.max(1, Math.min(thumb.maxW, thumb.maxH * a))   // fit aspect into hole
                height: width / a
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: parent.top
            }
            // caption — marquee-scrolls when the label is wider than the hole
            Item {
                id: cap
                anchors.top: view.bottom; anchors.topMargin: thumb.gap
                anchors.horizontalCenter: parent.horizontalCenter
                width: thumb.avail; implicitHeight: capTxt.implicitHeight; clip: true
                readonly property real scrollW: Math.max(0, capTxt.implicitWidth - width)
                property real off: 0
                Text {
                    id: capTxt
                    text: wheel.shownLabel
                    color: "white"; font.family: Theme.font; font.pixelSize: Theme.s(12) * wheel.uiScale
                    font.weight: Font.Medium; renderType: Text.NativeRendering
                    x: cap.scrollW > 0 ? cap.off : (cap.width - implicitWidth) / 2
                }
                SequentialAnimation on off {
                    running: cap.scrollW > 0
                    loops: Animation.Infinite
                    PauseAnimation { duration: 1200 }
                    NumberAnimation { from: 0; to: -cap.scrollW; duration: Math.max(1, cap.scrollW) * 18; easing.type: Easing.InOutQuad }
                    PauseAnimation { duration: 1200 }
                    NumberAnimation { from: -cap.scrollW; to: 0; duration: Math.max(1, cap.scrollW) * 18; easing.type: Easing.InOutQuad }
                }
            }
        }
    }

    // app icons (pure visuals; scale with the cursor-angle slice)
    Repeater {
        model: wheel.ring
        delegate: Item {
            id: slot
            required property int index
            required property var modelData
            readonly property real ang: -Math.PI / 2 + index * 2 * Math.PI / wheel.count
            width: wheel.iconBox; height: wheel.iconBox
            x: wheel.width / 2  + wheel.ringR * Math.cos(ang) - width / 2
            y: wheel.height / 2 + wheel.ringR * Math.sin(ang) - height / 2
            z: wheel.effIndex === index ? 2 : 1

            // open windows of this app (reactive on toplevels + selection)
            readonly property int winCount: { void Hyprland.toplevels.values; return modelData ? Launcher.windowsFor(modelData).length : 0 }
            readonly property int winSelIdx: { void Hyprland.toplevels.values; void Launcher.winSel; return modelData ? Launcher.selectedWindowIndex(modelData) : -1 }

            readonly property bool glyphOnly: !slot.modelData.icon && !!slot.modelData.glyph
            Image {
                id: iconImg
                anchors.centerIn: parent
                visible: !slot.glyphOnly
                width: wheel.iconBox - Theme.s(10) * wheel.uiScale; height: width
                sourceSize.width: width; sourceSize.height: width
                source: Launcher.iconSource(slot.modelData.icon)
                smooth: true
                scale: wheel.effIndex === slot.index ? 1.14 : 1
                Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutBack } }
            }
            // action items with no icon: render their font glyph (e.g. Close/Float)
            Text {
                anchors.centerIn: parent
                visible: slot.glyphOnly
                text: slot.modelData.glyph || ""
                font.family: Theme.iconFont; font.pixelSize: wheel.iconBox * 0.46
                color: "white"; renderType: Text.NativeRendering
                scale: wheel.effIndex === slot.index ? 1.14 : 1
                Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutBack } }
            }

            // running indicator: one dot per open window; selected one is accent-lit.
            // Scroll over the app to change the selection (see MouseArea.onWheel).
            Row {
                visible: slot.winCount > 0
                spacing: Theme.s(3) * wheel.uiScale
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: iconImg.bottom
                anchors.topMargin: Theme.s(2) * wheel.uiScale
                Repeater {
                    model: Math.min(slot.winCount, 6)
                    delegate: Rectangle {
                        required property int index
                        readonly property bool sel: slot.winCount > 1 && index === slot.winSelIdx
                        width: Theme.s(5) * wheel.uiScale; height: width; radius: width / 2
                        color: sel ? wheel.accentC
                             : (wheel.bandLum > 0.5 ? Qt.rgba(0, 0, 0, 0.55) : Qt.rgba(1, 1, 1, 0.85))
                        scale: sel ? 1.2 : 1
                        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutBack } }
                    }
                }
            }
        }
    }

    // single input surface: hover→angle→slice, click→slice action, centre→settings
    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.PointingHandCursor
        pressAndHoldInterval: 400
        onWheel: (w) => {                                       // scroll over an app → pick its window
            if (Launcher.mode === "actions") return
            var s = wheel.sliceAt(w.x, w.y)
            if (s >= 0) Launcher.cycleWindow(wheel.ring[s], w.angleDelta.y > 0 ? -1 : 1)
        }
        onPositionChanged: (m) => wheel.updateHover(m.x, m.y)
        onExited: {
            if (Launcher.settings.followOutside) return   // backdrop keeps tracking the cursor angle
            wheel.hoveredIndex = -1; centerTimer.stop(); wheel.showSettingsBtn = false
        }
        onPressAndHold: (m) => {
            if (Launcher.mode !== "apps") return                // arc is the per-app menu (apps mode)
            var s = wheel.sliceAt(m.x, m.y)
            if (s >= 0) Launcher.actionApp = wheel.ring[s]      // long-press → action arc
        }
        onClicked: (m) => {
            if (Launcher.actionApp !== null) return             // a long-press just opened the arc
            wheel.handleClick(m.x, m.y, m.button)
        }
    }

    // settings button (appears after 2s hovering the hole)
    Rectangle {
        anchors.centerIn: parent
        width: Theme.s(50) * wheel.uiScale; height: width; radius: width / 2
        color: Qt.rgba(30/255, 34/255, 44/255, 0.96)
        border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
        visible: wheel.showSettingsBtn || opacity > 0.01
        opacity: wheel.showSettingsBtn ? 1 : 0
        scale: wheel.showSettingsBtn ? 1 : 0.6
        Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }
        Text {
            anchors.centerIn: parent
            text: ""   // nf-fa-cog
            font.family: Theme.iconFont; font.pixelSize: Theme.s(22) * wheel.uiScale
            color: "white"; renderType: Text.NativeRendering
        }
    }
}
