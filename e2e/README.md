# Headless GNOME harness

Runs Glint in a private headless GNOME session (gnome-shell `--headless`, one virtual monitor) with its own D-Bus,
`XDG_RUNTIME_DIR`, HOME and PipeWire, so it never draws on or reads from the desktop you are logged into. The
portals (Screenshot, ScreenCast, GlobalShortcuts) are GNOME's, answered inside that session.

## Needs

Arch: `gnome-shell mutter xdg-desktop-portal-gnome gnome-control-center pipewire wireplumber gst-plugin-pipewire
gst-plugins-{base,good,bad,ugly} python-gobject`. `gnome-control-center` provides the GlobalShortcuts portal's
backend. A Glint build: `bun exact.mjs linux`.

## Use

```sh
e2e/session/start            # state in ../state (GLINT_E2E_STATE), 1920x1080 (GLINT_E2E_SIZE)
e2e/bin/relaunch             # (re)starts target/host-dev/glint-linux in it; log in $STATE/log/glint.log
e2e/bin/d shot /tmp/s.png    # screenshot of the headless session
e2e/bin/d click 493 239      # Glint's Screen button at 1920x1080
e2e/bin/d key ctrl+alt+shift+super+3
e2e/bin/run CMD...           # any command inside the session
e2e/bin/d help               # move, click, down/up, drag, scroll, key, keydown/keyup, type, shot, size, wait
e2e/session/stop             # kills every process whose XDG_RUNTIME_DIR is the session's
```

The library is `$STATE/home/.local/share/exact/dev.mchisolm0.glint/data/captures`. Captures land first in the folder
picked on first capture; before one is picked, in the shared `/tmp/glint` (`data/src/lib.rs`).

## Quirks

- `drive.py` holds a mutter RemoteDesktop session tied to a ScreenCast of the monitor (absolute pointer moves need a
  stream), so the orange sharing indicator is always on. gnome-shell answers `Screenshot` only for allowlisted bus
  names; the driver owns `org.gnome.SettingsDaemon.MediaKeys` on the private bus to be one.
- The capture portals see no app id (ashpd's connection is not registered), so dialogs say "An app".
- First Screen: "Allow Apps to Take Screenshots?" Click Allow (1065,616); the private permission store keeps it.
- First Record: the Share Screen chooser has the display preselected. Click Share (1245,328) only, since clicking
  the display deselects it. The restore token in `$XDG_STATE_HOME/glint/screencast.token` skips it after that.
- First launch: "Add Keyboard Shortcuts". Click Add (1161,334). Without `gnome-control-center` binding fails with
  "the portal ended the request".
- A busy picture for big recordings: `e2e/bin/run gst-launch-1.0 videotestsrc pattern=snow ! waylandsink`.

## Hyprland

`e2e/session/hypr` starts a nested Hyprland (`hyprland` and `xdg-desktop-portal-hyprland` installed) as a window
in the GNOME session, with its own HOME, D-Bus, PipeWire and portals under `$STATE/hypr`, and copies of your
`~/.config/xdg-desktop-portal/hyprland-portals.conf` and `~/.config/hypr/xdph.conf`. `GLINT_E2E_DESKTOP=hyprland`
points `bin/relaunch` and `bin/run` at it; `bin/d` still drives GNOME, and Hyprland's (0,0) is GNOME's (0,32).
`e2e/bin/run hyprctl ...` works there too.

- The folder chooser (`xdg-desktop-portal-gtk`) tiles beside Glint: the portal gets no parent window. Typing a path
  in its location bar and pressing Enter does nothing; click through Home instead.
- Area and Window both open xdph's region picker, which has no window mode. A click without a drag cancels it.
- Portal shortcuts need binds in `hyprland.conf`; `hyprctl globalshortcuts` lists the ids, e.g.
  `bind = CTRL ALT SHIFT SUPER, 3, global, dev.mchisolm0.glint:1/screen/Control+Alt+Shift+Meta+3`.
- xdph sends a frame only when something changes, so a recording of a still window may fail to start.
