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
    speak.py --stop                  stop whatever is speaking right now
    speak.py --mute | --unmute       global
    speak.py --mute-project [PATH]   silence one project only
    speak.py --unmute-project [PATH]
    speak.py --presets               list the voice pool
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

sys.path.insert(0, str(Path(__file__).resolve().parent))
import duck as ducking

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
IS_WIN = os.name == "nt"
IS_MAC = sys.platform == "darwin"

MAX_CHARS = int(os.environ.get("REVOICED_MAX_CHARS", "1200"))
KEEP = int(os.environ.get("REVOICED_HISTORY", "50"))
ENFORCE = os.environ.get("REVOICED_ENFORCE", "1") != "0"
LEASE_TTL = float(os.environ.get("REVOICED_LEASE_TTL", "43200"))  # 12h

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
    return s


def write_state(state: dict) -> None:
    tmp = STATE.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=2), encoding="utf-8")
    tmp.replace(STATE)


def is_muted(cwd: str) -> bool:
    s = read_state()
    return bool(s["global"]) or bool(s["projects"].get(str(cwd)))


# --- voice leases ----------------------------------------------------------
# One preset per session for as long as that terminal lives. Not per project:
# two terminals in the same repo should still sound different.

def lease_preset(session: str, cwd: str, project: str) -> dict:
    with lock("state", stale=5.0, timeout=15.0):
        st = read_state()
        presets = st.get("presets") or []
        if not presets:
            return DEFAULT_PRESET

        now = time.time()
        leases = st["leases"]
        for sid in [s for s, l in leases.items()
                    if now - float(l.get("last_used", 0)) > LEASE_TTL]:
            leases.pop(sid, None)          # terminal died without releasing

        by_id = {p["id"]: p for p in presets}
        held = leases.get(session)
        if held and held.get("preset") in by_id:
            held.update(last_used=now, cwd=cwd, project=project)
            write_state(st)
            return by_id[held["preset"]]

        taken = {l["preset"] for s, l in leases.items() if s != session}
        free = [p for p in presets if p["id"] not in taken]
        if free:
            chosen = free[0]
        else:
            # Pool exhausted: share out the voice idle longest.
            idle = {}
            for l in leases.values():
                pid = l.get("preset")
                idle[pid] = max(idle.get(pid, 0.0), float(l.get("last_used", 0)))
            chosen = min(presets, key=lambda p: idle.get(p["id"], 0.0))

        leases[session] = {"preset": chosen["id"], "cwd": cwd, "project": project,
                           "acquired": now, "last_used": now}
        write_state(st)
        return chosen


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
        content = entry.get("message", {}).get("content")
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            parts = [c.get("text", "") for c in content if c.get("type") == "text"]
            return "\n".join(p for p in parts if p)
        return ""
    return ""


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
                             text=True, timeout=60).stdout
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
# watches two tokens and stands down if either changes: its session's current
# job id (so a session interrupts only its own audio) and a global stop token.

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


def run_cancellable(cmd: list, job_id: str, session: str, stop_at_start: str) -> None:
    kwargs = {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    if IS_WIN:
        kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
    proc = subprocess.Popen(cmd, **kwargs)
    while proc.poll() is None:
        time.sleep(0.2)
        touch("speaker")
        if session_token(session) not in ("", job_id) or stop_token() != stop_at_start:
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
        "voice": (preset.get("name") or preset.get("voice")) if have_audio
                 else "system fallback voice",
        "audio": str(mp3) if have_audio else None,
    })

    stop_at_start = stop_token()
    if session_token(job["session"]) not in ("", job["id"]):
        return

    duck_file = RUN / "duck.json"
    ducking.recover(duck_file)      # a previous worker may have died mid-duck

    with lock("speaker", stale=5.0, timeout=600.0):
        if session_token(job["session"]) not in ("", job["id"]):
            return
        if stop_token() != stop_at_start:
            return
        cmd = player_cmd(mp3) if have_audio else fallback_cmd(spoken)
        if not cmd:
            return
        # Duck first: the player's own session doesn't exist yet, so it is
        # never caught by this and stays at full volume.
        level = float(read_state().get("duck", 15)) / 100.0
        ducking.duck(level, duck_file)
        try:
            run_cancellable(cmd, job["id"], job["session"], stop_at_start)
        finally:
            ducking.restore(duck_file)


# --- hook ------------------------------------------------------------------

def hook() -> None:
    payload = sys.stdin.read().lstrip("﻿").strip()
    if not payload:
        return
    try:
        data = json.loads(payload)
    except ValueError:
        return

    # SessionEnd: hand the voice back to the pool.
    if data.get("hook_event_name") == "SessionEnd":
        release_lease(data.get("session_id") or "default")
        return

    cwd = data.get("cwd") or os.getcwd()
    if is_muted(cwd):
        return

    # Newer Claude Code hands us the response directly; older builds don't.
    raw = data.get("last_assistant_message")
    if not isinstance(raw, str) or not raw.strip():
        transcript = data.get("transcript_path")
        if not transcript or not Path(transcript).exists():
            return
        raw = last_assistant_text(Path(transcript))
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

    project = Path(cwd).name
    job = {
        "id": uuid.uuid4().hex,
        "queued": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "project": project,
        "cwd": str(cwd),
        "session": session,
        "text": text,
        "preset": lease_preset(session, str(cwd), project),
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
    if cmd == "--worker":
        worker(Path(args[1]))
        return

    if cmd == "--release":
        session = args[1] if len(args) > 1 else ""
        if not session:                      # called as a SessionEnd hook
            try:
                session = json.loads(sys.stdin.read().lstrip("﻿") or "{}") \
                              .get("session_id", "")
            except ValueError:
                session = ""
        print("released" if session and release_lease(session) else "no lease")
        return

    state = read_state()
    if cmd == "--stop":
        (RUN / "stop").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("stopped")
    elif cmd in ("--mute", "--unmute"):
        state["global"] = cmd == "--mute"
        write_state(state)
        if cmd == "--mute":
            (RUN / "stop").write_text(uuid.uuid4().hex, encoding="utf-8")
        print("muted" if state["global"] else "unmuted")
    elif cmd in ("--mute-project", "--unmute-project"):
        target = str(Path(args[1]).resolve()) if len(args) > 1 else os.getcwd()
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
    elif cmd == "--leases":
        if not state["leases"]:
            print("no voices are currently leased")
        now = time.time()
        for sid, l in state["leases"].items():
            mins = (now - float(l.get("last_used", 0))) / 60
            print(f"  {l.get('project','?'):22} preset={l.get('preset')} "
                  f"idle={mins:.0f}m  session={sid[:8]}")
    elif cmd == "--status":
        print(f"global mute   : {state['global']}")
        print(f"muted projects: {[p for p, v in state['projects'].items() if v] or 'none'}")
        print(f"pool          : {len(state['presets'])} preset(s), "
              f"{len(state['leases'])} leased")
        print(f"history       : {len(read_history())} entries in {HISTORY}")
        print(f"edge-tts      : {edge_cmd() or 'NOT FOUND'}")
        print(f"elevenlabs key: {'set' if eleven_key() else 'not set'}")
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
