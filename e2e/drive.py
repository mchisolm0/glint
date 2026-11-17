"""Drives the headless GNOME session: pointer, scroll, keys and screenshots.

`drive.py serve` (session/start runs it) holds one mutter RemoteDesktop session, tied to a ScreenCast of the
virtual monitor so pointer moves are absolute, and listens on $XDG_RUNTIME_DIR/glint-drive.sock. Held keys and
buttons stay held between commands because the session outlives them. Screenshots come from gnome-shell's
org.gnome.Shell.Screenshot, which answers only allowlisted callers (GNOME 51: the media-keys daemon and the GNOME
portal). No settings daemon runs here, so the driver owns org.gnome.SettingsDaemon.MediaKeys on the private bus.

Every other invocation is a client: `drive.py <command> [args...]`, one command per call (see COMMANDS).
"""

import ctypes
import json
import os
import socket
import sys
import threading
import time

SOCK = os.path.join(os.environ["XDG_RUNTIME_DIR"], "glint-drive.sock")
BUTTONS = {"left": 0x110, "right": 0x111, "middle": 0x112}
ALIASES = {"ctrl": "Control_L", "control": "Control_L", "alt": "Alt_L", "shift": "Shift_L", "super": "Super_L",
           "meta": "Super_L", "esc": "Escape", "enter": "Return", "space": "space", "tab": "Tab",
           "backspace": "BackSpace", "delete": "Delete", "up": "Up", "down": "Down", "left": "Left", "right": "Right"}
COMMANDS = """move X Y | click X Y [left|right|middle] [count] | down [button] | up [button] | drag X1 Y1 X2 Y2
scroll DY [DX] (discrete steps, + is down/right) | key COMBO (e.g. ctrl+alt+shift+super+3) | keydown KEY | keyup KEY
type TEXT | shot PATH | size | wait SECONDS"""


def keysym(name):
    xkb = ctypes.CDLL("libxkbcommon.so.0")
    xkb.xkb_keysym_from_name.restype = ctypes.c_uint32
    name = ALIASES.get(name.lower(), name)
    sym = xkb.xkb_keysym_from_name(name.encode(), 0) or xkb.xkb_keysym_from_name(name.encode(), 1)
    if not sym:
        raise ValueError(f"no keysym named {name}")
    return sym


def char_keysym(ch):
    xkb = ctypes.CDLL("libxkbcommon.so.0")
    xkb.xkb_utf32_to_keysym.restype = ctypes.c_uint32
    return xkb.xkb_utf32_to_keysym(ord(ch))


class Driver:
    def __init__(self):
        from gi.repository import Gio, GLib

        self.Gio, self.GLib = Gio, GLib
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION)
        self.loop = GLib.MainLoop()
        Gio.bus_own_name_on_connection(self.bus, "org.gnome.SettingsDaemon.MediaKeys", Gio.BusNameOwnerFlags.NONE, None, None)
        rd = self.call("org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop",
                       "org.gnome.Mutter.RemoteDesktop", "CreateSession", None, "(o)")[0]
        self.rd = rd
        sid = self.prop(rd, "org.gnome.Mutter.RemoteDesktop.Session", "SessionId")
        sc = self.call("org.gnome.Mutter.ScreenCast", "/org/gnome/Mutter/ScreenCast", "org.gnome.Mutter.ScreenCast",
                       "CreateSession", GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},)),
                       "(o)")[0]
        self.stream = self.call("org.gnome.Mutter.ScreenCast", sc, "org.gnome.Mutter.ScreenCast.Session",
                                "RecordMonitor", GLib.Variant("(sa{sv})", ("", {})), "(o)")[0]
        self.added = threading.Event()
        self.bus.signal_subscribe("org.gnome.Mutter.ScreenCast", "org.gnome.Mutter.ScreenCast.Stream",
                                  "PipeWireStreamAdded", self.stream, None, Gio.DBusSignalFlags.NONE,
                                  lambda *a: self.added.set())
        self.bus.signal_subscribe("org.gnome.Mutter.RemoteDesktop", "org.gnome.Mutter.RemoteDesktop.Session",
                                  "Closed", rd, None, Gio.DBusSignalFlags.NONE, lambda *a: self.closed())
        self.rd_call("Start")
        self.held = set()

    def call(self, dest, path, iface, method, args=None, reply=None):
        out = self.bus.call_sync(dest, path, iface, method, args,
                                 self.GLib.VariantType(reply) if reply else None,
                                 self.Gio.DBusCallFlags.NONE, 10000, None)
        return out.unpack() if out else None

    def prop(self, path, iface, name):
        v = self.call("org.gnome.Mutter.RemoteDesktop", path, "org.freedesktop.DBus.Properties", "Get",
                      self.GLib.Variant("(ss)", (iface, name)), "(v)")
        return v[0]

    def rd_call(self, method, sig=None, *args):
        V = self.GLib.Variant
        return self.call("org.gnome.Mutter.RemoteDesktop", self.rd, "org.gnome.Mutter.RemoteDesktop.Session", method,
                         V(sig, args) if sig else None)

    def closed(self):
        print("remote desktop session closed", file=sys.stderr, flush=True)
        os._exit(1)

    def size(self):
        mon = self.call("org.gnome.Mutter.DisplayConfig", "/org/gnome/Mutter/DisplayConfig",
                        "org.gnome.Mutter.DisplayConfig", "GetCurrentState", None)
        logical = mon[2][0]
        x, y, scale = logical[0], logical[1], logical[2]
        connector = logical[5][0][0]
        mode = next(m for phys in mon[1] if phys[0][0] == connector for m in phys[1] if m[6].get("is-current"))
        return {"x": x, "y": y, "width": round(mode[1] / scale), "height": round(mode[2] / scale), "scale": scale}

    def move(self, x, y):
        if not self.added.wait(10):
            raise RuntimeError("the driver's screencast stream never started")
        self.rd_call("NotifyPointerMotionAbsolute", "(sdd)", self.stream, float(x), float(y))

    def button(self, name, pressed):
        self.rd_call("NotifyPointerButton", "(ib)", BUTTONS[name], pressed)

    def key(self, name, pressed):
        sym = keysym(name)
        self.rd_call("NotifyKeyboardKeysym", "(ub)", sym, pressed)
        (self.held.add if pressed else self.held.discard)(name)

    def run(self, argv):
        cmd, args = argv[0], argv[1:]
        if cmd == "move":
            self.move(*map(float, args[:2]))
        elif cmd == "click":
            self.move(*map(float, args[:2]))
            button = args[2] if len(args) > 2 else "left"
            for _ in range(int(args[3]) if len(args) > 3 else 1):
                self.button(button, True)
                time.sleep(0.03)
                self.button(button, False)
                time.sleep(0.06)
        elif cmd in ("down", "up"):
            self.button(args[0] if args else "left", cmd == "down")
        elif cmd == "drag":
            x1, y1, x2, y2 = map(float, args[:4])
            self.move(x1, y1)
            self.button("left", True)
            for i in range(1, 21):
                self.move(x1 + (x2 - x1) * i / 20, y1 + (y2 - y1) * i / 20)
                time.sleep(0.015)
            self.button("left", False)
        elif cmd == "scroll":
            dy = int(args[0])
            dx = int(args[1]) if len(args) > 1 else 0
            if dy:
                self.rd_call("NotifyPointerAxisDiscrete", "(ui)", 0, dy)
            if dx:
                self.rd_call("NotifyPointerAxisDiscrete", "(ui)", 1, dx)
        elif cmd == "key":
            keys = args[0].split("+")
            for k in keys:
                self.key(k, True)
                time.sleep(0.02)
            for k in reversed(keys):
                self.key(k, False)
                time.sleep(0.02)
        elif cmd in ("keydown", "keyup"):
            self.key(args[0], cmd == "keydown")
        elif cmd == "type":
            for ch in " ".join(args):
                sym = char_keysym(ch)
                self.rd_call("NotifyKeyboardKeysym", "(ub)", sym, True)
                self.rd_call("NotifyKeyboardKeysym", "(ub)", sym, False)
                time.sleep(0.01)
        elif cmd == "shot":
            path = os.path.abspath(args[0])
            ok, used = self.call("org.gnome.Shell.Screenshot", "/org/gnome/Shell/Screenshot",
                                 "org.gnome.Shell.Screenshot", "Screenshot",
                                 self.GLib.Variant("(bbs)", (False, False, path)), "(bs)")
            if not ok:
                raise RuntimeError("gnome-shell refused the screenshot")
            return used
        elif cmd == "size":
            return self.size()
        elif cmd == "held":
            return sorted(self.held)
        else:
            raise ValueError(f"unknown command {cmd}\n{COMMANDS}")
        return "ok"

    def serve(self):
        try:
            os.unlink(SOCK)
        except FileNotFoundError:
            pass
        server = socket.socket(socket.AF_UNIX)
        server.bind(SOCK)
        server.listen()
        lock = threading.Lock()

        def handle(conn):
            with conn:
                argv = json.loads(conn.makefile().readline())
                try:
                    with lock:
                        reply = {"ok": self.run(argv)}
                except Exception as e:  # The client prints it; the driver keeps serving.
                    reply = {"error": f"{type(e).__name__}: {e}"}
                conn.sendall((json.dumps(reply) + "\n").encode())

        def accept():
            while True:
                conn, _ = server.accept()
                threading.Thread(target=handle, args=(conn,), daemon=True).start()

        threading.Thread(target=accept, daemon=True).start()
        print(f"driving {self.rd}, stream {self.stream}", file=sys.stderr, flush=True)
        self.loop.run()


def client(argv):
    if not argv or argv[0] in ("-h", "--help", "help"):
        print(COMMANDS)
        return 0
    if argv[0] == "wait":
        time.sleep(float(argv[1]))
        return 0
    with socket.socket(socket.AF_UNIX) as s:
        s.connect(SOCK)
        s.sendall((json.dumps(argv) + "\n").encode())
        reply = json.loads(s.makefile().readline())
    if "error" in reply:
        print(reply["error"], file=sys.stderr)
        return 1
    if reply["ok"] != "ok":
        print(reply["ok"] if isinstance(reply["ok"], str) else json.dumps(reply["ok"]))
    return 0


if __name__ == "__main__":
    if sys.argv[1:2] == ["serve"]:
        import gi

        gi.require_version("Gio", "2.0")
        Driver().serve()
    else:
        sys.exit(client(sys.argv[1:]))
