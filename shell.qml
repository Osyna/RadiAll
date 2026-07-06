//@ pragma UseQApplication
import Quickshell
import Quickshell.Hyprland
import Quickshell.Io
import QtQuick
import "services"
import "launcher"

// Standalone radial launcher. Installed as the Quickshell config 'radiall' and
//     launched by path:  qs -p ~/.config/quickshell/radiall/shell.qml
// It registers three ring shortcuts + a settings shortcut as Hyprland globals,
// draws the ring overlay on every screen, and starts the tray icon.
ShellRoot {
    // Hyprland registers the ring shortcuts as global shortcuts, so the in-app
    // keybind settings work (bound in ~/.config/hypr/launcher-binds.conf). Other
    // compositors lack this protocol — bind your own key to `radiall --apps`,
    // which reaches us through the IpcHandler below.
    Loader {
        active: Compositor.isHyprland
        sourceComponent: Component {
            Item {
                GlobalShortcut { appid: "launcher"; name: "apps";     onPressed: Launcher.toggleMode("apps") }
                GlobalShortcut { appid: "launcher"; name: "windows";  onPressed: Launcher.toggleMode("windows") }
                GlobalShortcut { appid: "launcher"; name: "actions";  onPressed: Launcher.toggleMode("actions") }
                GlobalShortcut { appid: "launcher"; name: "settings"; onPressed: Launcher.openSettings() }
            }
        }
    }

    // Universal control surface (every compositor): the `radiall` CLI and tray fire
    //   qs -p …/radiall/shell.qml ipc call launcher <apps|windows|actions|settings>
    IpcHandler {
        target: "launcher"
        function apps(): void { Launcher.toggleMode("apps") }
        function windows(): void { Launcher.toggleMode("windows") }
        function actions(): void { Launcher.toggleMode("actions") }
        function settings(): void { Launcher.openSettings() }
    }

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
