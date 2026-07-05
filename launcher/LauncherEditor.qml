import Quickshell
import QtQuick
import QtQuick.Layouts
import "../services"

// Launcher settings: Apps (list + installed-app picker) and Look (appearance).
// Opened by press-and-hold on an empty zone of the wheel. Edits are live + persisted.
Rectangle {
    id: editor
    implicitWidth: Theme.s(720)
    implicitHeight: Theme.s(560)
    radius: Theme.s(20)
    color: Theme.panelBg
    border.width: 1
    border.color: Qt.rgba(1, 1, 1, 0.10)

    property string tab: "apps"      // apps | look
    property bool picking: false     // installed-app picker overlay
    property int editActionsIdx: -1  // app index whose action menu is being edited (-1 = none)
    property int iconPickerJ: -1     // custom-action index whose icon is being picked (-1 = none)

    // swallow stray clicks so they don't fall through to the backdrop (which closes)
    MouseArea { anchors.fill: parent }

    // ---------- reusable bits ----------
    component Field: Rectangle {
        id: field
        property alias text: input.text
        property string placeholder: ""
        signal edited(string t)
        implicitHeight: Theme.s(30)
        radius: Theme.s(8)
        color: Qt.rgba(1, 1, 1, 0.06)
        border.width: 1
        border.color: input.activeFocus ? Launcher.settings.accent : Qt.rgba(1, 1, 1, 0.08)
        TextInput {
            id: input
            anchors.fill: parent; anchors.leftMargin: Theme.s(8); anchors.rightMargin: Theme.s(8)
            verticalAlignment: TextInput.AlignVCenter
            color: Theme.fg; selectionColor: Launcher.settings.accent; selectByMouse: true; clip: true
            font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering
            onTextEdited: field.edited(text)
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter; x: Theme.s(8)
            visible: input.text === "" && !input.activeFocus
            text: field.placeholder; color: Theme.fgDim
            font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering
        }
    }

    component IconBtn: Rectangle {
        id: btn
        property string glyph: ""
        property bool enabledBtn: true
        signal clicked()
        implicitWidth: Theme.s(28); implicitHeight: Theme.s(28); radius: Theme.s(7)
        opacity: enabledBtn ? 1 : 0.3
        color: ba.containsMouse && enabledBtn ? Qt.rgba(1, 1, 1, 0.14) : Qt.rgba(1, 1, 1, 0.06)
        Text { anchors.centerIn: parent; text: btn.glyph; color: Theme.fg
               font.pixelSize: Theme.s(13); renderType: Text.NativeRendering }
        MouseArea { id: ba; anchors.fill: parent; hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor; onClicked: if (btn.enabledBtn) btn.clicked() }
    }

    component Slider: Item {
        id: sl
        property real from: 0
        property real to: 1
        property real value: 0
        property real step: 0
        signal moved(real v)
        implicitHeight: Theme.s(20)
        function apply(px) {
            var r = Math.max(0, Math.min(1, px / width))
            var v = sl.from + r * (sl.to - sl.from)
            if (sl.step > 0) v = Math.round(v / sl.step) * sl.step
            sl.moved(v)
        }
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width; height: Theme.s(4); radius: height / 2
            color: Qt.rgba(1, 1, 1, 0.14)
            Rectangle {
                width: parent.width * (sl.value - sl.from) / (sl.to - sl.from)
                height: parent.height; radius: height / 2; color: Launcher.settings.accent
            }
        }
        Rectangle {
            width: Theme.s(16); height: width; radius: width / 2; color: "white"
            anchors.verticalCenter: parent.verticalCenter
            x: (parent.width - width) * (sl.value - sl.from) / (sl.to - sl.from)
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                    onPressed: (m) => sl.apply(m.x); onPositionChanged: (m) => sl.apply(m.x) }
    }

    component Toggle: Rectangle {
        id: tg
        property bool on: false
        signal toggled(bool v)
        implicitWidth: Theme.s(42); implicitHeight: Theme.s(24); radius: height / 2
        color: on ? Launcher.settings.accent : Qt.rgba(1, 1, 1, 0.15)
        Behavior on color { ColorAnimation { duration: 120 } }
        Rectangle {
            width: parent.height - Theme.s(4); height: width; radius: width / 2; color: "white"
            y: Theme.s(2); x: tg.on ? parent.width - width - Theme.s(2) : Theme.s(2)
            Behavior on x { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: tg.toggled(!tg.on) }
    }

    component SettingRow: RowLayout {
        property string label: ""
        property real labelWidth: Theme.s(120)
        Layout.fillWidth: true
        spacing: Theme.s(12)
        Text { text: parent.label; color: Theme.fg; Layout.preferredWidth: parent.labelWidth
               font.family: Theme.font; font.pixelSize: Theme.s(13); renderType: Text.NativeRendering }
    }

    // a titled group of settings: uppercase caption above its rows
    component Group: ColumnLayout {
        id: grp
        property string heading: ""
        Layout.fillWidth: true
        spacing: Theme.s(9)
        Text {
            text: grp.heading; color: Theme.fgDim
            font.family: Theme.font; font.pixelSize: Theme.s(10); font.letterSpacing: Theme.s(1.5)
            font.weight: Font.DemiBold; renderType: Text.NativeRendering
            Layout.bottomMargin: Theme.s(1)
        }
    }

    // click-to-record shortcut cell. Shows `value`, emits `captured(str)` on a new
    // combo. Used for the per-ring opening shortcuts and per-app custom shortcuts.
    component ShortcutRecorder: Rectangle {
        id: rec
        property string value: ""              // current shortcut ("" = unset)
        property bool valid: true              // false → red border (invalid combo)
        property real cellWidth: Theme.s(150)
        property bool recording: false
        signal captured(string s)
        Layout.preferredWidth: cellWidth; implicitHeight: Theme.s(30); radius: Theme.s(8)
        color: recording ? Qt.rgba(1, 1, 1, 0.14) : Qt.rgba(1, 1, 1, 0.06)
        border.width: 1
        border.color: recording ? Launcher.settings.accent
                    : (valid ? Qt.rgba(1, 1, 1, 0.08) : "#e0555a")
        Text {
            anchors.centerIn: parent
            text: rec.recording ? "Press keys…"
                : (rec.value ? Launcher.prettyShortcut(rec.value) : "Click to set")
            color: rec.recording ? Theme.fg : (rec.value && rec.valid ? Theme.fg : Theme.fgDim)
            font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering
        }
        MouseArea {
            anchors.fill: parent; cursorShape: Qt.PointingHandCursor
            onClicked: { rec.recording = true; recKeys.forceActiveFocus() }
        }
        Item {
            id: recKeys
            anchors.fill: parent
            Keys.onPressed: (e) => {
                if (!rec.recording) return
                e.accepted = true
                if (e.key === Qt.Key_Escape) { rec.recording = false; focus = false; return }
                var s = Launcher.keyEventToShortcut(e.key, e.modifiers, e.text)
                if (s) { rec.recording = false; focus = false; rec.captured(s) }
            }
        }
    }

    // ---------- header ----------
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.s(18)
        spacing: Theme.s(14)

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.s(10)
            Image {
                Layout.preferredWidth: Theme.s(26); Layout.preferredHeight: Theme.s(26)
                sourceSize.width: Theme.s(26); sourceSize.height: Theme.s(26)
                source: "CuteRing.png"; smooth: true
            }
            Text {
                text: "Launcher"; color: Theme.fgStrong
                font.family: Theme.fontDisplay; font.pixelSize: Theme.s(18); font.weight: Font.DemiBold
                renderType: Text.NativeRendering
            }
            Item { Layout.fillWidth: true }
            // segmented tabs
            Rectangle {
                implicitWidth: seg.implicitWidth + Theme.s(6); implicitHeight: Theme.s(32)
                radius: Theme.s(9); color: Qt.rgba(1, 1, 1, 0.06)
                RowLayout { id: seg; anchors.centerIn: parent; spacing: 0
                    Repeater {
                        model: [{ k: "apps", t: "Apps" }, { k: "look", t: "Look" }]
                        delegate: Rectangle {
                            required property var modelData
                            implicitWidth: Theme.s(70); implicitHeight: Theme.s(26); radius: Theme.s(7)
                            color: editor.tab === modelData.k ? Launcher.settings.accent : "transparent"
                            Behavior on color { ColorAnimation { duration: 130 } }
                            Text { anchors.centerIn: parent; text: modelData.t
                                   color: editor.tab === modelData.k ? "white" : Theme.fg
                                   font.family: Theme.font; font.pixelSize: Theme.s(12); font.weight: Font.Medium
                                   renderType: Text.NativeRendering }
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                        onClicked: editor.tab = modelData.k }
                        }
                    }
                }
            }
            Rectangle {
                implicitWidth: Theme.s(66); implicitHeight: Theme.s(32); radius: Theme.s(9)
                color: doneMa.containsMouse ? Qt.lighter(Launcher.settings.accent, 1.15) : Launcher.settings.accent
                Text { anchors.centerIn: parent; text: "Done"; color: "white"
                       font.family: Theme.font; font.pixelSize: Theme.s(13); font.weight: Font.Medium
                       renderType: Text.NativeRendering }
                MouseArea { id: doneMa; anchors.fill: parent; hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor; onClicked: Launcher.commit() }
            }
        }

        // ---------- APPS TAB ----------
        ColumnLayout {
            Layout.fillWidth: true; Layout.fillHeight: true
            visible: editor.tab === "apps"
            spacing: Theme.s(10)

            Flickable {
                Layout.fillWidth: true; Layout.fillHeight: true
                contentHeight: rows.implicitHeight; clip: true
                boundsBehavior: Flickable.StopAtBounds
                ColumnLayout {
                    id: rows
                    width: parent.width; spacing: Theme.s(6)
                    Repeater {
                        model: Launcher.apps
                        delegate: RowLayout {
                            id: r
                            required property int index
                            required property var modelData
                            Layout.fillWidth: true; spacing: Theme.s(6)
                            Image {
                                Layout.preferredWidth: Theme.s(24); Layout.preferredHeight: Theme.s(24)
                                sourceSize.width: Theme.s(24); sourceSize.height: Theme.s(24)
                                source: r.modelData.icon ? Launcher.iconSource(r.modelData.icon) : ""
                                smooth: true
                            }
                            Field { Layout.preferredWidth: Theme.s(120); placeholder: "Name"
                                    Component.onCompleted: text = r.modelData.name
                                    onEdited: (t) => Launcher.setField(r.index, "name", t) }
                            Field { Layout.fillWidth: true; placeholder: "command args"
                                    Component.onCompleted: text = (r.modelData.exec || []).join(" ")
                                    onEdited: (t) => Launcher.setField(r.index, "exec",
                                        t.trim() === "" ? [""] : t.trim().split(/\s+/)) }
                            Field { Layout.preferredWidth: Theme.s(110); placeholder: "icon / path"
                                    Component.onCompleted: text = r.modelData.icon
                                    onEdited: (t) => Launcher.setField(r.index, "icon", t) }
                            Field { Layout.preferredWidth: Theme.s(84); placeholder: "class"
                                    Component.onCompleted: text = r.modelData.wmClass || ""
                                    onEdited: (t) => Launcher.setField(r.index, "wmClass", t) }
                            IconBtn { glyph: "⋮"; onClicked: editor.editActionsIdx = r.index }   // action menu
                            IconBtn { glyph: "↑"; enabledBtn: r.index > 0; onClicked: Launcher.moveApp(r.index, -1) }
                            IconBtn { glyph: "↓"; enabledBtn: r.index < Launcher.apps.length - 1; onClicked: Launcher.moveApp(r.index, 1) }
                            IconBtn { glyph: "✕"; onClicked: Launcher.removeApp(r.index) }
                        }
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true; spacing: Theme.s(8)
                Rectangle {
                    Layout.fillWidth: true; implicitHeight: Theme.s(34); radius: Theme.s(9)
                    color: pickMa.containsMouse ? Qt.lighter(Launcher.settings.accent, 1.15) : Launcher.settings.accent
                    Text { anchors.centerIn: parent; text: "＋  Add App"; color: "white"
                           font.family: Theme.font; font.pixelSize: Theme.s(13); font.weight: Font.Medium
                           renderType: Text.NativeRendering }
                    MouseArea { id: pickMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor; onClicked: editor.picking = true }
                }
                Rectangle {
                    Layout.preferredWidth: Theme.s(110); implicitHeight: Theme.s(34); radius: Theme.s(9)
                    color: manMa.containsMouse ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(1, 1, 1, 0.06)
                    Text { anchors.centerIn: parent; text: "Manual"; color: Theme.fg
                           font.family: Theme.font; font.pixelSize: Theme.s(13); renderType: Text.NativeRendering }
                    MouseArea { id: manMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor; onClicked: Launcher.addApp() }
                }
            }
        }

        // ---------- LOOK TAB ---------- (two columns; grouped, no scroll needed)
        RowLayout {
            Layout.fillWidth: true; Layout.fillHeight: true
            visible: editor.tab === "look"
            spacing: Theme.s(28)

            // -- left: colours + dimensions --
            ColumnLayout {
                Layout.fillWidth: true; Layout.preferredWidth: Theme.s(320)
                Layout.alignment: Qt.AlignTop; spacing: Theme.s(20)

                Group {
                    heading: "COLOURS"
                    SettingRow {
                        label: "Accent"; labelWidth: Theme.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Theme.s(8)
                            Repeater {
                                model: ["#0a84ff", "#5e5ce6", "#bf5af2", "#ff375f", "#ff9f0a", "#30d158", "#64d2ff"]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Theme.s(22); implicitHeight: Theme.s(22); radius: width / 2
                                    color: modelData
                                    border.width: Launcher.settings.accent === modelData ? 3 : 0
                                    border.color: Qt.rgba(1, 1, 1, 0.9)
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("accent", modelData) }
                                }
                            }
                            Item { Layout.fillWidth: true }
                        }
                    }
                    SettingRow {
                        label: "Background"; labelWidth: Theme.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Theme.s(8)
                            Repeater {
                                model: [
                                    { c: "#0b0b0d", n: "OLED" },
                                    { c: "#2b2b30", n: "Dark" },
                                    { c: "#dbe4f2", n: "Light" },
                                    { c: "#c9d8ef", n: "Blue" }
                                ]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Theme.s(22); implicitHeight: Theme.s(22); radius: width / 2
                                    color: modelData.c
                                    border.width: Launcher.settings.bg === modelData.c ? 3 : 1
                                    border.color: Launcher.settings.bg === modelData.c ? Qt.rgba(1, 1, 1, 0.9) : Qt.rgba(1, 1, 1, 0.2)
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("bg", modelData.c) }
                                }
                            }
                            Field {
                                id: bgField
                                Layout.preferredWidth: Theme.s(84); placeholder: "#custom"
                                Component.onCompleted: text = Launcher.settings.bg
                                onEdited: (t) => { if (/^#[0-9a-fA-F]{6}$/.test(t)) Launcher.setSetting("bg", t) }
                                Connections { target: Launcher; function onSettingsChanged() { bgField.text = Launcher.settings.bg } }
                            }
                        }
                    }
                }

                Group {
                    heading: "DIMENSIONS"
                    SettingRow {
                        label: "Icon size"; labelWidth: Theme.s(90)
                        Slider { Layout.fillWidth: true; from: 40; to: 84; step: 2
                                 value: Launcher.settings.iconSize
                                 onMoved: (v) => Launcher.setSetting("iconSize", v) }
                        Text { text: Launcher.settings.iconSize + "px"; color: Theme.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Theme.s(40)
                               font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Ring size"; labelWidth: Theme.s(90)
                        Slider { Layout.fillWidth: true; from: 110; to: 230; step: 5
                                 value: Launcher.settings.ringRadius
                                 onMoved: (v) => Launcher.setSetting("ringRadius", v) }
                        Text { text: Launcher.settings.ringRadius + "px"; color: Theme.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Theme.s(40)
                               font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Backdrop dim"; labelWidth: Theme.s(90)
                        Slider { Layout.fillWidth: true; from: 0; to: 0.7; step: 0.02
                                 value: Launcher.settings.dim
                                 onMoved: (v) => Launcher.setSetting("dim", v) }
                        Text { text: Math.round(Launcher.settings.dim * 100) + "%"; color: Theme.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Theme.s(40)
                               font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Wheel opacity"; labelWidth: Theme.s(90)
                        Slider { Layout.fillWidth: true; from: 0.5; to: 1; step: 0.02
                                 value: Launcher.settings.wheelOpacity
                                 onMoved: (v) => Launcher.setSetting("wheelOpacity", v) }
                        Text { text: Math.round(Launcher.settings.wheelOpacity * 100) + "%"; color: Theme.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Theme.s(40)
                               font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering }
                    }
                }
            }

            // -- right: behaviour + shortcuts --
            ColumnLayout {
                Layout.fillWidth: true; Layout.preferredWidth: Theme.s(320)
                Layout.alignment: Qt.AlignTop; spacing: Theme.s(20)

                Group {
                    heading: "BEHAVIOUR"
                    SettingRow {
                        label: "Show label"; labelWidth: Theme.s(150)
                        Toggle { on: Launcher.settings.showLabels; onToggled: (v) => Launcher.setSetting("showLabels", v) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Window thumbnails"; labelWidth: Theme.s(150)
                        Toggle { on: Launcher.settings.thumbnails; onToggled: (v) => Launcher.setSetting("thumbnails", v) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Follow cursor"; labelWidth: Theme.s(150)
                        Toggle { on: Launcher.settings.followOutside; onToggled: (v) => Launcher.setSetting("followOutside", v) }
                        Item { Layout.fillWidth: true }
                    }
                    Text {
                        text: "Follow cursor: the accent sector tracks your mouse even off the ring."
                        color: Theme.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap
                        font.family: Theme.font; font.pixelSize: Theme.s(11); renderType: Text.NativeRendering
                    }
                }

                Group {
                    heading: "RING SHORTCUTS"
                    Text {
                        text: "Global keys that open each ring."
                        color: Theme.fgDim; Layout.fillWidth: true; Layout.bottomMargin: Theme.s(2)
                        font.family: Theme.font; font.pixelSize: Theme.s(11); renderType: Text.NativeRendering
                    }
                    SettingRow {
                        label: "Apps"; labelWidth: Theme.s(110)
                        ShortcutRecorder {
                            cellWidth: Theme.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.apps) || ""
                            onCaptured: (s) => Launcher.setShortcut("apps", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Windows"; labelWidth: Theme.s(110)
                        ShortcutRecorder {
                            cellWidth: Theme.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.windows) || ""
                            onCaptured: (s) => Launcher.setShortcut("windows", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Focus actions"; labelWidth: Theme.s(110)
                        ShortcutRecorder {
                            cellWidth: Theme.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.actions) || ""
                            onCaptured: (s) => Launcher.setShortcut("actions", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                }
            }
        }
    }

    // ---------- APP PICKER OVERLAY ----------
    Rectangle {
        id: picker
        anchors.fill: parent
        radius: parent.radius
        color: parent.color
        visible: editor.picking
        MouseArea { anchors.fill: parent }   // swallow clicks

        property string query: ""

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Theme.s(18)
            spacing: Theme.s(12)

            RowLayout {
                Layout.fillWidth: true; spacing: Theme.s(10)
                IconBtn { glyph: "‹"; onClicked: editor.picking = false }
                Text { text: "Add an app"; color: Theme.fgStrong; Layout.fillWidth: true
                       font.family: Theme.fontDisplay; font.pixelSize: Theme.s(17); font.weight: Font.DemiBold
                       renderType: Text.NativeRendering }
            }
            Field {
                Layout.fillWidth: true; placeholder: "Search installed apps…"
                onEdited: (t) => picker.query = t.toLowerCase()
                Component.onCompleted: text = ""
            }
            Flickable {
                Layout.fillWidth: true; Layout.fillHeight: true
                contentHeight: plist.implicitHeight; clip: true
                boundsBehavior: Flickable.StopAtBounds
                ColumnLayout {
                    id: plist
                    width: parent.width; spacing: Theme.s(2)
                    Repeater {
                        model: {
                            var q = picker.query
                            if (!q) return Launcher.installed
                            return Launcher.installed.filter((a) => a.name.toLowerCase().indexOf(q) !== -1)
                        }
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true
                            implicitHeight: Theme.s(40); radius: Theme.s(8)
                            color: rowMa.containsMouse ? Qt.rgba(1, 1, 1, 0.10) : "transparent"
                            RowLayout {
                                anchors.fill: parent; anchors.leftMargin: Theme.s(10); anchors.rightMargin: Theme.s(10)
                                spacing: Theme.s(12)
                                Image {
                                    Layout.preferredWidth: Theme.s(26); Layout.preferredHeight: Theme.s(26)
                                    sourceSize.width: Theme.s(26); sourceSize.height: Theme.s(26)
                                    source: Launcher.iconSource(modelData.icon)
                                    smooth: true
                                }
                                Text { text: modelData.name; color: Theme.fg; Layout.fillWidth: true; elide: Text.ElideRight
                                       font.family: Theme.font; font.pixelSize: Theme.s(13); renderType: Text.NativeRendering }
                                Text { text: "＋"; color: Theme.fgDim
                                       font.pixelSize: Theme.s(15); renderType: Text.NativeRendering }
                            }
                            MouseArea {
                                id: rowMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                onClicked: { Launcher.addAppObj(modelData); editor.picking = false }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---------- PER-APP ACTION MENU EDITOR ----------
    Rectangle {
        id: actionsPanel
        anchors.fill: parent
        radius: parent.radius
        color: parent.color
        visible: editor.editActionsIdx >= 0
        MouseArea { anchors.fill: parent }

        readonly property var app: editor.editActionsIdx >= 0 ? Launcher.apps[editor.editActionsIdx] : null
        readonly property var templates: app ? Launcher.actionTemplates(app) : []
        readonly property var custom: app && app.customActions ? app.customActions : []
        readonly property var allIds: { var o = []; for (var i = 0; i < templates.length; i++) o.push(templates[i].id); return o }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Theme.s(18)
            spacing: Theme.s(12)

            RowLayout {
                Layout.fillWidth: true; spacing: Theme.s(10)
                IconBtn { glyph: "‹"; onClicked: { Launcher.flush(); editor.editActionsIdx = -1 } }
                Text {
                    Layout.fillWidth: true
                    text: "Actions — " + (actionsPanel.app ? actionsPanel.app.name : "")
                    color: Theme.fgStrong; font.family: Theme.fontDisplay; font.pixelSize: Theme.s(17); font.weight: Font.DemiBold
                    renderType: Text.NativeRendering
                }
                Rectangle {
                    implicitWidth: Theme.s(72); implicitHeight: Theme.s(32); radius: Theme.s(9)
                    color: saveMa.containsMouse ? Qt.lighter(Launcher.settings.accent, 1.15) : Launcher.settings.accent
                    Text { anchors.centerIn: parent; text: "Save"; color: "white"
                           font.family: Theme.font; font.pixelSize: Theme.s(13); font.weight: Font.Medium; renderType: Text.NativeRendering }
                    MouseArea { id: saveMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: { Launcher.flush(); editor.editActionsIdx = -1 } }
                }
            }

            Flickable {
                Layout.fillWidth: true; Layout.fillHeight: true
                contentHeight: acol.implicitHeight; clip: true; boundsBehavior: Flickable.StopAtBounds
                ColumnLayout {
                    id: acol
                    width: parent.width; spacing: Theme.s(6)

                    Text { text: "App & window actions"; color: Theme.fgDim; font.family: Theme.font; font.pixelSize: Theme.s(11); renderType: Text.NativeRendering }
                    Repeater {
                        model: actionsPanel.templates.filter(function (t) { return t.group !== "Custom" })
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true; implicitHeight: Theme.s(40); radius: Theme.s(8); color: Qt.rgba(1, 1, 1, 0.05)
                            RowLayout {
                                anchors.fill: parent; anchors.leftMargin: Theme.s(12); anchors.rightMargin: Theme.s(12); spacing: Theme.s(12)
                                Text { text: modelData.glyph; color: Theme.fg; font.family: Theme.iconFont; font.pixelSize: Theme.s(15); renderType: Text.NativeRendering }
                                Text { text: modelData.label; color: Theme.fg; Layout.fillWidth: true; elide: Text.ElideRight; font.family: Theme.font; font.pixelSize: Theme.s(13); renderType: Text.NativeRendering }
                                Text { text: modelData.group; color: Theme.fgDim; font.family: Theme.font; font.pixelSize: Theme.s(11); renderType: Text.NativeRendering }
                                Toggle {
                                    on: !actionsPanel.app || !actionsPanel.app.actionIds || actionsPanel.app.actionIds.indexOf(modelData.id) >= 0
                                    onToggled: (v) => Launcher.setActionEnabled(editor.editActionsIdx, modelData.id, v, actionsPanel.allIds)
                                }
                            }
                        }
                    }

                    Text { text: "Custom shortcuts  (sent to the running window)"; color: Theme.fgDim; font.family: Theme.font; font.pixelSize: Theme.s(11); Layout.topMargin: Theme.s(8); renderType: Text.NativeRendering }
                    Repeater {
                        model: actionsPanel.custom
                        delegate: RowLayout {
                            id: cr
                            required property int index
                            required property var modelData
                            Layout.fillWidth: true; spacing: Theme.s(6)
                            Rectangle {
                                implicitWidth: Theme.s(30); implicitHeight: Theme.s(30); radius: Theme.s(7)
                                color: iconMa.containsMouse ? Qt.rgba(1, 1, 1, 0.14) : Qt.rgba(1, 1, 1, 0.08)
                                readonly property bool hasPath: cr.modelData.icon && cr.modelData.icon.charAt(0) === "/"
                                Image {
                                    anchors.centerIn: parent; width: Theme.s(20); height: Theme.s(20); sourceSize.width: width; sourceSize.height: width
                                    visible: parent.hasPath; source: parent.hasPath ? "file://" + cr.modelData.icon : ""; smooth: true; asynchronous: true
                                }
                                Text {
                                    anchors.centerIn: parent; visible: !parent.hasPath
                                    text: Launcher.gKey; color: Theme.fgDim; font.family: Theme.iconFont; font.pixelSize: Theme.s(14); renderType: Text.NativeRendering
                                }
                                MouseArea { id: iconMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { picker2.query = ""; editor.iconPickerJ = cr.index } }
                            }
                            Field {
                                Layout.fillWidth: true; placeholder: "Label"
                                Component.onCompleted: text = cr.modelData.label
                                onEdited: (t) => Launcher.setCustomField(editor.editActionsIdx, cr.index, "label", t)
                            }
                            // shortcut recorder: click, then press the real key combo
                            ShortcutRecorder {
                                value: cr.modelData.shortcut || ""
                                valid: Launcher.shortcutValid(cr.modelData.shortcut)
                                onCaptured: (s) => Launcher.setCustomShortcut(editor.editActionsIdx, cr.index, s)
                            }
                            IconBtn { glyph: "✕"; onClicked: Launcher.removeCustomAction(editor.editActionsIdx, cr.index) }
                        }
                    }
                    Rectangle {
                        Layout.fillWidth: true; implicitHeight: Theme.s(32); radius: Theme.s(8)
                        color: addca.containsMouse ? Qt.rgba(1, 1, 1, 0.12) : Qt.rgba(1, 1, 1, 0.06)
                        Text { anchors.centerIn: parent; text: "＋  Add custom shortcut"; color: Theme.fg; font.family: Theme.font; font.pixelSize: Theme.s(12); renderType: Text.NativeRendering }
                        MouseArea { id: addca; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: Launcher.addCustomAction(editor.editActionsIdx) }
                    }
                }
            }
        }

        // ICON PICKER (searchable grid over the whole icon library)
        Rectangle {
            id: picker2
            anchors.fill: parent; radius: parent.radius; color: parent.color
            visible: editor.iconPickerJ >= 0
            property string query: ""
            MouseArea { anchors.fill: parent }
            ColumnLayout {
                anchors.fill: parent; anchors.margins: Theme.s(18); spacing: Theme.s(10)
                RowLayout {
                    Layout.fillWidth: true; spacing: Theme.s(10)
                    IconBtn { glyph: "‹"; onClicked: editor.iconPickerJ = -1 }
                    Text { Layout.fillWidth: true; text: "Pick an icon"; color: Theme.fgStrong; font.family: Theme.fontDisplay; font.pixelSize: Theme.s(17); font.weight: Font.DemiBold; renderType: Text.NativeRendering }
                    Text { text: Launcher.icons.length + " icons"; color: Theme.fgDim; font.family: Theme.font; font.pixelSize: Theme.s(11); renderType: Text.NativeRendering }
                }
                Field { Layout.fillWidth: true; placeholder: "Search icons…"; Component.onCompleted: text = ""; onEdited: (t) => picker2.query = t.toLowerCase() }
                GridView {
                    Layout.fillWidth: true; Layout.fillHeight: true; clip: true
                    cellWidth: Theme.s(52); cellHeight: Theme.s(52)
                    model: {
                        var q = picker2.query, all = Launcher.icons, out = []
                        for (var i = 0; i < all.length && out.length < 400; i++)
                            if (!q || all[i].name.toLowerCase().indexOf(q) !== -1) out.push(all[i])
                        return out
                    }
                    delegate: Item {
                        required property var modelData
                        width: Theme.s(52); height: Theme.s(52)
                        Rectangle {
                            anchors.centerIn: parent; width: Theme.s(46); height: width; radius: Theme.s(9)
                            color: im.containsMouse ? Qt.rgba(1, 1, 1, 0.12) : "transparent"
                            Image {
                                anchors.centerIn: parent; width: Theme.s(28); height: Theme.s(28); sourceSize.width: width; sourceSize.height: width
                                source: "file://" + modelData.path; smooth: true; asynchronous: true
                            }
                            MouseArea {
                                id: im; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                onClicked: { Launcher.setCustomIcon(editor.editActionsIdx, editor.iconPickerJ, modelData.path); editor.iconPickerJ = -1 }
                            }
                        }
                    }
                }
            }
        }
    }
}
