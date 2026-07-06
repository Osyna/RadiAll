#!/usr/bin/env python3
"""System-tray icon for the Quickshell radial launcher.

Publishes a real StatusNotifierItem (via libappindicator) whose menu opens each
ring and the settings by calling the launcher over Quickshell IPC
(`qs -p …/radiall/shell.qml ipc call launcher <target>`) — compositor-agnostic, so
the tray behaves the same on Hyprland and any other Wayland compositor.
"""
import os
import shutil
import subprocess

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("AppIndicator3", "0.1")
from gi.repository import AppIndicator3, Gtk  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
QS = shutil.which("qs") or shutil.which("quickshell") or "qs"
SHELL_QML = os.path.join(os.path.dirname(HERE), "shell.qml")  # launched by path (see radiall)

# (menu label, launcher IPC function). See the IpcHandler "launcher" in shell.qml.
ENTRIES = [
    ("Apps ring", "apps"),
    ("Windows ring", "windows"),
    ("Focus actions", "actions"),
    (None, None),            # separator
    ("Settings…", "settings"),
]


def dispatch(target):
    return lambda _item: subprocess.Popen(
        [QS, "-p", SHELL_QML, "ipc", "call", "launcher", target]
    )


def build_menu():
    menu = Gtk.Menu()
    for label, target in ENTRIES:
        if label is None:
            menu.append(Gtk.SeparatorMenuItem())
            continue
        item = Gtk.MenuItem(label=label)
        item.connect("activate", dispatch(target))
        menu.append(item)
    menu.append(Gtk.SeparatorMenuItem())
    quit_item = Gtk.MenuItem(label="Quit tray")
    quit_item.connect("activate", lambda _: Gtk.main_quit())
    menu.append(quit_item)
    menu.show_all()
    return menu


def main():
    ind = AppIndicator3.Indicator.new_with_path(
        "quickshell-launcher",
        "RadiAll",  # resolves to RadiAll.png in HERE
        AppIndicator3.IndicatorCategory.APPLICATION_STATUS,
        HERE,
    )
    ind.set_status(AppIndicator3.IndicatorStatus.ACTIVE)
    ind.set_title("RadiAll")
    menu = build_menu()
    ind.set_menu(menu)
    # middle-click opens the apps ring
    ind.set_secondary_activate_target(menu.get_children()[0])
    Gtk.main()


if __name__ == "__main__":
    main()
