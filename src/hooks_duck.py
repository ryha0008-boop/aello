"""Lower every other application's audio while revoiced speaks.

Windows only, via the per-application volume in the Core Audio API (pycaw). On
other platforms every call is a no-op.

Volumes are written to a file before being touched, so a worker killed
mid-sentence can't leave your music at 15% forever - the next run notices the
stale file and puts everything back. Every path through that file takes
`_lock` first, and nothing is deleted from it until it has been put back.
"""

import contextlib
import ctypes
import json
import os
import time
from pathlib import Path

STALE = 120.0  # seconds; a duck file older than this belongs to a dead worker
LOCK_STALE = 30.0  # seconds; a duck lock older than this belongs to a dead one


@contextlib.contextmanager
def _lock(store: Path):
    """Serialise everything that touches the store, across processes.

    The store is a read-modify-write and the only record of what normal was, so
    two callers that overlap both read "nothing is ducked", both lower, and the
    second's write buries the first's originals. Measured, two concurrent
    duck(0.5) calls: both returned True, every application ended at 0.25, and
    0.5 was recorded as its normal volume - so the restore put it back to half
    and deleted the only evidence. That is how three applications here ended up
    at 0.15 squared and one at the 0.01 floor.

    The speaker lock does not cover this and cannot: the station restores from
    its own poll thread, every two seconds, holding nothing.
    """
    path = store.with_name("duck.lock")
    held = False
    deadline = time.time() + 5.0
    while time.time() < deadline:
        try:
            os.close(os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY))
            held = True
            break
        except FileExistsError:
            try:
                if time.time() - path.stat().st_mtime > LOCK_STALE:
                    path.unlink()
                    continue
            except OSError:
                pass
            time.sleep(0.05)
        except OSError:
            break                          # no run dir; better unducked than stuck
    try:
        yield held
    finally:
        if held:
            try:
                path.unlink()
            except OSError:
                pass


def _alive(pid: int) -> bool:
    """Is that process still running? Used only to forget pending entries.

    Declare the types. A HANDLE is 64-bit and ctypes returns a signed 32-bit
    int by default, so the value is truncated - which reads as a dead process
    whenever the top half is what mattered, and hands CloseHandle a number
    belonging to nobody. Measured before the argtypes went in: the same live
    pid was kept by two polls and dropped by the third.
    """
    k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    k32.OpenProcess.restype = ctypes.c_void_p
    k32.OpenProcess.argtypes = (ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32)
    k32.CloseHandle.argtypes = (ctypes.c_void_p,)
    handle = k32.OpenProcess(0x1000, False, pid)   # QUERY_LIMITED_INFORMATION
    if not handle:
        return False
    k32.CloseHandle(handle)
    return True


def _sessions():
    # COM is initialised per thread, and comtypes only does it for whichever
    # thread imports it first. The station is a ThreadingHTTPServer, so its
    # first poll got a working enumeration and every one after it - a different
    # thread, every two seconds, for days - raised "CoInitialize has not been
    # called". Measured: thread 0 fine, threads 1-3 and the main thread after
    # them all raised. That is what deleted the record while the volumes were
    # still down. Cheap, idempotent, and the only place that needs it.
    try:
        import comtypes
        comtypes.CoInitialize()
    except Exception:
        pass
    from pycaw.pycaw import AudioUtilities
    return AudioUtilities.GetAllSessions()


def _volume(session):
    """pycaw exposes this as a property on newer versions, an interface on older."""
    try:
        return session.SimpleAudioVolume
    except AttributeError:
        from pycaw.pycaw import ISimpleAudioVolume
        return session._ctl.QueryInterface(ISimpleAudioVolume)


def _ident(session) -> str:
    """What the record is keyed on: a session, never a process.

    One pid owns several sessions - a browser or a game runs a media stream and
    a notification stream under the same process, and `GetAllSessions()` yields
    one control for each. Keyed on the pid, the second reading overwrote the
    first while *both* were lowered, so the restore raised one and left the
    other down with its record already deleted: 1.0 to 0.15 to 0.0225 to the
    0.01 floor in three turns, permanently. `InstanceIdentifier` carries a
    per-instance index (`...|1%b13596`) and separates them.

    The pid is what a process outliving its session is checked with, so it is
    kept alongside rather than instead.
    """
    try:
        return session.InstanceIdentifier
    except Exception:
        return f"pid:{session.Process.pid}"


def _entry(key: str, value) -> dict:
    """One saved reading, tolerating the pid-keyed records written before.

    A copy still on HOOK_VERSION 10 may have left a `{pid: float}` record on
    disk, and the machine has 41 of them. The key was the pid then, so it is
    read back as one - dropping these would leave exactly the stuck-quiet
    application this whole file exists to prevent.
    """
    if isinstance(value, dict):
        return {"vol": float(value.get("vol", 1.0)),
                "pid": int(value.get("pid") or 0)}
    return {"vol": float(value), "pid": int(key) if key.isdigit() else 0}


def duck(level: float, store: Path) -> bool:
    """Scale other apps to `level` (0.0 mutes, 1.0 disables ducking)."""
    if os.name != "nt" or level >= 1.0:
        return False
    with _lock(store) as held:
        # Not ducking is a line played over your music. Ducking without the
        # lock is your music at 15% of 15% for good, and this is the one thing
        # here that has no way back once the record is lost.
        return _duck(level, store) if held else False


def _duck(level: float, store: Path) -> bool:
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
        _restore(store)
    except (OSError, ValueError, TypeError):
        pass

    # Whatever the restore above could not reach is still lowered, and this
    # file is the only thing that knows what it was - so carry it into the new
    # record instead of letting a fresh one bury it. They have just been
    # retried, so what is left is genuinely unreachable, and it keeps being
    # retried on every cycle until its session is back or its process is gone.
    pending = {}
    try:
        left = json.loads(store.read_text(encoding="utf-8")).get("volumes", {})
        pending = {k: _entry(k, v) for k, v in left.items()}
    except (OSError, ValueError, AttributeError, TypeError):
        pass

    try:
        sessions = _sessions()
    except Exception:
        return False                       # pycaw missing or COM unavailable

    saved, lower = {}, []
    for s in sessions:
        if not s.Process:
            continue                       # system sounds, nothing to duck
        try:
            vol = _volume(s)
            current = vol.GetMasterVolume()
            if current <= 0.0:
                continue                   # already silent, leave it alone
            saved[_ident(s)] = {"vol": current, "pid": s.Process.pid}
            # Never land on exactly zero unless that is what was asked for.
            # A session at 0.0 is skipped by the guard above for good, so it
            # can never be saved and never be put back - it is the one value
            # this cannot recover from, and repeated ducking converges on it.
            target = current * level
            lower.append((vol, max(target, 0.01) if level > 0 else 0.0))
        except Exception:
            continue
    if not saved:
        return False
    # A pending reading wins over a live one for the same session: if it came
    # back between the retry and here, what is on the device is the volume this
    # left it at, and the file holds what it was before that.
    saved = {**saved, **pending}
    # Write the record first, then lower - the order the docstring has always
    # claimed and the code never had. Windows keeps a per-application volume
    # against the executable's path, so it outlives the audio session, the
    # process and the reboot: anything lowered without a record on disk stays
    # lowered for good, and the next duck reads that as its normal volume.
    try:
        store.write_text(json.dumps({"at": time.time(), "volumes": saved}),
                         encoding="utf-8")
    except OSError:
        return False                       # no record, so nothing gets touched
    for vol, target in lower:
        try:
            vol.SetMasterVolume(target, None)
        except Exception:
            continue
    return True


def restore(store: Path) -> None:
    if os.name != "nt":
        return
    with _lock(store) as held:
        # Contended is not wedged. Falling through on a mere 5s timeout let the
        # station's poll restore every session and delete the record while a
        # worker was still inside the lowering loop - the record gone, the
        # volumes on their way down, and nothing left that knew what normal
        # was. `_lock` already steals a genuinely dead lock after LOCK_STALE,
        # so a timeout here means somebody live is holding it: leave it to
        # them. The station retries in two seconds and every later duck retries
        # leftovers anyway, so nothing is lost by waiting.
        if held:
            _restore(store)


def _restore(store: Path) -> None:
    try:
        data = json.loads(store.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    saved = {k: _entry(k, v) for k, v in data.get("volumes", {}).items()}
    try:
        sessions = _sessions()
    except Exception:
        return          # keep the record: it is the only way back up from here
    for s in sessions:
        if not s.Process:
            continue
        key = _ident(s)
        want = saved.pop(key, None)
        if want is None:                   # a record from before sessions were the key
            key = str(s.Process.pid)
            want = saved.pop(key, None)
        if want is None:
            continue
        try:
            _volume(s).SetMasterVolume(float(want["vol"]), None)
        except Exception:
            saved[key] = want
    # Delete only what was actually put back. An application whose audio session
    # has gone quiet is not in the enumeration at all - a dictation app, a chat
    # notification, anything that opens the device per sound - and dropping the
    # record on its behalf left it at 15% with nothing that knew what it was.
    # What is left waits, at zero, which reads as a duck that is over: the next
    # duck, recover() and every station poll try it again, and it lands the
    # moment that application next has a session. Once its process is gone the
    # entry can never match anything again, so it is forgotten.
    saved = {k: v for k, v in saved.items() if _alive(v["pid"])}
    if not saved:
        store.unlink(missing_ok=True)
        return
    try:
        store.write_text(json.dumps({"at": 0, "volumes": saved}),
                         encoding="utf-8")
    except OSError:
        store.unlink(missing_ok=True)


def recover(store: Path) -> None:
    """Undo ducking left behind by a worker that was killed."""
    try:
        age = time.time() - json.loads(store.read_text(encoding="utf-8"))["at"]
    except (OSError, ValueError, KeyError):
        return
    if age > STALE:
        restore(store)
