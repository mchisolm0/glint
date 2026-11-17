# Sourced by the harness scripts, with $here set to this e2e/ folder. Everything the headless session sees
# lives under $STATE: its HOME, XDG dirs, runtime dir (D-Bus, Wayland and PipeWire sockets), pids and logs.
GLINT=$(cd "$here/.." && pwd)
STATE=${GLINT_E2E_STATE:-$(dirname "$GLINT")/state}
REAL_HOME=${REAL_HOME:-$HOME}
export REAL_HOME STATE GLINT

export HOME=$STATE/home
export XDG_RUNTIME_DIR=$STATE/run
export XDG_CONFIG_HOME=$HOME/.config XDG_DATA_HOME=$HOME/.local/share
export XDG_CACHE_HOME=$HOME/.cache XDG_STATE_HOME=$HOME/.local/state
export XDG_DATA_DIRS=$XDG_DATA_HOME:/usr/local/share:/usr/share XDG_CONFIG_DIRS=/etc/xdg
export XDG_CURRENT_DESKTOP=GNOME XDG_SESSION_DESKTOP=gnome XDG_SESSION_TYPE=wayland
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
export WAYLAND_DISPLAY=wayland-glint
unset DISPLAY XDG_SESSION_ID XDG_SEAT XDG_VTNR HYPRLAND_INSTANCE_SIGNATURE SWAYSOCK
export GDK_BACKEND=wayland NO_AT_BRIDGE=1
LOGS=$STATE/log PIDS=$STATE/pids
# Marks the harness's own shells, so session/stop spares them; what they launch runs without it.
export GLINT_E2E_HARNESS=1
