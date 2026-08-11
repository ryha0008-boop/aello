#!/usr/bin/env python3
"""Find the terminal window a session is running in, and raise it later.

Windows Terminal hosts every window in one process, and a child process is
given no handle to the window it lives in - WT_SESSION names the pane but
nothing consumes it, and the console title is whatever Claude Code last wrote
there. So the link is the window *title*, guessed from the project name and
overridable per project in the station.

Titles are re-resolved at focus time rather than stored as handles: a title
survives the window being closed and reopened, and picks up the status glyphs
Claude Code prefixes while it works.

Windows only. Elsewhere both calls report nothing and the station does not
offer the button - same shape as duck.py.
"""

import os
import re
import threading
import time

# The foreground and the clipboard are one per machine, and serve.py answers
# every request on a new thread. Anything that takes either one goes through
# this.
_INPUT_LOCK = threading.RLock()

IS_WIN = os.name == "nt"

if IS_WIN:
    import ctypes
    import ctypes.wintypes as w

    u32 = ctypes.windll.user32
    k32 = ctypes.windll.kernel32

    TH32CS_SNAPPROCESS = 0x2
    SW_RESTORE = 9

    MONITOR_DEFAULTTONEAREST = 2
    SWP_NOZORDER = 0x0004
    SWP_NOACTIVATE = 0x0010
    DWMWA_EXTENDED_FRAME_BOUNDS = 9

    INPUT_KEYBOARD = 1
    KEYEVENTF_KEYUP = 0x0002
    VK_RETURN = 0x0D
    VK_CONTROL = 0x11
    VK_V = 0x56
    VK_OEM_MINUS = 0xBD

    CF_UNICODETEXT = 13
    GMEM_MOVEABLE = 0x0002

    class _Proc(ctypes.Structure):
        _fields_ = [
            ("dwSize", w.DWORD), ("cntUsage", w.DWORD),
            ("th32ProcessID", w.DWORD),
            ("th32DefaultHeapID", ctypes.POINTER(ctypes.c_ulong)),
            ("th32ModuleID", w.DWORD), ("cntThreads", w.DWORD),
            ("th32ParentProcessID", w.DWORD), ("pcPriClassBase", ctypes.c_long),
            ("dwFlags", w.DWORD), ("szExeFile", ctypes.c_char * 260),
        ]

    # INPUT is a union over three shapes and SendInput checks the size it is
    # given, so the unused two have to be declared for it to come out right.
    class _Mouse(ctypes.Structure):
        _fields_ = [("dx", ctypes.c_long), ("dy", ctypes.c_long),
                    ("mouseData", w.DWORD), ("dwFlags", w.DWORD),
                    ("time", w.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

    class _Key(ctypes.Structure):
        _fields_ = [("wVk", w.WORD), ("wScan", w.WORD), ("dwFlags", w.DWORD),
                    ("time", w.DWORD), ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong))]

    class _Hardware(ctypes.Structure):
        _fields_ = [("uMsg", w.DWORD), ("wParamL", w.WORD), ("wParamH", w.WORD)]

    class _InputU(ctypes.Union):
        _fields_ = [("ki", _Key), ("mi", _Mouse), ("hi", _Hardware)]

    class _Input(ctypes.Structure):
        _fields_ = [("type", w.DWORD), ("u", _InputU)]

    class _MonitorInfo(ctypes.Structure):
        _fields_ = [("cbSize", w.DWORD), ("rcMonitor", w.RECT),
                    ("rcWork", w.RECT), ("dwFlags", w.DWORD)]

    dwm = ctypes.windll.dwmapi

    u32.SendInput.argtypes = (w.UINT, ctypes.POINTER(_Input), ctypes.c_int)
    # Window handles are pointer-sized, so the placement calls are declared
    # rather than left to ctypes' default int - a truncated hwnd names nothing.
    u32.MonitorFromWindow.argtypes = (w.HWND, w.DWORD)
    u32.MonitorFromWindow.restype = w.HANDLE
    u32.GetMonitorInfoW.argtypes = (w.HANDLE, ctypes.POINTER(_MonitorInfo))
    u32.GetWindowRect.argtypes = (w.HWND, ctypes.POINTER(w.RECT))
    u32.SetWindowPos.argtypes = (w.HWND, w.HWND, ctypes.c_int, ctypes.c_int,
                                 ctypes.c_int, ctypes.c_int, w.UINT)
    dwm.DwmGetWindowAttribute.argtypes = (w.HWND, w.DWORD, ctypes.POINTER(w.RECT),
                                          w.DWORD)
    k32.GlobalAlloc.restype = ctypes.c_void_p
    k32.GlobalLock.argtypes = (ctypes.c_void_p,)
    k32.GlobalLock.restype = ctypes.c_void_p
    k32.GlobalUnlock.argtypes = (ctypes.c_void_p,)
    k32.GlobalFree.argtypes = (ctypes.c_void_p,)
    u32.GetClipboardData.restype = ctypes.c_void_p
    u32.SetClipboardData.argtypes = (w.UINT, ctypes.c_void_p)
    u32.SetClipboardData.restype = ctypes.c_void_p

# Claude Code prefixes the title with a spinner or status mark while it works,
# so "revoiced" and "✳ revoiced" are the same window.
_NOISE = re.compile(r"^[^\w]+|[^\w]+$")


def _clean(title: str) -> str:
    return _NOISE.sub("", title or "").strip().lower()


def _parents() -> dict:
    """pid -> parent pid, for every process running right now."""
    snap = k32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    if snap == -1:
        return {}
    out = {}
    try:
        e = _Proc()
        e.dwSize = ctypes.sizeof(_Proc)
        ok = k32.Process32First(snap, ctypes.byref(e))
        while ok:
            out[e.th32ProcessID] = e.th32ParentProcessID
            ok = k32.Process32Next(snap, ctypes.byref(e))
    finally:
        k32.CloseHandle(snap)
    return out


def _title(hwnd: int) -> str:
    n = u32.GetWindowTextLengthW(hwnd) + 1
    buf = ctypes.create_unicode_buffer(n)
    u32.GetWindowTextW(hwnd, buf, n)
    return buf.value


def _class_of(hwnd: int) -> str:
    buf = ctypes.create_unicode_buffer(256)
    u32.GetClassNameW(hwnd, buf, 256)
    return buf.value


# Windows Terminal, then the classic console host.
_TERMINALS = ("CASCADIA_HOSTING_WINDOW_CLASS", "ConsoleWindowClass")
# File Explorer: a folder window, then the tree view of one.
_FOLDERS = ("CabinetWClass", "ExploreWClass")
# VS Code is Electron, and so is every Chromium browser and half the chat apps
# on the machine - the class alone would match all of them. The title is what
# separates it: VS Code always ends its title with the product name, and the
# Insiders build appends to that rather than replacing it.
_EDITORS = ("Chrome_WidgetWin_1",)
_EDITOR_MARK = "visual studio code"


def _windows_in(classes: tuple) -> list:
    """Every visible, titled, top-level window of these classes."""
    found = []

    def cb(hwnd, _):
        if (u32.IsWindowVisible(hwnd) and not u32.GetParent(hwnd)
                and u32.GetWindowTextLengthW(hwnd)
                and _class_of(hwnd) in classes):
            found.append(hwnd)
        return True

    u32.EnumWindows(ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)(cb), 0)
    return found


def _windows_of(pid: int = 0) -> list:
    """Visible top-level terminal windows: the process's own when given a pid,
    otherwise every terminal on the desktop. The station has to ask without a
    pid - it is launched from its own console and shares no ancestry with the
    sessions. A tab's ConPTY window is hidden and untitled, so it never shows up.

    The class check belongs to *both* branches, and for a release it was in the
    `elif`. `_terminal_pid` walks ancestors until one owns a window, so with the
    pid branch answering for any window at all the walk stopped at whatever
    non-terminal was in the chain - and `terminal_window`'s "a single window
    needs no name" shortcut then adopted it as the session's terminal.
    `send_text` pastes the user's prompt into that window and presses Enter.
    Measured on this desktop, read-only: `explorer.exe` owns exactly one visible
    titled top-level window, `Progman` / "Program Manager" - so a session under
    a shell whose ancestry runs through explorer recorded the Windows desktop,
    where Enter launches the selected icon. A shell in VS Code's integrated
    terminal is the mainstream shape of the same thing: the ConPTY console is
    hidden and untitled, so the walk carried on to `Code.exe` and typed into the
    open source file. With the check shared, an unrecognised chain finds nothing
    and the feature declines - which is the direction a false answer has to fail
    in here.
    """
    found = []

    def cb(hwnd, _):
        if not (u32.IsWindowVisible(hwnd) and not u32.GetParent(hwnd)
                and u32.GetWindowTextLengthW(hwnd)
                and _class_of(hwnd) in _TERMINALS):
            return True
        if pid:
            owner = w.DWORD()
            u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            if owner.value != pid:
                return True
        found.append(hwnd)
        return True

    u32.EnumWindows(ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)(cb), 0)
    return found


def window_pids() -> set:
    """Every pid owning a visible, titled, top-level window right now.

    A lease records the pid of the terminal its session ran in. When that pid
    is no longer in here the window has been closed, so the session is over -
    which is the only evidence available, since closing a terminal with the X
    never fires SessionEnd and the lease would otherwise sit lit for its full
    TTL. Empty off Windows, where callers must treat "unknown" as alive.
    """
    if not IS_WIN:
        return set()
    out = set()

    def cb(hwnd, _):
        if (u32.IsWindowVisible(hwnd) and not u32.GetParent(hwnd)
                and u32.GetWindowTextLengthW(hwnd)):
            owner = w.DWORD()
            u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
            out.add(owner.value)
        return True

    try:
        u32.EnumWindows(ctypes.WINFUNCTYPE(w.BOOL, w.HWND, w.LPARAM)(cb), 0)
    except OSError:
        return set()
    return out


def window_name_counts() -> dict:
    """How many terminal windows carry each name, normalised the way titles are
    compared everywhere here - status mark stripped, case folded.

    What a lease has to be checked against. Its pid cannot answer: Windows
    Terminal hosts every window in one process, so the pid a session recorded
    stays alive as long as *any* terminal is open, and a closed window looks
    exactly like a busy one.

    Counted, not just listed, because the number is the answer to a second
    question. Two windows on one profile really are two runs - but three leases
    against two windows means one of them ended, and its name still matches the
    windows that are left.
    """
    if not IS_WIN:
        return {}
    out = {}
    try:
        for h in _windows_of():
            name = _clean(_title(h))
            if name:
                out[name] = out.get(name, 0) + 1
    except OSError:
        return {}
    return out


def window_names() -> set:
    """Every terminal window on the desktop, by name."""
    return set(window_name_counts())


def name_key(title: str) -> str:
    """One title, normalised for comparison against `window_names()`."""
    return _clean(title)


def terminal_windows() -> set:
    """Every terminal window on the desktop right now, by handle.

    Taken just before a launch so the window that appears can be told from the
    ones already there. The title alone cannot do it: a profile started twice
    leaves two windows with the same name, and the one to arrange is the new one.
    """
    if not IS_WIN:
        return set()
    try:
        return set(_windows_of())
    except OSError:
        return set()


def _best(fresh: list, named: list) -> int:
    """Which window a poll should take: both, then named, then a lone new one.

    Three loops here polled for "the window that just appeared" and each settled
    ties differently - `named or fresh`, `fresh or named`, `named or fresh`
    again with the intersection in front - and the order is the one thing they
    have to share, because the difference only shows in the case that goes
    wrong. A window that is new *and* carries the name is unambiguous. A named
    one is the window that was asked for, however it got there - `code <folder>`
    and Explorer both reuse a window instead of opening a second.

    Novelty alone is a guess, so it answers only when there is exactly one new
    window. Several, with no name to choose between them, is **no answer**: two
    of the three callers resize and zoom whatever they are handed, and the
    window most likely to be new and nameless is a terminal the user opened
    while they were waiting.
    """
    both = [h for h in fresh if h in named]
    if both:
        return both[0]
    if named:
        return named[0]
    return fresh[0] if len(fresh) == 1 else 0


def new_window(before: set, title: str = "", wait: float = 6.0) -> int:
    """The terminal window that has appeared since `before` was taken, or 0.

    Polled rather than waited on: `wt.exe` hands off to the Windows Terminal
    process and exits, so the pid we spawned is gone before its window exists.
    A second is normal. Nothing at all means wt opened a tab in a window that
    was already there - a setting of theirs, and nothing to place.

    It used to answer the moment *anything* new turned up - `(named or fresh)[0]`
    inside `if fresh` - and Windows Terminal titles a window after creating it,
    so the unnamed case is the common one rather than the exception. Whatever
    came back was then resized and zoomed by `launch.start`, including a
    terminal the user opened during the wait. `_best` holds out for the name and
    settles for novelty only when there is one candidate; the loop carries on
    polling until the deadline rather than taking the first thing it sees.
    """
    if not IS_WIN:
        return 0
    deadline = time.monotonic() + wait
    while True:
        try:
            fresh = [h for h in _windows_of() if h not in before]
        except OSError:
            return 0
        named = [h for h in fresh if _clean(_title(h)) == _clean(title)]
        hit = _best(fresh, named)
        if hit:
            return hit
        if time.monotonic() >= deadline:
            return 0
        time.sleep(0.1)


def _work_area(hwnd: int) -> tuple:
    """The usable part of the screen this window is on - all of it bar the
    taskbar. Asked per window rather than of the primary monitor, so one that
    opened on a second screen is arranged on that screen.
    """
    info = _MonitorInfo()
    info.cbSize = ctypes.sizeof(_MonitorInfo)
    mon = u32.MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
    if not (mon and u32.GetMonitorInfoW(mon, ctypes.byref(info))):
        return (0, 0, u32.GetSystemMetrics(0), u32.GetSystemMetrics(1))
    r = info.rcWork
    return (r.left, r.top, r.right, r.bottom)


def _frame_padding(hwnd: int) -> tuple:
    """How far a window's rect reaches past what you can actually see.

    A window carries an invisible resize border several pixels wide, so one
    moved to an exact fraction of the screen sits visibly short of the join,
    with a gap down it. Snapping accounts for it; SetWindowPos does not, so the
    difference between the drawn frame and the window rect is added back. A
    call that fails gives no correction rather than a wrong one.
    """
    win, frame = w.RECT(), w.RECT()
    if not u32.GetWindowRect(hwnd, ctypes.byref(win)):
        return (0, 0, 0, 0)
    if dwm.DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS,
                                 ctypes.byref(frame), ctypes.sizeof(frame)):
        return (0, 0, 0, 0)
    return (frame.left - win.left, frame.top - win.top,
            win.right - frame.right, win.bottom - frame.bottom)


def right_third(hwnd: int) -> bool:
    """Put a window on the right third of its screen, full height.

    A third rather than the half Win+Right gives: the station is the thing being
    watched and a terminal only has to be readable beside it. Windows offers no
    "snap this" call for either, so the rectangle is worked out and set
    directly. Placed without activating: the window wt just opened already has
    the foreground, and taking it a second time from a background process is
    what Windows refuses anyway.
    """
    if not IS_WIN or not hwnd:
        return False
    try:
        if u32.IsIconic(hwnd) or u32.IsZoomed(hwnd):
            u32.ShowWindow(hwnd, SW_RESTORE)
        left, top, right, bottom = _work_area(hwnd)
        pl, pt, pr, pb = _frame_padding(hwnd)
        # Measured off the right edge, not along from the left: rounding a
        # third has to land the window flush with the screen edge, and
        # `left + 2 * w // 3` leaves a pixel of desktop showing at the far side.
        x = right - (right - left) // 3
        return bool(u32.SetWindowPos(
            hwnd, 0, x - pl, top - pt,
            (right - x) + pl + pr, (bottom - top) + pt + pb,
            SWP_NOZORDER | SWP_NOACTIVATE))
    except OSError:
        return False


def _match(titles: list, want: str, near: bool = True) -> str:
    """The window whose title names this project: exact first, then a title
    that is a shortened form of it - "revoice" for a project called "revoiced".

    `near` off leaves only the exact rule. The shortened-form rule is far too
    loose for a folder name that is not itself a project - a window titled
    "work" is a substring of "structuredwork" and would win.
    """
    want = _clean(want)
    if not want:
        return ""
    exact = [t for t in titles if _clean(t) == want]
    close = [t for t in titles if near and _clean(t) and _clean(t) in want]
    return (exact or close or [""])[0]


def _names(project: str) -> list:
    """Candidate window names: the project's own folder, then the one above it.
    `project` may be a full path or a bare name.

    A project nested inside a repo - aello\\site, desktop-automation\\client -
    is opened from the parent's terminal, so the title names the parent and
    nothing will ever match the leaf.
    """
    parts = [p for p in re.split(r"[\\/]+", project or "") if p and ":" not in p]
    return parts[-2:][::-1]


def _terminal_pid() -> int:
    """The nearest ancestor owning windows: speak.py -> claude -> shell -> term."""
    parents = _parents()
    pid = os.getpid()
    for _ in range(8):
        pid = parents.get(pid)
        if not pid:
            return 0
        if _windows_of(pid):
            return pid
    return 0


def terminal_pid() -> int:
    """This session's terminal, for a lease to watch. Callable from the hook
    only - the ancestry walk starts at us. 0 when it cannot be found, which
    callers must read as "no evidence" rather than as dead."""
    if not IS_WIN:
        return 0
    try:
        return _terminal_pid()
    except OSError:
        return 0


def open_windows() -> list:
    """Titles of every terminal window open right now, for the station to
    offer as the per-project override."""
    if not IS_WIN:
        return []
    return sorted({_title(h) for h in _windows_of()})


def terminal_window(project: str, override: str = "", cwd: str = "") -> dict:
    """Which window this session is in. Called from the hook, not the worker -
    the worker is detached and no longer under the terminal.

    An override always wins. Otherwise the best title match for the project
    name, which is right when windows are named after their project and simply
    finds nothing when they are not.
    """
    if not IS_WIN:
        return None
    try:
        pid = _terminal_pid()
        titles = [_title(h) for h in _windows_of(pid)] if pid else []
    except OSError:
        return None       # never cost the hook a turn over a nicety
    if not pid:
        return None

    if override:
        return {"pid": pid, "title": _match(titles, override) or override,
                "guessed": False}
    names = _names(cwd or project)
    best = _match(titles, names[0] if names else project)
    if not best and len(names) > 1:
        best = _match(titles, names[1], near=False)
    # The ancestry walk already proved this pid owns the session's terminal, so
    # a single window needs no name to be the right one. Windows Terminal hosts
    # every window in one process, which is why more than one means guess again.
    if not best and len(titles) == 1:
        best = titles[0]
    return {"pid": pid, "title": best, "guessed": True} if best else None


def _resolve(info: dict, project: str) -> int:
    """The window to raise. The recorded title first, then a fresh guess from
    the project name - windows get renamed, and an entry outlives the title it
    was written with. Recorded pid first, then any terminal, so the link also
    survives the terminal itself being restarted.
    """
    names = _names(project)
    for pid in dict.fromkeys([int((info or {}).get("pid") or 0), 0]):
        hwnds = _windows_of(pid)
        titles = [_title(h) for h in hwnds]
        for name in [(info or {}).get("title", "")] + names[:1]:
            best = _match(titles, name)
            if best:
                return hwnds[titles.index(best)]
        # Only now the parent, and only on an exact title - a nested project
        # borrows its parent's terminal, but a loose match here would raise
        # whatever window happened to sit under a generic folder name.
        for name in names[1:]:
            best = _match(titles, name, near=False)
            if best:
                return hwnds[titles.index(best)]
    return 0


def _folder_names(title: str) -> set:
    """What folder a window title says it is showing.

    Explorer does not title a window after its folder the way a terminal is
    titled after its project: it appends " - File Explorer", and with the
    full-path option on it names the whole path instead. Both are reduced to
    the folder's own name here. `rsplit`, so a folder with " - " in its name
    survives.
    """
    t = (title or "").strip()
    forms = {t}
    if " - " in t:
        forms.add(t.rsplit(" - ", 1)[0])
    return {os.path.basename(f.rstrip("\\/")) or f for f in forms}


def folder_windows() -> set:
    """Every open file-manager window, by handle. Taken before a folder is
    revealed, so the one that appears can be told from the ones already there."""
    if not IS_WIN:
        return set()
    try:
        return set(_windows_in(_FOLDERS))
    except OSError:
        return set()


def raise_folder(before: set, name: str, wait: float = 5.0) -> bool:
    """Bring the folder window just asked for to the front.

    The shell opens it, not us, so it is nobody's child and comes up behind
    whatever had the foreground - it lands in the taskbar wanting a second
    click. Polled, because the window does not exist yet when `os.startfile`
    returns; and matched by title as well as by novelty, because Explorer
    raises a window already showing that folder instead of opening another,
    in which case nothing new ever appears.
    """
    if not IS_WIN:
        return False
    deadline = time.monotonic() + wait
    while True:
        try:
            now = _windows_in(_FOLDERS)
        except OSError:
            return False
        fresh = [h for h in now if h not in before]
        want = _clean(name)
        named = [h for h in now
                 if want in {_clean(f) for f in _folder_names(_title(h))}]
        # Exact names only, after the title is reduced to one: the near-match
        # rule elsewhere would let `work` answer for `structuredwork`. The order
        # is `_best`'s, shared with the other two polls here - this one had
        # novelty ahead of the name, which is the opposite of what the editor's
        # loop did with the same three lists.
        hit = _best(fresh, named)
        if hit:
            return raise_hwnd(hit)
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.1)


def editor_windows() -> set:
    """Every VS Code window on the desktop right now, by handle.

    Taken just before opening one, for the same reason `terminal_windows()` is:
    the window that turns up has to be told from the ones already there.
    """
    if not IS_WIN:
        return set()
    try:
        return {h for h in _windows_in(_EDITORS)
                if _EDITOR_MARK in (_title(h) or "").lower()}
    except OSError:
        return set()


def new_editor_window(before: set, folder: str, wait: float = 30.0) -> int:
    """The VS Code window showing `folder`, once it exists. 0 if none does.

    Waits far longer than `new_window` does for a terminal. `code` is a shim
    that hands off and exits, and a cold start of Electron on this machine is
    several seconds - a six-second budget returned 0 while the window was still
    on its way, so the folder opened unplaced and the caller reported failure
    for something that had worked.

    Matched by title as well as by novelty, because `code <folder>` *reuses* a
    window already showing that folder rather than opening a second one, in
    which case nothing new ever appears. VS Code titles a window
    "<file> - <folder> - Visual Studio Code", so the folder is looked for as one
    of the ` - ` separated parts and compared exactly: the near-match rule
    elsewhere would let a window on `work` answer for `structuredwork`.
    """
    if not IS_WIN:
        return 0
    want = _clean(folder)
    deadline = time.monotonic() + wait
    while True:
        try:
            now = list(editor_windows())
        except OSError:
            return 0
        fresh = [h for h in now if h not in before]
        named = [h for h in now
                 if want in {_clean(p) for p in (_title(h) or "").split(" - ")}]
        hit = _best(fresh, named)
        if hit:
            return hit
        if time.monotonic() >= deadline:
            return 0
        time.sleep(0.2)


def left_two_thirds(hwnd: int) -> bool:
    """Put a window on the left two thirds of its screen, full height.

    The other side of `right_third`: an editor filling what a launched terminal
    leaves. Measured along from the left edge and stopped where the terminal
    starts - the *same* expression, `right - (right - left) // 3` - so the two
    always meet exactly, whatever the rounding does on a width that is not
    divisible by three. Working out two thirds independently is what leaves a
    seam or an overlap down the join.
    """
    if not IS_WIN or not hwnd:
        return False
    try:
        if u32.IsIconic(hwnd) or u32.IsZoomed(hwnd):
            u32.ShowWindow(hwnd, SW_RESTORE)
        left, top, right, bottom = _work_area(hwnd)
        pl, pt, pr, pb = _frame_padding(hwnd)
        x = right - (right - left) // 3
        return bool(u32.SetWindowPos(
            hwnd, 0, left - pl, top - pt,
            (x - left) + pl + pr, (bottom - top) + pt + pb,
            SWP_NOZORDER | SWP_NOACTIVATE))
    except OSError:
        return False


def raise_hwnd(hwnd: int) -> bool:
    """Bring one window to the front.

    Windows blocks a background process from taking the foreground, hence
    borrowing the current foreground thread's input queue for the call.
    """
    if not IS_WIN or not hwnd:
        return False

    # The foreground is one per machine and the station answers every request on
    # its own thread, so this is under the same lock `send_text` takes - four
    # `serve.py` routes reach here and two of them (`/api/focus`, a card's
    # double-click) are one press apart on the page. It is an `RLock`, so
    # `send_text`'s own hold nests rather than deadlocks. The comment at the top
    # of this file has said "anything that takes either one goes through this"
    # since it was written; this function did not.
    with _INPUT_LOCK:
        if u32.IsIconic(hwnd):
            u32.ShowWindow(hwnd, SW_RESTORE)
        fg = u32.GetForegroundWindow()
        if fg == hwnd:
            return True
        mine = k32.GetCurrentThreadId()
        theirs = u32.GetWindowThreadProcessId(fg, None) if fg else mine
        attached = (bool(u32.AttachThreadInput(theirs, mine, True))
                    if theirs != mine else False)
        try:
            u32.BringWindowToTop(hwnd)
            u32.SetForegroundWindow(hwnd)
        finally:
            if attached:
                u32.AttachThreadInput(theirs, mine, False)
        return u32.GetForegroundWindow() == hwnd


def raise_window(info: dict, project: str = "") -> bool:
    """Bring a session's terminal window to the front."""
    if not IS_WIN:
        return False
    return raise_hwnd(_resolve(info, project))


def _key(vk: int, up: bool = False) -> "_Input":
    return _Input(type=INPUT_KEYBOARD,
                  u=_InputU(ki=_Key(wVk=vk, dwFlags=KEYEVENTF_KEYUP if up else 0)))


def _keys(*events) -> bool:
    arr = (_Input * len(events))(*events)
    return u32.SendInput(len(events), arr, ctypes.sizeof(_Input)) == len(events)


def zoom_out(hwnd: int, steps: int = 2) -> bool:
    """Press ctrl+- in a window, `steps` times - Windows Terminal's own zoom.

    Sent as keys because there is no other way in: the command line has no font
    option, and the alternative is editing the user's Windows Terminal settings,
    which are theirs and not ours. This lasts the life of the window and leaves
    nothing behind. A virtual-key code is safe here where synthesised *text* is
    not - ctrl+- is a shortcut the terminal handles itself, never something
    translated into the input stream a program is reading.

    Sent only while the window actually holds the foreground, and re-checked
    between presses: ctrl+- zooms out most other things too, so a keystroke
    that missed would shrink whatever the user had switched to instead.
    """
    if not IS_WIN or not hwnd or steps <= 0:
        return False
    # Under the machine-wide lock, like every other sender: this presses a key
    # into whatever holds the foreground, and the re-check between presses is
    # worth nothing if another thread can raise a window between them.
    with _INPUT_LOCK:
        for _ in range(steps):
            if u32.GetForegroundWindow() != hwnd:
                return False
            if not _keys(_key(VK_CONTROL), _key(VK_OEM_MINUS),
                         _key(VK_OEM_MINUS, up=True), _key(VK_CONTROL, up=True)):
                return False
            time.sleep(0.1)
    return True


def _clip_open(tries: int = 6) -> bool:
    """Take the clipboard, retrying a refusal.

    Exactly one process may hold it at a time, and Office, every browser and
    every clipboard manager take it briefly whenever anything is copied - so a
    refusal here is transient by construction, the same shape as the state
    file's denied read, and one attempt was one attempt too few.
    """
    for attempt in range(tries):
        if u32.OpenClipboard(None):
            return True
        if attempt < tries - 1:
            time.sleep(0.02)
    return False


def _clip_get() -> str | None:
    """The clipboard's text, `""` for none, or **None** for could-not-look.

    Those last two used to be the same answer, and the difference decides
    whether the `finally` in `send_text` is allowed to clear what is there: a
    board we opened and found no text on is a board holding an image, and one we
    were refused is a board we know nothing about.
    """
    if not _clip_open():
        return None
    try:
        h = u32.GetClipboardData(CF_UNICODETEXT)
        if not h:
            return ""
        p = k32.GlobalLock(h)
        try:
            return ctypes.c_wchar_p(p).value or "" if p else ""
        finally:
            k32.GlobalUnlock(h)
    finally:
        u32.CloseClipboard()


def _clip_had_anything() -> bool:
    """Was there *something* on the clipboard, whatever its flavour?

    `_clip_get` returns "" for four different states - refused, no text format,
    lock failed, genuinely empty - and `if saved:` read all four as "nothing to
    put back". So the happy path lost data: take a screenshot with Win+Shift+S,
    press Send, and the image is gone with our line sitting in its place. This
    is what tells the two apart; it needs no open handle.
    """
    try:
        return int(u32.CountClipboardFormats()) > 0
    except Exception:
        return False


def _clip_clear() -> None:
    if not _clip_open():
        return
    try:
        u32.EmptyClipboard()
    finally:
        u32.CloseClipboard()


def _clip_set(text: str) -> bool:
    """Put one line on the clipboard. False means it is unchanged, or empty.

    `EmptyClipboard` has to come first - Windows requires it to take ownership
    before `SetClipboardData` - so a failure between the two leaves the board
    cleared and cannot be otherwise. What it must not also do is leak: on that
    path the clipboard did *not* take the handle, so this frees it, and a
    `GlobalLock` that answers null is no longer memmove'd to address zero.
    """
    buf = ctypes.create_unicode_buffer(text)
    h = k32.GlobalAlloc(GMEM_MOVEABLE, ctypes.sizeof(buf))
    if not h:
        return False
    p = k32.GlobalLock(h)
    if not p:
        k32.GlobalFree(h)
        return False
    ctypes.memmove(p, buf, ctypes.sizeof(buf))
    k32.GlobalUnlock(h)
    if not _clip_open():
        k32.GlobalFree(h)
        return False
    try:
        u32.EmptyClipboard()
        if u32.SetClipboardData(CF_UNICODETEXT, h):
            return True                    # the clipboard owns h from here
        k32.GlobalFree(h)                  # it does not, and nobody else will
        return False
    finally:
        u32.CloseClipboard()


def send_text(info: dict, project: str, text: str) -> bool:
    """Put a line into a session's terminal and press Enter.

    Sent as a clipboard paste, not as synthesised characters. Typing each
    character as a unicode key event works in cmd but not in anything reading
    raw input - PSReadLine, and Claude Code's own prompt - because a key event
    carrying no virtual-key code is dropped when the terminal translates it for
    a VT input stream. Paste is one event, keeps unicode intact, and is what
    the prompt is built to receive. The previous clipboard is put back.

    Focus is taken and then *checked*: Windows can refuse a foreground change,
    and pasting blind would put the text into whichever window happened to be
    in front. There is no undo - the receiving Claude Code submits on Enter -
    so a failed focus has to mean nothing is sent at all.

    Newlines collapse to spaces for the same reason. Enter is pressed once, at
    the end, on purpose; a pasted paragraph would otherwise submit line by line.
    """
    if not IS_WIN:
        return False
    line = " ".join(text.split())
    if not line:
        return False

    # One sender at a time. The clipboard and the foreground are machine-wide,
    # and the station answers every request on its own thread - two Sends
    # crossing meant each restored whatever the other had staged.
    with _INPUT_LOCK:
        hwnd = _resolve(info, project)
        if not hwnd:
            return False

        # Stage the clipboard *before* taking the foreground, so the only thing
        # between the check and the keys is the deliberate sleep. Both calls
        # block on whoever owns the clipboard, which made the old gap unbounded
        # rather than the 400ms it looked like.
        had = _clip_had_anything()
        saved = _clip_get()
        if not _clip_set(line):
            # `_clip_set` empties before it stores, because Windows makes that
            # the order, so a failure here has already cleared the board. Put
            # back what was read rather than leaving the user with nothing.
            if saved:
                _clip_set(saved)
            return False
        try:
            if not raise_hwnd(hwnd):
                return False
            time.sleep(0.15)   # the terminal needs a moment before it reads input
            # Re-check before *every* batch, which zoom_out has always done and
            # this - the one function that sends unrecoverable text into a
            # prompt that submits on Enter - did not. Windows applies no
            # foreground lock to a user-initiated activation, so a click, an
            # alt-tab, a UAC prompt or another /api/focus on another thread all
            # move it out from under us between the check and the paste.
            if u32.GetForegroundWindow() != hwnd:
                return False
            if not _keys(_key(VK_CONTROL), _key(VK_V),
                         _key(VK_V, up=True), _key(VK_CONTROL, up=True)):
                return False
            # Enter goes in its own call: the prompt can still be drawing the
            # pasted text, and a submit that races that loses characters.
            time.sleep(0.25)
            if u32.GetForegroundWindow() != hwnd:
                return False   # pasted but not submitted; the text is recoverable
            return _keys(_key(VK_RETURN), _key(VK_RETURN, up=True))
        finally:
            time.sleep(0.15)   # paste has to read it before it is taken away
            if saved:
                _clip_set(saved)
            elif had and saved == "":
                # Something was there that we could not carry - an image, a file
                # selection, a spreadsheet range. Leaving our line behind would
                # be a silent substitution; an empty clipboard is at least true.
                #
                # `saved == ""` and not `saved is None`: None means the board
                # was never opened, so "there was something" and "there was no
                # *text*" have not been told apart, and clearing on that is a
                # transient refusal deciding to throw the user's work away.
                _clip_clear()
