# Radial Launcher

A radial (pie-wheel) app launcher, window switcher, and window-action menu for
**Hyprland**, built on [Quickshell](https://quickshell.org). Three rings, each on
its own shortcut, plus a real system-tray icon.

| Ring | Shortcut | What it shows |
|------|----------|---------------|
| **Apps** | `Super + A` | Your chosen apps — launch, focus, running-window dots, long-press for per-app actions |
| **Windows** | `Super + W` | Every open window, grouped by app |
| **Focus actions** | `Super + D` | Actions for the focused window (close/float/fullscreen, `.desktop` actions, custom shortcuts) around its icon |

- **Live window thumbnails** when scrolling a multi-window app
- **Follow-cursor mode** — the accent sector tracks your mouse anywhere on screen
- **In-launcher settings** (colours, sizes, shortcuts, behaviour) — no config files to edit
- **Tray icon** to open any ring or the settings

## Requirements

- **Hyprland**
- **Quickshell** (`qs`) — the one hard dependency
- *Optional, for the tray icon:* `python-gobject` + `libappindicator-gtk3`

## Install

```sh
git clone https://github.com/YOURNAME/radial-launcher
cd radial-launcher
./install.sh
```

The installer copies a self-contained config to
`~/.config/quickshell/radial-launcher/`, runs it as its own Quickshell instance
(`qs -c radial-launcher` — it does **not** touch your existing Quickshell setup),
seeds the default keybinds, and links itself into `hyprland.conf`.

That's it — press `Super + A`.

## Usage

- Open a ring with its shortcut; **hover a slice** and click to activate.
- **Left-click the centre hole** (or press `Esc`) to dismiss.
- **Hover the centre for 2 s** to reveal the settings button.
- **Long-press an app** (Apps ring) for its action arc.
- **Scroll on an app** with multiple windows to pick which one to target.
- **Tray icon** → open any ring or **Settings…**.

Shortcuts and appearance are all editable in **Settings → Look**.

## Uninstall

```sh
./install.sh --uninstall
```

Removes the config, the tray, and the Hyprland wiring. Your saved apps and
settings are left in place unless you delete them yourself.

## How it works

It's a normal Quickshell config. `shell.qml` registers the ring shortcuts as
Hyprland globals, draws the overlay on every screen, and starts the tray helper.
State lives in `~/.config/quickshell/radial-launcher/` (`apps.json`,
`launcher-settings.json`); the ring keybinds live in
`~/.config/hypr/launcher-binds.conf`, rewritten whenever you change a shortcut.

## License

MIT — see [LICENSE](LICENSE).
