#!/usr/bin/env bash
# Radial Launcher installer. Installs a self-contained Quickshell config that
# runs as its own Quickshell config named 'radiall'; Hyprland-first, Wayland-ready.
#
#   ./install.sh              install / update
#   ./install.sh --uninstall  remove everything this installer added
#   ./install.sh --help
set -euo pipefail

NAME="radiall"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CFG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}"
DEST="$CFG_ROOT/quickshell/$NAME"
HYPR="$CFG_ROOT/hypr"
HYPR_CONF="$HYPR/hyprland.conf"
LAUNCHER_INC="$HYPR/$NAME.conf"          # our sourced snippet
BINDS="$HYPR/launcher-binds.conf"        # written by the settings UI at runtime
BIN_DIR="$HOME/.local/bin"               # where the `radiall` command goes
RADIALL_BIN="$BIN_DIR/radiall"

c() { printf '\033[%sm%s\033[0m' "$1" "$2"; }
info()  { echo "$(c '1;34' '::') $*"; }
ok()    { echo "$(c '1;32' ' ✔') $*"; }
warn()  { echo "$(c '1;33' ' !') $*" >&2; }
die()   { echo "$(c '1;31' ' ✘') $*" >&2; exit 1; }

# resolve the quickshell binary (qs is the modern name)
QS="$(command -v qs || command -v quickshell || true)"
QS_NAME="$(basename "${QS:-qs}")"
# Hyprland gets full auto-wiring (keybinds + autostart); other Wayland compositors
# get printed setup steps, since we can't safely edit an unknown config format.
if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || command -v hyprctl >/dev/null 2>&1; then IS_HYPR=1; else IS_HYPR=0; fi

usage() { sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

rl_procs() {   # every RadiAll process — qs instance(s) + tray helper(s), installed or run-from-source.
    # Path-launched instances aren't in `qs list`, and several can pile up, so match them all directly.
    pgrep -f "($DEST|$SRC_DIR)/(shell.qml|launcher/tray.py)" 2>/dev/null || true
}

uninstall() {
    info "Uninstalling $NAME"
    local pids; pids="$(rl_procs)"
    if [ -n "$pids" ]; then kill $pids 2>/dev/null || true; ok "stopped $(echo $pids | wc -w) running process(es)"; fi
    # Unlink from hyprland.conf FIRST — before deleting the file it sources — so
    # Hyprland's autoreload never catches a dangling `source=` mid-uninstall
    # (that's the "globbing error: found no match" popup).
    if [ -f "$HYPR_CONF" ] && grep -q "$NAME.conf" "$HYPR_CONF"; then
        cp -p "$HYPR_CONF" "$HYPR_CONF.bak-$(date +%Y%m%d%H%M%S)"
        sed -i "\#$NAME (managed)#d; \#$NAME.conf#d" "$HYPR_CONF" && ok "unlinked from hyprland.conf (backup saved)"
    fi
    rm -f "$LAUNCHER_INC" && ok "removed $LAUNCHER_INC"
    rm -rf "$DEST" && ok "removed $DEST"
    rm -f "$RADIALL_BIN" && ok "removed $RADIALL_BIN"
    warn "kept $BINDS and your saved apps/settings — delete them by hand if you want them gone."
    ok "Done."
    exit 0
}

[ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ] && usage
[ "${1:-}" = "--uninstall" ] && uninstall

# ---- checks ----
[ -n "$QS" ] || die "Quickshell not found. Install it first (package: quickshell), then re-run."
ok "Found Quickshell: $QS"
if [ "$IS_HYPR" = 1 ]; then ok "Hyprland detected — will wire up keybinds + autostart."
else info "Non-Hyprland Wayland — installing files; you'll bind keys yourself (steps below)."; fi

# tray is optional
if python3 - <<'PY' >/dev/null 2>&1
import gi; gi.require_version("AppIndicator3","0.1")
from gi.repository import AppIndicator3
PY
then ok "Tray dependencies present (python-gobject + libappindicator)."
else warn "Tray icon needs 'python-gobject' + 'libappindicator-gtk3'. The launcher still works without them."
fi

# ---- install files ----
info "Installing config to $DEST"
mkdir -p "$DEST"
cp -r "$SRC_DIR/shell.qml" "$SRC_DIR/services" "$SRC_DIR/launcher" "$SRC_DIR/themes" "$SRC_DIR/icons.json" "$DEST/"
chmod +x "$DEST/launcher/tray.py"
ok "Files installed."

# ---- the `radiall` command ----
mkdir -p "$BIN_DIR"
install -m755 "$SRC_DIR/radiall" "$RADIALL_BIN"
ok "Installed 'radiall' command to $RADIALL_BIN"
case ":$PATH:" in
    *":$BIN_DIR:"*) : ;;
    *) warn "$BIN_DIR is not on your PATH — add it, or call it as $RADIALL_BIN" ;;
esac

# ---- shortcuts + autostart ----
if [ "$IS_HYPR" != 1 ]; then
    info "Set up RadiAll in your compositor's config:"
    info "   autostart:  exec  $QS_NAME -p $DEST/shell.qml   (or run: radiall --start)"
    info "   keybinds:   bind a key to each of:  radiall --apps / --windows / --actions"
elif [ -d "$HYPR" ]; then
    if [ ! -f "$BINDS" ]; then
        cat > "$BINDS" <<EOF
# Radial launcher ring shortcuts. Managed by the launcher settings UI.
bind = SUPER, a, global, launcher:apps
bind = SUPER, w, global, launcher:windows
bind = SUPER, d, global, launcher:actions
EOF
        ok "Seeded default shortcuts (Super+A / Super+W / Super+D)."
    fi
    cat > "$LAUNCHER_INC" <<EOF
# Radial launcher — managed by install.sh. Remove with: install.sh --uninstall
exec-once = $QS_NAME -p $DEST/shell.qml
source = $BINDS
EOF
    if [ -f "$HYPR_CONF" ] && ! grep -q "$NAME.conf" "$HYPR_CONF"; then
        cp -p "$HYPR_CONF" "$HYPR_CONF.bak-$(date +%Y%m%d%H%M%S)"   # back up before the one edit we make
        [ -n "$(tail -c1 "$HYPR_CONF")" ] && printf '\n' >> "$HYPR_CONF"   # ensure a trailing newline first
        printf '# %s (managed) — remove with: install.sh --uninstall\nsource = %s\n' "$NAME" "$LAUNCHER_INC" >> "$HYPR_CONF"
        ok "Linked into hyprland.conf (backup saved next to it)."
    elif [ -f "$HYPR_CONF" ]; then
        ok "Already linked in hyprland.conf."
    else
        warn "No hyprland.conf found — add:  source = $LAUNCHER_INC"
    fi
else
    warn "No ~/.config/hypr — add these to your Hyprland config manually:"
    warn "    exec-once = $QS_NAME -p $DEST/shell.qml"
    warn "    source = $BINDS"
fi

# ---- start now ----
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    pids="$(rl_procs)"
    [ -n "$pids" ] && { kill $pids 2>/dev/null || true; sleep 0.5; }
    setsid -f "$QS" -p "$DEST/shell.qml" >/dev/null 2>&1 || true
    sleep 1
    ok "Launcher started."
else
    info "Start it with:  $QS_NAME -p $DEST/shell.qml   (or just re-login)"
fi

echo
ok "$(c '1;32' 'RadiAll installed.')"
if [ "$IS_HYPR" = 1 ]; then
    echo "   Keys:     Super+A apps   Super+W windows   Super+D focus actions   (change in Settings)"
else
    echo "   Keys:     bind  radiall --apps / --windows / --actions  in your compositor"
fi
echo "   Command:  radiall --help    ('radiall --binds' prints Hyprland-syntax binds)"
echo "   Settings: tray icon → Settings…, or hover the ring centre for 2s."
echo "   Uninstall: $SRC_DIR/install.sh --uninstall"
