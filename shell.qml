//@ pragma UseQApplication
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import QtQuick
import "services"
import "launcher"

// Standalone radial launcher. Run as its own Quickshell instance:
//     qs -c radial-launcher
// It registers three ring shortcuts + a settings shortcut as Hyprland globals,
// draws the ring overlay on every screen, and starts the tray icon.
ShellRoot {
    // Ring shortcuts. Keys are bound to these global targets in
    // ~/.config/hypr/launcher-binds.conf (written by the settings UI).
    GlobalShortcut { appid: "launcher"; name: "apps";     onPressed: Launcher.toggleMode("apps") }
    GlobalShortcut { appid: "launcher"; name: "windows";  onPressed: Launcher.toggleMode("windows") }
    GlobalShortcut { appid: "launcher"; name: "actions";  onPressed: Launcher.toggleMode("actions") }
    // fired by the tray icon (hyprctl dispatch global launcher:settings)
    GlobalShortcut { appid: "launcher"; name: "settings"; onPressed: Launcher.openSettings() }

    Variants {
        model: Quickshell.screens
        RadialMenu {}
    }

    // Tray icon (StatusNotifierItem). Optional — needs python-gobject +
    // libappindicator; if they're missing the helper just exits, no harm done.
    Process {
        running: true
        command: ["python3", Quickshell.shellDir + "/launcher/tray.py"]
    }
}
