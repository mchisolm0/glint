# Sourced after env.sh by the scripts that act inside the nested Hyprland session (session/hypr): its own HOME,
# runtime dir, D-Bus and PipeWire under $STATE/hypr. Hyprland draws as a window on the headless GNOME display,
# so `bin/d` still clicks and shoots through GNOME. `session/hypr` writes $STATE/hypr/env once Hyprland is up.
OUTER_WAYLAND=$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY
HYPR=$STATE/hypr
export HOME=$HYPR/home
# Short, since Hyprland's IPC socket path under it must fit in 108 bytes.
export XDG_RUNTIME_DIR=/tmp/glint-hypr-$(id -u)
export XDG_CONFIG_HOME=$HOME/.config XDG_DATA_HOME=$HOME/.local/share
export XDG_CACHE_HOME=$HOME/.cache XDG_STATE_HOME=$HOME/.local/state
export XDG_DATA_DIRS=$XDG_DATA_HOME:/usr/local/share:/usr/share
export XDG_CURRENT_DESKTOP=Hyprland XDG_SESSION_DESKTOP=Hyprland
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
LOGS=$HYPR/log PIDS=$HYPR/pids
if [ -f "$HYPR/env" ]; then . "$HYPR/env"; fi
