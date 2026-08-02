"""Lower every other application's audio while revoiced speaks.

Windows only, via the per-application volume in the Core Audio API (pycaw). On
other platforms every call is a no-op.

Volumes are written to a file before being touched, so a worker killed
mid-sentence can't leave your music at 15% forever - the next run notices the
stale file and puts everything back.
"""

import json
import os
import time
from pathlib import Path

STALE = 120.0  # seconds; a duck file older than this belongs to a dead worker


def _sessions():
    from pycaw.pycaw import AudioUtilities
    return AudioUtilities.GetAllSessions()


def _volume(session):
    """pycaw exposes this as a property on newer versions, an interface on older."""
    try:
        return session.SimpleAudioVolume
    except AttributeError:
        from pycaw.pycaw import ISimpleAudioVolume
        return session._ctl.QueryInterface(ISimpleAudioVolume)


def duck(level: float, store: Path) -> bool:
    """Scale other apps to `level` (0.0 mutes, 1.0 disables ducking)."""
    if os.name != "nt" or level >= 1.0:
        return False

    # Never duck twice without restoring in between: the second call would read
    # the already-lowered volume as the original, multiply the reduction, and
    # leave audio permanently quiet once restored. A fresh file means somebody
    # is speaking and it is their duck; a stale one is a worker that died, and
    # it is put back *here* rather than assumed to have been handled elsewhere.
    # Falling through to duck on top of it is exactly how volumes spiral.
    try:
        prev = json.loads(store.read_text(encoding="utf-8"))
        if time.time() - float(prev.get("at", 0)) <= STALE:
            return False
        restore(store)
    except (OSError, ValueError, TypeError):
        pass

    try:
        sessions = _sessions()
    except Exception:
        return False                       # pycaw missing or COM unavailable

    saved = {}
    for s in sessions:
        if not s.Process:
            continue                       # system sounds, nothing to duck
        try:
            vol = _volume(s)
            current = vol.GetMasterVolume()
            if current <= 0.0:
                continue                   # already silent, leave it alone
            saved[str(s.Process.pid)] = current
            # Never land on exactly zero unless that is what was asked for.
            # A session at 0.0 is skipped by the guard above for good, so it
            # can never be saved and never be put back - it is the one value
            # this cannot recover from, and repeated ducking converges on it.
            target = current * level
            vol.SetMasterVolume(max(target, 0.01) if level > 0 else 0.0, None)
        except Exception:
            continue
    if not saved:
        return False
    try:
        store.write_text(json.dumps({"at": time.time(), "volumes": saved}),
                         encoding="utf-8")
    except OSError:
        pass
    return True


def restore(store: Path) -> None:
    if os.name != "nt":
        return
    try:
        data = json.loads(store.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    saved = data.get("volumes", {})
    try:
        sessions = _sessions()
    except Exception:
        store.unlink(missing_ok=True)
        return
    for s in sessions:
        if not s.Process:
            continue
        want = saved.get(str(s.Process.pid))
        if want is None:
            continue
        try:
            _volume(s).SetMasterVolume(float(want), None)
        except Exception:
            continue
    store.unlink(missing_ok=True)


def recover(store: Path) -> None:
    """Undo ducking left behind by a worker that was killed."""
    try:
        age = time.time() - json.loads(store.read_text(encoding="utf-8"))["at"]
    except (OSError, ValueError, KeyError):
        return
    if age > STALE:
        restore(store)
