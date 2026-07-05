#!/usr/bin/env bash
# Radial Launcher installer. Installs a self-contained Quickshell config that
# runs as its own instance (`qs -c radial-launcher`) and wires up Hyprland.
#
#   ./install.sh              install / update
#   ./install.sh --uninstall  remove everything this installer added
#   ./install.sh --help
set -euo pipefail

NAME="radial-launcher"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CFG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}"
DEST="$CFG_ROOT/quickshell/$NAME"
HYPR="$CFG_ROOT/hypr"
HYPR_CONF="$HYPR/hyprland.conf"
LAUNCHER_INC="$HYPR/$NAME.conf"          # our sourced snippet
BINDS="$HYPR/launcher-binds.conf"        # written by the settings UI at runtime

c() { printf '\033[%sm%s\033[0m' "$1" "$2"; }
info()  { echo "$(c '1;34' '::') $*"; }
ok()    { echo "$(c '1;32' ' ✔') $*"; }
warn()  { echo "$(c '1;33' ' !') $*" >&2; }
die()   { echo "$(c '1;31' ' ✘') $*" >&2; exit 1; }

# resolve the quickshell binary (qs is the modern name)
QS="$(command -v qs || command -v quickshell || true)"
QS_NAME="$(basename "${QS:-qs}")"

usage() { sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0; }

running_pid() {   # PID of a running radial-launcher instance, or empty
    [ -n "$QS" ] || return 0
    "$QS" list 2>/dev/null | awk -v n="/$NAME/shell.qml" '
        /Process ID:/ { pid=$NF }
        index($0, n)  { print pid; exit }'
}

uninstall() {
    info "Uninstalling $NAME"
    local pid; pid="$(running_pid || true)"
    if [ -n "$pid" ]; then kill "$pid" 2>/dev/null && ok "stopped instance ($pid)"; fi
    rm -rf "$DEST" && ok "removed $DEST"
    rm -f "$LAUNCHER_INC" && ok "removed $LAUNCHER_INC"
    if [ -f "$HYPR_CONF" ] && grep -q "$NAME.conf" "$HYPR_CONF"; then
        cp -p "$HYPR_CONF" "$HYPR_CONF.bak-$(date +%Y%m%d%H%M%S)"
        sed -i "\#$NAME (managed)#d; \#$NAME.conf#d" "$HYPR_CONF" && ok "unlinked from hyprland.conf (backup saved)"
    fi
    warn "kept $BINDS and your saved apps/settings — delete them by hand if you want them gone."
    ok "Done."
    exit 0
}

[ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ] && usage
[ "${1:-}" = "--uninstall" ] && uninstall

# ---- checks ----
[ -n "$QS" ] || die "Quickshell not found. Install it first (package: quickshell), then re-run."
ok "Found Quickshell: $QS"
command -v hyprctl >/dev/null || warn "hyprctl not found — this launcher targets Hyprland and won't work elsewhere."

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
cp -r "$SRC_DIR/shell.qml" "$SRC_DIR/services" "$SRC_DIR/launcher" "$SRC_DIR/icons.json" "$DEST/"
chmod +x "$DEST/launcher/tray.py"
ok "Files installed."

# ---- Hyprland wiring ----
if [ -d "$HYPR" ]; then
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
exec-once = $QS_NAME -c $NAME
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
    warn "    exec-once = $QS_NAME -c $NAME"
    warn "    source = $BINDS"
fi

# ---- start now ----
if [ -n "${WAYLAND_DISPLAY:-}" ] && command -v hyprctl >/dev/null; then
    pid="$(running_pid || true)"
    [ -n "$pid" ] && { kill "$pid" 2>/dev/null || true; sleep 0.4; }
    setsid -f "$QS" -c "$NAME" >/dev/null 2>&1 || true
    sleep 1
    ok "Launcher started."
else
    info "Start it with:  $QS_NAME -c $NAME   (or just re-login)"
fi

echo
ok "$(c '1;32' 'Radial Launcher installed.')"
echo "   Super+A  apps ring      Super+W  windows ring      Super+D  focus actions"
echo "   Settings: tray icon → Settings…, or hover the ring centre for 2s."
echo "   Uninstall: $SRC_DIR/install.sh --uninstall"
