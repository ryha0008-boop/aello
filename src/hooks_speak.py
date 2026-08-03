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
"""

import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
import uuid
from contextlib import contextmanager
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
import duck as ducking

# focus and notify are optional siblings. speak.py gets copied places - aello
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
HOOK_VERSION = 7

MAX_CHARS = int(os.environ.get("REVOICED_MAX_CHARS", "1200"))
KEEP = int(os.environ.get("REVOICED_HISTORY", "200"))
# History records what you asked as well as what was answered, so a turn reads
# as a pair. Off with REVOICED_PROMPTS=0, which stops it being captured at all
# rather than merely hiding it - what you typed is the one thing here that was
# never the machine's to keep. The cap is per turn, and generous: a prompt is
# read on screen, not spoken, so there is no MAX_CHARS-style budget to respect,
# only the 200-line file. A pasted log is what it guards against.
PROMPTS = os.environ.get("REVOICED_PROMPTS", "1") != "0"
PROMPT_MAX = int(os.environ.get("REVOICED_PROMPT_MAX", "4000"))
ENFORCE = os.environ.get("REVOICED_ENFORCE", "1") != "0"
LEASE_TTL = float(os.environ.get("REVOICED_LEASE_TTL", "43200"))  # 12h
# How long a lease is believed on its own word, before its terminal has to
# still be on screen for it to count as running. Long enough that an agent
# thinking hard is never called dead; short enough that a window you closed
# stops claiming to be running while you are still looking at the page.
LEASE_GRACE = float(os.environ.get("REVOICED_LEASE_GRACE", "300"))  # 5min
# The longest a single line may hold the speaker lock. That lock is machine-wide
# and heartbeated for exactly as long as the player lives, so a player that
# never exits silences every env on the box and nothing else ever gives up.
# 0 turns the cap off. A full-length line is well under a minute of speech.
PLAY_MAX = float(os.environ.get("REVOICED_PLAY_MAX", "300"))  # 5min

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

@contextmanager
def lock(name: str, stale: float = 5.0, timeout: float = 600.0):
    path = RUN / f"{name}.lock"
    deadline = time.time() + timeout
    held = False
    while time.time() < deadline:
        try:
            fd = os.open(str(path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.close(fd)
            held = True
            break
        except FileExistsError:
            try:
                if time.time() - path.stat().st_mtime > stale:
                    path.unlink()
                    continue
            except OSError:
                pass
            time.sleep(0.2)
    try:
        yield held
    finally:
        if held:
            try:
                path.unlink()
            except OSError:
                pass


def touch(name: str) -> None:
    try:
        os.utime(RUN / f"{name}.lock", None)
    except OSError:
        pass


# --- state -----------------------------------------------------------------

def read_state() -> dict:
    try:
        s = json.loads(STATE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        s = {}
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
    return s


def write_state(state: dict) -> None:
    tmp = STATE.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=2), encoding="utf-8")
    tmp.replace(STATE)


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
    with lock("state", stale=5.0, timeout=15.0):
        st = read_state()
        dead = dead_leases(st["leases"])
        if not dead:
            return 0
        for sid in dead:
            st["leases"].pop(sid, None)
        write_state(st)
        return len(dead)


def lease_preset(session: str, key: str, project: str, cwd: str = "",
                 window: dict = None) -> dict:
    with lock("state", stale=5.0, timeout=15.0):
        st = read_state()
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
        write_state(st)
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
    with lock("state", stale=5.0, timeout=15.0):
        st = read_state()
        l = st["leases"].get(session)
        if not l:
            return False
        l["cleared"] = time.time()
        write_state(st)
        return True


def release_lease(session: str) -> bool:
    with lock("state", stale=5.0, timeout=15.0):
        st = read_state()
        if st["leases"].pop(session, None) is None:
            return False
        write_state(st)
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
NOISE = re.compile(
    r"(?s)<(system-reminder|local-command-stdout|command-message)>"
    r".*?</\1>|<\1\s*/>"
)
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
        if out and out[-1] == text:
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

def record(entry: dict) -> None:
    with lock("history", stale=10.0, timeout=15.0):
        with HISTORY.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(entry) + "\n")
        try:
            lines = HISTORY.read_text(encoding="utf-8").splitlines()
        except OSError:
            return
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


# --- cancellation ----------------------------------------------------------
# No cross-process killing, so no risk of signalling a recycled pid. A worker
# watches three tokens and stands down if any changes: its session's current
# job id (so a session interrupts only its own audio), a global stop token, and
# a skip token. Skip is read after the speaker lock is taken, so it only ever
# hits the one utterance actually playing - whoever is queued behind reads the
# new value when its turn comes and is untouched.

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

    # Synthesise before queueing, so waiting responses render in parallel.
    have_audio = synthesize(preset, spoken, mp3)

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
    })

    stop_at_start = stop_token()
    if session_token(job["session"]) not in ("", job["id"]):
        return

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
        # Duck first: the player's own session doesn't exist yet, so it is
        # never caught by this and stays at full volume.
        level = float(read_state().get("duck", 15)) / 100.0
        playing = RUN / "playing.json"
        ducking.duck(level, duck_file)
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
        with lock("state", stale=5.0, timeout=15.0):
            state = read_state()
            state["global"] = cmd == "--mute"
            write_state(state)
        if cmd == "--mute":
            (RUN / "stop").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("muted" if state["global"] else "unmuted")
    elif cmd in ("--mute-project", "--unmute-project"):
        target = str(Path(args[1]).resolve()) if len(args) > 1 else os.getcwd()
        with lock("state", stale=5.0, timeout=15.0):
            state = read_state()
            if cmd == "--mute-project":
                state["projects"][target] = True
            else:
                state["projects"].pop(target, None)
            write_state(state)
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
        with lock("state", stale=5.0, timeout=15.0):
            state = read_state()
            if pid in {p["id"] for p in state["presets"]}:
                state["voices"][target] = pid
                for l in state["leases"].values():
                    if l.get("cwd") == target:
                        l["preset"] = pid      # live sessions follow immediately
            else:
                state["voices"].pop(target, None)
                pid = ""
            write_state(state)
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
        print(f"edge-tts      : {edge_cmd() or 'NOT FOUND'}")
        print(f"elevenlabs key: {'set' if eleven_key() else 'not set'}")
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
