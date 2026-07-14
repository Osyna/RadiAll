pragma Singleton
import Quickshell
import Quickshell.Wayland
import Quickshell.Hyprland
import QtQuick

// Compositor adapter. RadiAll is Hyprland-first but runs on any wlroots-based
// Wayland compositor (sway, river, Wayfire, labwc, KWin, …) that speaks
// wlr-layer-shell + wlr-foreign-toplevel-management.
//
// Everything compositor-specific lives here so the rest of the app is generic:
//   - window listing / focus / close / fullscreen  → Hyprland IPC or wlr-toplevel
//   - float + send-keys are Hyprland-only           → guarded by canFloat / canSendKeys
//   - global keybinds are Hyprland-only             → guarded by canManageKeybinds
//     (elsewhere, users bind their compositor key to `radiall --apps`, which
//      reaches the running instance over Quickshell IPC — see shell.qml IpcHandler)
Singleton {
    id: comp

    readonly property bool isHyprland: !!Quickshell.env("HYPRLAND_INSTANCE_SIGNATURE")
    readonly property string backend: isHyprland ? "hyprland" : "wayland"

    // ---- capabilities (Hyprland exposes more than the generic wlr protocols) ----
    readonly property bool canManageKeybinds: isHyprland   // hyprctl keyword bind / global shortcuts
    readonly property bool canFloat: isHyprland            // togglefloating
    readonly property bool canSendKeys: isHyprland         // sendshortcut

    // ---- reactive window list (raw handles: HyprlandToplevel | Toplevel) ----
    // Reading this property in a binding subscribes to window open/close, so
    // consumers use `void Compositor.windows` where they call the helpers below.
    readonly property var windows: {
        if (isHyprland) {
            var out = []
            var tls = (Hyprland.toplevels && Hyprland.toplevels.values) || []
            for (var i = 0; i < tls.length; i++) {
                var t = tls[i]
                if (!appId(t) || !t.address) continue                       // skip phantoms
                if (t.lastIpcObject && t.lastIpcObject.mapped === false) continue
                out.push(t)
            }
            return out
        }
        return (ToplevelManager.toplevels && ToplevelManager.toplevels.values) || []
    }
    readonly property var activeWindow: isHyprland ? Hyprland.activeToplevel
                                                   : ToplevelManager.activeToplevel

    // focused monitor name (Hyprland); "" = unknown → ring shows on every output.
    readonly property string activeMonitor: (isHyprland && Hyprland.focusedMonitor) ? Hyprland.focusedMonitor.name : ""

    // ---- window property accessors (uniform across backends) ----
    function appId(w) {
        if (!w) return ""
        if (isHyprland)
            return (w.wayland && w.wayland.appId) ? w.wayland.appId
                 : (w.lastIpcObject && w.lastIpcObject.class) ? w.lastIpcObject.class : ""
        return w.appId || ""
    }
    function title(w) {
        if (!w) return ""
        if (isHyprland)
            return (w.lastIpcObject && w.lastIpcObject.title) ? w.lastIpcObject.title
                 : (w.wayland && w.wayland.title) ? w.wayland.title : ""
        return w.title || ""
    }
    function capture(w) {   // source for ScreencopyView thumbnails
        if (!w) return null
        return isHyprland ? w.wayland : w
    }

    // ---- window operations ----
    function activate(w) {
        if (!w) return
        if (isHyprland) Hyprland.dispatch("focuswindow address:0x" + w.address)
        else w.activate()
    }
    function closeWindow(w) {
        if (!w) return
        if (isHyprland) Hyprland.dispatch("closewindow address:0x" + w.address)
        else w.close()
    }
    function toggleFloat(w) {           // Hyprland only — guard callers with canFloat
        if (w && isHyprland) Hyprland.dispatch("togglefloating address:0x" + w.address)
    }
    function fullscreen(w) {
        if (!w) return
        if (isHyprland) { Hyprland.dispatch("focuswindow address:0x" + w.address); Hyprland.dispatch("fullscreen 1") }
        else w.fullscreen = !w.fullscreen
    }
    function sendKeys(w, mods, key) {   // Hyprland only — guard callers with canSendKeys
        if (w && isHyprland) Hyprland.dispatch("sendshortcut " + mods + ", " + key + ", address:0x" + w.address)
    }
}
