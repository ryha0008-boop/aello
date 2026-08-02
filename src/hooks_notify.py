#!/usr/bin/env python3
"""Raise a desktop notification - the OS kind, not one drawn inside a page.

The station's own pop-up only exists while you are looking at the station, so
it says nothing during the whole time an agent is working and you are in
another window. This one goes to the notification centre and appears over
whatever is in front of you.

Windows first, like duck.py and focus.py, and portable in the same shape:
macOS gets `osascript`, Linux gets `notify-send`, and anywhere without either
reports why it did nothing rather than raising. Only Windows gets buttons -
the other two have no notion of one.

Buttons are the whole point of it, and they are the hard part. A toast button
can only launch a URI, so revoiced registers a `revoiced:` protocol pointing at
action.py, and the buttons launch `revoiced://focus?...` and `revoiced://mute?...`.
That is also what buys the notification its own name and icon instead of
appearing as Windows PowerShell: an AppUserModelID registered in the same
place. Both are per-user registry keys, written idempotently at station
startup, and neither needs admin.

Fire and forget: the caller is the worker, and it is about to play audio. A
notification that delayed the voice it announces would be worse than none.
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path
from urllib.parse import quote
from xml.sax.saxutils import escape, quoteattr

IS_WIN = os.name == "nt"
IS_MAC = sys.platform == "darwin"
HERE = Path(__file__).resolve().parent
ICON = HERE.parent / "station.ico"

NO_WINDOW = subprocess.CREATE_NO_WINDOW if IS_WIN else 0

# What the toast is shown as, and what the protocol is called. The id is ours
# and registered by us; it is not a path to anything.
AUMID = "revoiced.station"
SCHEME = "revoiced"

# Where clicking the body of a toast goes. A second station on another port
# should send you to itself rather than to the first one.
STATION = os.environ.get("REVOICED_STATION", "http://127.0.0.1:8778/#live")


def enabled() -> bool:
    """The off switch. On unless REVOICED_TOAST says otherwise."""
    return (os.environ.get("REVOICED_TOAST", "1") or "").strip().lower() \
        not in ("0", "off", "no", "false")


def why_not() -> str:
    """Empty when a notification can be raised, else the reason it cannot."""
    if not enabled():
        return "REVOICED_TOAST is off"
    if IS_WIN:
        return "" if shutil.which("powershell.exe") else "powershell.exe not found"
    if IS_MAC:
        return "" if shutil.which("osascript") else "osascript not found"
    return "" if shutil.which("notify-send") else "notify-send not found"


def available() -> bool:
    return not why_not()


# --- registration ----------------------------------------------------------
# Both keys live under HKCU\Software\Classes, so this is a per-user change and
# never needs admin. Written on every station start rather than once: it is a
# handful of small values, and a machine that has had the repo moved would
# otherwise keep launching the old path forever.

def _handler_argv() -> list:
    """What Windows should run for a `revoiced://…` URI.

    pythonw, not python: the console host would flash a black window over
    whatever the user is doing every time they press a toast button, which is
    the same reason every other subprocess here passes CREATE_NO_WINDOW.
    """
    exe = Path(sys.executable)
    quiet = exe.with_name("pythonw.exe")
    return [str(quiet if quiet.exists() else exe), str(HERE / "action.py"), "%1"]


def serves_protocol() -> bool:
    """Whether this copy can actually handle a `revoiced://…` link.

    aello vendors the five hook-path files into each env dir, and `action.py`
    is not one of them - it belongs to the station. `HERE` is wherever *this*
    notify.py sits, so a copy in an env dir would otherwise point the machine's
    only `revoiced:` handler at a file that does not exist, killing both toast
    buttons everywhere and re-breaking them on each launch with a different
    env's path. A copy that cannot serve the protocol must not claim it.
    """
    return (HERE / "action.py").exists()


def register() -> str:
    """Register the protocol and the toast identity. Returns '' or a reason.

    Idempotent, and safe to call on a machine where it has already been done.

    The two halves are registered independently on purpose. The AUMID is just
    an identity - any copy may claim it, and a toast sent under an unregistered
    one is dropped by Windows with no error - while the protocol is a promise to
    run something, and only a copy with `action.py` beside it can keep that.
    """
    if not IS_WIN:
        return "registration is Windows-only"
    try:
        import winreg
    except ImportError:                              # pragma: no cover
        return "winreg unavailable"
    argv = _handler_argv()
    command = " ".join(f'"{a}"' if a != "%1" else '"%1"' for a in argv)
    try:
        if serves_protocol():
            with winreg.CreateKey(winreg.HKEY_CURRENT_USER,
                                  rf"Software\Classes\{SCHEME}") as k:
                winreg.SetValueEx(k, None, 0, winreg.REG_SZ, f"URL:{SCHEME}")
                winreg.SetValueEx(k, "URL Protocol", 0, winreg.REG_SZ, "")
            with winreg.CreateKey(
                    winreg.HKEY_CURRENT_USER,
                    rf"Software\Classes\{SCHEME}\shell\open\command") as k:
                winreg.SetValueEx(k, None, 0, winreg.REG_SZ, command)
        # The name and icon a toast is shown under. Without this it borrows
        # whichever application's id was used to send it.
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER,
                              rf"Software\Classes\AppUserModelId\{AUMID}") as k:
            winreg.SetValueEx(k, "DisplayName", 0, winreg.REG_SZ, "revoiced")
            if ICON.exists():
                winreg.SetValueEx(k, "IconUri", 0, winreg.REG_SZ, str(ICON))
    except OSError as e:
        return str(e)
    return ""


def registered() -> bool:
    """Whether the protocol currently points at this copy of action.py."""
    if not IS_WIN:
        return False
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                            rf"Software\Classes\{SCHEME}\shell\open\command") as k:
            return str(HERE / "action.py") in winreg.QueryValueEx(k, None)[0]
    except (OSError, ImportError):
        return False


# --- showing ---------------------------------------------------------------

def _toast_xml(title: str, body: str, buttons: list) -> str:
    """The toast document. `buttons` is [(label, uri), …].

    Silent on purpose: the agent is already talking, and a chime over the top
    of its own voice is the definition of annoying.
    """
    acts = "".join(
        f"<action content={quoteattr(label)} activationType='protocol' "
        f"arguments={quoteattr(uri)}/>"
        for label, uri in buttons)
    return (
        f"<toast activationType='protocol' launch={quoteattr(STATION)}>"
        f"<visual><binding template='ToastGeneric'>"
        f"<text>{escape(title)}</text><text>{escape(body)}</text>"
        f"</binding></visual>"
        f"<actions>{acts}</actions>"
        f"<audio silent='true'/></toast>")


def buttons_for(key: str, turn: str = "") -> list:
    """The two things worth doing about a notification you just heard: go and
    look, or stop it reading this one out.

    Skip, not Mute. Mute here silenced the project for good, from a button on a
    toast that is gone a second later and with nothing on the page to show for
    it afterwards - which is how seven projects ended up silent for a day. The
    thing you actually want while a line is being read is to stop *this* line.

    Skip carries the turn it belongs to, and *this line* is meant literally: a
    toast stays in the notification centre for three days, so without the id the
    button dropped whatever happened to be speaking when it was finally pressed
    - which, with 39 environments raising toasts, is usually somebody else.
    """
    if not key:
        return []
    q = quote(key, safe="")
    skip = f"{SCHEME}://skip?project={q}"
    if turn:
        skip += f"&id={quote(turn, safe='')}"
    return [("Go to terminal", f"{SCHEME}://focus?project={q}"),
            ("Skip this one", skip)]


def _cmd(title: str, body: str, key: str, turn: str = "") -> list | None:
    if IS_WIN:
        return ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                "-File", str(HERE / "win_audio.ps1"), "-Mode", "toast",
                "-Aumid", AUMID, "-Xml",
                _toast_xml(title, body, buttons_for(key, turn))]
    if IS_MAC:
        esc = lambda s: s.replace("\\", "\\\\").replace('"', '\\"')
        return ["osascript", "-e",
                f'display notification "{esc(body)}" with title "{esc(title)}"']
    return ["notify-send", "-a", "revoiced", title, body]


def show(title: str, body: str, key: str = "", turn: str = "") -> bool:
    """Raise one notification. True when it was handed off, not when it was seen.

    Nothing here is worth a traceback in a hook: every failure is a
    notification that did not appear, which is what the feature being off looks
    like anyway.
    """
    if not available():
        return False
    cmd = _cmd((title or "revoiced").strip(), " ".join((body or "").split()),
               key, turn)
    if not cmd:
        return False
    try:
        subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         creationflags=NO_WINDOW, close_fds=True)
        return True
    except OSError:
        return False


if __name__ == "__main__":
    # Registration on its own, for a machine that runs aello envs but never
    # starts the station. A toast sent under an unregistered AppUserModelID is
    # dropped by Windows with no error whatsoever, so without this every env on
    # such a machine is silently notification-less - the same failure the
    # three-file vendor caused, one layer out. No test toast: this runs at setup
    # time, where a pop-up would be noise.
    if "--register" in sys.argv[1:]:
        # Say which half was written. A copy without action.py registers the
        # identity only, and that is the normal case for a vendored one - it is
        # not a failure, but it is not the whole thing either.
        print(register() or ("registered" if serves_protocol()
                             else "registered identity only - no action.py "
                                  "beside this copy, so the protocol is left "
                                  "to whoever can serve it"))
        raise SystemExit(0)

    reason = why_not()
    if reason:
        print(f"cannot notify: {reason}")
        raise SystemExit(1)
    print("register:", register() or "ok")
    print("registered:", registered())
    args = sys.argv[1:]
    print("shown" if show(args[0] if args else "revoiced",
                          args[1] if len(args) > 1 else "test notification",
                          args[2] if len(args) > 2 else "")
          else "failed")
