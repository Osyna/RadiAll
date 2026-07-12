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
    rm -f "$SNIPPET" "$BIN"
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
    echo
    echo "Done. Non-Hyprland setup (two steps):"
    echo "  1. Autostart the daemon:   radiall --daemon"
    echo "     GNOME/KDE: add it to Startup Applications / Autostart."
    echo "  2. Bind keys to:           radiall --apps | --windows | --actions"
    echo "     GNOME: Settings → Keyboard → Custom Shortcuts"
    echo "     KDE:   System Settings → Shortcuts → Add Command"
    echo
    echo "Start it now with:  radiall --start"
fi
