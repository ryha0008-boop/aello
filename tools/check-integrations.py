"""Check every aello integration in a repo, and prove each one rather than assume it.

    python tools/check-integrations.py [repo-path]     (default: current directory)

Every check here is written so that "silently absent" cannot read as "fine":

  * the voice hook is asked its version by EXECUTING it, not by reading the file
  * the pre-commit hook is fired at a real staged key and must refuse
  * CI is read from its last actual run, not from the file existing
  * Renovate is reported as "seeded, not confirmed" unless a PR or a dashboard
    issue proves it is running
  * a lockfile is judged on whether the TRANSITIVE set is pinned, not on whether
    the file is present

Exit code is 1 if anything is a FAIL. WARN does not fail the run.
"""
import json, os, re, subprocess, sys

repo = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else ".")
OK, WARN, FAIL, INFO = "PASS", "WARN", "FAIL", "----"
rows = []


def add(status, check, detail=""):
    rows.append((status, check, detail))


def git(*a, cwd=None):
    return subprocess.run(["git", "-C", cwd or repo, *a], capture_output=True, text=True)


def sh(cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd or repo, capture_output=True, text=True)


# ---------------------------------------------------------------- git basics
if not os.path.isdir(os.path.join(repo, ".git")):
    print(f"{repo} is not a git repo — nothing to check.")
    sys.exit(1)

remote = git("remote", "get-url", "origin").stdout.strip()
add(OK if remote else WARN, "git remote", remote or "no origin — CI and Renovate cannot run")

# ---------------------------------------------------------------- env dirs
envs = [d for d in os.listdir(repo) if d.startswith((".claude-env-", ".cline-env-"))
        and os.path.isfile(os.path.join(repo, d, ".aello.toml"))]
add(OK if envs else WARN, "aello envs placed", ", ".join(envs) or "none in this repo")

# ---------------------------------------------------------------- voice hook
for e in envs:
    if e.startswith(".cline-env-"):
        add(INFO, f"voice [{e}]", "Cline env — ships no voice by design")
        continue
    speak = os.path.join(repo, e, "hooks", "speak.py")
    if not os.path.exists(speak):
        add(FAIL, f"voice hook [{e}]", "hooks/speak.py missing — env never placed by a current aello")
        continue
    # Executed, not read: a partial copy answers this and nothing else.
    r = sh([sys.executable, speak, "--hook-version"])
    v = r.stdout.strip()
    add(OK if v == "24" else FAIL, f"voice hook [{e}]",
        f"version {v or r.returncode}" + ("" if v == "24" else " — expected 24; run `aello run` to refresh"))

    st = os.path.join(repo, e, "settings.json")
    if os.path.exists(st):
        try:
            hooks = json.load(open(st, encoding="utf-8-sig")).get("hooks", {})
            want = {"Stop", "SessionEnd", "SessionStart", "PostCompact", "UserPromptSubmit", "PreToolUse"}
            missing = sorted(want - set(hooks))
            add(OK if not missing else WARN, f"hooks registered [{e}]",
                "all 6 events" if not missing else "missing: " + ", ".join(missing))
        except Exception as ex:
            add(FAIL, f"hooks registered [{e}]", f"settings.json unreadable: {ex}")

# ---------------------------------------------------------------- pre-commit
hook = os.path.join(repo, ".githooks", "pre-commit")
hp = git("config", "--local", "--get", "core.hooksPath").stdout.strip()
if not os.path.exists(hook):
    add(FAIL, "pre-commit hook", "no .githooks/pre-commit — run `aello run <bp>` to seed it")
elif hp != ".githooks":
    add(FAIL, "pre-commit wired", f"core.hooksPath={hp!r} — the file exists but git never runs it")
else:
    marker = re.search(r"aello-pre-commit v(\d+)", open(hook, encoding="utf-8", errors="replace").read())
    tracked = bool(git("ls-files", "--", ".githooks/pre-commit").stdout.strip())
    add(OK if tracked else WARN, "pre-commit committed",
        "tracked" if tracked else "on disk but NOT tracked — a fresh clone gets no guard")
    # Fire it. A hook that was written is not a hook that runs.
    canary = os.path.join(repo, "_aello_canary.md")
    try:
        open(canary, "w", encoding="utf-8", newline="").write(
            "note:\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n")
        git("add", "--", "_aello_canary.md")
        r = git("commit", "-m", "canary (must be blocked)")
        blocked = r.returncode != 0 and "PRIVATE KEY" in (r.stdout + r.stderr)
        add(OK if blocked else FAIL, "pre-commit REFUSES a key",
            (f"v{marker.group(1)} blocked it" if marker else "blocked it")
            if blocked else "IT DID NOT BLOCK — the guard is decorative")
    finally:
        git("reset", "-q", "HEAD", "--", "_aello_canary.md")
        if os.path.exists(canary):
            os.remove(canary)

# ---------------------------------------------------------------- CI
ci = os.path.join(repo, ".github", "workflows", "ci.yml")
if not os.path.exists(ci):
    add(FAIL, "test+audit CI", "no .github/workflows/ci.yml")
else:
    tracked = bool(git("ls-files", "--", ".github/workflows/ci.yml").stdout.strip())
    add(OK if tracked else FAIL, "CI committed",
        "tracked" if tracked else "on disk but NOT tracked — GitHub has never seen it")
    if remote and tracked:
        r = sh(["gh", "run", "list", "--workflow", "ci.yml", "--limit", "1",
                "--json", "conclusion,status,url"])
        try:
            d = json.loads(r.stdout or "[]")
        except Exception:
            d = []
        if not d:
            add(WARN, "CI last run", "no run recorded yet — push once to prove it fires")
        else:
            c = d[0]["conclusion"] or d[0]["status"]
            add(OK if c == "success" else FAIL, "CI last run", f"{c}  {d[0]['url']}")

# ---------------------------------------------------------------- Renovate
rn = os.path.join(repo, ".github", "renovate.json")
if not os.path.exists(rn):
    add(WARN, "Renovate config", "no .github/renovate.json")
else:
    tracked = bool(git("ls-files", "--", ".github/renovate.json").stdout.strip())
    evidence = ""
    if remote and tracked:
        pr = sh(["gh", "pr", "list", "--author", "app/renovate", "--state", "all",
                 "--limit", "1", "--json", "number"])
        iss = sh(["gh", "issue", "list", "--search", "Dependency Dashboard",
                  "--limit", "1", "--json", "number"])
        got = lambda o: o.stdout.strip() not in ("", "[]")
        if got(pr) or got(iss):
            evidence = "Renovate PR or dashboard seen — App IS installed"
    add(OK if evidence else WARN, "Renovate running",
        evidence or "seeded, not confirmed running — install github.com/apps/renovate")

# ---------------------------------------------------------------- lockfiles
j = lambda *x: os.path.join(repo, *x)
found = False
if os.path.exists(j("package.json")):
    found = True
    add(OK if os.path.exists(j("package-lock.json")) else FAIL, "node lock",
        "package-lock.json" if os.path.exists(j("package-lock.json"))
        else "package.json with no committed lock — nothing can reproduce this install")
if os.path.exists(j("Cargo.toml")):
    found = True
    add(OK if os.path.exists(j("Cargo.lock")) else FAIL, "rust lock",
        "Cargo.lock" if os.path.exists(j("Cargo.lock")) else "Cargo.toml with no lock")
if os.path.exists(j("uv.lock")):
    found = True
    add(OK, "python lock", "uv.lock")
elif os.path.exists(j("requirements.txt")):
    found = True
    txt = open(j("requirements.txt"), encoding="utf-8", errors="replace").read()
    head = "\n".join(txt.splitlines()[:3]).lower()
    compiled = any(k in head for k in ("uv pip compile", "pip-compile", "autogenerated"))
    # Package lines only. Counting `--hash=` continuations as unpinned reports a
    # correctly compiled lock as broken — it did exactly that here once.
    pkgs = [l for l in txt.splitlines()
            if l.strip() and not l.lstrip().startswith(("#", "-", "--")) and not l[:1].isspace()]
    pinned = [l for l in pkgs if "==" in l]
    if compiled and pkgs and len(pinned) == len(pkgs):
        add(OK, "python lock", f"compiled, {len(pkgs)} pinned, {txt.count('--hash=')} hashes")
    else:
        add(FAIL, "python lock",
            f"hand-written, {len(pinned)}/{len(pkgs)} pinned — the transitive set floats")
elif os.path.exists(j("pyproject.toml")):
    found = True
    add(FAIL, "python lock", "pyproject.toml with no uv.lock or compiled requirements.txt")
if not found:
    add(INFO, "lockfiles", "no Python/Node/Rust manifest at the repo root")

# ---------------------------------------------------------------- mirror
vis = ""
if remote:
    r = sh(["gh", "repo", "view", "--json", "visibility", "--jq", ".visibility"])
    vis = r.stdout.strip()
in_repo = git("ls-files", "claude-internal").stdout.strip().splitlines()
if vis == "PUBLIC" and in_repo:
    add(FAIL, "env mirror", f"{len(in_repo)} mirror files tracked in a PUBLIC repo — "
                            "your agent's memory is world-readable. Use `aello edit <bp> --mirror-dir`")
elif in_repo:
    add(OK, "env mirror", f"{len(in_repo)} files tracked, repo is {vis or 'not on GitHub'}")
else:
    add(OK if vis == "PUBLIC" else INFO, "env mirror",
        "none tracked here" + (" — correct for a public repo" if vis == "PUBLIC" else ""))

gi = open(j(".gitignore"), encoding="utf-8", errors="replace").read() if os.path.exists(j(".gitignore")) else ""
add(OK if ".claude-env-*" in gi else FAIL, ".gitignore",
    "ignores .claude-env-*" if ".claude-env-*" in gi
    else "MISSING .claude-env-* — an env dir can hold credentials")

# ---------------------------------------------------------------- report
w = max(len(c) for _, c, _ in rows)
print(f"\n{os.path.basename(repo)}  ({repo})\n")
for status, check, detail in rows:
    print(f"  [{status}] {check.ljust(w)}  {detail}")
bad = [r for r in rows if r[0] == FAIL]
warn = [r for r in rows if r[0] == WARN]
print(f"\n  {len(rows)} checks — {len(bad)} FAIL, {len(warn)} WARN\n")
sys.exit(1 if bad else 0)
