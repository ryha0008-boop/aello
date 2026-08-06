#!/usr/bin/env python3
"""revoiced - speak Claude Code's responses out loud.

Registered as a Stop hook. Reads the hook payload on stdin, takes the trailing
TL;DR line of the response, and speaks it prefixed by the project directory name.

Voices come from a pool of presets (a base voice plus rate/pitch tweaks, Edge or
ElevenLabs). Each terminal leases one preset for as long as it is working and
hands it back when the session ends, so concurrent projects sound different but
a single project keeps one voice throughout.

Synthesis runs in parallel; playback is serialised behind a lock so several
sessions finishing at once queue instead of talking over each other.

    speak.py --status
    speak.py --stop                  stop everything, queue included
    speak.py --skip                  skip only what is speaking now; queue plays on
    speak.py --mute | --unmute       global
    speak.py --mute-project [PATH]   silence one project only
    speak.py --unmute-project [PATH]
    speak.py --presets               list the voice pool
    speak.py --voices                which project is pinned to which voice
    speak.py --pin PATH [PRESET]     pin a project's voice; no preset unpins
    speak.py --leases                who currently holds which voice
    speak.py --release [SESSION]     hand a voice back (SessionEnd hook)
    speak.py --sweep                 is any app still left quiet by a duck?
"""

import html
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
import uuid
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))

# duck, focus and notify are optional siblings. speak.py gets copied places - aello
# vendors it into an env dir - and a partial copy must still speak rather than
# ImportError on every response. Absent, they degrade to exactly what they
# already return off Windows: no window, no pid, no toast. Every caller handles
# that case today.
#
# But the degradation must be *loud*. A three-file vendor into every env dir
# turned notifications off everywhere for two hours, and because the fallback
# said nothing, it was reported as broken twice and measured as healthy twice -
# the tests ran this repo's copy, which has its siblings, rather than the copy
# the hook executes. MISSING is what makes that visible: it is recorded on every
# turn and printed by --status.
MISSING = []
# duck was the one imported unguarded, which made it the one that could take
# everything down: a four-file vendor raised ImportError at module scope, before
# hook() existed to swallow it and before --hook-version could answer - so the
# env fell totally silent and all three diagnostics built for exactly this were
# dead too. Absent, ducking is simply not done, which is what it already does
# off Windows.
try:
    import duck as ducking
except ImportError:
    MISSING.append("duck")
    ducking = SimpleNamespace(duck=lambda *a, **k: False,
                              restore=lambda *a, **k: None,
                              recover=lambda *a, **k: None,
                              sweep=lambda *a, **k: [])
try:
    import focus as focusing
except ImportError:
    MISSING.append("focus")
    focusing = SimpleNamespace(window_pids=lambda: set(),
                               window_names=lambda: set(),
                               window_name_counts=lambda: {},
                               name_key=lambda t: (t or "").strip().lower(),
                               terminal_pid=lambda: 0,
                               terminal_window=lambda *a, **k: None)
try:
    import notify as notifying
except ImportError:
    MISSING.append("notify")
    notifying = SimpleNamespace(show=lambda *a, **k: False)

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
IS_WIN = os.name == "nt"
IS_MAC = sys.platform == "darwin"

# Windows gives every child process its own console unless told otherwise, and
# synthesis runs on each response - so a black window flashed up over whatever
# you were doing, every single time. Every subprocess here is headless.
NO_WINDOW = {"creationflags": subprocess.CREATE_NO_WINDOW} if IS_WIN else {}

# Bumped whenever anything on the hook path changes - this file, duck.py or
# win_audio.ps1. aello vendors those three and records the value it copied, so
# comparing it against the value here is how a vendored copy learns it has
# fallen behind. Station-only changes leave it alone: a version that moves for
# reasons the hook never executes trains everyone to ignore the warning.
HOOK_VERSION = 19


def _num(name: str, default, cast=int):
    """An env var that is set but empty must not kill the hook.

    `int(os.environ.get(NAME, "1200"))` applies its default only to an *absent*
    key: `REVOICED_PLAY_MAX=""` in a settings.json env block yields `''` and
    raises at module scope - before hook() exists to swallow it, and taking
    serve.py's `import speak` down with it. Unguardable from outside, and it
    prints a traceback into the user's session on every response, which is the
    one thing this file must never do. Anything unparseable falls back rather
    than raising, for the same reason.
    """
    try:
        return cast(os.environ.get(name, "").strip() or default)
    except (TypeError, ValueError):
        return cast(default)


MAX_CHARS = _num("REVOICED_MAX_CHARS", 1200)
KEEP = _num("REVOICED_HISTORY", 1000)
# History records what you asked as well as what was answered, so a turn reads
# as a pair. Off with REVOICED_PROMPTS=0, which stops it being captured at all
# rather than merely hiding it - what you typed is the one thing here that was
# never the machine's to keep. The cap is per turn, and generous: a prompt is
# read on screen, not spoken, so there is no MAX_CHARS-style budget to respect,
# only the trimmed file itself. A pasted log is what it guards against.
PROMPTS = os.environ.get("REVOICED_PROMPTS", "1") != "0"
PROMPT_MAX = _num("REVOICED_PROMPT_MAX", 4000)
ENFORCE = os.environ.get("REVOICED_ENFORCE", "1") != "0"
LEASE_TTL = _num("REVOICED_LEASE_TTL", 43200, float)  # 12h
# How long a lease is believed on its own word, before its terminal has to
# still be on screen for it to count as running. Long enough that an agent
# thinking hard is never called dead; short enough that a window you closed
# stops claiming to be running while you are still looking at the page.
LEASE_GRACE = _num("REVOICED_LEASE_GRACE", 300, float)  # 5min
# The longest a single line may hold the speaker lock. That lock is machine-wide
# and heartbeated for exactly as long as the player lives, so a player that
# never exits silences every env on the box and nothing else ever gives up.
# 0 turns the cap off. A full-length line is well under a minute of speech.
PLAY_MAX = _num("REVOICED_PLAY_MAX", 300, float)  # 5min
# The duck puts itself back in a finally, and the station puts back what a dead
# worker left - but neither can reach an application that has gone quiet or
# closed, because both work from the live session enumeration. `duck.sweep`
# reads the volumes Windows has persisted instead, which is the only view that
# outlives the session, the process and the reboot.
#
# Three settings, because what is safe depends on a fact about the person:
# `0` off; `signature` claims only what this duck level could have produced;
# and the default, which claims everything below full. The default is the
# user's own answer - they never set an application's volume, it should always
# be at maximum - and it also closes a hole the narrow rule has, because the
# signature is computed from the duck level *as it is now*: change it in the
# station and every value the old one left is orphaned, claimed by nothing and
# reported by nothing.
SWEEP = os.environ.get("REVOICED_SWEEP", "1") != "0"
SWEEP_SIGNATURE = os.environ.get("REVOICED_SWEEP", "1") == "signature"

# Used when the pool is empty, so a fresh install still speaks.
DEFAULT_PRESET = {
    "id": "default",
    "name": "Andrew",
    "provider": "edge",
    "voice": os.environ.get("REVOICED_VOICE", "en-US-AndrewMultilingualNeural"),
    "rate": os.environ.get("REVOICED_RATE", "+0%"),
    "pitch": "+0Hz",
    "volume": "+0%",
}


def data_dir() -> Path:
    if IS_WIN:
        base = os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local"
    elif IS_MAC:
        base = Path.home() / "Library" / "Application Support"
    else:
        base = os.environ.get("XDG_DATA_HOME") or Path.home() / ".local" / "share"
    d = Path(base) / "revoiced"
    (d / "audio").mkdir(parents=True, exist_ok=True)
    (d / "run").mkdir(parents=True, exist_ok=True)
    return d


DATA = data_dir()
HISTORY = DATA / "history.jsonl"
STATE = DATA / "state.json"
RUN = DATA / "run"
AUDIO = DATA / "audio"


# --- small file lock -------------------------------------------------------
# Holders touch the lock while they work; anything that stops being touched is
# assumed dead and stolen, so a killed worker can't wedge the queue.
_HELD = threading.local()

@contextmanager
def lock(name: str, stale: float = 5.0, timeout: float = 600.0):
    """A lock file that knows who owns it.

    It used to be empty and anonymous, and released with a bare `unlink` of the
    path. So a holder that ran long enough to be legitimately stolen from went
    on to delete its *successor's* live lock file on the way out, and a third
    caller then acquired while the second still believed it held - two
    concurrent read-modify-writes of state.json, which is the one thing this
    project's central invariant exists to prevent. No race window needed: the
    steal is the normal, documented path.

    So the token is written into the file and checked before the unlink, and
    the steal goes through `os.replace` of a uniquely named file - atomic on
    Windows and POSIX alike, so of two waiters that both judge a lock stale
    exactly one can win.
    """
    path = RUN / f"{name}.lock"
    token = f"{os.getpid()}-{uuid.uuid4().hex}"
    deadline = time.time() + timeout
    held = False
    # Which locks *this thread* holds, so write_state can refuse an unlocked
    # write. Thread-local because the station serves every request on a new one.
    names = getattr(_HELD, "names", None)
    if names is None:
        names = _HELD.names = set()
    while time.time() < deadline:
        try:
            fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.write(fd, token.encode("utf-8"))
            os.close(fd)
            held = True
            break
        except FileExistsError:
            try:
                if time.time() - path.stat().st_mtime > stale:
                    mine = path.with_name(f"{name}.{token}.steal")
                    mine.write_text(token, encoding="utf-8")
                    os.replace(mine, path)
                    # Whoever's rename landed last owns it, and everyone else
                    # reads back somebody else's token and keeps waiting.
                    held = _owns(path, token)
                    if held:
                        break
                    mine.unlink(missing_ok=True)
                    continue
            except OSError:
                pass
            time.sleep(0.2)
    if held:
        names.add(name)
    try:
        yield held
    finally:
        if held:
            names.discard(name)
            try:
                if _owns(path, token):
                    path.unlink()
            except OSError:
                pass


def _owns(path: Path, token: str) -> bool:
    try:
        return path.read_text(encoding="utf-8").strip() == token
    except OSError:
        return False


def touch(name: str) -> None:
    try:
        os.utime(RUN / f"{name}.lock", None)
    except OSError:
        pass


# --- state -----------------------------------------------------------------

def read_state() -> dict:
    # "Absent" and "there but unreadable" are two different answers and used to
    # be one. A truncated state.json parsed as ValueError, came back as bare
    # defaults, and the next of the twenty-odd writers persisted those defaults
    # over the real file - every preset, pin, lease, colour and mute gone, with
    # no error anywhere. `_intact` is what write_state refuses to overwrite on.
    s, intact = {}, True
    try:
        s = json.loads(STATE.read_text(encoding="utf-8"))
        if not isinstance(s, dict):
            s, intact = {}, False
    except OSError:
        pass                      # no file yet, which is a legitimate empty
    except ValueError:
        intact = False            # present and corrupt: defaults are not the truth
    s["_intact"] = intact
    s.setdefault("global", False)
    s.setdefault("projects", {})
    s.setdefault("presets", [])
    s.setdefault("leases", {})
    s.setdefault("duck", 15)      # percent to drop other audio to; 100 = off
    s.setdefault("windows", {})   # cwd -> terminal window title, when guessing is wrong
    s.setdefault("voices", {})    # cwd -> preset id, pinned for good
    s.setdefault("images", {})    # cwd -> picture filename under images/
    s.setdefault("colors", {})    # cwd -> that picture's accent colour
    s.setdefault("prompts", {})   # cwd -> {subject, style, energy, palette, extra}
    s.setdefault("promptbase", "")  # the tail every generated prompt ends with
    s.setdefault("profiles", {})  # env dir -> {name, cwd, blueprint, launch, pid}
    s.setdefault("spoken", 0)     # turns ever spoken; survives the history trim
    return s


def write_state(state: dict) -> None:
    """Publish state.json, or decline to.

    Three ways this used to lose everything in the file:

    A shared staging path. `STATE.with_suffix(".tmp")` is one name for all 41
    environments and the station, so two writers overlapping meant one
    published the other's bytes and the loser's `replace` raised - uncaught all
    the way out to a traceback in the user's session. It is per-process and
    unique now.

    No flush. `replace` is atomic with respect to the *name*, not to bytes that
    are still in the OS cache, so a crash mid-write published a truncated file.

    And a write on top of a file it could not read: see read_state. If what we
    are holding was reconstructed out of defaults because the real thing was
    unparseable, writing it is how a corrupt file becomes a lost one.
    """
    if not state.pop("_intact", True):
        return
    if "state" not in getattr(_HELD, "names", ()):
        # Every one of the twenty-odd call sites is inside lock("state"), but
        # lock() yields False on timeout rather than raising and not one site
        # checked - so a starve turned the project's central invariant off
        # silently. Refusing here costs one dropped write and cannot corrupt.
        return
    tmp = STATE.with_name(f"state.{os.getpid()}-{uuid.uuid4().hex}.tmp")
    try:
        with open(tmp, "w", encoding="utf-8") as fh:
            json.dump(state, fh, indent=2, allow_nan=False)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp, STATE)
    except (OSError, ValueError):
        # allow_nan=False raises on a NaN that reached state some other way:
        # json.dumps would happily emit a literal NaN, which every browser's
        # JSON.parse rejects - and the page polls this every two seconds, so
        # one bad float killed it permanently. Better to drop the write.
        try:
            tmp.unlink(missing_ok=True)
        except OSError:
            pass


@contextmanager
def mutate_state(stale: float = 5.0, timeout: float = 15.0):
    """The only safe way to change state.json: read, modify, write, under one lock.

    Twenty-one call sites spelled this out by hand, all with these same two
    numbers, and the shape is load-bearing rather than decorative - the file has
    two writers (aello owns `global`, `projects` and the stop token; revoiced
    owns everything else), so a read taken earlier and written later silently
    reverts whatever the other one did in between. Getting that wrong loses an
    aello-set mute, which is the one bug users do not forgive.

    Writes only when the block actually changed something. That is not an
    optimisation, it is the contract `/api/state` already depended on: it calls
    `reap_leases` on every poll, every two seconds, for as long as a tab is open,
    and a context manager that published unconditionally would rewrite the file
    forever against forty-one hooks contending for the same lock. It also
    removes the older footgun in the other direction - a block that mutated the
    dict and forgot `write_state` lost the change with nothing to show for it.

    An exception inside the block skips the write, which is what you want:
    half-applied is worse than not applied.
    """
    with lock("state", stale=stale, timeout=timeout):
        st = read_state()
        before = _fingerprint(st)
        yield st
        if before is None or _fingerprint(st) != before:
            write_state(st)


def _fingerprint(state: dict):
    """A cheap "has this changed" signature, or None when it cannot be taken -
    which is read as *assume it changed*, because dropping a real write is worse
    than making a redundant one."""
    try:
        return json.dumps(state, sort_keys=True, default=str)
    except (TypeError, ValueError):
        return None


def is_muted(cwd: str, key: str = "") -> bool:
    s = read_state()
    if s["global"] or s["projects"].get(str(cwd)):
        return True
    # `projects` is aello's key, so it is always keyed by working directory and
    # is checked above whatever this session is pinned under.
    return bool(key and s["projects"].get(key))


# --- profiles --------------------------------------------------------------
# A profile is a declared agent: one `.claude-env-<name>` directory, the folder
# it runs in, and how to start it. The env dir is the identity because aello
# already exports it as CLAUDE_CONFIG_DIR, so a session states who it is rather
# than being guessed at.
#
# Working directory was the old answer and it is wrong twice over: an agent that
# cd's into a subfolder looked like a new project (`desktop-automation/client`
# was never a project, just somewhere an agent stood), and several agents
# sharing one repo - cleaning-website has three - were forced to share a voice.

def env_dir() -> str:
    """This session's env directory, or "" when not running under aello."""
    return (os.environ.get("CLAUDE_CONFIG_DIR") or "").strip().rstrip("\\/")


def find_profile(env: str, profiles: dict) -> str:
    """The profile key matching this env dir, or "". Windows paths compare
    case-insensitively, and the same dir arrives spelled both ways."""
    if not env:
        return ""
    want = os.path.normcase(os.path.normpath(env))
    for key in profiles:
        if os.path.normcase(os.path.normpath(key)) == want:
            return key
    return ""


def identity(cwd: str, profiles: dict) -> tuple:
    """(pin key, profile or None) for this session.

    Falls back to the working directory whenever there is no profile, so an
    unprofiled session keeps the voice and picture it already had. Nothing
    switches over until a profile is declared for it - which is what makes
    adding them one at a time safe.
    """
    key = find_profile(env_dir(), profiles)
    return (key, profiles[key]) if key else (str(cwd), None)


# --- voice pins ------------------------------------------------------------
# A project keeps its voice for good. The pin (key -> preset) is the source of
# truth and is set from the station; leases only record which session is live
# where. This replaced a per-session lease, which handed each new terminal
# whatever preset happened to be free - so one project drifted through five
# different voices in a day and none of them meant anything.
#
# The key is a profile's env dir where one is declared, and the working
# directory otherwise - see identity().

def pin_preset(key: str, presets: list, pins: dict) -> str:
    """The preset id pinned to this project, claiming one if it has none."""
    by_id = {p["id"]: p for p in presets}
    if pins.get(key) in by_id:
        return pins[key]
    used = [pins[c] for c in pins if c != key]
    free = [p for p in presets if p["id"] not in used]
    # More projects than voices: share out the one fewest projects already use.
    chosen = free[0] if free else min(presets, key=lambda p: used.count(p["id"]))
    pins[key] = chosen["id"]
    return chosen["id"]


def dead_leases(leases: dict) -> list:
    """The sessions in `leases` that are over: idle past the TTL, or running in
    a terminal window that has since been closed.

    Only SessionEnd used to clear a lease, and closing a terminal with the X
    does not fire it - so a finished agent read as "running" in the station for
    the full 12 hours. A lease with no recorded pid, or one taken off Windows,
    falls back to the TTL alone.

    The pid is not enough on its own, and on this desktop it is worth nothing:
    Windows Terminal hosts every window in one process, so every session records
    the same pid and it stays alive while any terminal anywhere is open. A
    closed window was indistinguishable from a busy one. So a lease that has
    gone quiet is also checked against the titles on screen, and dropped when
    nothing out there is called what it is. Only after GRACE, and only when
    there are titles to check against: an agent mid-turn must never be judged
    dead, and no evidence still means alive.
    """
    now = time.time()
    alive = focusing.window_pids()
    counts = focusing.window_name_counts()
    titles = set(counts)
    out = []

    def named(l):
        return {focusing.name_key(l.get("title") or ""),
                focusing.name_key(l.get("project") or "")} - {""}

    for sid, l in leases.items():
        idle = now - float(l.get("last_used", 0))
        if idle > LEASE_TTL:
            out.append(sid)
            continue
        pid = int(l.get("pid") or 0)
        if pid and alive and pid not in alive:
            out.append(sid)
            continue
        if titles and idle > LEASE_GRACE:
            want = named(l)
            if want and not (want & titles):
                out.append(sid)

    # More leases on one profile than it has windows. Two windows on one profile
    # are two runs and both are kept - but close a terminal with the X and open
    # another for the same project and the first lease lives on, because no
    # SessionEnd fires and the name it recorded still matches the window that
    # replaced it. That read as "2 running" beside a profile with one terminal.
    # Newest first, so the survivors are the ones that spoke most recently.
    if counts:
        groups = {}
        for sid, l in leases.items():
            if sid not in out:
                groups.setdefault(l.get("key") or l.get("cwd"), []).append((sid, l))
        for group in groups.values():
            if len(group) < 2:
                continue
            room = max((counts.get(n, 0) for n in named(group[0][1])), default=0)
            if not room or len(group) <= room:
                continue                       # no evidence, or room for them all
            group.sort(key=lambda g: float(g[1].get("last_used", 0)), reverse=True)
            out += [sid for sid, l in group[room:]
                    if now - float(l.get("last_used", 0)) > LEASE_GRACE]
    return out


def reap_leases() -> int:
    """Drop every finished lease. Writes only when there is something to drop,
    so the station can call this on each poll without churning state.json."""
    with mutate_state() as st:
        dead = dead_leases(st["leases"])
        if not dead:
            return 0
        for sid in dead:
            st["leases"].pop(sid, None)
        return len(dead)


def lease_preset(session: str, key: str, project: str, cwd: str = "",
                 window: dict = None) -> dict:
    with mutate_state() as st:
        presets = st.get("presets") or []
        if not presets:
            return DEFAULT_PRESET

        now = time.time()
        leases = st["leases"]
        for sid in dead_leases(leases):
            leases.pop(sid, None)          # terminal died without releasing

        # Whose window closing means this session is over. No title matched
        # still leaves a usable pid, so ask for it directly rather than only
        # through `window`.
        pid = int((window or {}).get("pid") or 0) or focusing.terminal_pid()

        # A session that ended in /clear keeps its lease - that is what stops
        # the station blinking out - so retire it here, as the session that
        # replaced it speaks for the first time. Only ones actually marked
        # cleared: two windows on one profile are two live runs and both must
        # survive, and the pid cannot tell them apart because Windows Terminal
        # gives every window the same one.
        for sid in [s for s, l in leases.items()
                    if s != session and l.get("cleared")
                    and (l.get("key") or l.get("cwd")) == key]:
            leases.pop(sid)

        chosen = pin_preset(key, presets, st["voices"])
        prev = leases.get(session, {})
        leases[session] = {"preset": chosen, "key": key, "cwd": cwd or key,
                           "project": project, "pid": pid,
                           # The window this ran in, by name. dead_leases has
                           # nothing else to check once the pid is shared.
                           "title": (window or {}).get("title") or "",
                           "acquired": prev.get("acquired", now), "last_used": now}
        return {p["id"]: p for p in presets}[chosen]


# SessionEnd fires on /clear and /resume too, and neither ends anything you can
# see: the terminal stays open and a new session takes over in it within
# seconds. Releasing there dropped the project out of the station's running bar
# until it next happened to speak, which is minutes of a window that never
# stopped working reading as idle. What ends a run is the window closing, and
# dead_leases() already watches for that.
SESSION_CONTINUES = {"clear", "resume"}


def mark_cleared(session: str) -> bool:
    """Keep this session's lease, but record that it is a placeholder.

    /clear ends a session and starts another in the same window. Dropping the
    lease blanked the agent out of the station until it next spoke; keeping it
    untouched left a second lease behind when the replacement arrived. Marked,
    it survives until the session that replaced it speaks - and if none ever
    does, dead_leases sees a lease with no window on screen and drops it.
    """
    with mutate_state() as st:
        l = st["leases"].get(session)
        if not l:
            return False
        l["cleared"] = time.time()
        return True


def release_lease(session: str) -> bool:
    with mutate_state() as st:
        if st["leases"].pop(session, None) is None:
            return False
        return True


# --- text ------------------------------------------------------------------

TLDR = re.compile(
    r"(?im)^[ \t]*(?:[-*]\s*)?(?:\*\*|__)?TL;?DR(?:\*\*|__)?\s*[:\-—]\s*"
    r"(?:\*\*|__)?\s*(.+?)[ \t]*$"
)


def extract_tldr(md: str) -> str:
    """The trailing TL;DR line, which is all we speak.

    Reading a whole response aloud is useless - you skim for keywords, you don't
    listen to an essay. Set REVOICED_FULL=1 to speak everything instead.
    """
    if os.environ.get("REVOICED_FULL") == "1":
        return md
    found = TLDR.findall(md)
    return found[-1].strip() if found else ""


def to_speakable(md: str) -> str:
    t = md
    t = re.sub(r"```.*?```", " ... code block ... ", t, flags=re.S)
    t = re.sub(r"`([^`]*)`", r"\1", t)
    t = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", t)
    t = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", t)
    t = re.sub(r"(?m)^\s{0,3}#{1,6}\s*", "", t)
    t = re.sub(r"(?m)^\s*\|.*\|\s*$", "", t)
    t = re.sub(r"(?m)^\s*[-*+]\s+", "", t)
    t = re.sub(r"(?m)^\s*[-*_]{3,}\s*$", "", t)
    t = re.sub(r"(?m)^\s*>\s?", "", t)
    t = re.sub(r"\*\*([^*]+)\*\*", r"\1", t)
    t = re.sub(r"(?<!\*)\*([^*]+)\*(?!\*)", r"\1", t)
    t = re.sub(r"[ \t]+", " ", t)
    t = re.sub(r"(\r?\n){2,}", "\n", t)
    return t.strip()


def _entry_text(entry: dict) -> str:
    content = entry.get("message", {}).get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [c.get("text", "") for c in content
                 if isinstance(c, dict) and c.get("type") == "text"]
        return "\n".join(p for p in parts if p)
    return ""


def last_assistant_text(transcript: Path) -> str:
    """Only the final assistant entry - going further back re-speaks old turns."""
    try:
        lines = transcript.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return ""
    for line in reversed(lines):
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        if entry.get("type") != "assistant":
            continue
        return _entry_text(entry)
    return ""


# Injected into a user message by the harness, and none of it was typed by the
# person: reminders, the transcript of a slash command's own output, and the
# skill text a command expands into. A prompt shown back to you has to be what
# you wrote, or the pair is worse than no pair at all.
# The self-closing arm has to name the tags again rather than reuse \1: in that
# branch group 1 never participated in the match, so the backreference could not
# match anything and `<system-reminder/>` sailed through into the recorded
# prompt as something the user typed - the one thing this is here to stop.
_NOISE_TAGS = "system-reminder|local-command-stdout|command-message"
NOISE = re.compile(rf"(?s)<({_NOISE_TAGS})>.*?</\1>|<(?:{_NOISE_TAGS})\s*/>")
# A slash command reaches the transcript as its own little document. The name is
# the ask - "/handoff" is a thing you asked for - so it is kept and the wrapper
# around it is not.
#
# The arguments are kept too, and that is not a detail: for "/note <the whole
# message>" the arguments ARE the prompt, and the first version of this dropped
# them as wrapper noise. A 272-character note recorded as "/note". A bare name
# is worse than recording nothing, because it reads as complete rather than as
# truncated - and it is not one command's problem: "/loop 5m /foo" recorded as
# "/loop". Found by TechnicalDirector, against a real transcript.
COMMAND = re.compile(r"(?s)<command-name>\s*(.*?)\s*</command-name>")
ARGS = re.compile(r"(?s)<command-args>\s*(.*?)\s*</command-args>")
# Written by the harness where you pressed escape, not typed by you. Dropping it
# loses nothing: the message you interrupted it with is the next one along, and
# that is kept.
INTERRUPT = re.compile(r"^\[Request interrupted by user[^\]]*\]$")
PROMPT_RUN = 8   # backstop, see below


def user_prompts(transcript: Path, answer: str = "") -> list:
    """What you typed for the turn that is ending, oldest message first.

    Walks back to the previous turn's TL;DR and keeps every message after it, so
    an ask you interrupted twice is recorded as the three things you actually
    said rather than only the last one - which, with the way work arrives here,
    would usually be a correction with its subject missing.

    That boundary is the answer to "when did the last turn end", and it holds
    because a turn without a TL;DR is blocked from ending. Should one slip
    through - enforcement gives up after a single retry - the worst case is two
    turns' messages merged, so PROMPT_RUN caps how far back that can run.

    `answer` is the response now ending, and it is not optional in practice:
    walking backwards, the first TL;DR in the file is almost always this turn's
    own, and stopping at it collects nothing at all. Comparing rather than
    skipping-the-first because the transcript is not always written by the time
    the hook runs - when it isn't, the first TL;DR really is the previous turn's.
    """
    try:
        lines = transcript.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return []
    out = []
    for line in reversed(lines):
        line = line.strip()
        if not line:
            continue
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        kind = entry.get("type")
        if kind == "assistant":
            said = _entry_text(entry)
            if TLDR.search(said) and said.strip() != answer.strip():
                break
            continue
        if kind != "user" or entry.get("isMeta"):
            continue      # a tool call, a mode change, a skill's own preamble
        text = _entry_text(entry)
        if not text:
            continue      # a tool result: content is blocks, none of them text
        named = COMMAND.search(text)
        if named:
            args = ARGS.search(text)
            text = (named.group(1) + " " + (args.group(1) if args else "")).strip()
        else:
            text = NOISE.sub("", text)
        text = text.strip()
        if not text or INTERRUPT.match(text):
            continue
        # A message can reach the transcript twice - queued and then sent is the
        # usual way - and reading your own words back to yourself twice looks
        # like the feature is broken.
        #
        # A prefix counts as the same message, not just an exact repeat. Add a
        # sentence to something you already typed and the transcript holds both
        # the short version and the long one; keeping each recorded a 396-word
        # ask as 721 words, the same paragraph twice with one extra line at the
        # end. `out` is filled newest-first, so out[-1] is the later of the two
        # and the shorter one being dropped is the earlier draft.
        if out and (out[-1] == text or out[-1].startswith(text)):
            continue
        out.append(text)
        if len(out) >= PROMPT_RUN:
            break
    out.reverse()
    return out


# --- synthesis -------------------------------------------------------------

def edge_cmd() -> list | None:
    override = os.environ.get("REVOICED_EDGE_TTS")
    if override and Path(override).exists():
        return [override]
    venv = REPO / ".venv" / ("Scripts/edge-tts.exe" if IS_WIN else "bin/edge-tts")
    if venv.exists():
        return [str(venv)]
    found = shutil.which("edge-tts")
    if found:
        return [found]
    try:
        import edge_tts  # noqa: F401
        return [sys.executable, "-m", "edge_tts"]
    except ImportError:
        return None


def edge_voices() -> list:
    cmd = edge_cmd()
    if not cmd:
        return []
    try:
        out = subprocess.run(cmd + ["--list-voices"], capture_output=True,
                             text=True, timeout=60, **NO_WINDOW).stdout
    except (OSError, subprocess.SubprocessError):
        return []
    return sorted(set(re.findall(r"\b([a-z]{2}-[A-Z]{2}-[A-Za-z]+Neural)\b", out)))


def synth_edge(preset: dict, text: str, mp3: Path) -> bool:
    cmd = edge_cmd()
    if not cmd:
        return False
    txt = mp3.with_suffix(".txt")
    try:
        txt.write_text(text, encoding="utf-8")
        # `--rate=-5%`, never `--rate -5%`: argparse reads a leading '-' as the
        # start of another flag and refuses the value.
        subprocess.run(
            cmd + ["--file", str(txt),
                   "--voice", preset.get("voice") or DEFAULT_PRESET["voice"],
                   f"--rate={preset.get('rate', '+0%')}",
                   f"--pitch={preset.get('pitch', '+0Hz')}",
                   f"--volume={preset.get('volume', '+0%')}",
                   "--write-media", str(mp3)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120,
            **NO_WINDOW,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    finally:
        txt.unlink(missing_ok=True)
    return mp3.exists() and mp3.stat().st_size > 0


def eleven_key() -> str:
    return os.environ.get("ELEVENLABS_API_KEY", "").strip()


def eleven_voices() -> list:
    key = eleven_key()
    if not key:
        return []
    req = urllib.request.Request("https://api.elevenlabs.io/v1/voices",
                                 headers={"xi-api-key": key})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            data = json.loads(r.read())
    except Exception:
        return []
    return [{"id": v.get("voice_id"), "name": v.get("name")}
            for v in data.get("voices", []) if v.get("voice_id")]


def synth_eleven(preset: dict, text: str, mp3: Path) -> bool:
    key = eleven_key()
    if not key or not preset.get("voice"):
        return False
    body = {
        "text": text,
        "model_id": preset.get("model", "eleven_flash_v2_5"),
        "voice_settings": {
            "stability": float(preset.get("stability", 0.5)),
            "similarity_boost": float(preset.get("similarity", 0.75)),
            "speed": float(preset.get("speed", 1.0)),
        },
    }
    url = (f"https://api.elevenlabs.io/v1/text-to-speech/{preset['voice']}"
           f"?output_format=mp3_44100_128")
    req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                 headers={"xi-api-key": key,
                                          "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=90) as r:
            audio = r.read()
    except Exception:
        return False
    if not audio:
        return False
    mp3.write_bytes(audio)
    return True


def synthesize(preset: dict, text: str, mp3: Path) -> bool:
    """ElevenLabs when the preset asks for it, falling back to Edge on failure."""
    if preset.get("provider") == "elevenlabs":
        if synth_eleven(preset, text, mp3):
            return True
        return synth_edge(DEFAULT_PRESET, text, mp3)
    return synth_edge(preset, text, mp3)


def player_cmd(mp3: Path) -> list | None:
    if IS_WIN:
        return ["powershell.exe", "-NoProfile", "-Sta", "-ExecutionPolicy", "Bypass",
                "-File", str(HERE / "win_audio.ps1"), "-Mode", "play", "-Path", str(mp3)]
    if IS_MAC:
        return ["afplay", str(mp3)]
    for exe, args in (("mpv", ["--no-video", "--really-quiet"]),
                      ("ffplay", ["-nodisp", "-autoexit", "-loglevel", "quiet"]),
                      ("mpg123", ["-q"]),
                      ("cvlc", ["--play-and-exit", "--intf", "dummy"])):
        if shutil.which(exe):
            return [exe] + args + [str(mp3)]
    return None


def fallback_cmd(text: str) -> list | None:
    """Offline system voice, used when no network synthesis worked."""
    if IS_WIN:
        return ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass",
                "-File", str(HERE / "win_audio.ps1"), "-Mode", "say", "-Text", text]
    if IS_MAC:
        return ["say", text]
    for exe in ("spd-say", "espeak-ng", "espeak"):
        if shutil.which(exe):
            return [exe, "-w", text] if exe == "spd-say" else [exe, text]
    return None


# --- history ---------------------------------------------------------------

def count_spoken(seed: int) -> None:
    """Total turns ever spoken, which history itself can never answer.

    `history.jsonl` is trimmed to KEEP on every write, so counting its lines
    stops dead at exactly that and reads as a total - the page said "200 kept"
    for weeks because that was the only honest thing it could say. This is the
    one number here that has to outlive the trim, so it goes in state.json: a
    read-modify-write inside lock("state") like everything else there, never a
    wholesale rewrite, because aello owns three keys in the same file.

    `seed` is what history can still see, used only the first time. Everything
    said before this existed was trimmed away and is not recoverable, so the
    count starts as a floor rather than as a fiction or a zero.

    Taken inside lock("history"), which is a new order - nothing anywhere takes
    the history lock while holding state, so it cannot cycle.
    """
    with mutate_state() as st:
        counted = int(st.get("spoken") or 0)
        st["spoken"] = counted + 1 if counted else seed


def record(entry: dict) -> None:
    with lock("history", stale=10.0, timeout=15.0):
        with HISTORY.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry) + "\n")
        try:
            lines = HISTORY.read_text(encoding="utf-8").splitlines()
        except OSError:
            return
        count_spoken(len(lines))
        if len(lines) <= KEEP:
            return
        lines = lines[-KEEP:]
        HISTORY.write_text("\n".join(lines) + "\n", encoding="utf-8")
        live = set()
        for line in lines:
            try:
                live.add(json.loads(line.lstrip("﻿"))["id"])
            except (ValueError, KeyError):
                pass
        for f in AUDIO.glob("*.mp3"):
            if f.stem not in live:
                try:
                    f.unlink()
                except OSError:
                    pass  # being played right now (Windows locks open files)


def read_history() -> list:
    try:
        lines = HISTORY.read_text(encoding="utf-8").splitlines()
    except OSError:
        return []
    out = []
    for line in lines:
        try:
            out.append(json.loads(line.lstrip("﻿")))
        except ValueError:
            pass
    return out


def entry_age(entry: dict) -> float:
    """Hours since this entry was queued, or -1 when it cannot be told.

    `queued` is written as local time with no zone, so it is read back the
    same way.
    """
    try:
        return (time.time() - time.mktime(time.strptime(
            entry.get("queued", ""), "%Y-%m-%dT%H:%M:%S"))) / 3600.0
    except (ValueError, TypeError):
        return -1.0


def recent_repairs(hist: list, hours: float = 24.0) -> list:
    """Volume repairs within `hours`, oldest first, as (age, description).

    A window in *entries* was the wrong shape on a fleet: 39 environments
    append to one `history.jsonl`, so the last 50 entries are a few minutes of
    everyone's activity and a repair from an hour ago had already fallen off
    the end - "none" then reads as a clean machine when it only means the
    window was short. Found by TechnicalDirector. Still bounded by KEEP, which
    is why `--status` prints how far back the file itself reaches: a count off
    a trimmed file cannot see past the trim, and saying so is the difference
    between a measurement and a ceiling.
    """
    out = []
    for e in hist:
        age = entry_age(e)
        if e.get("swept") and 0 <= age <= hours:
            out.append((age, f"{e.get('project','?')}: {', '.join(e['swept'])}"))
    return out


# --- cancellation ----------------------------------------------------------
# No cross-process killing, so no risk of signalling a recycled pid. A worker
# watches three tokens and stands down if any changes: its session's current
# job id (so a session interrupts only its own audio), a global stop token, and
# a skip token. Skip is read after the speaker lock is taken, so it only ever
# hits the one utterance actually playing - whoever is queued behind reads the
# new value when its turn comes and is untouched.

def _sweep_tokens(keep: float = 172800.0) -> None:
    """Drop session claim files nobody can ever read again.

    One is written per session and removed by nothing, so they accumulate for
    the life of the machine - 135 were on disk against four live leases. Inert,
    because `session_token` looks one up by exact uuid and a stale file can
    never collide with a new session. But a directory that only grows is a leak
    whoever reads it next has to reason about, and two days is far longer than
    any session that could still be claimed.
    """
    cut = time.time() - keep
    try:
        for f in RUN.glob("session-*.job"):
            try:
                if f.stat().st_mtime < cut:
                    f.unlink()
            except OSError:
                pass       # in use, or already gone; the next turn tries again
    except OSError:
        pass


def session_token(session: str) -> str:
    try:
        return (RUN / f"session-{session}.job").read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def stop_token() -> str:
    try:
        return (RUN / "stop").read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def skip_token() -> str:
    try:
        return (RUN / "skip").read_text(encoding="utf-8").strip()
    except OSError:
        return ""


def run_cancellable(cmd: list, job_id: str, session: str, stop_at_start: str,
                    skip_at_start: str) -> None:
    kwargs = {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    if IS_WIN:
        kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
    proc = subprocess.Popen(cmd, **kwargs)
    # Cancellation is cooperative, so a player that hangs is never asked to
    # stand down by anything - and the touch() below keeps its lock looking
    # alive the whole time. The deadline is the only thing that ends it.
    deadline = time.time() + PLAY_MAX if PLAY_MAX > 0 else float("inf")
    while proc.poll() is None:
        time.sleep(0.2)
        touch("speaker")
        if (session_token(session) not in ("", job_id)
                or stop_token() != stop_at_start
                or skip_token() != skip_at_start
                or time.time() > deadline):
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
            return


# --- telegram --------------------------------------------------------------

# Off unless an environment opts in, which aello's blueprint does by setting
# this in its env block - so turning an agent's phone messages on and off is an
# edit to the blueprint and needs nothing here. Absent, every path below stops
# before the first socket. The token and the chat id come from the environment
# for the same reason the kie and ElevenLabs keys do, and one harder one:
# anyone holding this token can type into a terminal running with permissions
# bypassed, and state.json is a file the station serves the contents of.
def tg_env(name: str, default: str = "") -> str:
    """An environment variable, falling back to the one Windows has persisted.

    Windows does not push a newly-set **User** variable into processes that are
    already running, and Claude Code sessions run for hours. So setting
    `REVOICED_TELEGRAM` machine-wide turned Telegram on for terminals opened
    afterwards and left every session already on screen sending nothing - which
    looks exactly like the feature being off, and says nothing about it.
    Reported by TechnicalDirector on 2026-08-06 and confirmed here: this env,
    freshly vendored at 13, could not see any of the three.

    Only when the variable is **absent** from the process, never when it is
    present and empty or `0`. A blueprint that switches Telegram off for one
    project must stay switched off, or the machine-wide default would silently
    override the per-project decision - which is the opt-in's whole point.
    Present-and-empty is returned as `""` here; whether that reads as off is the
    *caller's* to decide, and for a while `TELEGRAM` decided it read as on while
    this docstring said otherwise.

    Read once, at import. The hook is a fresh process every turn, so it picks up
    a change on the next response either way; the station is not, and a registry
    hit on every send would be paid forty-one times a turn for an answer that
    changes about once a month.
    """
    got = os.environ.get(name)
    if got is not None:
        return got
    return _PERSISTED.get(name, default)


def _user_env() -> dict:
    """Everything at Windows User scope, or {} anywhere else."""
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as k:
            out, i = {}, 0
            while True:
                try:
                    name, value, _ = winreg.EnumValue(k, i)
                except OSError:
                    return out
                out[name] = str(value)
                i += 1
    except Exception:
        return {}


_PERSISTED = _user_env()

# Off unless an environment opts in. See `tg_env` for why this is not a plain
# `os.environ.get`.
#
# Empty is off, and it was on until TechnicalDirector measured it. `tg_env` is
# correct - it returns "" for a present-but-empty name and does not fall back to
# the User scope - but `"" != "0"` is True, so a blueprint that set the variable
# to nothing opted *in* while the docstring one line up promised the opposite.
# `not in ("", "0")` is the whole fix. Worth saying how they caught it, because
# the obvious test agrees with the bug: PowerShell's `$env:X = ''` **deletes**
# the variable rather than emptying it, so testing that way measures the absent
# case and reports a pass. It takes a subprocess with an explicit empty value.
TELEGRAM = tg_env("REVOICED_TELEGRAM", "0").strip() not in ("", "0")
TG_API = "https://api.telegram.org/bot{}/{}"


def _tg_post(method: str, fields: dict, mp3: Path | None = None) -> bool:
    """One multipart POST to the Bot API, by hand.

    Multipart rather than JSON because sendAudio has to carry the file, and
    building the body here is a dozen lines against a dependency the hook
    cannot have - this file runs in 41 vendored copies with nothing installed
    beside it.
    """
    token = tg_env("TELEGRAM_BOT_TOKEN", "")
    boundary = "----revoiced" + uuid.uuid4().hex
    body = bytearray()
    for key, value in fields.items():
        body += (f"--{boundary}\r\nContent-Disposition: form-data; "
                 f'name="{key}"\r\n\r\n{value}\r\n').encode("utf-8")
    if mp3:
        body += (f"--{boundary}\r\nContent-Disposition: form-data; "
                 f'name="audio"; filename="{mp3.name}"\r\n'
                 "Content-Type: audio/mpeg\r\n\r\n").encode("utf-8")
        body += mp3.read_bytes() + b"\r\n"
    body += f"--{boundary}--\r\n".encode("utf-8")
    req = urllib.request.Request(
        TG_API.format(token, method), data=bytes(body),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return bool(json.loads(resp.read().decode("utf-8")).get("ok"))


def telegram_send(project: str, text: str, mp3: Path | None) -> bool:
    """The TL;DR to your phone, then the audio behind it.

    Sent from the hook rather than the station, so it goes out on a machine
    where the station is closed - receiving is the station's half, because a
    long poll needs a process that outlives a turn.

    Called *before* the speaker lock is taken. Inside it, a 30s upload would
    hold a machine-wide lock, and all 41 environments queue behind that one.

    The project's name is the first line, and that is load-bearing rather than
    decoration: a reply carries the whole quoted message back, so the station
    resolves it to a profile from the text itself and needs no stored map of
    message ids - nothing to grow, nothing to lose across a restart.
    """
    if not (TELEGRAM and tg_env("TELEGRAM_BOT_TOKEN")):
        return False
    chat = tg_env("TELEGRAM_CHAT_ID", "")
    if not chat:
        return False
    try:
        ok = _tg_post("sendMessage", {
            "chat_id": chat, "parse_mode": "HTML",
            "text": f"<b>{html.escape(project)}</b>\n\n{html.escape(text)}",
        })
        if not ok:
            # The API answered, and answered no. A revoked token, a chat that
            # blocked the bot, a message the parser rejected - all arrive here.
            return _tg_failed("the API returned ok:false", project)
        if mp3 and mp3.exists():
            # sendAudio, never sendVoice: a voice note must be ogg/opus and
            # Telegram refuses an mp3 posted there outright.
            if not _tg_post("sendAudio", {
                "chat_id": chat, "title": project, "performer": "revoiced",
            }, mp3):
                # The text landed and the audio did not, which is a real state
                # and not the same as nothing arriving. Named as such.
                return _tg_failed("audio refused; the text was delivered", project)
        return _tg_ok()
    except Exception as e:
        return _tg_failed(f"{type(e).__name__}: {e}", project)


# Nothing looked at what telegram_send returned, and it ends in a bare
# `except: return False` - so a 30s timeout, a revoked token, a wrong chat id
# and an ok:false from the API all produced exactly nothing. No history field,
# no stderr, no retry, and the user believing a message had been delivered.
# TechnicalDirector's, 2026-08-06.
#
# It lands in state.json rather than on the history entry, and that is a
# concession rather than a preference: `record()` has already appended by the
# time the send is attempted - deliberately, because a 30s upload must not sit
# between a finished turn and the station showing it - so putting it on the
# entry would mean rewriting the whole file on the hook path, once per turn,
# across 39 environments. `mutate_state` publishes only on a change, so a
# machine where Telegram works never writes this at all.
def _tg_failed(reason: str, project: str) -> bool:
    with mutate_state() as st:
        st["telegram_error"] = {"at": time.time(), "reason": reason[:200],
                                "project": project}
    return False


def _tg_ok() -> bool:
    # Clearing on the next success is what makes the record mean "right now"
    # rather than "at some point". Nothing to clear is not a change, so this
    # writes nothing on the ordinary path.
    with mutate_state() as st:
        st.pop("telegram_error", None)
    return True


def sweep_ducks() -> list:
    """Put back any volume still carrying a duck's signature, and say what.

    Refuses while anything is speaking, because a duck in progress is
    indistinguishable from one that failed - a scan taken mid-line while this
    was being written reported three applications as damaged and every one of
    them was correct. Two independent signs, and both have to be quiet: a fresh
    speaker lock, which is heartbeated every 200ms across all 41 environments,
    and the duck record itself. Whichever env speaks next does the sweep, so a
    turn skipped here costs nothing.
    """
    if not SWEEP or not IS_WIN:
        return []
    try:
        if time.time() - (RUN / "speaker.lock").stat().st_mtime <= 3.0:
            return []
    except OSError:
        pass                          # no lock at all, so nobody is speaking
    if (RUN / "duck.json").exists():
        return []
    level = float(read_state().get("duck", 15)) / 100.0
    # A duck of 100% is ducking switched off; there is no signature to look for
    # and every quiet application would match nothing anyway.
    if not 0.0 < level < 1.0:
        return []
    # The registry names an application by its full device path. Keep the
    # executable, drop the volume prefix and the session suffix.
    return [f"{n.split(chr(92))[-1].split('%b')[0] or '?'} {v:.4f} ({how})"
            for n, v, how in ducking.sweep(level, SWEEP_SIGNATURE)]


# --- worker ----------------------------------------------------------------

def worker(job_file: Path) -> None:
    try:
        job = json.loads(job_file.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    job_file.unlink(missing_ok=True)

    preset = job.get("preset") or DEFAULT_PRESET
    spoken = f"{job['project']}. {job['text']}"
    mp3 = AUDIO / f"{job['id']}.mp3"

    # Sample the stop token *before* synthesis, not after. Synthesis is the
    # slow part - an edge-tts subprocess, or an ElevenLabs request - and every
    # writer of this token writes a fresh uuid, so a stop or a mute pressed
    # while it ran used to become this worker's own baseline and compare equal
    # forever after. With 41 environments some worker is inside that window
    # most of the time, which is why Stop sometimes did nothing.
    stop_at_start = stop_token()

    # Synthesise before queueing, so waiting responses render in parallel.
    have_audio = synthesize(preset, spoken, mp3)

    # Check the last turn's restore actually landed, before this one ducks on
    # top of whatever it left. It runs here rather than after our own restore
    # because the registry lags the session by several seconds - measured, the
    # stored value still read 0.15 five seconds after `duck.json` was gone - so
    # a sweep taken straight after speaking would find its own duck and "repair"
    # it. One turn later there is no such doubt.
    swept = sweep_ducks()

    record({
        "id": job["id"], "queued": job["queued"], "project": job["project"],
        "cwd": job["cwd"], "session": job["session"], "text": job["text"],
        # Empty when REVOICED_PROMPTS=0, and absent altogether on every turn
        # recorded before this existed - the page treats both the same.
        "prompt": job.get("prompt", ""),
        # What this turn is pinned under - the profile's env dir, or the cwd
        # when it has no profile. The feed groups on it.
        "key": job.get("key") or job["cwd"], "env": job.get("env", ""),
        "window": job.get("window"),
        "voice": (preset.get("name") or preset.get("voice")) if have_audio
                 else "system fallback voice",
        "audio": str(mp3) if have_audio else None,
        # Absent on a complete copy, so the field itself is the alarm: it names
        # the siblings this copy was vendored without, and therefore what it
        # silently is not doing - no toast without notify, no window titles
        # without focus.
        **({"missing": list(MISSING)} if MISSING else {}),
        # Only when it actually put something back. A repair that says nothing
        # is a fallback firing in silence, which is how a total edge-tts
        # breakage stayed hidden for hours - and here it is also the only
        # evidence of how often the restore misses.
        **({"swept": swept} if swept else {}),
    })

    if session_token(job["session"]) not in ("", job["id"]):
        return
    if stop_token() != stop_at_start:
        return                      # stopped while this was being synthesised

    # To your phone, out here rather than beside the desktop notification: that
    # one is drawn from inside the speaker lock, and this is a network upload.
    telegram_send(job["project"], job["text"], mp3 if have_audio else None)

    duck_file = RUN / "duck.json"
    ducking.recover(duck_file)      # a previous worker may have died mid-duck

    with lock("speaker", stale=5.0, timeout=600.0) as held:
        # Timing out here means ten minutes of queue ahead of a line two
        # sentences long: it is stale, and playing it anyway would talk over
        # whoever does hold the lock. Only reachable if a player outlives
        # PLAY_MAX, which is now capped.
        if not held:
            return
        if session_token(job["session"]) not in ("", job["id"]):
            return
        if stop_token() != stop_at_start:
            return
        cmd = player_cmd(mp3) if have_audio else fallback_cmd(spoken)
        if not cmd:
            return
        # Read skip only now we hold the lock: anything still queued picks up
        # the current value at its own turn, so a skip never reaches past this.
        skip_at_start = skip_token()
        # Keep the lock's mtime moving across everything before playback. The
        # only heartbeat used to be inside the player loop, so from acquisition
        # until Popen the mtime sat frozen through a state read, ducking's cold
        # `import comtypes` and a full session enumeration - and the station
        # calls a duck file with a speaker lock older than 3s abandoned. That
        # is how a poll came to restore the volume of a worker that was about
        # to speak, and then delete the record.
        touch("speaker")
        # Duck first: the player's own session doesn't exist yet, so it is
        # never caught by this and stays at full volume.
        level = float(read_state().get("duck", 15)) / 100.0
        playing = RUN / "playing.json"
        touch("speaker")
        ducking.duck(level, duck_file)
        touch("speaker")
        # Everything from here to the restore is inside the try, including the
        # bookkeeping. Anything that throws between lowering the volume and the
        # finally leaves every other application at 15% until something else
        # happens to speak - which, if nothing does, is never.
        try:
            # What is playing, for the station to put on screen. The speaker
            # lock alone says only that *something* is - and with several
            # agents talking the newest entry is not the one being heard,
            # because entries are recorded at synthesis and played in lock
            # order. Read together with the lock's heartbeat, so a worker that
            # dies mid-play leaves nothing showing.
            try:
                playing.write_text(json.dumps({
                    "id": job["id"], "project": job["project"],
                    "key": job.get("key") or job["cwd"], "session": job["session"],
                    "text": job["text"],
                }), encoding="utf-8")
            except OSError:
                pass
            # The desktop notification goes out at the same moment, and for the
            # same reason: you are almost never looking at the station while an
            # agent works, so a pop-up drawn inside it announces nothing.
            notifying.show(job["project"], job["text"],
                           job.get("key") or job["cwd"], job["id"])
            touch("speaker")
            run_cancellable(cmd, job["id"], job["session"], stop_at_start,
                            skip_at_start)
        finally:
            playing.unlink(missing_ok=True)
            ducking.restore(duck_file)


# --- hook ------------------------------------------------------------------

def hook() -> None:
    # Read bytes, not text: on Windows sys.stdin decodes with the console code
    # page, which turned every em dash in a TL;DR into "a-euro-quote" on screen.
    payload = sys.stdin.buffer.read().decode("utf-8-sig", "replace").strip()
    if not payload:
        return
    try:
        data = json.loads(payload)
    except ValueError:
        return

    # SessionEnd: hand the voice back to the pool, unless the terminal carries
    # on without it.
    if data.get("hook_event_name") == "SessionEnd":
        session = data.get("session_id") or "default"
        if data.get("reason") in SESSION_CONTINUES:
            mark_cleared(session)
        else:
            release_lease(session)
        return

    cwd = data.get("cwd") or os.getcwd()
    key, profile = identity(cwd, read_state()["profiles"])
    if is_muted(cwd, key):
        return

    # Newer Claude Code hands us the response directly; older builds don't.
    raw = data.get("last_assistant_message")
    said = data.get("transcript_path")
    script = Path(said) if said and Path(said).exists() else None
    if not isinstance(raw, str) or not raw.strip():
        if script is None:
            return
        raw = last_assistant_text(script)
    if not raw:
        return

    session = data.get("session_id") or "default"
    retry = RUN / f"retry-{session}"
    summary = extract_tldr(raw)

    if not summary:
        # Exit 2 blocks the turn from ending and feeds stderr back, so the
        # response gets a TL;DR added. Only ever once per turn: if we already
        # asked and it still isn't there, give up quietly rather than loop.
        asked = retry.exists() or bool(data.get("stop_hook_active"))
        retry.unlink(missing_ok=True)
        if ENFORCE and not asked:
            retry.write_text("1", encoding="utf-8")
            sys.stderr.write(
                "revoiced: this response has no TL;DR line, so nothing can be "
                "spoken. Add a final line of exactly the form "
                "'TL;DR: <two sentences>' summarising the outcome, then stop.\n"
            )
            sys.exit(2)
        return

    retry.unlink(missing_ok=True)
    text = to_speakable(summary)
    if not text:
        return
    if len(text) > MAX_CHARS:
        text = text[:MAX_CHARS] + "..."

    # What was asked, so History reads as a pair rather than as half a
    # conversation. Nothing downstream depends on it: a turn with no prompt is
    # recorded exactly as it always was.
    prompt = ""
    if PROMPTS and script is not None:
        prompt = "\n\n".join(user_prompts(script, raw))
        if len(prompt) > PROMPT_MAX:
            prompt = prompt[:PROMPT_MAX] + "..."

    # A profile names itself; everything else is still called after its folder.
    project = (profile or {}).get("name") or Path(cwd).name
    # Only findable from here: the worker is detached from this tree. Resolved
    # before the lease, which records the pid so a closed window ends it.
    window = focusing.terminal_window(
        project, read_state()["windows"].get(key, ""), str(cwd))
    job = {
        "id": uuid.uuid4().hex,
        "queued": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "project": project,
        "cwd": str(cwd),
        "key": key,
        "env": env_dir(),
        "session": session,
        "text": text,
        "prompt": prompt,
        "preset": lease_preset(session, key, project, str(cwd), window),
        "window": window,
    }

    # Claim the session: any worker still speaking for it stands down.
    (RUN / f"session-{session}.job").write_text(job["id"], encoding="utf-8")
    _sweep_tokens()

    job_file = RUN / f"{job['id']}.job"
    job_file.write_text(json.dumps(job), encoding="utf-8")

    kwargs = {"stdin": subprocess.DEVNULL, "stdout": subprocess.DEVNULL,
              "stderr": subprocess.DEVNULL, "close_fds": True}
    if IS_WIN:
        kwargs["creationflags"] = subprocess.DETACHED_PROCESS | subprocess.CREATE_NO_WINDOW
    else:
        kwargs["start_new_session"] = True
    subprocess.Popen([sys.executable, str(Path(__file__).resolve()),
                      "--worker", str(job_file)], **kwargs)


# --- cli -------------------------------------------------------------------

def main() -> None:
    args = sys.argv[1:]
    if not args:
        hook()
        return

    cmd = args[0]
    if cmd == "--hook-version":
        # For aello: read it from the copy it vendored and compare with this
        # repo's, without importing a module whose imports may not be there.
        print(HOOK_VERSION)
        return

    if cmd == "--worker":
        worker(Path(args[1]))
        return

    if cmd == "--release":
        session = args[1] if len(args) > 1 else ""
        if not session:                      # called as a SessionEnd hook
            try:
                data = json.loads(
                    sys.stdin.buffer.read().decode("utf-8-sig", "replace") or "{}")
            except ValueError:
                data = {}
            if data.get("reason") in SESSION_CONTINUES:
                session = data.get("session_id", "")
                print("kept" if session and mark_cleared(session) else "no lease")
                return
            session = data.get("session_id", "")
        print("released" if session and release_lease(session) else "no lease")
        return

    state = read_state()
    if cmd == "--stop":
        (RUN / "stop").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("stopped")
    elif cmd == "--skip":
        (RUN / "skip").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("skipped")
    elif cmd in ("--mute", "--unmute"):
        # Re-read under the lock, like every other writer. aello writes `global`
        # and `projects` to this same file, so a mute of its own set between the
        # read above and this write would be silently thrown away.
        with mutate_state() as state:
            state["global"] = cmd == "--mute"
        if cmd == "--mute":
            (RUN / "stop").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("muted" if state["global"] else "unmuted")
    elif cmd in ("--mute-project", "--unmute-project"):
        target = str(Path(args[1]).resolve()) if len(args) > 1 else os.getcwd()
        with mutate_state() as state:
            if cmd == "--mute-project":
                state["projects"][target] = True
            else:
                state["projects"].pop(target, None)
        print(f"{'muted' if cmd == '--mute-project' else 'unmuted'}: {target}")
    elif cmd == "--presets":
        if not state["presets"]:
            print("pool empty - using the built-in default voice")
        for p in state["presets"]:
            extra = (f"rate={p.get('rate')} pitch={p.get('pitch')}"
                     if p.get("provider") != "elevenlabs"
                     else f"speed={p.get('speed')} stability={p.get('stability')}")
            print(f"  {p['id']:10} {p.get('name','?'):22} {p.get('provider'):11} "
                  f"{p.get('voice','')}  {extra}")
    elif cmd == "--voices":
        if not state["voices"]:
            print("no project has a pinned voice yet")
        names = {p["id"]: p.get("name", "?") for p in state["presets"]}
        for cwd, pid in sorted(state["voices"].items()):
            print(f"  {Path(cwd).name:22} {names.get(pid, pid + ' (missing)'):22} {cwd}")
    elif cmd == "--pin":
        target = str(Path(args[1]).resolve()) if len(args) > 1 else os.getcwd()
        pid = args[2] if len(args) > 2 else ""
        with mutate_state() as state:
            if pid in {p["id"] for p in state["presets"]}:
                state["voices"][target] = pid
                for l in state["leases"].values():
                    if l.get("cwd") == target:
                        l["preset"] = pid      # live sessions follow immediately
            else:
                state["voices"].pop(target, None)
                pid = ""
        print(f"{'pinned ' + pid if pid else 'unpinned'}: {target}")
    elif cmd == "--leases":
        if not state["leases"]:
            print("no voices are currently leased")
        now = time.time()
        for sid, l in state["leases"].items():
            mins = (now - float(l.get("last_used", 0))) / 60
            print(f"  {l.get('project','?'):22} preset={l.get('preset')} "
                  f"idle={mins:.0f}m  session={sid[:8]}")
    elif cmd == "--status":
        print(f"hook version  : {HOOK_VERSION}"
              + ("  INCOMPLETE COPY - no "
                 + ", ".join(f"{m}.py" for m in MISSING) + " beside it"
                 if MISSING else ""))
        print(f"global mute   : {state['global']}")
        print(f"muted projects: {[p for p, v in state['projects'].items() if v] or 'none'}")
        print(f"pool          : {len(state['presets'])} preset(s), "
              f"{len(state['leases'])} leased")
        print(f"pinned voices : {len(state['voices'])} project(s)")
        hist = read_history()
        print(f"history       : {len(hist)} entries in {HISTORY}")
        print(f"prompts kept  : {'yes' if PROMPTS else 'no (REVOICED_PROMPTS=0)'}"
              f", {sum(1 for e in hist if e.get('prompt'))} of {len(hist)} "
              f"entries carry one")
        # What had to be put back lately - a window in time, because this file
        # is shared by every environment on the machine and 50 entries of it is
        # minutes, not turns. Nothing here is still not the healthy reading:
        # it says the last day's restores landed, not that the machine is clean.
        # `--sweep` is the question "is it clean right now".
        repairs = recent_repairs(hist)
        reach = entry_age(hist[0]) if hist else -1.0
        if repairs:
            line = (f"{len(repairs)} in the last 24h, newest: "
                    + "; ".join(t for _, t in repairs[-3:]))
        else:
            line = "none in the last 24h"
            if reach >= 0:
                line += f" (this file only reaches back {reach:.0f}h)"
        print(f"volume repairs: {line}"
              + ("" if SWEEP else "  (REVOICED_SWEEP=0, sweep is off)"))
        print(f"edge-tts      : {edge_cmd() or 'NOT FOUND'}")
        print(f"elevenlabs key: {'set' if eleven_key() else 'not set'}")
        # Where each one came from, not just whether it is there. "off" and
        # "set at User scope but this process never got it" used to print the
        # same line, and the second is the one that looks like the feature
        # being broken - reported by TechnicalDirector after `--status` from a
        # shell older than the variables told them Telegram was off.
        def source(name):
            if name in os.environ:
                return "set"
            if name in _PERSISTED:
                return "set at User scope, picked up from there"
            return "not set"
        print(f"telegram      : {'on' if TELEGRAM else 'off (REVOICED_TELEGRAM)'}"
              f", token {source('TELEGRAM_BOT_TOKEN')}"
              f", chat {source('TELEGRAM_CHAT_ID')}")
        # Cleared by the next send that works, so this is "right now", not
        # "ever". Absent is the healthy reading.
        err = state.get("telegram_error")
        if err:
            mins = (time.time() - float(err.get("at", 0))) / 60
            print(f"  last failure: {err.get('reason')} "
                  f"({err.get('project', '?')}, {mins:.0f}m ago)")
    elif cmd == "--sweep":
        # "Is this machine clean, right now." The hook does this once a turn and
        # says nothing when there is nothing to do, so this is the only way to
        # ask directly - and the only way to watch the guard catch the case it
        # was written for rather than read that it would.
        level = float(state.get("duck", 15)) / 100.0
        mode = ("off" if not SWEEP else
                "signature only" if SWEEP_SIGNATURE else "everything below full")
        print(f"duck level    : {level:.2f}")
        print(f"sweep claims  : {mode}")
        for key, name, value in ducking._stored():
            if 0.0 < value < 0.999:
                claimed = ducking.is_duck(value, level) if SWEEP_SIGNATURE else True
                print(f"  {value:.4f}  {'CLAIM' if claimed else 'kept '}"
                      f"  {name.split(chr(92))[-1][:52]}")
        fixed = sweep_ducks()
        print(f"repaired      : {fixed or 'nothing'}")
    else:
        print(__doc__)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        # Fail silent is the rule for the hook and there was nothing enforcing
        # it at the top: any unhandled error - a corrupt payload, a state file
        # mid-write, a Windows API refusing - printed a traceback straight into
        # the user's Claude Code session, on a path that runs after every single
        # response. A hook that says nothing is the contract; --status and the
        # MISSING field are where problems are meant to surface.
        if os.environ.get("REVOICED_DEBUG"):
            raise
        sys.exit(0)
