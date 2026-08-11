//! `aello check` — verify a repo's aello integrations by proving each one.
//!
//! Every integration aello seeds has a failure mode that is **indistinguishable
//! from success in a file listing**: a `pre-commit` hook git never runs because
//! `core.hooksPath` is unset, a workflow that exists on disk and was never
//! committed, a Renovate config with no GitHub App behind it, a
//! `requirements.txt` listing only direct imports, and an env mirror quietly
//! tracked in a public repo. Reading files cannot tell any of those from
//! working, which is why this module executes things instead.
//!
//! The rule every check here follows: **an inability to test is never a pass.**
//! Where evidence cannot be obtained the row is `WARN`/`UNKNOWN` and says what
//! is missing, because "no output" is how this project has most often been
//! wrong.

use crate::models::Agent;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Info,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Info => "----",
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct Row {
    pub status: Status,
    pub check: String,
    pub detail: String,
}

#[derive(Debug, serde::Serialize)]
pub struct RepoReport {
    pub repo: String,
    pub rows: Vec<Row>,
}

impl RepoReport {
    pub fn failed(&self) -> usize {
        self.rows.iter().filter(|r| r.status == Status::Fail).count()
    }
    pub fn warned(&self) -> usize {
        self.rows.iter().filter(|r| r.status == Status::Warn).count()
    }
    fn add(&mut self, status: Status, check: impl Into<String>, detail: impl Into<String>) {
        self.rows.push(Row { status, check: check.into(), detail: detail.into() });
    }
}

/// Run git in `repo`. A git that will not spawn at all yields empty output with
/// a failing status, which every caller already treats as "no evidence" — the
/// alternative was a panic on a machine with no git, in a command whose whole
/// job is to report rather than to crash.
fn git(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap_or_else(|_| empty_output())
}

/// A failing, empty `Output` — the "could not run it at all" value.
///
/// `ExitStatusExt` is per-platform and **both** imports need their own `cfg`.
/// The windows one was ungated, which compiles fine on Windows and fails on
/// Linux with `cannot find windows in os` — caught by CI, not by the suite here,
/// because the suite only ever runs on Windows on this machine. That is the
/// exact gap the CI test job was added for.
#[cfg(windows)]
fn empty_output() -> std::process::Output {
    use std::os::windows::process::ExitStatusExt as _;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

#[cfg(not(windows))]
fn empty_output() -> std::process::Output {
    use std::os::unix::process::ExitStatusExt as _;
    std::process::Output {
        status: std::process::ExitStatus::from_raw(1),
        stdout: Vec::new(),
        stderr: Vec::new(),
    }
}

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn tracked(repo: &Path, path: &str) -> bool {
    !out(&git(repo, &["ls-files", "--", path])).is_empty()
}

/// Run every check against one repo.
pub fn check_repo(repo: &Path) -> RepoReport {
    let mut r = RepoReport { repo: repo.display().to_string(), rows: Vec::new() };

    if !repo.join(".git").exists() {
        r.add(Status::Fail, "git repo", "not a git repo — nothing aello seeds applies here");
        return r;
    }

    let remote = out(&git(repo, &["remote", "get-url", "origin"]));
    r.add(
        if remote.is_empty() { Status::Warn } else { Status::Pass },
        "git remote",
        if remote.is_empty() {
            "no origin — CI and Renovate cannot run".into()
        } else {
            remote.clone()
        },
    );

    check_envs(repo, &mut r);
    check_pre_commit(repo, &mut r);
    check_ci(repo, &remote, &mut r);
    check_renovate(repo, &remote, &mut r);
    check_locks(repo, &mut r);
    check_mirror(repo, &remote, &mut r);

    let gi = std::fs::read_to_string(repo.join(".gitignore")).unwrap_or_default();
    let pat = Agent::Claude.gitignore_pattern();
    r.add(
        if gi.contains(pat) { Status::Pass } else { Status::Fail },
        ".gitignore",
        if gi.contains(pat) {
            format!("ignores {pat}")
        } else {
            format!("MISSING {pat} — an env dir can hold Claude Code's own credentials")
        },
    );
    r
}

/// Placed envs, and for each Claude env the voice hook version and the hook
/// registrations.
fn check_envs(repo: &Path, r: &mut RepoReport) {
    let mut envs: Vec<PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(repo) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let is_env = name.starts_with(Agent::Claude.env_prefix())
                || name.starts_with(Agent::Cline.env_prefix());
            // The manifest, not the directory name: 73 `.claude-env-*` dirs exist
            // on this machine and only 41 are real placements.
            if is_env && e.path().join(".aello.toml").is_file() {
                envs.push(e.path());
            }
        }
    }
    envs.sort();
    let names: Vec<String> =
        envs.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    r.add(
        if envs.is_empty() { Status::Warn } else { Status::Pass },
        "aello envs placed",
        if envs.is_empty() { "none in this repo".into() } else { names.join(", ") },
    );

    for env in &envs {
        let name = env.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with(Agent::Cline.env_prefix()) {
            r.add(Status::Info, format!("voice [{name}]"), "Cline env — ships no voice by design");
            continue;
        }
        let speak = env.join("hooks").join("speak.py");
        if !speak.exists() {
            r.add(
                Status::Fail,
                format!("voice hook [{name}]"),
                "hooks/speak.py missing — this env has never been placed by a current aello",
            );
        } else {
            // Executed, not read. `--hook-version` prints before any optional
            // import, so even a partial copy answers it — which is the whole
            // reason it is asked this way.
            let py = if cfg!(windows) { "python" } else { "python3" };
            let o = Command::new(py).arg(&speak).arg("--hook-version").output();
            let v = o.map(|o| out(&o)).unwrap_or_default();
            let want = crate::project::HOOK_VERSION.to_string();
            r.add(
                if v == want { Status::Pass } else if v.is_empty() { Status::Warn } else { Status::Fail },
                format!("voice hook [{name}]"),
                if v == want {
                    format!("version {v}")
                } else if v.is_empty() {
                    "could not run python — version UNKNOWN, not assumed good".into()
                } else {
                    format!("version {v} — expected {want}; `aello run` refreshes it")
                },
            );
        }
        check_settings_hooks(env, &name, r);
    }
}

/// The six events a current placement registers. A missing one is silent: the
/// env simply stops speaking, or stops archiving, with nothing said anywhere.
const WANTED_HOOKS: &[&str] =
    &["Stop", "SessionEnd", "SessionStart", "PostCompact", "UserPromptSubmit", "PreToolUse"];

fn check_settings_hooks(env: &Path, name: &str, r: &mut RepoReport) {
    let path = env.join("settings.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        r.add(Status::Fail, format!("hooks registered [{name}]"), "settings.json unreadable");
        return;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
    else {
        r.add(Status::Fail, format!("hooks registered [{name}]"), "settings.json is not valid JSON");
        return;
    };
    let have = v.get("hooks").and_then(|h| h.as_object());
    let missing: Vec<&str> = WANTED_HOOKS
        .iter()
        .copied()
        .filter(|k| have.map_or(true, |o| !o.contains_key(*k)))
        .collect();
    r.add(
        if missing.is_empty() { Status::Pass } else { Status::Warn },
        format!("hooks registered [{name}]"),
        if missing.is_empty() {
            format!("all {} events", WANTED_HOOKS.len())
        } else {
            format!("missing: {}", missing.join(", "))
        },
    );
}

/// The `pre-commit` guard: present, wired, committed, and — the only check that
/// matters — actually refusing.
fn check_pre_commit(repo: &Path, r: &mut RepoReport) {
    let hook = repo.join(".githooks").join("pre-commit");
    if !hook.exists() {
        r.add(Status::Fail, "pre-commit hook", "no .githooks/pre-commit — `aello run` seeds it");
        return;
    }
    let configured = out(&git(repo, &["config", "--local", "--get", "core.hooksPath"]));
    if configured != ".githooks" {
        r.add(
            Status::Fail,
            "pre-commit wired",
            format!("core.hooksPath is {configured:?} — the file exists but git never runs it"),
        );
        return;
    }
    let is_tracked = tracked(repo, ".githooks/pre-commit");
    r.add(
        if is_tracked { Status::Pass } else { Status::Warn },
        "pre-commit committed",
        if is_tracked {
            "tracked".into()
        } else {
            "on disk but NOT tracked — a fresh clone of this repo has no guard".to_string()
        },
    );
    match fire_pre_commit(repo) {
        Ok(true) => r.add(Status::Pass, "pre-commit REFUSES a key", "blocked a staged private key"),
        Ok(false) => r.add(
            Status::Fail,
            "pre-commit REFUSES a key",
            "IT DID NOT BLOCK — the guard is decorative",
        ),
        Err(e) => r.add(
            Status::Warn,
            "pre-commit REFUSES a key",
            format!("could not test ({e}) — UNKNOWN, not assumed good"),
        ),
    }
}

/// Stage a real armored key into a **throwaway index** and run the hook through
/// `git hook run`, which resolves git's own shell and honours `core.hooksPath`.
///
/// Deliberately not `git commit`: a checker that commits is a checker that
/// lands a canary commit on the one repo where the guard is broken — the exact
/// case it exists to find. Nothing here touches the real index, the worktree or
/// HEAD. The blob is written to the object store and is unreferenced, so it is
/// collected by the next `git gc`.
fn fire_pre_commit(repo: &Path) -> Result<bool> {
    use std::io::Write as _;
    const CANARY: &str = "note:\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\n";

    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("could not run git hash-object")?;
    child.stdin.take().unwrap().write_all(CANARY.as_bytes())?;
    let blob = out(&child.wait_with_output()?);
    if blob.is_empty() {
        anyhow::bail!("git hash-object produced nothing");
    }

    let index = std::env::temp_dir().join(format!("aello-check-{}.index", std::process::id()));
    let _guard = TempFile(index.clone());
    let add = Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_INDEX_FILE", &index)
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(format!("100644,{blob},_aello_canary.md"))
        .output()
        .context("could not stage the canary into a temporary index")?;
    if !add.status.success() {
        anyhow::bail!("git update-index failed");
    }

    let run = Command::new("git")
        .arg("-C")
        .arg(repo)
        .env("GIT_INDEX_FILE", &index)
        .args(["hook", "run", "pre-commit"])
        .output()
        .context("could not run the hook")?;
    // `git hook run` arrived in git 2.36. An older git reports an unknown
    // subcommand, which must not read as "the hook passed".
    let stderr = String::from_utf8_lossy(&run.stderr);
    if stderr.contains("is not a git command") || stderr.contains("usage: git hook") {
        anyhow::bail!("`git hook run` unsupported — needs git 2.36+");
    }
    Ok(!run.status.success())
}

/// Deletes a temp file when it goes out of scope, including on an early return.
struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn check_ci(repo: &Path, remote: &str, r: &mut RepoReport) {
    let ci = repo.join(".github").join("workflows").join("ci.yml");
    if !ci.exists() {
        r.add(
            Status::Fail,
            "test+audit CI",
            "no .github/workflows/ci.yml — tests and the audit run only where someone types them",
        );
        return;
    }
    let is_tracked = tracked(repo, ".github/workflows/ci.yml");
    r.add(
        if is_tracked { Status::Pass } else { Status::Fail },
        "CI committed",
        if is_tracked {
            "tracked".into()
        } else {
            "on disk but NOT tracked — GitHub has never seen it".to_string()
        },
    );
    if remote.is_empty() || !is_tracked {
        return;
    }
    // The last real run, not the file's existence.
    let o = Command::new("gh")
        .current_dir(repo)
        .args(["run", "list", "--workflow", "ci.yml", "--limit", "1", "--json",
               "conclusion,status,url"])
        .output();
    let Ok(o) = o else {
        r.add(Status::Warn, "CI last run", "gh not available — UNKNOWN");
        return;
    };
    let runs: Vec<serde_json::Value> = serde_json::from_slice(&o.stdout).unwrap_or_default();
    match runs.first() {
        None => r.add(Status::Warn, "CI last run", "no run recorded yet — push once to prove it fires"),
        Some(run) => {
            let c = run
                .get("conclusion")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| run.get("status").and_then(|v| v.as_str()))
                .unwrap_or("unknown");
            let url = run.get("url").and_then(|v| v.as_str()).unwrap_or("");
            r.add(
                if c == "success" { Status::Pass } else { Status::Fail },
                "CI last run",
                format!("{c}  {url}"),
            );
        }
    }
}

/// Renovate is the one integration aello cannot finish: the GitHub App is a
/// manual install. So a seeded file is reported as seeded — never as running —
/// unless a PR or a dependency dashboard proves otherwise.
fn check_renovate(repo: &Path, remote: &str, r: &mut RepoReport) {
    let rn = repo.join(".github").join("renovate.json");
    if !rn.exists() {
        r.add(Status::Warn, "Renovate config", "no .github/renovate.json");
        return;
    }
    if remote.is_empty() || !tracked(repo, ".github/renovate.json") {
        r.add(Status::Warn, "Renovate running", "config not tracked or no remote — cannot run");
        return;
    }
    let seen = |args: &[&str]| -> bool {
        Command::new("gh")
            .current_dir(repo)
            .args(args)
            .output()
            .map(|o| {
                let s = out(&o);
                !s.is_empty() && s != "[]"
            })
            .unwrap_or(false)
    };
    let evidence = seen(&["pr", "list", "--author", "app/renovate", "--state", "all", "--limit",
                          "1", "--json", "number"])
        || seen(&["issue", "list", "--search", "Dependency Dashboard", "--limit", "1", "--json",
                  "number"]);
    r.add(
        if evidence { Status::Pass } else { Status::Warn },
        "Renovate running",
        if evidence {
            "a Renovate PR or dashboard is present — the App is installed".into()
        } else {
            "seeded, not confirmed running — install github.com/apps/renovate".to_string()
        },
    );
}

/// Whether an install can be reproduced — which is not the same question as
/// whether a lockfile is present.
fn check_locks(repo: &Path, r: &mut RepoReport) {
    let j = |p: &str| repo.join(p);
    let mut found = false;

    if j("package.json").exists() {
        found = true;
        let ok = j("package-lock.json").exists();
        r.add(
            if ok { Status::Pass } else { Status::Fail },
            "node lock",
            if ok {
                "package-lock.json".into()
            } else {
                "package.json with no committed lock — nothing can reproduce this install"
                    .to_string()
            },
        );
    }
    if j("Cargo.toml").exists() {
        found = true;
        let ok = j("Cargo.lock").exists();
        r.add(
            if ok { Status::Pass } else { Status::Fail },
            "rust lock",
            if ok { "Cargo.lock" } else { "Cargo.toml with no Cargo.lock" },
        );
    }
    if j("uv.lock").exists() {
        found = true;
        r.add(Status::Pass, "python lock", "uv.lock");
    } else if j("requirements.txt").exists() {
        found = true;
        let text = std::fs::read_to_string(j("requirements.txt")).unwrap_or_default();
        let (status, detail) = judge_requirements(&text);
        r.add(status, "python lock", detail);
    } else if j("pyproject.toml").exists() {
        found = true;
        r.add(
            Status::Fail,
            "python lock",
            "pyproject.toml with no uv.lock or compiled requirements.txt",
        );
    }
    if !found {
        r.add(Status::Info, "lockfiles", "no Python/Node/Rust manifest at the repo root");
    }
}

/// A `requirements.txt` reproduces an install only if the **transitive** set is
/// pinned. A hand-written one listing the ten packages the code imports leaves
/// everything underneath resolving to whatever is newest that day — measured
/// here, that put a *beta* signing library into a live order path.
///
/// Package lines only. Counting `--hash=` continuations as unpinned reported a
/// correctly compiled 79-package lock as "79/1898 pinned", against the one repo
/// that had just fixed it.
fn judge_requirements(text: &str) -> (Status, String) {
    let head: String = text.lines().take(3).collect::<Vec<_>>().join("\n").to_lowercase();
    let compiled = ["uv pip compile", "pip-compile", "autogenerated"]
        .iter()
        .any(|k| head.contains(k));
    let pkgs: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.is_empty()
                && !t.starts_with('#')
                && !t.starts_with('-')
                && !l.starts_with([' ', '\t'])
        })
        .collect();
    let pinned = pkgs.iter().filter(|l| l.contains("==")).count();
    if pkgs.is_empty() {
        return (Status::Fail, "requirements.txt has no package lines".into());
    }
    if compiled && pinned == pkgs.len() {
        let hashes = text.matches("--hash=").count();
        return (
            Status::Pass,
            format!("compiled, {} pinned, {hashes} hashes", pkgs.len()),
        );
    }
    if pinned == pkgs.len() {
        return (
            Status::Warn,
            format!("all {} pinned but no compiler header — the transitive set may float", pkgs.len()),
        );
    }
    (
        Status::Fail,
        format!("hand-written, {pinned}/{} pinned — the transitive set floats", pkgs.len()),
    )
}

/// An env mirror tracked in a public repo publishes the agent's memory. That is
/// a decision the user makes, never a side effect — so it is a FAIL here.
fn check_mirror(repo: &Path, remote: &str, r: &mut RepoReport) {
    let visibility = if remote.is_empty() {
        String::new()
    } else {
        Command::new("gh")
            .current_dir(repo)
            .args(["repo", "view", "--json", "visibility", "--jq", ".visibility"])
            .output()
            .map(|o| out(&o))
            .unwrap_or_default()
    };
    let n = out(&git(repo, &["ls-files", "claude-internal"])).lines().count();
    match (visibility.as_str(), n) {
        ("PUBLIC", n) if n > 0 => r.add(
            Status::Fail,
            "env mirror",
            format!(
                "{n} mirror files tracked in a PUBLIC repo — the agent's memory is \
                 world-readable. `aello edit <bp> --mirror-dir <private clone>`"
            ),
        ),
        ("PUBLIC", _) => {
            r.add(Status::Pass, "env mirror", "none tracked here — correct for a public repo")
        }
        (v, n) if n > 0 => r.add(
            Status::Pass,
            "env mirror",
            format!("{n} files tracked, repo is {}", if v.is_empty() { "not on GitHub" } else { v }),
        ),
        _ => r.add(Status::Info, "env mirror", "none tracked here"),
    }
}

/// Every repo under `root` that holds at least one placed env.
///
/// Keyed on the `.aello.toml` manifest rather than the directory name: this
/// machine carries 73 `.claude-env-*` directories and 41 real placements, the
/// rest being archived shells.
pub fn find_repos(root: &Path) -> Vec<PathBuf> {
    const SKIP: &[&str] =
        &["node_modules", "target", ".git", "AppData", ".cargo", ".rustup", "dist", "build"];
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut children = Vec::new();
        let mut is_env = false;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                if !SKIP.contains(&name.as_str()) {
                    children.push(e.path());
                }
            } else if name == ".aello.toml" {
                is_env = true;
            }
        }
        if is_env {
            // `dir` is the env dir; its parent is the project.
            if let Some(p) = dir.parent() {
                let p = p.to_path_buf();
                if !found.contains(&p) {
                    found.push(p);
                }
            }
            continue; // never descend into an env dir
        }
        stack.extend(children);
    }
    found.sort();
    found
}

/// Print one repo's report. Returns the number of failures.
pub fn print_report(rep: &RepoReport) -> usize {
    let width = rep.rows.iter().map(|r| r.check.len()).max().unwrap_or(0);
    println!("\n{}\n", rep.repo);
    for row in &rep.rows {
        println!("  [{}] {:width$}  {}", row.status.label(), row.check, row.detail);
    }
    let (f, w) = (rep.failed(), rep.warned());
    println!("\n  {} checks — {f} FAIL, {w} WARN", rep.rows.len());
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The finding a "does the file exist" check misses entirely: a manifest
    /// that is present and still cannot reproduce an install.
    #[test]
    fn a_hand_written_requirements_file_is_the_same_finding_as_no_lock() {
        let (s, d) = judge_requirements("requests\nflask\nnumpy==1.2.3\n");
        assert_eq!(s, Status::Fail, "{d}");
        assert!(d.contains("1/3"), "{d}");
    }

    /// And the false finding that produced: a compiled lock is mostly `--hash=`
    /// continuations, which are not unpinned packages.
    #[test]
    fn a_compiled_lock_is_not_judged_by_its_hash_lines() {
        let text = "# This file was autogenerated by uv via the following command:\n\
                    #    uv pip compile --generate-hashes requirements.in\n\
                    altair==6.2.2 \\\n\
                    \x20   --hash=sha256:aaaa \\\n\
                    \x20   --hash=sha256:bbbb\n\
                    \x20   # via streamlit\n\
                    flask==3.0.0 \\\n\
                    \x20   --hash=sha256:cccc\n";
        let (s, d) = judge_requirements(text);
        assert_eq!(s, Status::Pass, "{d}");
        assert!(d.contains("2 pinned"), "{d}");
        assert!(d.contains("3 hashes"), "{d}");
    }

    /// Pinned but not compiled is not the same as compiled: nothing proves the
    /// transitive set was resolved rather than hand-listed.
    #[test]
    fn pinned_without_a_compiler_header_is_a_warning_not_a_pass() {
        let (s, _) = judge_requirements("requests==2.0.0\nflask==3.0.0\n");
        assert_eq!(s, Status::Warn);
    }

    /// A repo with no `.git` cannot be checked, and must say so rather than
    /// producing an empty all-clear.
    #[test]
    fn a_non_repo_fails_rather_than_reporting_nothing() {
        let d = tempfile::tempdir().unwrap();
        let rep = check_repo(d.path());
        assert_eq!(rep.failed(), 1);
        assert!(rep.rows[0].detail.contains("not a git repo"));
    }

    /// `find_repos` keys on the manifest and never descends into an env dir —
    /// a directory named `.claude-env-*` with no `.aello.toml` is an archived
    /// shell, and this machine has more of those than real placements.
    #[test]
    fn find_repos_keys_on_the_manifest_not_the_directory_name() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("proj");
        std::fs::create_dir_all(real.join(".claude-env-a")).unwrap();
        std::fs::write(real.join(".claude-env-a").join(".aello.toml"), "name='a'").unwrap();
        // An archived shell: right name, no manifest.
        let shell = root.path().join("old");
        std::fs::create_dir_all(shell.join(".claude-env-b")).unwrap();

        let found = find_repos(root.path());
        assert_eq!(found, vec![real], "only the repo with a manifest counts");
    }
}
