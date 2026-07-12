#!/usr/bin/env bash
# RadiAll installer — standalone Rust + Slint build.
# Builds the release binary, installs it to ~/.local/bin, and (on Hyprland)
# wires up autostart + the ring keybinds. Config lives in ~/.config/radiall/
# and is NEVER touched by --uninstall.
set -euo pipefail

BIN="$HOME/.local/bin/radiall"
HYPR="${XDG_CONFIG_HOME:-$HOME/.config}/hypr"
SNIPPET="$HYPR/radiall.conf"
BINDS="$HYPR/launcher-binds.conf"
APP_ID="io.github.Osyna.RadiAll"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP="$DATA/applications/$APP_ID.desktop"
ICON_DIR="$DATA/icons/hicolor/256x256/apps"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$APP_ID.desktop"

die() { echo "install: $*" >&2; exit 1; }

is_hyprland() {
    [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || command -v hyprctl >/dev/null 2>&1
}

uninstall() {
    "$BIN" --stop >/dev/null 2>&1 || true
    # unlink from hyprland.conf first so autoreload never sees a dangling source
    if [ -f "$HYPR/hyprland.conf" ] && grep -q 'radiall.conf' "$HYPR/hyprland.conf"; then
        cp "$HYPR/hyprland.conf" "$HYPR/hyprland.conf.bak-$(date +%Y%m%d%H%M%S)"
        sed -i '\#radiall (managed)#d; \#radiall\.conf#d' "$HYPR/hyprland.conf"
        echo "install: unlinked from hyprland.conf (backup kept)"
    fi
    rm -f "$SNIPPET" "$BIN" "$DESKTOP" "$AUTOSTART" "$ICON_DIR/$APP_ID.png"
    echo "install: removed. Your config in ~/.config/radiall/ was kept."
    echo "         ($BINDS was kept too — delete it if you no longer want the binds.)"
    exit 0
}

[ "${1:-}" = "--uninstall" ] && uninstall

command -v cargo >/dev/null 2>&1 || die "cargo not found. Install a Rust toolchain first (rustup.rs)."

echo "install: building release binary (first build takes a few minutes)…"
cargo build --release

mkdir -p "$(dirname "$BIN")"
install -m755 target/release/radiall "$BIN"
echo "install: installed $BIN"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "install: NOTE: ~/.local/bin is not on your PATH" ;;
esac

# App-id desktop entry. Two jobs: a launcher menu entry, and the app identity
# the XDG global-shortcuts portal REQUIRES before it will bind keys for a
# non-sandboxed app (GNOME / KDE Wayland shortcut support).
mkdir -p "$(dirname "$DESKTOP")" "$ICON_DIR"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=RadiAll
Comment=Radial app launcher, window switcher, and action menu
Exec=$BIN --settings
Icon=$APP_ID
Terminal=false
Categories=Utility;
EOF
[ -f RadiAll.png ] && install -m644 RadiAll.png "$ICON_DIR/$APP_ID.png"
echo "install: installed $DESKTOP"

if is_hyprland; then
    mkdir -p "$HYPR"
    # Seed the binds file only if absent — the settings UI manages it afterwards.
    if [ ! -f "$BINDS" ]; then
        cat > "$BINDS" <<'EOF'
# RadiAll ring shortcuts. Managed by the RadiAll settings UI.
bind = SUPER, a, exec, radiall --apps
bind = SUPER, w, exec, radiall --windows
bind = SUPER, d, exec, radiall --actions
EOF
        echo "install: seeded $BINDS (Super+A/W/D)"
    fi
    cat > "$SNIPPET" <<EOF
# RadiAll — managed by install.sh. Remove with: install.sh --uninstall
exec-once = $BIN --daemon
source = $BINDS
EOF
    if [ -f "$HYPR/hyprland.conf" ] && ! grep -q 'radiall.conf' "$HYPR/hyprland.conf"; then
        cp "$HYPR/hyprland.conf" "$HYPR/hyprland.conf.bak-$(date +%Y%m%d%H%M%S)"
        tail -c1 "$HYPR/hyprland.conf" | read -r _ || echo >> "$HYPR/hyprland.conf"
        {
            echo "# radiall (managed) — remove with: install.sh --uninstall"
            echo "source = $SNIPPET"
        } >> "$HYPR/hyprland.conf"
        echo "install: linked into hyprland.conf (backup kept)"
    fi
    # (re)start now
    "$BIN" --stop >/dev/null 2>&1 || true
    sleep 0.4
    "$BIN" --start
    echo
    echo "Done. Press Super+A."
else
    # Generic Linux: XDG autostart (GNOME, KDE, XFCE, Cinnamon, MATE, ... all
    # honor it). Shortcuts bind themselves through the desktop portal where
    # available (GNOME / KDE Wayland) or X11 grabs; the CLI always works too.
    mkdir -p "$(dirname "$AUTOSTART")"
    cat > "$AUTOSTART" <<EOF
[Desktop Entry]
Type=Application
Name=RadiAll daemon
Exec=$BIN --daemon
Icon=$APP_ID
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF
    echo "install: autostart entry $AUTOSTART"
    "$BIN" --stop >/dev/null 2>&1 || true
    sleep 0.4
    "$BIN" --start
    echo
    echo "Done. Open your ring keys in Settings (menu: RadiAll), or bind"
    echo "keys to the CLI yourself:  radiall --apps | --windows | --actions"
    echo "  GNOME/KDE Wayland: keys go through the desktop's shortcuts portal —"
    echo "  your desktop may ask once to confirm them."
    echo "  Any X11 desktop: keys are grabbed directly, no setup needed."
fi
