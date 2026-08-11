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
import struct
import time
import uuid
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
    # The lock says who owns it, for the reason `speak.lock` does and with the
    # same two halves. Stealing was a stat and then an unlink with nothing tying
    # the file it looked at to the file it deleted, so two waiters that both
    # judged the lock stale both deleted and both created - and the outcome of
    # two holders here is the permanent volume loss this file's own docstring
    # describes. The steal is an `os.replace` of a uniquely named file, atomic
    # on Windows and POSIX alike, so exactly one of them wins; and the release
    # checks the token first, or a holder legitimately stolen from deletes its
    # successor's live lock on the way out.
    token = f"{os.getpid()}-{uuid.uuid4().hex}"

    def owns() -> bool:
        try:
            return path.read_text(encoding="utf-8").strip() == token
        except OSError:
            return False

    held = False
    deadline = time.time() + 5.0
    while time.time() < deadline:
        try:
            fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.write(fd, token.encode("utf-8"))
            os.close(fd)
            held = True
            break
        except FileExistsError:
            try:
                if time.time() - path.stat().st_mtime > LOCK_STALE:
                    mine = path.with_name(f"duck.{token}.steal")
                    mine.write_text(token, encoding="utf-8")
                    os.replace(mine, path)
                    held = owns()
                    if held:
                        break
                    # Nothing left to tidy: `os.replace` renamed `mine` away
                    # whether or not this rename was the last one to land.
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
                if owns():
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


# --- the sweep -------------------------------------------------------------
#
# Everything above this line works from `duck.json` and the live session
# enumeration, and both are blind in the same place. `_restore` can only reach
# an application that currently holds a session, and it forgets any leftover
# whose process has exited - so an application that goes quiet while ducked and
# is then closed stays lowered with nothing on disk that knows what it was.
# Reproduced 2026-08-06 with stubbed sessions: 1.0 to 0.15, restore, still 0.15,
# record GONE. A reboot is the same failure for every application at once, which
# is what left Wispr Flow and Telegram at 15% this morning: the duck wrote at
# 11:15:39 and the machine went down at 11:16:05.
#
# It also compounds, because `InstanceIdentifier` embeds the pid. An application
# that comes back is a *new* key whose stored volume is still the lowered one,
# so the next duck records 0.15 as its normal: 1.0, 0.15, 0.0225, and a restore
# that puts it back to 0.15. That is the `firefox.exe` at 0.0225 on this machine.
#
# The registry is the only view that sees any of it. Windows persists a volume
# per executable *per endpoint*, so it survives the session going quiet, the
# process exiting and the reboot - which is exactly why the damage is permanent,
# and exactly what makes it findable afterwards.

FLOOR = 0.01          # the clamp in `_duck`; a duck can land here and no lower
STORE = (r"Software\Microsoft\Internet Explorer\LowRegistry"
         r"\Audio\PolicyConfig\PropertyStore")
_VOLUME = "3"         # the PROPVARIANT holding the level, float32 at offset 8


def _stored():
    """Every persisted per-application volume, across every endpoint.

    Yields `(subkey, name, value)`. The live enumeration cannot answer this:
    measured by TechnicalDirector, `firefox.exe` read a healthy 1.0 on the
    active device while sitting at 0.0225 on another.
    """
    import winreg
    try:
        root = winreg.OpenKey(winreg.HKEY_CURRENT_USER, STORE)
    except OSError:
        return
    with root:
        i = 0
        while True:
            try:
                app = winreg.EnumKey(root, i)
            except OSError:
                return
            i += 1
            try:
                with winreg.OpenKey(root, app) as k:
                    try:
                        name = str(winreg.QueryValueEx(k, None)[0])
                    except OSError:
                        name = ""
                    j = 0
                    while True:
                        try:
                            end = winreg.EnumKey(k, j)
                        except OSError:
                            break
                        j += 1
                        try:
                            with winreg.OpenKey(k, end) as e:
                                raw = winreg.QueryValueEx(e, _VOLUME)[0]
                                yield (app + "\\" + end, name,
                                       struct.unpack_from("<f", raw, 8)[0])
                        except (OSError, struct.error):
                            continue
            except OSError:
                continue


def is_duck(value: float, level: float) -> bool:
    """Could this value have been produced by ducking, and nothing else?

    A duck multiplies by `level` and clamps at FLOOR, so what it can leave
    behind is level, level squared, and so on down to the clamp. Anything else
    is a volume somebody chose.

    **Exactly 0 is never a duck** and must never be touched. `_duck` skips a
    session already at zero and the target is floored, so 0.15 to the fourth is
    still not zero - a zero is the user's own mute, and raising it is the one
    mistake here that is louder than the bug. Measured on this machine: one
    endpoint sits at 0.0 and has since 07-31.
    """
    if not 0.0 < value < 0.999 or not 0.0 < level < 1.0:
        return False
    if abs(value - FLOOR) < 1e-6:
        return True
    step = level
    for _ in range(6):                 # 0.15**6 is far below the clamp already
        if abs(value - step) < 1e-4:
            return True
        step *= level
    return False


def _app(ident: str) -> str:
    """The executable's own name out of either endpoint string, lowercased.

    A stored registry key and a live `InstanceIdentifier` name the same
    application through two different endpoint representations, so nothing
    either of them shares can be compared whole. Measured read-only on this
    machine, the two strings for Wispr Flow:

        stored = {2}.\\\\?\\hdaudio#func_01&ven_10ec…\\rearlineouttopohap/00010001
                 |\\Device\\…\\app-1.5.1146\\Wispr Flow.exe%b{0000…}
        live   = {0.0.0.00000000}.{e36d1d1a-…}
                 |\\Device\\…\\app-1.6.447\\Wispr Flow.exe%b{0000…}|1%b21420

    The prefixes differ, and so does the install directory, because the
    application auto-updates - which is why this goes all the way down to the
    basename rather than stopping at the first `|`. `speak.py` already computes
    exactly this reduction to print a sweep's result; the matching did not.

    Only a name ending `.exe` is allowed to pair, so the device-level entries
    (`…/00010001|#`) cannot claim a session by accident.
    """
    name = ident.split("\\")[-1].split("%b")[0].lower()
    return name if name.endswith(".exe") else ""


def sweep(level: float, signature_only: bool = False) -> list:
    """Put back anything left quiet. Returns what it did.

    By default this claims **every** stored volume between 0 and full, because
    the user's answer to "do you ever set an application's volume yourself" was
    no, it should always be at maximum (2026-08-06). That is not a shortcut: the
    signature rule reads `level` as it is *now*, so changing the duck in the
    station from 15% to 25% would orphan every 0.15 already on disk - nothing
    would ever claim them again and nothing would say so.

    `signature_only` restores the narrow rule, for a machine where some
    application is deliberately quiet. It is the safer half and the less useful
    one, and `is_duck` is what it means.

    **Neither mode touches an exact 0**, which is the one value Windows itself
    writes here: with communications ducking set to mute, starting a microphone
    capture takes other applications to 0.0 and puts them back within seconds -
    measured here, four of them. If that setting is ever "reduce by 80%" instead,
    those transients arrive as 0.2 and the wide mode *will* fight them, at which
    point this wants `signature_only`.

    The caller must know that nothing is speaking: a duck in progress looks
    exactly like a duck that failed, and undoing a live one raises the volume
    the duck is there to lower. Measured while writing this - a scan taken
    mid-line reported three applications as damaged and all three were correct.

    Repair through the live session where there is one, because setting a
    session's volume writes through to every stored entry for that executable:
    restoring five sessions here took all 40 registry entries back to 1.0. Only
    where there is no session at all does this write the value itself, which is
    the whole point of the sweep and also the half that cannot be verified from
    here - the audio engine may hold a cached copy for an application that is
    running but silent.

    That branch was dead for a release. It paired the two by substring, between
    two endpoint representations that share nothing - **0 of 49** stored entries
    matched any of the 6 live sessions here, including the four applications
    this feature exists for. Every repair fell through to the registry, which is
    the half whose write-through is *not* verified, and `fixed` reported them as
    repaired either way. `_app` compares the executable's basename instead:
    **27 of 49** on the same reading. Every session an application owns is put
    back, not the first - a browser holds several.
    """
    if os.name != "nt":
        return []
    claims = ((lambda v: is_duck(v, level)) if signature_only
              else (lambda v: 0.0 < v < 0.999))
    try:
        stored = [(k, n, v) for k, n, v in _stored() if claims(v)]
    except Exception:
        return []
    if not stored:
        return []

    live = {}
    try:
        for s in _sessions():
            try:
                app = _app(_ident(s))
                if app:            # "" is every session `_app` cannot name, and
                    live.setdefault(app, []).append(_volume(s))
            except Exception:
                continue
    except Exception:
        pass                           # pycaw missing: the registry path still works

    fixed = []
    for key, name, value in stored:
        vols = live.get(_app(name)) or []      # …a stored one would pair with it
        for vol in vols:
            try:
                vol.SetMasterVolume(1.0, None)
            except Exception:
                vols = []              # fall through to the registry instead
                break
        if vols:
            fixed.append((name, value, "session"))
            continue
        if _write_stored(key, 1.0):
            fixed.append((name, value, "registry"))
    return fixed


def _write_stored(key: str, value: float) -> bool:
    """Set one persisted volume in place, keeping the rest of the PROPVARIANT.

    The blob carries its own type tag and padding; only the float at offset 8 is
    ours to change. Rewriting the whole value would be guessing at a structure
    Windows owns.
    """
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, STORE + "\\" + key, 0,
                            winreg.KEY_READ | winreg.KEY_WRITE) as k:
            raw, typ = winreg.QueryValueEx(k, _VOLUME)
            blob = bytearray(raw)
            struct.pack_into("<f", blob, 8, value)
            winreg.SetValueEx(k, _VOLUME, 0, typ, bytes(blob))
        return True
    except (OSError, struct.error):
        return False


def recover(store: Path) -> None:
    """Undo ducking left behind by a worker that was killed."""
    try:
        age = time.time() - json.loads(store.read_text(encoding="utf-8"))["at"]
    except (OSError, ValueError, KeyError):
        return
    if age > STALE:
        restore(store)
