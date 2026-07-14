import Quickshell
import QtQuick
import QtQuick.Layouts
import QtQuick.Effects
import Qt.labs.folderlistmodel
import "../services"

// Launcher settings: Apps (list + installed-app picker) and Look (appearance).
// Opened by press-and-hold on an empty zone of the wheel. Edits are live + persisted.
Rectangle {
    id: editor
    implicitWidth: Skin.s(790)
    implicitHeight: Skin.s(560)
    width: implicitWidth; height: implicitHeight   // pin: content must not resize the panel (it's in a centered Row → any size change makes it jump)
    radius: Skin.s(20)
    color: Skin.panelBg
    border.width: 1
    border.color: Skin.tint(0.10)

    property string tab: "apps"      // apps | look
    property bool picking: false     // installed-app picker overlay
    property int editActionsIdx: -1  // app index whose action menu is being edited (-1 = none)
    property int iconPickerJ: -1     // custom-action index whose icon is being picked (-1 = none)
    property int colorActionJ: -1    // custom-action index whose glyph colour is being picked (-1 = none)
    property bool colorActionGlobal: false  // colorActionJ indexes globalActions (not per-app customActions)
    property bool iconPickerGlobal: false   // iconPickerJ indexes globalActions (not per-app customActions)
    property real colorActionX: 0; property real colorActionY: 0
    property string colorTarget: ""  // which setting the colour picker edits ("accent"|"bg"|"")
    property bool themeMenuOpen: false                 // theme dropdown open?
    property real themeMenuX: 0; property real themeMenuY: 0; property real themeMenuW: 0

    // swallow stray clicks so they don't fall through to the backdrop (which closes)
    MouseArea { anchors.fill: parent }

    // every themes/*.json is a selectable theme — drop a file in, it shows up here.
    FolderListModel {
        id: themeFiles
        folder: "file://" + Quickshell.shellDir + "/themes"
        nameFilters: ["*.json"]
        showDirs: false
        sortField: FolderListModel.Name
    }

    // ---------- reusable bits ----------
    component Field: Rectangle {
        id: field
        property alias text: input.text
        property alias hasFocus: input.activeFocus
        property string placeholder: ""
        signal edited(string t)
        implicitHeight: Skin.s(30)
        radius: Skin.s(8)
        color: Skin.tint(0.06)
        border.width: 1
        border.color: input.activeFocus ? Skin.accent : Skin.tint(0.08)
        TextInput {
            id: input
            anchors.fill: parent; anchors.leftMargin: Skin.s(8); anchors.rightMargin: Skin.s(8)
            verticalAlignment: TextInput.AlignVCenter
            color: Skin.fg; selectionColor: Skin.accent; selectByMouse: true; clip: true
            font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering
            onTextEdited: field.edited(text)
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter; x: Skin.s(8)
            visible: input.text === "" && !input.activeFocus
            text: field.placeholder; color: Skin.fgDim
            font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering
        }
    }

    // rainbow chip that opens the visual colour picker
    component CustomChip: Rectangle {
        id: chip
        signal clicked()
        implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
        border.width: 1; border.color: Skin.tint(0.4)
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0.0; color: "#ff375f" }
            GradientStop { position: 0.5; color: "#30d158" }
            GradientStop { position: 1.0; color: "#0a84ff" }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: chip.clicked() }
    }

    component IconBtn: Rectangle {
        id: btn
        property string glyph: ""
        property bool enabledBtn: true
        signal clicked()
        implicitWidth: Skin.s(28); implicitHeight: Skin.s(28); radius: Skin.s(7)
        opacity: enabledBtn ? 1 : 0.3
        color: ba.containsMouse && enabledBtn ? Skin.tint(0.14) : Skin.tint(0.06)
        Text { anchors.centerIn: parent; text: btn.glyph; color: Skin.fg
               font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
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
        implicitHeight: Skin.s(20)
        function apply(px) {
            var r = Math.max(0, Math.min(1, px / width))
            var v = sl.from + r * (sl.to - sl.from)
            if (sl.step > 0) v = Math.round(v / sl.step) * sl.step
            sl.moved(v)
        }
        Rectangle {
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width; height: Skin.s(4); radius: height / 2
            color: Skin.tint(0.14)
            Rectangle {
                width: parent.width * (sl.value - sl.from) / (sl.to - sl.from)
                height: parent.height; radius: height / 2; color: Skin.accent
            }
        }
        Rectangle {
            width: Skin.s(16); height: width; radius: width / 2; color: "white"
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
        implicitWidth: Skin.s(42); implicitHeight: Skin.s(24); radius: height / 2
        color: on ? Skin.accent : Skin.tint(0.15)
        Behavior on color { ColorAnimation { duration: 120 } }
        Rectangle {
            width: parent.height - Skin.s(4); height: width; radius: width / 2; color: "white"
            y: Skin.s(2); x: tg.on ? parent.width - width - Skin.s(2) : Skin.s(2)
            Behavior on x { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: tg.toggled(!tg.on) }
    }

    component SettingRow: RowLayout {
        property string label: ""
        property real labelWidth: Skin.s(120)
        Layout.fillWidth: true
        spacing: Skin.s(12)
        Text { text: parent.label; color: Skin.fg; Layout.preferredWidth: parent.labelWidth
               font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
    }

    // a titled group of settings: uppercase caption above its rows
    component Group: ColumnLayout {
        id: grp
        property string heading: ""
        Layout.fillWidth: true
        spacing: Skin.s(9)
        Text {
            text: grp.heading; color: Skin.fgDim
            font.family: Skin.font; font.pixelSize: Skin.s(10); font.letterSpacing: Skin.s(1.5)
            font.weight: Font.DemiBold; renderType: Text.NativeRendering
            Layout.bottomMargin: Skin.s(1)
        }
    }

    // click-to-record shortcut cell. Shows `value`, emits `captured(str)` on a new
    // combo. Used for the per-ring opening shortcuts and per-app custom shortcuts.
    component ShortcutRecorder: Rectangle {
        id: rec
        property string value: ""              // current shortcut ("" = unset)
        property bool valid: true              // false → red border (invalid combo)
        property real cellWidth: Skin.s(150)
        property bool recording: false
        signal captured(string s)
        Layout.preferredWidth: cellWidth; implicitHeight: Skin.s(30); radius: Skin.s(8)
        color: recording ? Skin.tint(0.14) : Skin.tint(0.06)
        border.width: 1
        border.color: recording ? Skin.accent
                    : (valid ? Skin.tint(0.08) : "#e0555a")
        Text {
            anchors.centerIn: parent
            text: rec.recording ? "Press keys…"
                : (rec.value ? Launcher.prettyShortcut(rec.value) : "Click to set")
            color: rec.recording ? Skin.fg : (rec.value && rec.valid ? Skin.fg : Skin.fgDim)
            font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering
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
        anchors.margins: Skin.s(18)
        spacing: Skin.s(14)

        RowLayout {
            Layout.fillWidth: true
            spacing: Skin.s(10)
            Image {
                Layout.preferredWidth: Skin.s(42); Layout.preferredHeight: Skin.s(42)
                sourceSize.width: Skin.s(42); sourceSize.height: Skin.s(42)
                source: "RadiAll.png"; smooth: true
            }
            Text {
                text: "RadiAll"; color: Skin.fgStrong
                font.family: Skin.fontDisplay; font.pixelSize: Skin.s(18); font.weight: Font.DemiBold
                renderType: Text.NativeRendering
            }
            Item { Layout.fillWidth: true }
            // segmented tabs
            Rectangle {
                implicitWidth: seg.implicitWidth + Skin.s(6); implicitHeight: Skin.s(32)
                radius: Skin.s(9); color: Skin.tint(0.06)
                RowLayout { id: seg; anchors.centerIn: parent; spacing: 0
                    Repeater {
                        model: [{ k: "apps", t: "Apps" }, { k: "look", t: "Look" }]
                        delegate: Rectangle {
                            required property var modelData
                            implicitWidth: Skin.s(70); implicitHeight: Skin.s(26); radius: Skin.s(7)
                            color: editor.tab === modelData.k ? Skin.accent : "transparent"
                            Behavior on color { ColorAnimation { duration: 130 } }
                            Text { anchors.centerIn: parent; text: modelData.t
                                   color: editor.tab === modelData.k ? "white" : Skin.fg
                                   font.family: Skin.font; font.pixelSize: Skin.s(12); font.weight: Font.Medium
                                   renderType: Text.NativeRendering }
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                        onClicked: editor.tab = modelData.k }
                        }
                    }
                }
            }
            Rectangle {
                implicitWidth: Skin.s(66); implicitHeight: Skin.s(32); radius: Skin.s(9)
                color: resetMa.containsMouse ? Skin.tint(0.12) : Skin.tint(0.06)
                Text { anchors.centerIn: parent; text: "Reset"; color: Skin.fg
                       font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium
                       renderType: Text.NativeRendering }
                MouseArea { id: resetMa; anchors.fill: parent; hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor; onClicked: Launcher.resetSettings() }
            }
            Rectangle {
                implicitWidth: Skin.s(66); implicitHeight: Skin.s(32); radius: Skin.s(9)
                color: doneMa.containsMouse ? Qt.lighter(Skin.accent, 1.15) : Skin.accent
                Text { anchors.centerIn: parent; text: "Done"; color: "white"
                       font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium
                       renderType: Text.NativeRendering }
                MouseArea { id: doneMa; anchors.fill: parent; hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor; onClicked: Launcher.commit() }
            }
        }

        // ---------- APPS TAB ----------
        ColumnLayout {
            Layout.fillWidth: true; Layout.fillHeight: true
            visible: editor.tab === "apps"
            spacing: Skin.s(10)

            Flickable {
                Layout.fillWidth: true; Layout.fillHeight: true
                contentHeight: rows.implicitHeight; clip: true
                boundsBehavior: Flickable.StopAtBounds
                ColumnLayout {
                    id: rows
                    width: parent.width; spacing: Skin.s(6)
                    Repeater {
                        model: Launcher.apps
                        delegate: Rectangle {
                            id: r
                            required property int index
                            required property var modelData
                            Layout.fillWidth: true
                            implicitHeight: Skin.s(46); radius: Skin.s(10)
                            color: rhov.containsMouse ? Skin.tint(0.07) : Skin.tint(0.035)
                            HoverHandler { id: rhov }
                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: Skin.s(10); anchors.rightMargin: Skin.s(8)
                                spacing: Skin.s(6)
                                Image {
                                    Layout.preferredWidth: Skin.s(26); Layout.preferredHeight: Skin.s(26)
                                    sourceSize.width: Skin.s(26); sourceSize.height: Skin.s(26)
                                    source: r.modelData.icon ? Launcher.iconSource(r.modelData.icon) : ""
                                    smooth: true
                                }
                                Field { Layout.preferredWidth: Skin.s(118); placeholder: "Name"
                                        Component.onCompleted: text = r.modelData.name
                                        onEdited: (t) => Launcher.setField(r.index, "name", t) }
                                Field { Layout.fillWidth: true; placeholder: "command args"
                                        Component.onCompleted: text = (r.modelData.exec || []).join(" ")
                                        onEdited: (t) => Launcher.setField(r.index, "exec",
                                            t.trim() === "" ? [""] : t.trim().split(/\s+/)) }
                                Field { Layout.preferredWidth: Skin.s(104); placeholder: "icon / path"
                                        Component.onCompleted: text = r.modelData.icon
                                        onEdited: (t) => Launcher.setField(r.index, "icon", t) }
                                Field { Layout.preferredWidth: Skin.s(80); placeholder: "class"
                                        Component.onCompleted: text = r.modelData.wmClass || ""
                                        onEdited: (t) => Launcher.setField(r.index, "wmClass", t) }
                                IconBtn { glyph: "⋮"; onClicked: editor.editActionsIdx = r.index }   // action menu
                                Rectangle { Layout.preferredWidth: 1; Layout.preferredHeight: Skin.s(20)
                                            Layout.leftMargin: Skin.s(2); Layout.rightMargin: Skin.s(2)
                                            color: Skin.tint(0.10) }   // separates edit from reorder/delete
                                IconBtn { glyph: "↑"; enabledBtn: r.index > 0; onClicked: Launcher.moveApp(r.index, -1) }
                                IconBtn { glyph: "↓"; enabledBtn: r.index < Launcher.apps.length - 1; onClicked: Launcher.moveApp(r.index, 1) }
                                IconBtn { glyph: "✕"; onClicked: Launcher.removeApp(r.index) }
                            }
                        }
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true; spacing: Skin.s(8)
                Rectangle {
                    Layout.fillWidth: true; implicitHeight: Skin.s(34); radius: Skin.s(9)
                    color: pickMa.containsMouse ? Qt.lighter(Skin.accent, 1.15) : Skin.accent
                    RowLayout {
                        anchors.centerIn: parent; spacing: Skin.s(8)
                        Text { text: "\uf067"; color: "white"; font.family: Skin.iconFont
                               font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                        Text { text: "Add App"; color: "white"
                               font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium
                               renderType: Text.NativeRendering }
                    }
                    MouseArea { id: pickMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor; onClicked: editor.picking = true }
                }
                Rectangle {
                    Layout.preferredWidth: Skin.s(110); implicitHeight: Skin.s(34); radius: Skin.s(9)
                    color: manMa.containsMouse ? Skin.tint(0.12) : Skin.tint(0.06)
                    Text { anchors.centerIn: parent; text: "Manual"; color: Skin.fg
                           font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                    MouseArea { id: manMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor; onClicked: Launcher.addApp() }
                }
            }
        }

        // ---------- LOOK TAB ---------- (two columns; grouped, no scroll needed)
        RowLayout {
            Layout.fillWidth: true; Layout.fillHeight: true
            visible: editor.tab === "look"
            spacing: Skin.s(16)

            // -- left card: theme + colours + dimensions --
            Rectangle {
                Layout.fillWidth: true; Layout.fillHeight: true; Layout.preferredWidth: Skin.s(330)
                radius: Skin.s(14); color: Skin.tint(0.035)
                border.width: 1; border.color: Skin.tint(0.07)
                ColumnLayout {
                    anchors.fill: parent; anchors.margins: Skin.s(16)
                    spacing: Skin.s(18)

                Group {
                    heading: "THEME"
                    // dropdown — collapses to one row, opens a scrollable list, so it
                    // stays tidy whether there are 3 themes or 30.
                    Rectangle {
                        id: themeBtn
                        Layout.fillWidth: true; implicitHeight: Skin.s(32); radius: Skin.s(8)
                        color: tdd.containsMouse || editor.themeMenuOpen ? Skin.tint(0.12) : Skin.tint(0.06)
                        border.width: 1
                        border.color: editor.themeMenuOpen ? Skin.accent : Skin.tint(0.10)
                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: Skin.s(12); anchors.rightMargin: Skin.s(10)
                            spacing: Skin.s(8)
                            Text {
                                text: Skin.name; color: Skin.fg; Layout.fillWidth: true
                                font.family: Skin.font; font.pixelSize: Skin.s(13)
                                font.capitalization: Font.Capitalize; elide: Text.ElideRight
                                renderType: Text.NativeRendering
                            }
                            Text {
                                text: "⌄"; color: Skin.fgDim; font.pixelSize: Skin.s(15)
                                rotation: editor.themeMenuOpen ? 180 : 0
                                Behavior on rotation { NumberAnimation { duration: 130 } }
                                renderType: Text.NativeRendering
                            }
                        }
                        MouseArea {
                            id: tdd; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                var p = themeBtn.mapToItem(editor, 0, themeBtn.height + Skin.s(4))
                                editor.themeMenuX = p.x; editor.themeMenuY = p.y; editor.themeMenuW = themeBtn.width
                                editor.themeMenuOpen = true
                            }
                        }
                    }
                    Text {
                        text: "Accent & Background below apply to themes that don't set their own."
                        color: Skin.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap
                        font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering
                    }
                    RowLayout {
                        Layout.fillWidth: true; spacing: Skin.s(8)
                        Field {
                            id: themeNameField
                            Layout.fillWidth: true; placeholder: "New theme name"
                            Component.onCompleted: text = ""
                        }
                        Rectangle {
                            implicitWidth: Skin.s(104); implicitHeight: Skin.s(30); radius: Skin.s(8)
                            opacity: themeNameField.text.trim() === "" ? 0.4 : 1
                            color: saveThemeMa.containsMouse && themeNameField.text.trim() !== ""
                                 ? Qt.lighter(Skin.accent, 1.15) : Skin.accent
                            Text { anchors.centerIn: parent; text: "Save as theme"; color: "white"
                                   font.family: Skin.font; font.pixelSize: Skin.s(11); font.weight: Font.Medium
                                   renderType: Text.NativeRendering }
                            MouseArea {
                                id: saveThemeMa; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    var n = themeNameField.text.trim()
                                    if (n !== "") { Launcher.saveAsTheme(n); themeNameField.text = "" }
                                }
                            }
                        }
                    }
                }

                Group {
                    heading: "LAYOUT"
                    SettingRow {
                        label: "Shape"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(6)
                            Repeater {
                                model: [{ k: "radial", t: "Radial" }, { k: "bar", t: "Bar" }, { k: "half", t: "Half" }]
                                delegate: Rectangle {
                                    required property var modelData
                                    Layout.fillWidth: true; implicitHeight: Skin.s(28); radius: Skin.s(7)
                                    color: Launcher.settings.layout === modelData.k ? Skin.accent : Skin.tint(0.06)
                                    Behavior on color { ColorAnimation { duration: 130 } }
                                    Text { anchors.centerIn: parent; text: modelData.t
                                           color: Launcher.settings.layout === modelData.k ? "white" : Skin.fg
                                           font.family: Skin.font; font.pixelSize: Skin.s(12); font.weight: Font.Medium
                                           renderType: Text.NativeRendering }
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("layout", modelData.k) }
                                }
                            }
                        }
                    }
                    SettingRow {
                        label: "Position"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(6)
                            Repeater {
                                model: [{ k: "center", t: "●" }, { k: "left", t: "◀" }, { k: "right", t: "▶" }, { k: "top", t: "▲" }, { k: "bottom", t: "▼" }]
                                delegate: Rectangle {
                                    required property var modelData
                                    Layout.fillWidth: true; implicitHeight: Skin.s(28); radius: Skin.s(7)
                                    color: Launcher.settings.position === modelData.k ? Skin.accent : Skin.tint(0.06)
                                    Behavior on color { ColorAnimation { duration: 130 } }
                                    Text { anchors.centerIn: parent; text: modelData.t
                                           color: Launcher.settings.position === modelData.k ? "white" : Skin.fg
                                           font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium
                                           renderType: Text.NativeRendering }
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("position", modelData.k) }
                                }
                            }
                        }
                    }
                }

                Group {
                    heading: "COLOURS"
                    SettingRow {
                        label: "Accent"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(8)
                            Repeater {
                                model: ["#e44854", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff9f0a", "#30d158", "#64d2ff"]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                    color: modelData
                                    border.width: Skin.accent === modelData ? 3 : 0
                                    border.color: Skin.tint(0.9)
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("accent", modelData) }
                                }
                            }
                            CustomChip { onClicked: editor.colorTarget = "accent" }
                            Item { Layout.fillWidth: true }
                        }
                    }
                    SettingRow {
                        label: "Background"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(8)
                            Repeater {
                                model: [
                                    { c: "#0b0b0d", n: "OLED" },
                                    { c: "#2b2b30", n: "Dark" },
                                    { c: "#dbe4f2", n: "Light" },
                                    { c: "#c9d8ef", n: "Blue" }
                                ]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                    color: modelData.c
                                    border.width: Launcher.settings.bg === modelData.c ? 3 : 1
                                    border.color: Launcher.settings.bg === modelData.c ? Skin.tint(0.9) : Skin.tint(0.2)
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("bg", modelData.c) }
                                }
                            }
                            CustomChip { onClicked: editor.colorTarget = "bg" }
                            Item { Layout.fillWidth: true }
                        }
                    }
                    SettingRow {
                        label: "Inactive fill"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(8)
                            Repeater {
                                model: ["", "#22ffffff", "#40ffffff", "#18000000", "#28000000", "#40000000"]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                    color: modelData === "" ? "transparent" : modelData
                                    border.width: Launcher.settings.segBg === modelData ? 3 : 1
                                    border.color: Launcher.settings.segBg === modelData ? Skin.tint(0.9) : Skin.tint(0.2)
                                    Text { anchors.centerIn: parent; visible: modelData === ""; text: "∅"; color: Skin.fgDim
                                           font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("segBg", modelData) }
                                }
                            }
                            CustomChip { onClicked: editor.colorTarget = "segBg" }
                            Item { Layout.fillWidth: true }
                        }
                    }
                    SettingRow {
                        label: "Border"; labelWidth: Skin.s(80)
                        RowLayout {
                            Layout.fillWidth: true; spacing: Skin.s(8)
                            Repeater {
                                model: ["", "#000000", "#24242c", "#6b6b78", "#ffffff", "#e44854", "#0a84ff"]
                                delegate: Rectangle {
                                    required property var modelData
                                    implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                    color: modelData === "" ? "transparent" : modelData
                                    border.width: Launcher.settings.border === modelData ? 3 : 1
                                    border.color: Launcher.settings.border === modelData ? Skin.tint(0.9) : Skin.tint(0.2)
                                    Text { anchors.centerIn: parent; visible: modelData === ""; text: "A"; color: Skin.fgDim
                                           font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                                    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                                onClicked: Launcher.setSetting("border", modelData) }
                                }
                            }
                            CustomChip { onClicked: editor.colorTarget = "border" }
                            Item { Layout.fillWidth: true }
                        }
                    }
                }

                Group {
                    heading: "DIMENSIONS"
                    SettingRow {
                        label: "Icon size"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 40; to: 84; step: 2
                                 value: Launcher.settings.iconSize
                                 onMoved: (v) => Launcher.setSetting("iconSize", v) }
                        Text { text: Launcher.settings.iconSize + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Ring size"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 110; to: 230; step: 5
                                 value: Launcher.settings.ringRadius
                                 onMoved: (v) => Launcher.setSetting("ringRadius", v) }
                        Text { text: Launcher.settings.ringRadius + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Backdrop dim"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 0; to: 0.7; step: 0.02
                                 value: Launcher.settings.dim
                                 onMoved: (v) => Launcher.setSetting("dim", v) }
                        Text { text: Math.round(Launcher.settings.dim * 100) + "%"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Wheel opacity"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 0.5; to: 1; step: 0.02
                                 value: Launcher.settings.wheelOpacity
                                 onMoved: (v) => Launcher.setSetting("wheelOpacity", v) }
                        Text { text: Math.round(Launcher.settings.wheelOpacity * 100) + "%"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Hole size"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 40; to: 130; step: 1
                                 value: Launcher.settings.holeSize
                                 onMoved: (v) => Launcher.setSetting("holeSize", v) }
                        Text { text: Launcher.settings.holeSize + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Auto border"; labelWidth: Skin.s(90)
                        Toggle { on: Launcher.settings.borderWidth < 0
                                 onToggled: (v) => Launcher.setSetting("borderWidth", v ? -1 : 2) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Border width"; labelWidth: Skin.s(90)
                        Slider { Layout.fillWidth: true; from: 0; to: 12; step: 0.5
                                 enabled: Launcher.settings.borderWidth >= 0
                                 opacity: Launcher.settings.borderWidth >= 0 ? 1 : 0.35
                                 value: Math.max(0, Launcher.settings.borderWidth)
                                 onMoved: (v) => Launcher.setSetting("borderWidth", v) }
                        Text { text: Launcher.settings.borderWidth < 0 ? "auto" : (Launcher.settings.borderWidth + "px")
                               color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                }
                    Item { Layout.fillHeight: true }   // push groups to top, card fills height
                }
            }

            // -- right card: behaviour + shortcuts --
            Rectangle {
                Layout.fillWidth: true; Layout.fillHeight: true; Layout.preferredWidth: Skin.s(330)
                radius: Skin.s(14); color: Skin.tint(0.035)
                border.width: 1; border.color: Skin.tint(0.07)
                ColumnLayout {
                    anchors.fill: parent; anchors.margins: Skin.s(16)
                    spacing: Skin.s(18)

                Group {
                    heading: "BEHAVIOUR"
                    SettingRow {
                        label: "Show label"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.showLabels; onToggled: (v) => Launcher.setSetting("showLabels", v) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Window thumbnails"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.thumbnails; onToggled: (v) => Launcher.setSetting("thumbnails", v) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Follow cursor"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.followOutside; onToggled: (v) => Launcher.setSetting("followOutside", v) }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        label: "Window dots"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.showDots; onToggled: (v) => Launcher.setSetting("showDots", v) }
                        Item { Layout.fillWidth: true }
                    }
                    Text {
                        text: "Follow cursor: the accent sector tracks your mouse even off the ring."
                        color: Skin.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap
                        font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering
                    }
                }

                Group {
                    heading: "SECTIONS"
                    SettingRow {
                        label: "Active radius"; labelWidth: Skin.s(110)
                        Slider { Layout.fillWidth: true; from: 0; to: 28; step: 0.5
                                 value: Launcher.settings.activeRadius
                                 onMoved: (v) => Launcher.setSetting("activeRadius", v) }
                        Text { text: Launcher.settings.activeRadius + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Inactive radius"; labelWidth: Skin.s(110)
                        Slider { Layout.fillWidth: true; from: 0; to: 28; step: 0.5
                                 value: Launcher.settings.inactiveRadius
                                 onMoved: (v) => Launcher.setSetting("inactiveRadius", v) }
                        Text { text: Launcher.settings.inactiveRadius + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Edge padding"; labelWidth: Skin.s(110)
                        Slider { Layout.fillWidth: true; from: 0; to: 20; step: 0.5
                                 value: Launcher.settings.edgePadding
                                 onMoved: (v) => Launcher.setSetting("edgePadding", v) }
                        Text { text: Launcher.settings.edgePadding + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                    SettingRow {
                        label: "Section gap"; labelWidth: Skin.s(110)
                        Slider { Layout.fillWidth: true; from: 0; to: 24; step: 0.5
                                 value: Launcher.settings.sectionGap
                                 onMoved: (v) => Launcher.setSetting("sectionGap", v) }
                        Text { text: Launcher.settings.sectionGap + "px"; color: Skin.fgDim; horizontalAlignment: Text.AlignRight
                               Layout.preferredWidth: Skin.s(40)
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                    }
                }

                Group {
                    heading: "RING SHORTCUTS"
                    Text {
                        text: Compositor.canManageKeybinds
                            ? "Global keys that open each ring."
                            : "Your compositor has no global-shortcut API RadiAll can drive. Bind these to keys in your compositor's config:"
                        color: Skin.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap; Layout.bottomMargin: Skin.s(2)
                        font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering
                    }
                    SettingRow {
                        visible: Compositor.canManageKeybinds
                        label: "Enabled"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.shortcutsEnabled; onToggled: (v) => Launcher.setShortcutsEnabled(v) }
                        Item { Layout.fillWidth: true }
                    }
                    Text {
                        visible: Compositor.canManageKeybinds && !Launcher.settings.shortcutsEnabled
                        text: "Off — RadiAll grabs no keys. Open a ring from the tray icon, or bind  radiall --apps  yourself."
                        color: Skin.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap; Layout.bottomMargin: Skin.s(4)
                        font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering
                    }
                    Text {
                        visible: !Compositor.canManageKeybinds
                        text: "radiall --apps\nradiall --windows\nradiall --actions"
                        color: Skin.fg; Layout.fillWidth: true; Layout.leftMargin: Skin.s(4); Layout.bottomMargin: Skin.s(4)
                        font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering
                    }
                    SettingRow {
                        visible: Compositor.canManageKeybinds && Launcher.settings.shortcutsEnabled
                        label: "Bind in Hyprland"; labelWidth: Skin.s(150)
                        Toggle { on: Launcher.settings.persistBinds; onToggled: (v) => Launcher.setPersistBinds(v) }
                        Item { Layout.fillWidth: true }
                    }
                    Text {
                        visible: Compositor.canManageKeybinds && Launcher.settings.shortcutsEnabled
                        text: Launcher.settings.persistBinds
                            ? "Keys are written to ~/.config/hypr/launcher-binds.conf and survive reloads."
                            : "Keys are applied live via hyprctl — your Hyprland config is left untouched (re-applied on reload)."
                        color: Skin.fgDim; Layout.fillWidth: true; wrapMode: Text.WordWrap; Layout.bottomMargin: Skin.s(4)
                        font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering
                    }
                    SettingRow {
                        visible: Compositor.canManageKeybinds && Launcher.settings.shortcutsEnabled
                        label: "Apps"; labelWidth: Skin.s(110)
                        ShortcutRecorder {
                            cellWidth: Skin.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.apps) || ""
                            onCaptured: (s) => Launcher.setShortcut("apps", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        visible: Compositor.canManageKeybinds && Launcher.settings.shortcutsEnabled
                        label: "Windows"; labelWidth: Skin.s(110)
                        ShortcutRecorder {
                            cellWidth: Skin.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.windows) || ""
                            onCaptured: (s) => Launcher.setShortcut("windows", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                    SettingRow {
                        visible: Compositor.canManageKeybinds && Launcher.settings.shortcutsEnabled
                        label: "Focus actions"; labelWidth: Skin.s(110)
                        ShortcutRecorder {
                            cellWidth: Skin.s(150)
                            value: (Launcher.settings.shortcuts && Launcher.settings.shortcuts.actions) || ""
                            onCaptured: (s) => Launcher.setShortcut("actions", s)
                        }
                        Item { Layout.fillWidth: true }
                    }
                }
                    Item { Layout.fillHeight: true }   // push groups to top, card fills height
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
            anchors.margins: Skin.s(18)
            spacing: Skin.s(12)

            RowLayout {
                Layout.fillWidth: true; spacing: Skin.s(10)
                IconBtn { glyph: "‹"; onClicked: editor.picking = false }
                Text { text: "Add an app"; color: Skin.fgStrong; Layout.fillWidth: true
                       font.family: Skin.fontDisplay; font.pixelSize: Skin.s(17); font.weight: Font.DemiBold
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
                    width: parent.width; spacing: Skin.s(2)
                    Repeater {
                        model: {
                            var q = picker.query
                            if (!q) return Launcher.installed
                            return Launcher.installed.filter((a) => a.name.toLowerCase().indexOf(q) !== -1)
                        }
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true
                            implicitHeight: Skin.s(40); radius: Skin.s(8)
                            color: rowMa.containsMouse ? Skin.tint(0.10) : "transparent"
                            RowLayout {
                                anchors.fill: parent; anchors.leftMargin: Skin.s(10); anchors.rightMargin: Skin.s(10)
                                spacing: Skin.s(12)
                                Image {
                                    Layout.preferredWidth: Skin.s(26); Layout.preferredHeight: Skin.s(26)
                                    sourceSize.width: Skin.s(26); sourceSize.height: Skin.s(26)
                                    source: Launcher.iconSource(modelData.icon)
                                    smooth: true
                                }
                                Text { text: modelData.name; color: Skin.fg; Layout.fillWidth: true; elide: Text.ElideRight
                                       font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                                Text { text: "＋"; color: Skin.fgDim
                                       font.pixelSize: Skin.s(15); renderType: Text.NativeRendering }
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
            anchors.margins: Skin.s(18)
            spacing: Skin.s(12)

            RowLayout {
                Layout.fillWidth: true; spacing: Skin.s(10)
                IconBtn { glyph: "‹"; onClicked: { Launcher.flush(); editor.editActionsIdx = -1 } }
                Text {
                    Layout.fillWidth: true
                    text: "Actions — " + (actionsPanel.app ? actionsPanel.app.name : "")
                    color: Skin.fgStrong; font.family: Skin.fontDisplay; font.pixelSize: Skin.s(17); font.weight: Font.DemiBold
                    renderType: Text.NativeRendering
                }
                Rectangle {
                    implicitWidth: Skin.s(72); implicitHeight: Skin.s(32); radius: Skin.s(9)
                    color: saveMa.containsMouse ? Qt.lighter(Skin.accent, 1.15) : Skin.accent
                    Text { anchors.centerIn: parent; text: "Save"; color: "white"
                           font.family: Skin.font; font.pixelSize: Skin.s(13); font.weight: Font.Medium; renderType: Text.NativeRendering }
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
                    width: parent.width; spacing: Skin.s(6)

                    Text { text: "Accent"; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                    RowLayout {
                        Layout.fillWidth: true; spacing: Skin.s(8)
                        Rectangle {
                            implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                            color: "transparent"
                            readonly property bool sel: !actionsPanel.app || !actionsPanel.app.accent
                            border.width: sel ? 3 : 1; border.color: sel ? Skin.tint(0.9) : Skin.tint(0.2)
                            Text { anchors.centerIn: parent; text: "A"; color: Skin.fgDim
                                   font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                        onClicked: Launcher.setAppAccent(editor.editActionsIdx, "") }
                        }
                        Repeater {
                            model: ["#e44854", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff9f0a", "#30d158", "#64d2ff", "#ff2d55"]
                            delegate: Rectangle {
                                required property var modelData
                                implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                color: modelData
                                readonly property bool sel: actionsPanel.app && actionsPanel.app.accent === modelData
                                border.width: sel ? 3 : 0; border.color: Skin.tint(0.9)
                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                            onClicked: Launcher.setAppAccent(editor.editActionsIdx, modelData) }
                            }
                        }
                        CustomChip { onClicked: editor.colorTarget = "appAccent" }
                        Item { Layout.fillWidth: true }
                    }
                    Rectangle { Layout.fillWidth: true; implicitHeight: 1; color: Skin.tint(0.08); Layout.topMargin: Skin.s(4); Layout.bottomMargin: Skin.s(4) }
                    Text { text: "App & window actions"; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                    Repeater {
                        model: actionsPanel.templates.filter(function (t) { return t.group !== "Custom" })
                        delegate: Rectangle {
                            required property var modelData
                            Layout.fillWidth: true; implicitHeight: Skin.s(40); radius: Skin.s(8); color: Skin.tint(0.05)
                            RowLayout {
                                anchors.fill: parent; anchors.leftMargin: Skin.s(12); anchors.rightMargin: Skin.s(12); spacing: Skin.s(12)
                                Text { text: modelData.glyph; color: Skin.fg; font.family: Skin.iconFont; font.pixelSize: Skin.s(15); renderType: Text.NativeRendering }
                                Text { text: modelData.label; color: Skin.fg; Layout.fillWidth: true; elide: Text.ElideRight; font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                                Text { text: modelData.group; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                                Toggle {
                                    on: !actionsPanel.app || !actionsPanel.app.actionIds || actionsPanel.app.actionIds.indexOf(modelData.id) >= 0
                                    onToggled: (v) => Launcher.setActionEnabled(editor.editActionsIdx, modelData.id, v, actionsPanel.allIds)
                                }
                            }
                        }
                    }

                    Text { text: "Custom shortcuts  (sent to the running window)"; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); Layout.topMargin: Skin.s(8); renderType: Text.NativeRendering }
                    Repeater {
                        model: actionsPanel.custom
                        delegate: RowLayout {
                            id: cr
                            required property int index
                            required property var modelData
                            Layout.fillWidth: true; spacing: Skin.s(6)
                            Rectangle {
                                implicitWidth: Skin.s(30); implicitHeight: Skin.s(30); radius: Skin.s(7)
                                color: iconMa.containsMouse ? Skin.tint(0.14) : Skin.tint(0.08)
                                readonly property bool hasPath: cr.modelData.icon && cr.modelData.icon.charAt(0) === "/"
                                Image {
                                    anchors.centerIn: parent; width: Skin.s(20); height: Skin.s(20); sourceSize.width: width; sourceSize.height: width
                                    visible: parent.hasPath; source: parent.hasPath ? "file://" + cr.modelData.icon : ""; smooth: true; asynchronous: true
                                    layer.enabled: parent.hasPath
                                    layer.effect: MultiEffect { colorization: 1.0; colorizationColor: cr.modelData.color || Skin.fg }
                                }
                                Text {
                                    anchors.centerIn: parent; visible: !parent.hasPath
                                    text: Launcher.gKey; color: cr.modelData.color || Skin.fgDim
                                    font.family: Skin.iconFont; font.pixelSize: Skin.s(14); renderType: Text.NativeRendering
                                }
                                MouseArea { id: iconMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { picker2.query = ""; editor.iconPickerGlobal = false; editor.iconPickerJ = cr.index } }
                            }
                            // glyph colour — click to pick a tint for the fallback key icon
                            Rectangle {
                                id: colorChip
                                implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                color: cr.modelData.color || "transparent"
                                border.width: cr.modelData.color ? 0 : 1; border.color: Skin.tint(0.3)
                                Rectangle {   // inner ring hint when no colour is set
                                    anchors.centerIn: parent; width: Skin.s(8); height: width; radius: width / 2
                                    visible: !cr.modelData.color; color: "transparent"
                                    border.width: 1; border.color: Skin.tint(0.3)
                                }
                                MouseArea {
                                    anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        var p = colorChip.mapToItem(actionsPanel, 0, colorChip.height + Skin.s(6))
                                        editor.colorActionX = p.x; editor.colorActionY = p.y
                                        editor.colorActionGlobal = false
                                        editor.colorActionJ = cr.index
                                    }
                                }
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
                        Layout.fillWidth: true; implicitHeight: Skin.s(32); radius: Skin.s(8)
                        color: addca.containsMouse ? Skin.tint(0.12) : Skin.tint(0.06)
                        RowLayout {
                            anchors.centerIn: parent; spacing: Skin.s(8)
                            Text { text: "\uf11c"; color: Skin.fg; font.family: Skin.iconFont
                                   font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                            Text { text: "Add custom shortcut"; color: Skin.fg
                                   font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                        }
                        MouseArea { id: addca; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: Launcher.addCustomAction(editor.editActionsIdx) }
                    }
                    Text { text: "Global actions (all apps)"; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); Layout.topMargin: Skin.s(8); renderType: Text.NativeRendering }
                    Repeater {
                        model: Launcher.settings.globalActions
                        delegate: RowLayout {
                            id: gr
                            required property int index
                            required property var modelData
                            Layout.fillWidth: true; spacing: Skin.s(6)
                            Rectangle {
                                implicitWidth: Skin.s(30); implicitHeight: Skin.s(30); radius: Skin.s(7)
                                color: gIconMa.containsMouse ? Skin.tint(0.14) : Skin.tint(0.08)
                                readonly property bool hasPath: gr.modelData.icon && gr.modelData.icon.charAt(0) === "/"
                                Image {
                                    anchors.centerIn: parent; width: Skin.s(20); height: Skin.s(20); sourceSize.width: width; sourceSize.height: width
                                    visible: parent.hasPath; source: parent.hasPath ? "file://" + gr.modelData.icon : ""; smooth: true; asynchronous: true
                                    layer.enabled: parent.hasPath
                                    layer.effect: MultiEffect { colorization: 1.0; colorizationColor: gr.modelData.color || Skin.fg }
                                }
                                Text {
                                    anchors.centerIn: parent; visible: !parent.hasPath
                                    text: Launcher.gKey; color: gr.modelData.color || Skin.fgDim
                                    font.family: Skin.iconFont; font.pixelSize: Skin.s(14); renderType: Text.NativeRendering
                                }
                                MouseArea { id: gIconMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: { picker2.query = ""; editor.iconPickerGlobal = true; editor.iconPickerJ = gr.index } }
                            }
                            Rectangle {
                                id: gColorChip
                                implicitWidth: Skin.s(22); implicitHeight: Skin.s(22); radius: width / 2
                                color: gr.modelData.color || "transparent"
                                border.width: gr.modelData.color ? 0 : 1; border.color: Skin.tint(0.3)
                                Rectangle {
                                    anchors.centerIn: parent; width: Skin.s(8); height: width; radius: width / 2
                                    visible: !gr.modelData.color; color: "transparent"
                                    border.width: 1; border.color: Skin.tint(0.3)
                                }
                                MouseArea {
                                    anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                    onClicked: {
                                        var p = gColorChip.mapToItem(actionsPanel, 0, gColorChip.height + Skin.s(6))
                                        editor.colorActionX = p.x; editor.colorActionY = p.y
                                        editor.colorActionGlobal = true
                                        editor.colorActionJ = gr.index
                                    }
                                }
                            }
                            Field {
                                Layout.fillWidth: true; placeholder: "Label"
                                Component.onCompleted: text = gr.modelData.label
                                onEdited: (t) => Launcher.setGlobalField(gr.index, "label", t)
                            }
                            ShortcutRecorder {
                                value: gr.modelData.shortcut || ""
                                valid: Launcher.shortcutValid(gr.modelData.shortcut)
                                onCaptured: (s) => Launcher.setGlobalShortcut(gr.index, s)
                            }
                            IconBtn { glyph: "✕"; onClicked: Launcher.removeGlobalAction(gr.index) }
                        }
                    }
                    Rectangle {
                        Layout.fillWidth: true; implicitHeight: Skin.s(32); radius: Skin.s(8)
                        color: addga.containsMouse ? Skin.tint(0.12) : Skin.tint(0.06)
                        RowLayout {
                            anchors.centerIn: parent; spacing: Skin.s(8)
                            Text { text: "\uf11c"; color: Skin.fg; font.family: Skin.iconFont
                                   font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                            Text { text: "Add global action"; color: Skin.fg
                                   font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                        }
                        MouseArea { id: addga; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: Launcher.addGlobalAction() }
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
                anchors.fill: parent; anchors.margins: Skin.s(18); spacing: Skin.s(10)
                RowLayout {
                    Layout.fillWidth: true; spacing: Skin.s(10)
                    IconBtn { glyph: "‹"; onClicked: editor.iconPickerJ = -1 }
                    Text { Layout.fillWidth: true; text: "Pick an icon"; color: Skin.fgStrong; font.family: Skin.fontDisplay; font.pixelSize: Skin.s(17); font.weight: Font.DemiBold; renderType: Text.NativeRendering }
                    Text { text: Launcher.icons.length + " icons"; color: Skin.fgDim; font.family: Skin.font; font.pixelSize: Skin.s(11); renderType: Text.NativeRendering }
                }
                Field { Layout.fillWidth: true; placeholder: "Search icons…"; Component.onCompleted: text = ""; onEdited: (t) => picker2.query = t.toLowerCase() }
                GridView {
                    Layout.fillWidth: true; Layout.fillHeight: true; clip: true
                    cellWidth: Skin.s(52); cellHeight: Skin.s(52)
                    model: {
                        var q = picker2.query, all = Launcher.icons, out = []
                        for (var i = 0; i < all.length && out.length < 400; i++)
                            if (!q || all[i].name.toLowerCase().indexOf(q) !== -1) out.push(all[i])
                        return out
                    }
                    delegate: Item {
                        required property var modelData
                        width: Skin.s(52); height: Skin.s(52)
                        Rectangle {
                            anchors.centerIn: parent; width: Skin.s(46); height: width; radius: Skin.s(9)
                            color: im.containsMouse ? Skin.tint(0.12) : "transparent"
                            Image {
                                anchors.centerIn: parent; width: Skin.s(28); height: Skin.s(28); sourceSize.width: width; sourceSize.height: width
                                source: "file://" + modelData.path; smooth: true; asynchronous: true
                                layer.enabled: true   // symbolic icons ship light — tint to fg so they're visible
                                layer.effect: MultiEffect { colorization: 1.0; colorizationColor: Skin.fg }
                            }
                            MouseArea {
                                id: im; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                onClicked: { if (editor.iconPickerGlobal) Launcher.setGlobalIcon(editor.iconPickerJ, modelData.path); else Launcher.setCustomIcon(editor.editActionsIdx, editor.iconPickerJ, modelData.path); editor.iconPickerJ = -1 }
                            }
                        }
                    }
                }
            }
        }

        // ---------- CUSTOM-ACTION GLYPH COLOUR POPOVER ----------
        Item {
            anchors.fill: parent
            visible: editor.colorActionJ >= 0 && editor.colorTarget === ""
            z: 60
            MouseArea { anchors.fill: parent; onClicked: editor.colorActionJ = -1 }   // click away → close
            Rectangle {
                x: Math.min(editor.colorActionX, parent.width - width - Skin.s(10))
                y: editor.colorActionY
                width: Skin.s(152)
                implicitHeight: pcol.implicitHeight + Skin.s(16); height: implicitHeight
                radius: Skin.s(10); color: Skin.panelBg
                border.width: 1; border.color: Skin.tint(0.14)
                MouseArea { anchors.fill: parent }   // swallow clicks inside
                ColumnLayout {
                    id: pcol
                    anchors { left: parent.left; right: parent.right; top: parent.top; margins: Skin.s(8) }
                    spacing: Skin.s(8)
                    Flow {
                        Layout.fillWidth: true; spacing: Skin.s(6)
                        Repeater {
                            model: ["#e44854", "#ff9f0a", "#ffd60a", "#30d158", "#64d2ff", "#0a84ff", "#5e5ce6", "#bf5af2", "#ff2d55", "#ffffff"]
                            delegate: Rectangle {
                                required property var modelData
                                readonly property bool sel: editor.colorActionJ >= 0 && (editor.colorActionGlobal
                                    ? (Launcher.settings.globalActions[editor.colorActionJ] && Launcher.settings.globalActions[editor.colorActionJ].color === modelData)
                                    : (actionsPanel.custom[editor.colorActionJ] && actionsPanel.custom[editor.colorActionJ].color === modelData))
                                width: Skin.s(24); height: width; radius: width / 2
                                color: modelData
                                border.width: sel ? 2 : 0; border.color: Skin.tint(0.9)
                                MouseArea {
                                    anchors.fill: parent; cursorShape: Qt.PointingHandCursor
                                    onClicked: { if (editor.colorActionGlobal) Launcher.setGlobalColor(editor.colorActionJ, modelData); else Launcher.setCustomColor(editor.editActionsIdx, editor.colorActionJ, modelData); editor.colorActionJ = -1 }
                                }
                            }
                        }
                        // full HSV picker — same one used for Accent/Background
                        CustomChip { onClicked: editor.colorTarget = editor.colorActionGlobal ? "globalColor" : "action" }
                    }
                    Rectangle {   // clear back to the theme default
                        Layout.fillWidth: true; implicitHeight: Skin.s(26); radius: Skin.s(7)
                        color: clrMa.containsMouse ? Skin.tint(0.12) : Skin.tint(0.06)
                        Text { anchors.centerIn: parent; text: "Default colour"; color: Skin.fg
                               font.family: Skin.font; font.pixelSize: Skin.s(12); renderType: Text.NativeRendering }
                        MouseArea { id: clrMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                    onClicked: { if (editor.colorActionGlobal) Launcher.setGlobalColor(editor.colorActionJ, ""); else Launcher.setCustomColor(editor.editActionsIdx, editor.colorActionJ, ""); editor.colorActionJ = -1 } }
                    }
                }
            }
        }
    }

    // ---------- THEME DROPDOWN MENU ----------
    Item {
        anchors.fill: parent
        visible: editor.themeMenuOpen
        z: 50
        MouseArea { anchors.fill: parent; onClicked: editor.themeMenuOpen = false }   // click away → close
        Rectangle {
            id: themeMenu
            x: editor.themeMenuX; y: editor.themeMenuY
            width: Math.max(editor.themeMenuW, Skin.s(150))
            // show up to ~6 rows, then scroll — bounded height keeps it tidy at any count
            height: Math.min(list.contentHeight + Skin.s(8), Skin.s(212))
            radius: Skin.s(10); color: Skin.panelBg
            border.width: 1; border.color: Skin.tint(0.14)
            MouseArea { anchors.fill: parent }   // swallow clicks inside the menu
            ListView {
                id: list
                anchors.fill: parent; anchors.margins: Skin.s(4)
                clip: true; model: themeFiles
                boundsBehavior: Flickable.StopAtBounds
                delegate: Rectangle {
                    required property string fileBaseName
                    readonly property bool active: Skin.name === fileBaseName
                    width: ListView.view.width; height: Skin.s(34); radius: Skin.s(7)
                    color: dhov.containsMouse ? Skin.tint(0.10) : "transparent"
                    RowLayout {
                        anchors.fill: parent; anchors.leftMargin: Skin.s(10); anchors.rightMargin: Skin.s(10)
                        spacing: Skin.s(8)
                        Text {
                            text: fileBaseName; Layout.fillWidth: true; elide: Text.ElideRight
                            color: active ? Skin.accent : Skin.fg
                            font.family: Skin.font; font.pixelSize: Skin.s(13)
                            font.weight: active ? Font.DemiBold : Font.Normal
                            font.capitalization: Font.Capitalize; renderType: Text.NativeRendering
                        }
                        Text { text: active ? "✓" : ""; color: Skin.accent
                               font.family: Skin.font; font.pixelSize: Skin.s(13); renderType: Text.NativeRendering }
                    }
                    MouseArea {
                        id: dhov; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                        onClicked: { Launcher.setSetting("theme", fileBaseName); editor.themeMenuOpen = false }
                    }
                }
            }
        }
    }

    // ---------- COLOUR PICKER OVERLAY ----------
    Rectangle {
        id: colorPicker
        anchors.fill: parent; radius: parent.radius
        color: Qt.rgba(0, 0, 0, 0.45)
        visible: editor.colorTarget !== ""
        MouseArea { anchors.fill: parent; onClicked: { editor.colorTarget = ""; editor.colorActionJ = -1 } }   // click backdrop → close

        // HSV working state — the source of truth while the picker is open.
        property real hue: 0
        property real sat: 0
        property real bri: 1
        property bool loading: false
        property color seed: "black"
        readonly property color current: Qt.hsva(hue, sat, bri, 1)
        // colour target routes to the right setter: settings key (accent/bg/segBg/border),
        // per-app accent, custom-action glyph, or global-action glyph.
        readonly property string title:
              editor.colorTarget === "action" || editor.colorTarget === "globalColor" ? "Icon colour"
            : editor.colorTarget === "appAccent" ? "App accent"
            : editor.colorTarget === "bg" ? "Background"
            : editor.colorTarget === "segBg" ? "Inactive fill"
            : editor.colorTarget === "border" ? "Border"
            : "Accent"

        function h2(x) { var s = Math.round(x * 255).toString(16); return s.length < 2 ? "0" + s : s }
        function toHex(c) { return "#" + h2(c.r) + h2(c.g) + h2(c.b) }
        function readSource() {
            if (editor.colorTarget === "action") {
                var a = actionsPanel.custom[editor.colorActionJ]
                return (a && a.color) ? a.color : Skin.accent
            }
            if (editor.colorTarget === "globalColor") {
                var g = Launcher.settings.globalActions[editor.colorActionJ]
                return (g && g.color) ? g.color : Skin.accent
            }
            if (editor.colorTarget === "appAccent") {
                var ap = Launcher.apps[editor.editActionsIdx]
                return (ap && ap.accent) ? ap.accent : Skin.accent
            }
            return Launcher.settings[editor.colorTarget]
        }
        function writeValue(hex) {
            if (editor.colorTarget === "action") Launcher.setCustomColor(editor.editActionsIdx, editor.colorActionJ, hex)
            else if (editor.colorTarget === "globalColor") Launcher.setGlobalColor(editor.colorActionJ, hex)
            else if (editor.colorTarget === "appAccent") Launcher.setAppAccent(editor.editActionsIdx, hex)
            else Launcher.setSetting(editor.colorTarget, hex)
        }
        function loadFrom(c) {
            seed = c                       // coerces a hex string → color
            loading = true
            hue = seed.hsvHue >= 0 ? seed.hsvHue : hue   // keep hue when achromatic
            sat = seed.hsvSaturation
            bri = seed.hsvValue
            loading = false
            if (!hexField.hasFocus) hexField.text = toHex(current)
        }
        onVisibleChanged: if (visible) loadFrom(readSource())
        onCurrentChanged: {
            if (!visible || loading) return
            writeValue(toHex(current))
            if (!hexField.hasFocus) hexField.text = toHex(current)
        }

        Rectangle {
            id: cardBox
            anchors.centerIn: parent
            width: Skin.s(300)
            implicitHeight: card.implicitHeight + Skin.s(36); height: implicitHeight
            radius: Skin.s(16); color: Skin.panelBg
            border.width: 1; border.color: Skin.tint(0.12)
            MouseArea { anchors.fill: parent }   // swallow clicks inside the card

            ColumnLayout {
                id: card
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: Skin.s(18) }
                spacing: Skin.s(14)

                RowLayout {
                    Layout.fillWidth: true
                    Text { text: colorPicker.title; color: Skin.fgStrong; Layout.fillWidth: true
                           font.family: Skin.fontDisplay; font.pixelSize: Skin.s(16); font.weight: Font.DemiBold
                           renderType: Text.NativeRendering }
                    Rectangle {   // live preview
                        implicitWidth: Skin.s(26); implicitHeight: Skin.s(26); radius: Skin.s(7)
                        color: colorPicker.current; border.width: 1; border.color: Skin.tint(0.25)
                    }
                }

                // saturation (x) × brightness (y) square
                Rectangle {
                    id: svBox
                    Layout.fillWidth: true; implicitHeight: Skin.s(150)
                    radius: Skin.s(10); clip: true
                    color: Qt.hsva(colorPicker.hue, 1, 1, 1)
                    Rectangle { anchors.fill: parent
                        gradient: Gradient { orientation: Gradient.Horizontal
                            GradientStop { position: 0; color: "#ffffffff" }
                            GradientStop { position: 1; color: "#00ffffff" } } }
                    Rectangle { anchors.fill: parent
                        gradient: Gradient { orientation: Gradient.Vertical
                            GradientStop { position: 0; color: "#00000000" }
                            GradientStop { position: 1; color: "#ff000000" } } }
                    Rectangle {   // handle
                        width: Skin.s(15); height: width; radius: width / 2
                        color: "transparent"; border.width: 2; border.color: "white"
                        x: colorPicker.sat * parent.width - width / 2
                        y: (1 - colorPicker.bri) * parent.height - height / 2
                        Rectangle { anchors.fill: parent; anchors.margins: 2; radius: width / 2
                                    color: "transparent"; border.width: 1; border.color: Qt.rgba(0, 0, 0, 0.4) }
                    }
                    MouseArea {
                        anchors.fill: parent
                        function apply(m) {
                            colorPicker.sat = Math.max(0, Math.min(1, m.x / width))
                            colorPicker.bri = Math.max(0, Math.min(1, 1 - m.y / height))
                        }
                        onPressed: (m) => apply(m); onPositionChanged: (m) => apply(m)
                    }
                }

                // hue slider
                Rectangle {
                    id: hueBar
                    Layout.fillWidth: true; implicitHeight: Skin.s(14); radius: height / 2
                    gradient: Gradient { orientation: Gradient.Horizontal
                        GradientStop { position: 0.000; color: "#ff0000" }
                        GradientStop { position: 0.167; color: "#ffff00" }
                        GradientStop { position: 0.333; color: "#00ff00" }
                        GradientStop { position: 0.500; color: "#00ffff" }
                        GradientStop { position: 0.667; color: "#0000ff" }
                        GradientStop { position: 0.833; color: "#ff00ff" }
                        GradientStop { position: 1.000; color: "#ff0000" } }
                    Rectangle {   // handle
                        width: Skin.s(6); height: parent.height + Skin.s(4); radius: width / 2
                        y: -Skin.s(2); x: colorPicker.hue * parent.width - width / 2
                        color: "white"; border.width: 1; border.color: Qt.rgba(0, 0, 0, 0.35)
                    }
                    MouseArea {
                        anchors.fill: parent
                        function apply(m) { colorPicker.hue = Math.max(0, Math.min(1, m.x / width)) }
                        onPressed: (m) => apply(m); onPositionChanged: (m) => apply(m)
                    }
                }

                // hex input + done
                RowLayout {
                    Layout.fillWidth: true; spacing: Skin.s(10)
                    Field {
                        id: hexField
                        Layout.fillWidth: true; placeholder: "#rrggbb"
                        onEdited: (t) => {
                            var s = t.replace("#", "")
                            if (/^[0-9a-fA-F]{6}$/.test(s)) {
                                colorPicker.loadFrom("#" + s)
                                colorPicker.writeValue("#" + s)
                            }
                        }
                    }
                    Rectangle {
                        implicitWidth: Skin.s(70); implicitHeight: Skin.s(30); radius: Skin.s(8)
                        color: dMa.containsMouse ? Qt.lighter(Skin.accent, 1.15) : Skin.accent
                        Text { anchors.centerIn: parent; text: "Done"; color: "white"
                               font.family: Skin.font; font.pixelSize: Skin.s(12); font.weight: Font.Medium
                               renderType: Text.NativeRendering }
                        MouseArea { id: dMa; anchors.fill: parent; hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor; onClicked: { editor.colorTarget = ""; editor.colorActionJ = -1 } }
                    }
                }
            }
        }
    }
}
