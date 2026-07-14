import Quickshell
import Quickshell.Wayland
import QtQuick
import QtQuick.Shapes
import QtQuick.Effects
import "../services"

// The wheel — three layouts (radial donut / linear bar / half ring), an accent
// section that tracks the cursor's angle (gapless zones), optional segmented
// sections, per-app accent, a centre hole (radial/half) with name pill +
// settings button, and open-window dots. Reused by RadialMenu (interactive) and
// the settings preview (enabled:false). Layout/section/colour geometry mirrors
// the Rust+Slint standalone.
Item {
    id: wheel
    property real uiScale: 1.0
    property string forceLabel: ""   // preview: pill text
    property int forceSlice: -1      // preview: force a highlighted slice

    readonly property var st: Launcher.settings
    readonly property var ring: Launcher.ringModel
    readonly property int count: ring.length

    // ---- layout engine ----
    readonly property int layoutMode: st.layout === "bar" ? 1 : (st.layout === "half" ? 2 : 0)
    readonly property bool isBar: layoutMode === 1
    readonly property bool isHalf: layoutMode === 2
    readonly property string pos: st.position || "center"

    readonly property real ringR:   Skin.s(st.ringRadius) * uiScale
    readonly property real iconBox:  Skin.s(st.iconSize) * uiScale
    readonly property real outerR:   ringR + iconBox * 0.96
    readonly property real innerR:   Math.max(Skin.s(st.holeSize) * uiScale, ringR - iconBox * 0.90)
    // linear-bar metrics
    readonly property real barPitch: iconBox + Skin.s(18) * uiScale
    readonly property real barThick: iconBox + Skin.s(26) * uiScale
    readonly property real barPad:   Skin.s(16) * uiScale
    readonly property real barLen:   count * barPitch + 2 * barPad
    readonly property bool barVertical: isBar && (pos === "left" || pos === "right")

    // span + base angle (radial = full circle from the top; half = 180° opening
    // away from the anchored screen edge)
    readonly property real spanRad: isHalf ? Math.PI : 2 * Math.PI
    readonly property real segAng:  count > 0 ? spanRad / count : 0
    readonly property real a0Deg: {
        if (!isHalf) return 0
        switch (pos) {
        case "top":   return 0      // opens downward
        case "left":  return -90    // opens right
        case "right": return 90     // opens left
        default:      return 180    // bottom / center: opens up
        }
    }
    readonly property real a0Rad: a0Deg * Math.PI / 180
    readonly property real wcBase: isHalf ? a0Rad + segAng / 2 : -Math.PI / 2   // centre of slice 0
    readonly property real midRad: a0Rad + Math.PI / 2                          // half: toward visible hemisphere
    function sliceAng(i) { return isHalf ? a0Rad + (i + 0.5) * segAng : -Math.PI / 2 + i * segAng }

    property int hoveredIndex: -1
    readonly property int effIndex: hoveredIndex >= 0 ? hoveredIndex : forceSlice
    property int activeIndex: 0
    onEffIndexChanged: if (effIndex >= 0) activeIndex = effIndex

    // label: app name on hover; multi-window apps also show the selected window's
    // title + "(i/N)" so scrolling previews which window will be targeted.
    readonly property string shownLabel: {
        if (effIndex < 0 || effIndex >= count) return forceLabel
        void Launcher.winSel; void Compositor.windows
        var app = ring[effIndex]
        var n = Launcher.windowsFor(app).length
        if (n <= 1) return app.name
        var t = Launcher.windowTitle(Launcher.windowFor(app))
        if (t.length > 42) t = t.slice(0, 42) + "…"
        return app.name + (t ? "  ·  " + t : "") + "   " + (Launcher.selectedWindowIndex(app) + 1) + "/" + n
    }
    property bool showSettingsBtn: false

    // live thumbnail of the selected window (settings-gated; radial/half only,
    // never the tiny settings preview or the bar).
    readonly property var thumbSource: {
        void Launcher.winSel; void Compositor.windows
        if (!st.thumbnails || isBar || effIndex < 0 || effIndex >= count) return null
        var w = Launcher.windowFor(ring[effIndex])
        return w ? Compositor.capture(w) : null
    }
    readonly property bool thumbActive: thumbSource !== null

    // accent rotation (radial/half): shortest-path accumulator (no full-circle roll)
    property real sectorRotation: 0
    onActiveIndexChanged: {
        if (count <= 0) return
        var target = activeIndex * (spanRad / count) * 180 / Math.PI
        var curNorm = ((sectorRotation % 360) + 360) % 360
        var delta = ((target - curNorm) % 360 + 540) % 360 - 180
        sectorRotation += delta
    }

    // ---- palette ----
    function mixw(c, t) { return Qt.rgba(c.r * (1 - t) + t, c.g * (1 - t) + t, c.b * (1 - t) + t, 1) }
    readonly property color accentC: Skin.accent
    // per-app accent: the active slice's own accent overrides the global one
    readonly property color activeAccent: {
        var it = (effIndex >= 0 && effIndex < count) ? ring[effIndex] : null
        return (it && it.accent) ? it.accent : accentC
    }
    readonly property color bandC:   Skin.bg
    readonly property color sectorC: Skin.sector.a > 0.001 ? Skin.sector : mixw(activeAccent, 0.04)
    readonly property real  bandLum: 0.299 * bandC.r + 0.587 * bandC.g + 0.114 * bandC.b
    readonly property color inkC:    Skin.onBand.a > 0.001 ? Skin.onBand : (bandLum > 0.5 ? "#191a2e" : "white")
    readonly property color labelFg: Skin.labelFg
    // ring outline: theme rim > theme edge (cell) > adaptive auto edge
    readonly property bool  cellEdge: Skin.edge.a > 0.001
    readonly property color edgeC:   Skin.rim.a > 0.001 ? Skin.rim
                                    : (cellEdge ? Skin.edge : (bandLum > 0.5 ? Qt.rgba(0, 0, 0, 1) : Qt.rgba(1, 1, 1, 1)))
    readonly property real  edgeA:   (Skin.rim.a > 0.001 || cellEdge) ? edgeC.a : (bandLum > 0.5 ? 0.10 : 0.14)
    // width: theme rimWidth > Look→Border width > auto (cell edgeWidth / 1px)
    readonly property real  edgeW:   (Skin.rimWidth >= 0 ? Skin.rimWidth
                                     : (st.borderWidth >= 0 ? st.borderWidth
                                     : (cellEdge ? Skin.edgeWidth : 1))) * uiScale

    implicitWidth:  isBar ? (barVertical ? barThick : barLen) : outerR * 2
    implicitHeight: isBar ? (barVertical ? barLen : barThick) : outerR * 2

    // ---- geometry ----
    readonly property real cx: width / 2
    readonly property real cy: height / 2

    // ---- section styling (segmented-pie: inset, rounded corners, gap) ----
    readonly property real gk: uiScale
    readonly property real srad: cellEdge ? 0 : Skin.s(st.activeRadius) * gk
    readonly property real irad: cellEdge ? 0 : Skin.s(st.inactiveRadius) * gk
    readonly property real sinset: cellEdge ? edgeW / 2 : Skin.s(st.edgePadding) * gk
    readonly property real hgap: Skin.s(st.sectionGap) * gk / 2
    readonly property real wspanRad: count <= 1 ? (359.9 * Math.PI / 180) : segAng
    // active wedge (fixed at the top, rotated to the active slice)
    readonly property real sro: Math.max(outerR - sinset - srad, 1)
    readonly property real sri: Math.max(Math.min(innerR + sinset + srad, sro - 0.5), 1)
    readonly property real wgo: Math.min((hgap + srad) / sro, wspanRad / 2 - 0.001)
    readonly property real wgi: Math.min((hgap + srad) / sri, wspanRad / 2 - 0.001)
    readonly property real wa0o: wcBase - wspanRad / 2 + wgo
    readonly property real wa1o: wcBase + wspanRad / 2 - wgo
    readonly property real wa0i: wcBase - wspanRad / 2 + wgi
    readonly property real wa1i: wcBase + wspanRad / 2 - wgi
    function degOf(r) { return r * 180 / Math.PI }

    // centre-content offset (half ring nudges it into the visible hemisphere)
    readonly property real ccOffX: isHalf ? innerR * 0.5 * Math.cos(midRad) : 0
    readonly property real ccOffY: isHalf ? innerR * 0.5 * Math.sin(midRad) : 0

    // which slice a point falls in (-1 = hole / outside)
    function sliceAt(x, y) {
        if (count <= 0) return -1
        if (isBar) {
            var along = barVertical ? y : x
            var lo = (barVertical ? (height - barLen) / 2 : (width - barLen) / 2) + barPad
            var rel = along - lo
            if (!st.followOutside && (rel < 0 || rel > count * barPitch)) return -1
            return Math.max(0, Math.min(count - 1, Math.floor(rel / barPitch)))
        }
        var dx = x - cx, dy = y - cy
        var d = Math.sqrt(dx * dx + dy * dy)
        if (d < innerR) return -1
        if (d > outerR && !st.followOutside) return -1
        var a = Math.atan2(dy, dx)
        if (isHalf) {
            var loc = a - a0Rad
            while (loc < 0) loc += 2 * Math.PI
            while (loc >= 2 * Math.PI) loc -= 2 * Math.PI
            if (loc > Math.PI) {                      // hidden hemisphere
                if (!st.followOutside) return -1
                loc = (loc > Math.PI * 1.5) ? 0 : Math.PI    // snap to nearest visible end
            }
            return Math.max(0, Math.min(count - 1, Math.floor(loc / segAng)))
        }
        return ((Math.round((a + Math.PI / 2) / segAng) % count) + count) % count
    }
    function inHole(x, y) {
        if (isBar) return false
        var dx = x - cx, dy = y - cy
        return Math.sqrt(dx * dx + dy * dy) < innerR
    }
    // centre of ring item `i` in wheel-local coords (used to place the action arc)
    function itemCenter(i) {
        if (isBar) {
            var lo = (barVertical ? (height - barLen) / 2 : (width - barLen) / 2) + barPad + i * barPitch + barPitch / 2
            return barVertical ? Qt.point(width / 2, lo) : Qt.point(lo, height / 2)
        }
        var a = sliceAng(i)
        return Qt.point(cx + ringR * Math.cos(a), cy + ringR * Math.sin(a))
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
        if (s < 0) { Launcher.close(); return }
        Launcher.activateItem(ring[s], button)
    }

    Timer { id: centerTimer; interval: 2000; onTriggered: wheel.showSettingsBtn = true }

    // ---- radial / half: band with a transparent hole ----
    Canvas {
        id: donut
        anchors.fill: parent
        visible: !wheel.isBar
        onPaint: {
            var ctx = getContext("2d"); ctx.reset()
            if (wheel.isBar) return
            var op = wheel.st.wheelOpacity
            function css(col, a) { return "rgba(" + Math.round(col.r * 255) + "," + Math.round(col.g * 255) + "," + Math.round(col.b * 255) + "," + a + ")" }
            ctx.beginPath(); ctx.arc(wheel.cx, wheel.cy, wheel.outerR, 0, 2 * Math.PI); ctx.closePath()
            ctx.fillStyle = css(wheel.bandC, op); ctx.fill()
            ctx.globalCompositeOperation = "destination-out"
            ctx.beginPath(); ctx.arc(wheel.cx, wheel.cy, wheel.innerR, 0, 2 * Math.PI); ctx.closePath(); ctx.fill()
            ctx.globalCompositeOperation = "source-over"
        }
        Connections { target: Launcher; function onSettingsChanged() { donut.requestPaint() } }
        Connections { target: wheel; function onBandCChanged() { donut.requestPaint() } }
        Connections { target: wheel; function onEdgeCChanged() { donut.requestPaint() } }
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
    }

    // ---- radial / half: inactive segmented sections (only when a fill is set) ----
    Repeater {
        model: (!wheel.isBar && Skin.segBg.a > 0.001) ? wheel.count : 0
        delegate: Shape {
            id: seg
            required property int index
            anchors.fill: parent
            antialiasing: true
            layer.enabled: true; layer.smooth: true; layer.samples: 4
            readonly property real ca: wheel.sliceAng(index)
            readonly property real gro: Math.max(wheel.outerR - wheel.sinset - wheel.irad, 1)
            readonly property real gri: Math.max(Math.min(wheel.innerR + wheel.sinset + wheel.irad, gro - 0.5), 1)
            readonly property real ggo: Math.min((wheel.hgap + wheel.irad) / gro, wheel.wspanRad / 2 - 0.001)
            readonly property real ggi: Math.min((wheel.hgap + wheel.irad) / gri, wheel.wspanRad / 2 - 0.001)
            ShapePath {
                fillColor: Skin.segBg; strokeColor: Skin.segBg
                strokeWidth: 2 * wheel.irad + 0.5
                joinStyle: ShapePath.RoundJoin; capStyle: ShapePath.RoundCap
                startX: wheel.cx + seg.gro * Math.cos(seg.ca - wheel.wspanRad / 2 + seg.ggo)
                startY: wheel.cy + seg.gro * Math.sin(seg.ca - wheel.wspanRad / 2 + seg.ggo)
                PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: seg.gro; radiusY: seg.gro
                               startAngle: wheel.degOf(seg.ca - wheel.wspanRad / 2 + seg.ggo)
                               sweepAngle: wheel.degOf(wheel.wspanRad - 2 * seg.ggo) }
                PathLine { x: wheel.cx + seg.gri * Math.cos(seg.ca + wheel.wspanRad / 2 - seg.ggi)
                           y: wheel.cy + seg.gri * Math.sin(seg.ca + wheel.wspanRad / 2 - seg.ggi) }
                PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: seg.gri; radiusY: seg.gri
                               startAngle: wheel.degOf(seg.ca + wheel.wspanRad / 2 - seg.ggi)
                               sweepAngle: -wheel.degOf(wheel.wspanRad - 2 * seg.ggi) }
                PathLine { x: wheel.cx + seg.gro * Math.cos(seg.ca - wheel.wspanRad / 2 + seg.ggo)
                           y: wheel.cy + seg.gro * Math.sin(seg.ca - wheel.wspanRad / 2 + seg.ggo) }
            }
        }
    }

    // ---- radial / half: accent sector wedge (rounded annular sector, SSAA) ----
    Shape {
        id: sector
        anchors.fill: parent
        visible: !wheel.isBar && wheel.effIndex >= 0 && wheel.count > 0
        antialiasing: true
        layer.enabled: true
        layer.smooth: true
        layer.samples: 8
        layer.textureSize: Qt.size(width * 2, height * 2)
        opacity: visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 110; easing.type: Easing.OutCubic } }
        transformOrigin: Item.Center
        rotation: wheel.sectorRotation
        Behavior on rotation { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
        ShapePath {
            fillColor: wheel.sectorC
            strokeColor: wheel.sectorC
            strokeWidth: wheel.cellEdge ? 1 : Math.max(2 * wheel.srad, 1)
            joinStyle: ShapePath.RoundJoin
            capStyle: ShapePath.RoundCap
            startX: wheel.cx + wheel.sro * Math.cos(wheel.wa0o)
            startY: wheel.cy + wheel.sro * Math.sin(wheel.wa0o)
            PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: wheel.sro; radiusY: wheel.sro
                           startAngle: wheel.degOf(wheel.wa0o); sweepAngle: wheel.degOf(wheel.wa1o - wheel.wa0o) }
            PathLine { x: wheel.cx + wheel.sri * Math.cos(wheel.wa1i); y: wheel.cy + wheel.sri * Math.sin(wheel.wa1i) }
            PathAngleArc { centerX: wheel.cx; centerY: wheel.cy; radiusX: wheel.sri; radiusY: wheel.sri
                           startAngle: wheel.degOf(wheel.wa1i); sweepAngle: -wheel.degOf(wheel.wa1i - wheel.wa0i) }
            PathLine { x: wheel.cx + wheel.sro * Math.cos(wheel.wa0o); y: wheel.cy + wheel.sro * Math.sin(wheel.wa0o) }
        }
        // cell-shading only: the two radial divider edges, tucked under the rim
        ShapePath {
            fillColor: "transparent"
            strokeColor: wheel.cellEdge ? wheel.edgeC : "transparent"
            strokeWidth: wheel.cellEdge ? wheel.edgeW : 0
            capStyle: ShapePath.FlatCap
            startX: wheel.cx + (wheel.outerR - wheel.edgeW / 2) * Math.cos(wheel.wcBase - wheel.wspanRad / 2)
            startY: wheel.cy + (wheel.outerR - wheel.edgeW / 2) * Math.sin(wheel.wcBase - wheel.wspanRad / 2)
            PathLine { x: wheel.cx + (wheel.innerR + wheel.edgeW / 2) * Math.cos(wheel.wcBase - wheel.wspanRad / 2)
                       y: wheel.cy + (wheel.innerR + wheel.edgeW / 2) * Math.sin(wheel.wcBase - wheel.wspanRad / 2) }
            PathMove { x: wheel.cx + (wheel.outerR - wheel.edgeW / 2) * Math.cos(wheel.wcBase + wheel.wspanRad / 2)
                       y: wheel.cy + (wheel.outerR - wheel.edgeW / 2) * Math.sin(wheel.wcBase + wheel.wspanRad / 2) }
            PathLine { x: wheel.cx + (wheel.innerR + wheel.edgeW / 2) * Math.cos(wheel.wcBase + wheel.wspanRad / 2)
                       y: wheel.cy + (wheel.innerR + wheel.edgeW / 2) * Math.sin(wheel.wcBase + wheel.wspanRad / 2) }
        }
    }

    // ---- radial / half: rim outline (two navy annuli, drawn on top) ----
    Canvas {
        id: rim
        anchors.fill: parent
        visible: !wheel.isBar && wheel.edgeW > 0.01
        onPaint: {
            var ctx = getContext("2d"); ctx.reset()
            if (wheel.isBar || wheel.edgeW <= 0.01) return
            var op = wheel.st.wheelOpacity, ew = wheel.edgeW
            function css(col, a) { return "rgba(" + Math.round(col.r * 255) + "," + Math.round(col.g * 255) + "," + Math.round(col.b * 255) + "," + a + ")" }
            ctx.lineWidth = ew; ctx.strokeStyle = css(wheel.edgeC, wheel.edgeA * op)
            ctx.beginPath(); ctx.arc(wheel.cx, wheel.cy, wheel.outerR - ew / 2, 0, 2 * Math.PI); ctx.stroke()
            ctx.beginPath(); ctx.arc(wheel.cx, wheel.cy, wheel.innerR + ew / 2, 0, 2 * Math.PI); ctx.stroke()
        }
        Connections { target: Launcher; function onSettingsChanged() { rim.requestPaint() } }
        Connections { target: wheel; function onBandCChanged() { rim.requestPaint() } }
        Connections { target: wheel; function onEdgeCChanged() { rim.requestPaint() } }
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
    }

    // ---- bar layout: band + inactive slots + sliding accent ----
    Item {
        anchors.fill: parent
        visible: wheel.isBar
        Rectangle {   // band
            anchors.centerIn: parent
            width: wheel.barVertical ? wheel.barThick : wheel.barLen
            height: wheel.barVertical ? wheel.barLen : wheel.barThick
            radius: Math.min(Skin.s(18) * wheel.uiScale, wheel.barThick / 2)
            color: Qt.rgba(wheel.bandC.r, wheel.bandC.g, wheel.bandC.b, wheel.st.wheelOpacity)
            border.width: wheel.edgeW
            border.color: Qt.rgba(wheel.edgeC.r, wheel.edgeC.g, wheel.edgeC.b, wheel.edgeA * wheel.st.wheelOpacity)
        }
        Repeater {   // inactive slots
            model: Skin.segBg.a > 0.001 ? wheel.count : 0
            delegate: Rectangle {
                required property int index
                readonly property real along: (wheel.barVertical ? (wheel.height - wheel.barLen) / 2 : (wheel.width - wheel.barLen) / 2)
                                               + wheel.barPad + index * wheel.barPitch + wheel.hgap + wheel.sinset
                readonly property real sz: wheel.barPitch - 2 * wheel.hgap - 2 * wheel.sinset
                readonly property real across: wheel.barThick - 4 * wheel.sinset
                x: wheel.barVertical ? (wheel.width - across) / 2 : along
                y: wheel.barVertical ? along : (wheel.height - across) / 2
                width: wheel.barVertical ? across : sz
                height: wheel.barVertical ? sz : across
                radius: wheel.irad
                color: Skin.segBg
            }
        }
        Rectangle {   // sliding accent
            property real along: (wheel.barVertical ? (wheel.height - wheel.barLen) / 2 : (wheel.width - wheel.barLen) / 2)
                                          + wheel.barPad + Math.max(wheel.activeIndex, 0) * wheel.barPitch + wheel.hgap + wheel.sinset
            Behavior on along { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
            readonly property real sz: wheel.barPitch - 2 * wheel.hgap - 2 * wheel.sinset
            readonly property real across: wheel.barThick - 4 * wheel.sinset
            x: wheel.barVertical ? (wheel.width - across) / 2 : along
            y: wheel.barVertical ? along : (wheel.height - across) / 2
            width: wheel.barVertical ? across : sz
            height: wheel.barVertical ? sz : across
            radius: Math.max(wheel.srad, 1)
            color: wheel.sectorC
            border.width: wheel.cellEdge ? wheel.edgeW : 0
            border.color: wheel.edgeC
            opacity: wheel.effIndex >= 0 && wheel.count > 0 ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 110; easing.type: Easing.OutCubic } }
        }
    }

    // ---- centre name pill (radial/half in the hole; bar above the strip) ----
    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.horizontalCenterOffset: wheel.isBar ? 0 : wheel.ccOffX
        anchors.verticalCenterOffset: wheel.isBar
            ? -(wheel.barThick / 2 + implicitHeight / 2 + Skin.s(10) * wheel.uiScale)
            : (wheel.ccOffY + (Launcher.mode === "actions" && !wheel.isHalf ? wheel.innerR * 0.46 : 0))
        z: 3
        implicitWidth: label.implicitWidth + Skin.s(24) * wheel.uiScale
        implicitHeight: label.implicitHeight + Skin.s(12) * wheel.uiScale
        radius: Skin.s(9) * wheel.uiScale
        color: Skin.labelPillBg
        visible: wheel.st.showLabels && !wheel.showSettingsBtn && !wheel.thumbActive
        opacity: wheel.shownLabel !== "" ? 1 : 0
        scale: wheel.shownLabel !== "" ? 1 : 0.85
        Behavior on opacity { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 160; easing.type: Easing.OutBack } }
        Text {
            id: label
            anchors.centerIn: parent
            text: wheel.shownLabel
            color: wheel.labelFg
            font.family: Skin.font; font.pixelSize: Skin.s(14) * wheel.uiScale; font.weight: Font.Medium
            renderType: Text.NativeRendering
        }
    }

    // "actions" mode: the focused window's app icon, upper half of the hole
    Image {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.horizontalCenterOffset: wheel.ccOffX
        anchors.verticalCenterOffset: wheel.ccOffY - wheel.innerR * 0.24
        readonly property var fa: Launcher.focusedApp
        visible: !wheel.isBar && Launcher.mode === "actions" && !wheel.showSettingsBtn && fa !== null
        width: wheel.innerR * 0.74; height: width
        sourceSize.width: width; sourceSize.height: width
        source: fa ? Launcher.iconSource(fa.icon) : ""
        smooth: true
    }

    // live window thumbnail of the selected window, centred in the hole
    Loader {
        anchors.centerIn: parent
        anchors.horizontalCenterOffset: wheel.ccOffX
        anchors.verticalCenterOffset: wheel.ccOffY
        active: wheel.thumbActive
        visible: active && !wheel.showSettingsBtn
        sourceComponent: Item {
            id: thumb
            readonly property real avail: wheel.innerR * 1.3
            readonly property real m: Skin.s(4) * wheel.uiScale
            readonly property real gap: Skin.s(6) * wheel.uiScale
            readonly property real maxW: avail - 2 * m
            readonly property real maxH: avail - cap.implicitHeight - gap - 2 * m
            implicitWidth: avail
            implicitHeight: view.height + 2 * m + gap + cap.implicitHeight
            Rectangle {
                anchors.fill: view; anchors.margins: -thumb.m
                color: Qt.rgba(8 / 255, 8 / 255, 10 / 255, 0.94)
                radius: Skin.s(10) * wheel.uiScale
                border.width: 1; border.color: Qt.rgba(1, 1, 1, 0.16)
            }
            ScreencopyView {
                id: view
                captureSource: wheel.thumbSource
                live: true; paintCursor: false
                readonly property real a: (sourceSize.width > 0 && sourceSize.height > 0)
                                          ? sourceSize.width / sourceSize.height : 1.6
                width:  Math.max(1, Math.min(thumb.maxW, thumb.maxH * a))
                height: width / a
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: parent.top
            }
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
                    color: wheel.labelFg; font.family: Skin.font; font.pixelSize: Skin.s(12) * wheel.uiScale
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

    // ---- app icons (scale with the cursor-angle slice) ----
    Repeater {
        model: wheel.ring
        delegate: Item {
            id: slot
            required property int index
            required property var modelData
            readonly property real ang: wheel.sliceAng(index)
            width: wheel.iconBox; height: wheel.iconBox
            x: wheel.isBar
               ? (wheel.barVertical ? (wheel.width - width) / 2
                                     : ((wheel.width - wheel.barLen) / 2 + wheel.barPad + index * wheel.barPitch + (wheel.barPitch - width) / 2))
               : (wheel.cx + wheel.ringR * Math.cos(ang) - width / 2)
            y: wheel.isBar
               ? (wheel.barVertical ? ((wheel.height - wheel.barLen) / 2 + wheel.barPad + index * wheel.barPitch + (wheel.barPitch - height) / 2)
                                     : (wheel.height - height) / 2)
               : (wheel.cy + wheel.ringR * Math.sin(ang) - height / 2)
            z: wheel.effIndex === index ? 2 : 1

            readonly property int winCount: { void Compositor.windows; return modelData ? Launcher.windowsFor(modelData).length : 0 }
            readonly property int winSelIdx: { void Compositor.windows; void Launcher.winSel; return modelData ? Launcher.selectedWindowIndex(modelData) : -1 }
            readonly property color slotAccent: (modelData && modelData.accent) ? modelData.accent : wheel.accentC

            readonly property bool glyphOnly: !slot.modelData.icon && !!slot.modelData.glyph
            readonly property string iconSrc: Launcher.iconSource(slot.modelData.icon)
            Image {
                id: iconImg
                anchors.centerIn: parent
                visible: !slot.glyphOnly && slot.iconSrc !== "" && iconImg.status !== Image.Error
                width: wheel.iconBox - Skin.s(10) * wheel.uiScale; height: width
                sourceSize.width: width; sourceSize.height: width
                source: slot.iconSrc
                smooth: true
                scale: wheel.effIndex === slot.index ? 1.14 : 1
                Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutBack } }
                layer.enabled: !!slot.modelData.isAction
                layer.effect: MultiEffect {
                    colorization: 1.0
                    colorizationColor: slot.modelData.color ? slot.modelData.color : wheel.inkC
                }
            }
            Text {
                anchors.centerIn: parent
                visible: !slot.glyphOnly && (slot.iconSrc === "" || iconImg.status === Image.Error)
                text: slot.modelData.glyph || Launcher.gApp
                font.family: Skin.iconFont; font.pixelSize: wheel.iconBox * 0.46
                color: wheel.inkC; renderType: Text.NativeRendering
                scale: wheel.effIndex === slot.index ? 1.14 : 1
                Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutBack } }
            }
            Text {
                anchors.centerIn: parent
                visible: slot.glyphOnly
                text: slot.modelData.glyph || ""
                font.family: Skin.iconFont; font.pixelSize: wheel.iconBox * 0.46
                color: slot.modelData.color ? slot.modelData.color : wheel.inkC; renderType: Text.NativeRendering
                scale: wheel.effIndex === slot.index ? 1.14 : 1
                Behavior on scale { NumberAnimation { duration: 150; easing.type: Easing.OutBack } }
            }

            Row {
                visible: wheel.st.showDots && slot.winCount > 0
                spacing: Skin.s(3) * wheel.uiScale
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: iconImg.bottom
                anchors.topMargin: Skin.s(2) * wheel.uiScale
                Repeater {
                    model: Math.min(slot.winCount, 6)
                    delegate: Rectangle {
                        required property int index
                        readonly property bool sel: slot.winCount > 1 && index === slot.winSelIdx
                        width: Skin.s(5) * wheel.uiScale; height: width; radius: width / 2
                        color: sel ? slot.slotAccent
                             : (Skin.dot.a > 0.001 ? Skin.dot : (wheel.bandLum > 0.5 ? Qt.rgba(0, 0, 0, 0.55) : Qt.rgba(1, 1, 1, 0.85)))
                        scale: sel ? 1.2 : 1
                        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutBack } }
                    }
                }
            }
        }
    }

    // ---- single input surface ----
    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        cursorShape: Qt.PointingHandCursor
        pressAndHoldInterval: 400
        onWheel: (w) => {
            if (Launcher.mode === "actions") return
            var s = wheel.sliceAt(w.x, w.y)
            if (s >= 0) Launcher.cycleWindow(wheel.ring[s], w.angleDelta.y > 0 ? -1 : 1)
        }
        onPositionChanged: (m) => wheel.updateHover(m.x, m.y)
        onExited: {
            if (Launcher.settings.followOutside) return
            wheel.hoveredIndex = -1; centerTimer.stop(); wheel.showSettingsBtn = false
        }
        onPressAndHold: (m) => {
            if (Launcher.mode !== "apps") return
            var s = wheel.sliceAt(m.x, m.y)
            if (s >= 0) { Launcher.actionSlice = s; Launcher.actionApp = wheel.ring[s] }
        }
        onClicked: (m) => {
            if (Launcher.actionApp !== null) return
            wheel.handleClick(m.x, m.y, m.button)
        }
    }

    // ---- settings button (radial/half; appears after 2s hovering the hole) ----
    Item {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.horizontalCenterOffset: wheel.ccOffX
        anchors.verticalCenterOffset: wheel.ccOffY
        width: Skin.s(82) * wheel.uiScale; height: width
        visible: !wheel.isBar && (wheel.showSettingsBtn || opacity > 0.01)
        opacity: wheel.showSettingsBtn ? 1 : 0
        scale: wheel.showSettingsBtn ? 1 : 0.6
        Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
        Behavior on scale   { NumberAnimation { duration: 180; easing.type: Easing.OutBack } }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width; height: width
            radius: width / 2
            color: Skin.settingsBtn
            layer.enabled: true
            layer.effect: MultiEffect {
                shadowEnabled: true
                shadowColor: Qt.rgba(0, 0, 0, 0.45)
                shadowBlur: 0.7
                shadowVerticalOffset: 3
                blurMax: 24
            }
        }
        Image {
            anchors.centerIn: parent
            width: parent.width * 0.82; height: width
            sourceSize.width: width; sourceSize.height: width
            source: "RadiAll.png"
            smooth: true
        }
    }
}
