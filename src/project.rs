//! Placing a blueprint into a project: the env dir, its `.aello.toml`,
//! `settings.json`, optional CLAUDE.md, and the PostCompact hook script.

use crate::models::{Capabilities, Instance};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const POST_COMPACT_SCRIPT: &str = include_str!("hooks_post_compact.py");
const SESSION_END_SCRIPT: &str = include_str!("hooks_session_end.py");
const SESSION_START_SCRIPT: &str = include_str!("hooks_session_start.py");
const USER_PROMPT_SUBMIT_SCRIPT: &str = include_str!("hooks_user_prompt_submit.py");
const PRE_TOOL_USE_SCRIPT: &str = include_str!("hooks_pre_tool_use.py");

/// The text-to-speech hook (vendored from the `revoiced` project). `speak.py`
/// imports `duck`, `focus` and `notify` as siblings and shells out to
/// `win_audio.ps1` next to it, so all five land in `<env>/hooks/` together.
/// Vendoring them per-env is what removes the absolute path to a checkout;
/// their shared state (voice pool, leases, mute) lives in a machine-wide data
/// dir, so every env still queues behind one playback lock.
///
/// `focus` and `notify` used to be left out, because `speak.py` guards those
/// two imports and a partial copy still speaks. It does — silently, with
/// desktop notifications off in every env, which is how they were broken for
/// two hours without anything saying so. Guarded does not mean optional: vendor
/// all five.
const SPEAK_SCRIPT: &str = include_str!("hooks_speak.py");
const DUCK_SCRIPT: &str = include_str!("hooks_duck.py");
const FOCUS_SCRIPT: &str = include_str!("hooks_focus.py");
const NOTIFY_SCRIPT: &str = include_str!("hooks_notify.py");
const WIN_AUDIO_SCRIPT: &str = include_str!("hooks_win_audio.ps1");

/// The `pre-commit` hook seeded into every `github` project — see
/// [`seed_pre_commit_hook`] for why it exists and how it is kept current.
const PRE_COMMIT_HOOK: &str = include_str!("pre_commit_hook.sh");
/// Bump together with the `aello-pre-commit v<N>` marker inside the script; a
/// placed copy below this is replaced on the next placement. A test pins them
/// to each other, since a widened pattern that never reaches an already
/// scaffolded project is a guard that only looks deployed.
const PRE_COMMIT_VERSION: u32 = 1;

/// The `HOOK_VERSION` of the vendored copy above. Upstream bumps its constant
/// whenever one of the five hook-path files changes, so comparing the two is
/// how this copy learns it has fallen behind — see the test at the bottom of
/// this file, which fails if a re-vendor moves the scripts without moving this.
///
/// **Including changes that never run on the hook path.** 19 was a `--status`
/// fix and by the letter of upstream's rule could have stayed at 18; the digest
/// test here covers all five files byte-for-byte and cannot tell which half of a
/// file moved, so an unbumped change fails it with no version to explain the
/// mismatch. Asked and settled with revoiced on 2026-08-06: bump on any change
/// to the five, and the version means "this exact set of bytes", not "the hook
/// path changed".
///
/// A version, not a commit sha: revoiced's CI commits a `VERSION` bump on every
/// push to main, so local work rebases onto that and every unpushed sha is
/// rewritten. A recorded sha goes stale by itself; a recorded version cannot.
/// Surfaced by `aello voice status`, so checking a machine does not mean
/// finding an env dir and running Python in it.
pub const HOOK_VERSION: u32 = 24;

/// Starter memory seeded on first placement so a fresh env boots with the
/// user's working-style note already loaded in `/context`. The body is bundled;
/// `MEMORY.md` is a one-line index pointing at it.
const MEMORY_WORKING_STYLE: &str = include_str!("../templates/memory-working-style.md");
const MEMORY_INDEX: &str =
    "- [working style](working-style.md) — user does not read plans, give decisions to choose from\n";

/// Stack-agnostic CI workflow seeded for `github` blueprints. On every push to
/// `main` it bumps the patch in a plain `VERSION` file and commits it back with
/// `[skip ci]` — a `GITHUB_TOKEN` push does not re-trigger workflows, so there's
/// no loop. Mirrors aello's own release lessons; deliberately tied to no build
/// system, so it drops into any project.
const VERSION_WORKFLOW: &str = r#"name: version

# Auto-bump the patch in VERSION on every push to main, then commit it back with
# [skip ci] so the bump commit does not re-trigger this workflow (GITHUB_TOKEN
# pushes never do). Seeded by aello — stack-agnostic; VERSION is a plain x.y.z
# file. Bump minor/major by hand in VERSION for bigger releases.
on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: write

jobs:
  bump:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: main
      - name: bump patch in VERSION
        run: |
          cur=$(cat VERSION 2>/dev/null || echo 0.0.0)
          IFS=. read -r MA MI PA <<< "$cur"
          new="$MA.$MI.$((PA + 1))"
          echo "$new" > VERSION
          echo "bumped $cur -> $new"
          git config user.name  "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git commit -am "release: v$new [skip ci]"
          git push origin main
"#;

/// Test + dependency-audit CI, seeded for `github` blueprints beside
/// `version.yml`.
///
/// Why aello seeds this rather than leaving it to each project: a repo that grew
/// organically has its tests and its audits running **only where somebody
/// remembers to type them**, which is the developer's desktop and never the
/// server. Measured in one 14-month-old project on 2026-08-11, on the first two
/// runs after CI was added: one suite was silently collecting **384 of 420
/// tests** because a test-only dependency was undeclared and the import raised
/// instead of degrading — and the suite still printed OK — and four more tests
/// only passed by reading a *gitignored* file that happened to exist on that one
/// machine, with a skip guard that checked the wrong verdict and so never fired.
/// Neither is exotic; both are invisible without a second machine running the
/// suite, and CI is the cheapest second machine.
///
/// **Stack-agnostic the same way `version.yml` is** — the file is generic and the
/// *detection happens at run time*, because aello does not know a project's
/// ecosystem at placement and guessing wrong seeds a workflow that fails forever.
/// A repo with neither Python nor Node manifests runs the job and reports that it
/// found nothing to do, which is a true answer rather than a green tick earned by
/// skipping.
///
/// The audit **fails the build**. A non-blocking audit is a guard that never
/// fires, which is the shape this codebase spends most of its guards preventing.
/// Delete the file in a repo where that is not wanted.
const CI_WORKFLOW: &str = r#"name: ci

# Run this project's tests and audit its dependencies on every push and PR.
# Seeded by aello. Deliberately stack-agnostic: nothing is detected at seed time,
# because the project's ecosystem is not known then and a wrong guess is a
# workflow that fails forever. Detection happens below, at run time.
#
# The audit FAILS the build on a known vulnerability. That is the point — an
# advisory that only prints is one nobody reads. Delete this file if a repo does
# not want it.
on:
  push:
  pull_request:
  workflow_dispatch:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: detect ecosystem
        id: detect
        run: |
          if [ -f pyproject.toml ] || [ -f requirements.txt ] || [ -f uv.lock ]; then
            echo "python=1" >> "$GITHUB_OUTPUT"
          fi
          if [ -f package.json ]; then
            echo "node=1" >> "$GITHUB_OUTPUT"
          fi
          if [ -f Cargo.toml ]; then
            echo "rust=1" >> "$GITHUB_OUTPUT"
          fi

      # --- Python ---------------------------------------------------------
      # Pin this to the interpreter the project is DEPLOYED on, not the one the
      # developer happens to have. A lock compiled against 3.12 and installed on
      # 3.11 can resolve differently, which makes CI green and the server wrong.
      - uses: actions/setup-python@v5
        if: steps.detect.outputs.python
        with:
          python-version: '3.12'
      - name: install (python)
        if: steps.detect.outputs.python
        run: |
          python -m pip install --upgrade pip
          # Install from the LOCK where there is one. A deploy must never
          # re-resolve, and CI is only telling the truth if it installs what a
          # deploy would.
          if [ -f requirements.txt ]; then
            pip install -r requirements.txt
          elif [ -f pyproject.toml ]; then
            pip install -e .
          fi
          pip install pytest pip-audit
      - name: test (python)
        if: steps.detect.outputs.python
        run: |
          if ls test_*.py tests/ */tests/ >/dev/null 2>&1; then
            pytest -q
          else
            echo "no pytest targets found"
          fi
      - name: audit (python)
        if: steps.detect.outputs.python
        run: pip-audit --strict

      # --- Node -----------------------------------------------------------
      - uses: actions/setup-node@v4
        if: steps.detect.outputs.node
        with:
          node-version: '22'
      - name: install (node)
        if: steps.detect.outputs.node
        run: |
          # `npm ci` requires a committed lockfile and installs it exactly.
          # Its absence is the finding, so say so rather than falling back to
          # `npm install`, which resolves fresh and hides the gap.
          if [ -f package-lock.json ]; then
            npm ci
          else
            echo "::error::package.json with no committed package-lock.json — nothing can reproduce this install"
            exit 1
          fi
      - name: test (node)
        if: steps.detect.outputs.node
        run: npm test --if-present
      - name: audit (node)
        if: steps.detect.outputs.node
        run: npm audit --audit-level=high

      # --- Rust -----------------------------------------------------------
      - name: test (rust)
        if: steps.detect.outputs.rust
        # No --locked: a project whose CI commits a version bump without
        # touching Cargo.lock has a lockfile permanently one behind, and
        # --locked then fails every run on a meaningless mismatch.
        run: cargo test
      - name: audit (rust)
        if: steps.detect.outputs.rust
        run: |
          cargo install cargo-audit --locked >/dev/null 2>&1 || cargo install cargo-audit --locked
          cargo audit

      - name: nothing to do
        if: '!steps.detect.outputs.python && !steps.detect.outputs.node && !steps.detect.outputs.rust'
        run: echo "no Python, Node or Rust manifest found — no tests or audit to run"
"#;

/// Renovate config seeded for `github` blueprints.
///
/// Renovate detects ecosystems itself, so unlike the CI workflow this one is
/// genuinely generic. The policy encoded here is the one decided once so that
/// agents stop re-deriving it per project:
///
/// * grouped minor/patch weekly, majors always on their own PR;
/// * security updates any time, ignoring the schedule;
/// * **nothing automerges** — a dependency bump that lands unattended on a system
///   holding real money is not a convenience;
/// * it edits the **manifest**, never a generated lockfile.
///
/// Installing the GitHub App is a manual step aello cannot do, and the free
/// product is "Renovate" — mend.io's wizard offers "Mend Application Security"
/// first, which needs a paid licence. `place` says so on stdout rather than
/// reporting this configured.
const RENOVATE_JSON: &str = r#"{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": ["config:recommended"],
  "timezone": "UTC",
  "schedule": ["before 6am on monday"],
  "packageRules": [
    {
      "description": "Minor and patch together, once a week — one PR to review, not thirty.",
      "matchUpdateTypes": ["minor", "patch"],
      "groupName": "minor + patch"
    },
    {
      "description": "A major is a breaking change and gets read on its own.",
      "matchUpdateTypes": ["major"],
      "groupName": null
    }
  ],
  "vulnerabilityAlerts": {
    "enabled": true,
    "schedule": null,
    "labels": ["security"]
  },
  "automerge": false,
  "dependencyDashboard": true,
  "prConcurrentLimit": 5
}
"#;

/// Env dir for a blueprint inside a project — `project/.claude-env-<name>`.
pub fn env_dir(project: &Path, name: &str) -> PathBuf {
    project.join(format!("{}{name}", crate::models::Agent::Claude.env_prefix()))
}

/// This env's memory dir for this project. The `<encoded-cwd>` component is
/// derived from the project path, so the same env restored onto a second machine
/// — a different absolute path, quite possibly a different OS — resolves to that
/// machine's spelling rather than the one baked into the mirror.
pub fn memory_dir(env_dir: &Path, project: &Path) -> PathBuf {
    env_dir
        .join("projects")
        .join(crate::sessions::encode_project_path(project))
        .join("memory")
}

/// Where the tracked mirror of a blueprint's env lives.
///
/// `root` is [`Instance::mirror_root`] — normally `None`, meaning the project's
/// own `claude-internal/`. When set it points at a working tree of some *other*
/// repo, which is how a public project keeps its product public and its memory
/// private. The `<blueprint>/` component is appended either way, so one
/// destination can hold several blueprints without them clobbering each other.
///
/// Resolved on every call rather than stored: the destination is a path on this
/// machine and the second machine spells it differently, exactly like
/// [`memory_dir`]'s `<encoded-cwd>`.
pub fn mirror_dir(project: &Path, blueprint: &str, root: Option<&str>) -> PathBuf {
    match root {
        Some(r) => crate::config::expand_home(r).join(blueprint),
        None => project.join("claude-internal").join(blueprint),
    }
}

/// The mirror's snapshot of the resume note. Deliberately **not** named
/// `<blueprint>.HANDOFF.md`: this repo (and any repo that adds the pattern)
/// gitignores `*.HANDOFF.md`, and the whole point of the snapshot is to be
/// committed so the note reaches another machine.
pub const MIRRORED_HANDOFF: &str = "handoff.md";

/// The live resume note the SessionStart hook reads and deletes, at the project
/// root.
pub fn handoff_path(project: &Path, blueprint: &str) -> PathBuf {
    project.join(format!("{blueprint}.HANDOFF.md"))
}

/// Move a placed blueprint's on-disk artifacts when it's renamed: the env dir
/// `<prefix><old>` → `<prefix><new>` (whichever agent's prefix that is), the
/// `name` in its `.aello.toml`,
/// and the tracked `claude-internal/<old>/` mirror → `<new>/`. Returns true when
/// the env dir was present (i.e. the blueprint is placed in this project);
/// false is a clean no-op for a blueprint that isn't placed here. Errors if a
/// destination already exists, so a rename never clobbers another env. Skills
/// and mirror content that embed the old name are refreshed on the next `run`.
pub fn rename_placed(
    project: &Path,
    agent: crate::models::Agent,
    old: &str,
    new: &str,
) -> Result<bool> {
    let old_env = agent.env_dir(project, old);
    if !old_env.exists() {
        return Ok(false);
    }
    let new_env = agent.env_dir(project, new);
    let old_mirror = project.join("claude-internal").join(old);
    let new_mirror = project.join("claude-internal").join(new);

    // Pre-check BOTH destinations before moving anything. Previously the env dir
    // was renamed first and the mirror collision was only caught afterwards, so
    // a `bail!` there left the env dir already moved while cmd_edit's `?` skipped
    // config::save — config still said <old>, disk said <new>, and `run <old>`
    // then re-scaffolded a fresh env, orphaning the renamed one.
    //
    // A pure case-flip (`coder` → `Coder`) is exempt: on Windows and macOS the
    // default filesystem is case-insensitive, so `.claude-env-Coder`.exists() is
    // true because it *is* `.claude-env-coder` — the source was being reported as
    // the obstruction, and `aello edit coder --rename Coder` could never run on
    // the two platforms aello ships binaries for.
    let case_flip = old.eq_ignore_ascii_case(new);
    if !case_flip && new_env.exists() {
        bail!("{} already exists — cannot rename", new_env.display());
    }
    if !case_flip && old_mirror.exists() && new_mirror.exists() {
        bail!("{} already exists — cannot rename", new_mirror.display());
    }

    rename_dir(&old_env, &new_env, case_flip)
        .with_context(|| format!("could not move env dir to {}", new_env.display()))?;

    // Move the tracked mirror too, when the github cap seeded one. On any
    // failure here, roll the env-dir move back so disk and config stay in sync.
    if old_mirror.exists() {
        if let Err(e) = rename_dir(&old_mirror, &new_mirror, case_flip) {
            let _ = rename_dir(&new_env, &old_env, case_flip);
            return Err(e).with_context(|| format!("could not move mirror to {}", new_mirror.display()));
        }
    }

    // The transient root-level files are addressed by blueprint name, and both
    // consumers key strictly off the new one — the SessionStart/SessionEnd hooks
    // derive it from CLAUDE_CONFIG_DIR, and /handoff is told to write exactly
    // `<name>.HANDOFF.md`. Left behind, a pending resume note or cross-env inbox
    // would be addressed to a name that no longer exists, with no producer and no
    // reader. Best-effort: never clobber another env's file, and never fail the
    // rename over one.
    for suffix in ["HANDOFF.md", "NOTE.md"] {
        let from = project.join(format!("{old}.{suffix}"));
        let to = project.join(format!("{new}.{suffix}"));
        if from.exists() && (case_flip || !to.exists()) {
            let _ = std::fs::rename(&from, &to);
        }
    }

    // Both dirs moved — point the placed instance at the new name so it launches
    // under it. (Done last; the moves are the part that must stay consistent.)
    if let Some(mut inst) = load_instance(&new_env) {
        inst.name = new.to_string();
        std::fs::write(new_env.join(".aello.toml"), toml::to_string_pretty(&inst)?)
            .context("could not update .aello.toml after rename")?;
    }
    Ok(true)
}

/// Rename a directory, routing a case-only change through a temp name.
///
/// `fs::rename` to a name differing only in case is a no-op (or an error) on a
/// case-insensitive filesystem — source and destination are the same directory —
/// so the two-step is the portable way to actually change the case on disk.
fn rename_dir(from: &Path, to: &Path, case_flip: bool) -> std::io::Result<()> {
    if !case_flip {
        return std::fs::rename(from, to);
    }
    let mut tmp = from.to_path_buf();
    tmp.set_file_name(format!(
        ".aello-rename-{}-{}",
        std::process::id(),
        from.file_name().and_then(|n| n.to_str()).unwrap_or("env")
    ));
    std::fs::rename(from, &tmp)?;
    std::fs::rename(&tmp, to).inspect_err(|_| {
        // Never leave the directory parked under the temp name.
        let _ = std::fs::rename(&tmp, from);
    })
}

/// Read a file that may legitimately not exist yet: `Ok(None)` only for
/// `NotFound`, every other IO error propagates.
///
/// `read_to_string(...).unwrap_or_default()` is the shape this replaces, and it
/// is a data-loss bug wherever the caller writes the result back. `read_to_string`
/// also fails on any non-UTF-8 byte and on a Windows sharing violation — a
/// `.gitignore` Notepad saved as UTF-16, a `CLAUDE.md` with one Latin-1 byte, a
/// `.claude.json` open in another process — and the default turns "I could not
/// read your file" into "your file was empty", which the very next line then
/// makes true. Same distinction `config::load` draws, generalized: the July
/// audit got it fixed there and the pattern survived in three more places.
fn read_existing(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

/// Mark the env as onboarded so interactive `claude` skips its first-run
/// wizard (theme/login) and goes straight in — auth is handled by the shared
/// token. Merges `hasCompletedOnboarding: true` into `.claude.json`.
pub fn mark_onboarded(env_dir: &Path) -> Result<()> {
    let path = env_dir.join(".claude.json");
    let mut v: serde_json::Value = match read_existing(&path)? {
        // Claude Code owns this file too; if it exists but we can't parse it,
        // leave it untouched rather than overwrite real state with `{}`.
        Some(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        },
        None => serde_json::json!({}),
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert("hasCompletedOnboarding".into(), serde_json::Value::Bool(true));
    }
    std::fs::write(&path, serde_json::to_string_pretty(&v)?)
        .context("could not write .claude.json")
}

pub fn load_instance(env_dir: &Path) -> Option<Instance> {
    let text = std::fs::read_to_string(env_dir.join(".aello.toml")).ok()?;
    toml::from_str(&text).ok()
}

/// Place an instance into its env dir: write `.aello.toml`, and seed
/// `settings.json`, CLAUDE.md, and the PostCompact hook if absent. Then, from
/// `caps`, regenerate the `/sync` skill and scaffold the project files this
/// blueprint maintains.
pub fn place(
    env_dir: &Path,
    inst: &Instance,
    claude_md: Option<&str>,
    caps: &Capabilities,
) -> Result<()> {
    // Before anything is seeded: a tracked mirror with no env dir beside it is a
    // fresh clone, and the mirror is the only copy of this blueprint's skills,
    // memory and persona on the machine.
    restore_from_mirror(env_dir, &inst.name, inst.mirror_root.as_deref())?;

    std::fs::create_dir_all(env_dir).context("could not create env dir")?;

    std::fs::write(env_dir.join(".aello.toml"), toml::to_string_pretty(inst)?)
        .context("could not write .aello.toml")?;

    let settings = env_dir.join("settings.json");
    if !settings.exists() {
        std::fs::write(&settings, settings_json(&inst.model))
            .context("could not write settings.json")?;
    } else {
        // Existing env: never clobber a (possibly user-edited) settings.json, but
        // self-heal the SessionEnd hook into it so envs placed before it existed
        // start capturing /clear + exit sessions.
        ensure_own_hook(&settings, "SessionEnd", "session-end.py")?;
        ensure_own_hook(&settings, "SessionStart", "session-start.py")?;
        // The per-turn response rules. Registered before ensure_tldr_instruction
        // runs, so an env adopting the hook stops getting the persona append on
        // the same placement rather than one run later.
        ensure_own_hook(&settings, "UserPromptSubmit", "user-prompt-submit.py")?;
        // The other half of the no-plans rule: the text above asks, this denies.
        ensure_own_hook_matching(
            &settings,
            "PreToolUse",
            "pre-tool-use.py",
            Some(PLAN_TOOLS),
        )?;
        // Same for the voice hook, so an env placed before voice was universal
        // starts speaking on its next run.
        sync_voice_hooks(&settings)?;
        // And the model, so `aello edit <name> --model` actually reaches the env.
        ensure_model(&settings, &inst.model)?;
        // Stop Claude Code deleting the transcripts contextdb points at.
        ensure_cleanup_period(&settings)?;
        // The in-session usage readout, so an env placed before it existed
        // starts showing the plan limits and its own token spend.
        ensure_statusline(&settings)?;
    }

    // Global persona — set once, never clobbered (the user may have edited it).
    if let Some(content) = claude_md {
        let path = env_dir.join("CLAUDE.md");
        if !path.exists() {
            std::fs::write(&path, content).context("could not write CLAUDE.md")?;
        }
    }

    // Fallback only: the bundled UserPromptSubmit hook now carries the TL;DR
    // instruction, so this appends nothing unless that hook was unregistered by
    // hand. Appends (never clobbers) so an existing persona keeps its text.
    ensure_tldr_instruction(env_dir)?;

    // Always refresh the hook script so updates (e.g. AELLO_CONTEXTDB support)
    // propagate to existing envs on the next run.
    std::fs::create_dir_all(env_dir.join("hooks")).context("could not create hooks dir")?;
    std::fs::write(env_dir.join("hooks").join("post-compact.py"), POST_COMPACT_SCRIPT)
        .context("could not write post-compact.py")?;
    std::fs::write(env_dir.join("hooks").join("session-end.py"), SESSION_END_SCRIPT)
        .context("could not write session-end.py")?;
    std::fs::write(env_dir.join("hooks").join("session-start.py"), SESSION_START_SCRIPT)
        .context("could not write session-start.py")?;
    std::fs::write(
        env_dir.join("hooks").join("user-prompt-submit.py"),
        USER_PROMPT_SUBMIT_SCRIPT,
    )
    .context("could not write user-prompt-submit.py")?;
    std::fs::write(env_dir.join("hooks").join("pre-tool-use.py"), PRE_TOOL_USE_SCRIPT)
        .context("could not write pre-tool-use.py")?;

    // Voice hook + its four siblings, refreshed like the others so fixes reach
    // existing envs. All five, always: a copy missing `notify.py` speaks but
    // never raises a desktop notification, and says nothing about it.
    let hooks = env_dir.join("hooks");
    std::fs::write(hooks.join("speak.py"), SPEAK_SCRIPT)
        .context("could not write speak.py")?;
    std::fs::write(hooks.join("duck.py"), DUCK_SCRIPT)
        .context("could not write duck.py")?;
    std::fs::write(hooks.join("focus.py"), FOCUS_SCRIPT)
        .context("could not write focus.py")?;
    std::fs::write(hooks.join("notify.py"), NOTIFY_SCRIPT)
        .context("could not write notify.py")?;
    std::fs::write(hooks.join("win_audio.ps1"), WIN_AUDIO_SCRIPT)
        .context("could not write win_audio.ps1")?;

    // Regenerate the tailored /sync skill from current caps (or remove it if the
    // blueprint no longer maintains anything). A kept skill is left alone
    // entirely — including the removal, since a hand-written /sync is not stale
    // just because the blueprint stopped maintaining docs.
    let skill = env_dir.join("skills").join("sync").join("SKILL.md");
    if skill_kept(skill.parent().unwrap()) {
        // hand-edited: neither regenerated nor removed
    } else if caps.any() {
        std::fs::create_dir_all(skill.parent().unwrap())
            .context("could not create skills dir")?;
        std::fs::write(&skill, crate::templates::render_sync_skill(caps, &inst.name, inst.mirror_root.as_deref()))
            .context("could not write sync SKILL.md")?;
    } else if let Err(e) = std::fs::remove_file(&skill) {
        // A stale /sync skill left behind (blueprint dropped all caps) would keep
        // getting re-mirrored and committed, so a real removal failure must
        // surface — only a concurrent already-gone is fine to ignore.
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e).with_context(|| format!("could not remove stale {}", skill.display()));
        }
    }

    // Always seed the /handoff skill — unlike /sync it is universal (every
    // blueprint, regardless of caps), since a clean resume note helps even a
    // blueprint that maintains no docs.
    seed_skill(env_dir, "handoff", crate::templates::render_handoff_skill(&inst.name))?;

    // Always seed the /note skill — universal too: leave a note for another
    // environment sharing this repo (distinct from /handoff, a note to self).
    seed_skill(env_dir, "note", crate::templates::render_note_skill(&inst.name))?;

    // Always seed the /twosentences skill — also universal (role-independent):
    // condense the previous response into two sentences.
    seed_skill(env_dir, "twosentences", crate::templates::render_twosentences_skill())?;

    let project = env_dir.parent().unwrap_or(env_dir);

    // Seed a starter memory on first placement (never clobbers existing memory).
    // Done before scaffolding so the claude-internal mirror captures it.
    seed_memory(env_dir, project)?;

    // Scaffold the project-dir files this blueprint maintains (only if missing),
    // and mirror this env's internal config into the tracked claude-internal/.
    scaffold_project(project, env_dir, &inst.name, caps, inst.mirror_root.as_deref())?;

    Ok(())
}

/// The opt-out marker: `<env>/skills/<name>/.aello-keep` means "this skill was
/// hand-edited for this project — don't regenerate it".
///
/// Every other seeded skill is rewritten on each `place`, which is what makes a
/// capability change reach an existing env. That is the right default, but it
/// silently discarded a project-specific rewrite of `/sync`, and the mirror then
/// carried the generated version over the custom one in git. The marker lives
/// beside the skill it protects (not in `config.toml`) so it travels with the
/// env dir and is visible where the editing happens.
pub const KEEP_MARKER: &str = ".aello-keep";

fn skill_kept(skill_dir: &Path) -> bool {
    skill_dir.join(KEEP_MARKER).exists()
}

/// Seed one of the universal skills, unless it has been marked kept.
fn seed_skill(env_dir: &Path, name: &str, content: String) -> Result<()> {
    let dir = env_dir.join("skills").join(name);
    if skill_kept(&dir) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create {name} skills dir"))?;
    std::fs::write(dir.join("SKILL.md"), content)
        .with_context(|| format!("could not write {name} SKILL.md"))
}

/// Seed the env's starter memory so a freshly placed env loads the user's
/// working-style note into `/context` from the first run. Claude reads memory
/// from `<CLAUDE_CONFIG_DIR>/projects/<encoded-cwd>/memory/`, the same path
/// encoding `sessions` uses. Written only when there is no `MEMORY.md` yet, so
/// a re-place over an established memory leaves the user's notes untouched.
fn seed_memory(env_dir: &Path, project: &Path) -> Result<()> {
    let mem = env_dir
        .join("projects")
        .join(crate::sessions::encode_project_path(project))
        .join("memory");
    let index = mem.join("MEMORY.md");
    if index.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(&mem).context("could not create memory dir")?;
    let ws = mem.join("working-style.md");
    if !ws.exists() {
        std::fs::write(&ws, MEMORY_WORKING_STYLE)
            .context("could not write working-style memory")?;
    }
    std::fs::write(&index, MEMORY_INDEX).context("could not write MEMORY.md")?;
    Ok(())
}

/// Create the docs the enabled capabilities expect, only when absent — so a
/// fresh project gets its CHANGELOG/README/docs/CLAUDE.md, and existing files
/// are left untouched. The `github` cap additionally seeds release hygiene and
/// the tracked `claude-internal/` mirror of this env's internal config.
fn scaffold_project(
    project: &Path,
    env_dir: &Path,
    blueprint: &str,
    caps: &Capabilities,
    mirror_root: Option<&str>,
) -> Result<()> {
    let name = project
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    if caps.changelog {
        let p = project.join("CHANGELOG.md");
        if !p.exists() {
            std::fs::write(&p, "# Changelog\n\n## [Unreleased]\n")
                .context("could not write CHANGELOG.md")?;
        }
    }
    if caps.readme {
        let p = project.join("README.md");
        if !p.exists() {
            std::fs::write(&p, format!("# {name}\n")).context("could not write README.md")?;
        }
    }
    if caps.docs {
        std::fs::create_dir_all(project.join("docs")).context("could not create docs dir")?;
    }
    if caps.project_md {
        let p = project.join("CLAUDE.md");
        if !p.exists() {
            std::fs::write(&p, format!("# {name}\n\nProject-specific instructions for Claude.\n"))
                .context("could not write project CLAUDE.md")?;
        }
    }
    // Keep env dirs out of the repo — unconditional, exactly as the Cline side
    // is. It was gated on `github` as a tidiness measure, on the premise that a
    // Claude env holds no secret because auth arrives as an env var at launch.
    // That premise is false whenever no shared token is configured: Claude Code
    // then writes its own `.credentials.json` into the env dir (`run_blueprint`
    // probes for exactly that file), and `standalone` — the *default* role — has
    // `github: false`, so the one line that would have kept it out of a
    // `git add -A` was never written. The line costs nothing in a project with
    // no git, and a blueprint with no git duties still shares the working tree
    // with one that has them.
    ensure_gitignore_entry(project, crate::models::Agent::Claude.gitignore_pattern())?;
    if caps.github {
        // Normalize line endings so multi-OS blueprints sharing a repo don't
        // churn CRLF/LF on every commit.
        let ga = project.join(".gitattributes");
        if !ga.exists() {
            std::fs::write(&ga, "* text=auto\n").context("could not write .gitattributes")?;
        }
        // Seed a stack-agnostic VERSION + patch-bump CI workflow for the target
        // project (mirrors aello's own release machinery, build-system agnostic).
        let ver = project.join("VERSION");
        if !ver.exists() {
            std::fs::write(&ver, "0.1.0\n").context("could not write VERSION")?;
        }
        let wf = project.join(".github").join("workflows").join("version.yml");
        if !wf.exists() {
            std::fs::create_dir_all(wf.parent().unwrap())
                .context("could not create .github/workflows dir")?;
            std::fs::write(&wf, VERSION_WORKFLOW).context("could not write version.yml")?;
        }
        // Test + audit CI and a Renovate policy, beside version.yml. Both
        // written only when absent: a project that has tuned its own must keep
        // it, and unlike a skill these are not regenerated from a role.
        let ci = project.join(".github").join("workflows").join("ci.yml");
        if !ci.exists() {
            std::fs::create_dir_all(ci.parent().unwrap())
                .context("could not create .github/workflows dir")?;
            std::fs::write(&ci, CI_WORKFLOW).context("could not write ci.yml")?;
        }
        let rn = project.join(".github").join("renovate.json");
        if !rn.exists() {
            std::fs::create_dir_all(rn.parent().unwrap())
                .context("could not create .github dir")?;
            std::fs::write(&rn, RENOVATE_JSON).context("could not write renovate.json")?;
            // Said once, on the placement that seeds it. Renovate does nothing
            // at all until the GitHub App is installed, and aello cannot do
            // that — reporting the file as "configured" would be a lie of the
            // kind this codebase keeps having to undo.
            println!(
                "note: seeded .github/renovate.json — it does nothing until you install the \
                 Renovate GitHub App (github.com/apps/renovate). Pick \"Renovate\", not \
                 \"Mend Application Security\", which needs a paid licence."
            );
        }
        seed_pre_commit_hook(project)?;
    }
    // Seed the tracked claude-internal/ mirror so the env's skills, memory, and
    // persona are version-controlled from the first commit. Deliberately NOT
    // added to the .claude-env-* gitignore line — this folder is tracked.
    //
    // Runs outside the `github` gate on purpose. Inside it, dropping the cap
    // froze the folder in git forever: the old github-flavoured /sync skill (git
    // sections, Bash tool) stayed committed and the memory/persona snapshots
    // stopped tracking the env, with `remove --purge` the only way to clear it.
    // With the cap off there is nothing to mirror *into* git, so the pass runs in
    // prune-only mode and clears what a previous github placement left behind.
    mirror_env_internal(project, env_dir, blueprint, caps.github, mirror_root)?;
    Ok(())
}

/// One-way mirror of this env's internal config into the project-tracked
/// `claude-internal/<blueprint>/` folder, so the skills, memory, and persona
/// that live in the gitignored env dir are captured in git. The live env dir
/// stays the single source of truth; this only copies from it. The persona
/// snapshot is renamed to `persona.CLAUDE.md` so Claude Code never auto-loads it
/// as a second persona. Namespacing per blueprint keeps multi-blueprint repos
/// from clobbering each other's mirror.
///
/// With `track` false (the `github` cap is off) the whole folder is removed
/// instead: nothing here is committed, so a mirror left from an earlier github
/// placement is stale content that only `remove --purge` could otherwise clear.
fn mirror_env_internal(
    project: &Path,
    env_dir: &Path,
    blueprint: &str,
    track: bool,
    root: Option<&str>,
) -> Result<()> {
    let dest = mirror_dir(project, blueprint, root);
    if !track {
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .context("could not prune the claude-internal mirror")?;
        }
        // Leave `claude-internal/` itself alone — another blueprint in this repo
        // may still be mirroring into its own namespaced subfolder.
        return Ok(());
    }
    copy_dir_all(&env_dir.join("skills"), &dest.join("skills"))
        .context("could not mirror skills into claude-internal")?;
    // Memory is a union, not a mirror — see `merge_dir`. The extras it reports are
    // notes another machine committed and this env has never seen, which is
    // exactly the state `aello restore` exists to resolve; the alternative was
    // deleting them here, on a launch, with nothing printed.
    let orphaned = merge_dir(&memory_dir(env_dir, project), &dest.join("memory"))
        .context("could not mirror memory into claude-internal")?;
    if !orphaned.is_empty() {
        println!(
            "note: {} memory note(s) in {}/memory/ are not in this env:",
            orphaned.len(),
            dest.display()
        );
        for name in &orphaned {
            println!("  {name}");
        }
        println!("  `aello restore {blueprint}` to adopt them, or `git rm` one to drop it.");
    }
    let persona = env_dir.join("CLAUDE.md");
    if persona.exists() {
        std::fs::create_dir_all(&dest).context("could not create claude-internal dir")?;
        std::fs::copy(&persona, dest.join("persona.CLAUDE.md"))
            .context("could not snapshot persona into claude-internal")?;
    }
    // Snapshot the resume note so it can reach another machine. The root file is
    // still the live one — written by `/handoff`, read and deleted on the next
    // boot — and is still never committed itself; this copy is, under a name the
    // `*.HANDOFF.md` ignore rule does not match. Absent means "not written this
    // session", never "delete the snapshot": the last note stands until a new one
    // replaces it, the way the persona snapshot does.
    let handoff = handoff_path(project, blueprint);
    if handoff.exists() {
        std::fs::create_dir_all(&dest).context("could not create claude-internal dir")?;
        std::fs::copy(&handoff, dest.join(MIRRORED_HANDOFF))
            .context("could not snapshot the handoff into claude-internal")?;
    }
    Ok(())
}

/// Seed a missing env dir from the tracked `claude-internal/<blueprint>/` mirror.
///
/// The mirror is tracked precisely so the skills, memory and persona that live
/// in a gitignored directory survive a clone — but nothing read it back, so the
/// first `aello run` on a fresh clone seeded a bare env and then mirrored *that*
/// over the tracked copy, deleting it. (Measured on this repository: 11 tracked
/// memory notes and 6 skills against 2 and 4 seeded.) The mirror is a snapshot
/// of the env everywhere else; this is the one direction that reads it, and only
/// when there is no env to contradict it.
///
/// Everything below still gets rewritten by the rest of `place` — a regenerated
/// `/sync`, a persona the config would seed — so this restores what nothing else
/// can: memory, hand-kept skills, and a `custom` persona.
fn restore_from_mirror(env_dir: &Path, blueprint: &str, root: Option<&str>) -> Result<()> {
    if env_dir.exists() {
        return Ok(());
    }
    let project = env_dir.parent().unwrap_or(env_dir);
    let src = mirror_dir(project, blueprint, root);
    if !src.exists() {
        return Ok(());
    }

    // `copy_dir_all` prunes, but the destinations here do not exist yet, so it
    // is a plain copy — and its symlink skipping is wanted either way.
    copy_dir_all(&src.join("skills"), &env_dir.join("skills"))
        .context("could not restore skills from claude-internal")?;
    copy_dir_all(&src.join("memory"), &memory_dir(env_dir, project))
        .context("could not restore memory from claude-internal")?;
    let persona = src.join("persona.CLAUDE.md");
    if persona.exists() {
        std::fs::create_dir_all(env_dir).context("could not create env dir")?;
        std::fs::copy(&persona, env_dir.join("CLAUDE.md"))
            .context("could not restore the persona from claude-internal")?;
    }
    // The resume note the other machine left. Nothing local can conflict — there
    // is no env dir here yet, so there has never been a session to write one.
    let handoff = src.join(MIRRORED_HANDOFF);
    if handoff.exists() {
        std::fs::copy(&handoff, handoff_path(project, blueprint))
            .context("could not restore the handoff from claude-internal")?;
    }
    println!("Restored '{blueprint}' from claude-internal/ (no env dir here yet).");
    Ok(())
}

/// What `restore` did, so the caller can print it without re-reading the disk.
#[derive(Debug)]
pub struct Restored {
    pub memory: usize,
    pub skills: usize,
    pub handoff: bool,
    /// The env's persona differs from the mirror's snapshot and was left alone.
    pub persona_differs: bool,
}

/// Adopt the tracked mirror into an env dir **that already exists** — the inbound
/// half of working one env across two machines.
///
/// `restore_from_mirror` covers only the fresh-clone case, and deliberately: it
/// must never contradict a live env. But that leaves the return trip broken. Come
/// home to the machine that already has the env dir, pull the notes the other
/// device committed, and nothing reads them — then `place` mirrors the local env
/// back over them. Before memory became a union that silently deleted them; now
/// it strands them. Either way the user needs one command that says "the mirror
/// has moved on, take it".
///
/// **Nothing here overwrites local work.** Memory and skills are unions, so a note
/// this machine holds and the mirror does not survives. The persona is reported
/// and left alone — it is the file aello never clobbers, and `aello persona` is
/// the command that exists to replace one deliberately. The one exception is the
/// resume note, which is *replaced*: the local root file is by then a note this
/// machine already snapshotted into the mirror and committed, so it is recoverable
/// from git history, while the mirror's copy is the one just pulled — the whole
/// reason for running this.
pub fn restore(
    project: &Path,
    env_dir: &Path,
    blueprint: &str,
    mirror_root: Option<&str>,
) -> Result<Restored> {
    let src = mirror_dir(project, blueprint, mirror_root);
    if !src.exists() {
        anyhow::bail!(
            "no mirror at {} — nothing to restore from. It is written by the `github` \
             role's /sync step, so a standalone blueprint has none.",
            src.display()
        );
    }

    // The extras each returns are files this env has and the mirror does not —
    // local work that has not been synced yet. Not this command's business; the
    // point is only that a union never deletes them.
    merge_dir(&src.join("memory"), &memory_dir(env_dir, project))
        .context("could not restore memory from claude-internal")?;
    merge_dir(&src.join("skills"), &env_dir.join("skills"))
        .context("could not restore skills from claude-internal")?;

    let snapshot = src.join("persona.CLAUDE.md");
    let live = env_dir.join("CLAUDE.md");
    let persona_differs = match (read_existing(&snapshot)?, read_existing(&live)?) {
        // Compare the text, not the bytes. The scaffolded `.gitattributes` sets
        // `* text=auto`, so the snapshot is stored LF and checked out with the
        // platform's newlines while the env's CLAUDE.md — never touched by git —
        // keeps whatever wrote it. A byte compare therefore reported "the persona
        // differs, run aello persona" for two identical files on any machine whose
        // checkout disagreed with the writer, which is the whole Windows↔Linux case
        // this command exists for. Measured before fixing.
        (Some(a), Some(b)) => normalize_newlines(&a) != normalize_newlines(&b),
        (Some(a), None) => {
            std::fs::create_dir_all(env_dir).context("could not create env dir")?;
            std::fs::write(&live, a).context("could not restore the persona")?;
            false
        }
        _ => false,
    };

    let handoff = src.join(MIRRORED_HANDOFF);
    let restored_handoff = handoff.exists();
    if restored_handoff {
        std::fs::copy(&handoff, handoff_path(project, blueprint))
            .context("could not restore the handoff from claude-internal")?;
    }

    // The counts are of what the *mirror* holds, not of what changed: a note
    // identical on both sides is still one this env now provably has, and
    // reporting "0 restored" for a healthy round trip reads like a failure.
    Ok(Restored {
        memory: count_entries(&src.join("memory"), false),
        skills: count_entries(&src.join("skills"), true),
        handoff: restored_handoff,
        persona_differs,
    })
}

/// Strip `\r` so two spellings of the same text compare equal. Only for
/// comparisons — nothing is rewritten on disk, since the newlines a file already
/// has are the ones its platform wants.
fn normalize_newlines(s: &str) -> String {
    s.replace('\r', "")
}

/// Entries directly inside `dir` — directories when `dirs`, else regular files.
/// A skill is a directory holding `SKILL.md`; a memory note is a file.
fn count_entries(dir: &Path, dirs: bool) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir() == dirs).count())
        .unwrap_or(0)
}

/// One-way *sync* of `src` into `dst`: copy every regular file/subdir from `src`,
/// then delete anything in `dst` that no longer exists in `src`. Pruning keeps
/// the tracked mirror from accumulating orphaned files — a deleted memory note,
/// or a skill the blueprint no longer seeds — which a copy-only mirror would keep
/// committing forever. (Dropping the `github` cap is handled a level up, in
/// `mirror_env_internal`, which removes the folder outright rather than diffing
/// it.) Symlinks are skipped: the env is the
/// single source of truth and must not pull foreign content into git.
///
/// A missing `src` leaves `dst` **alone**. It used to prune it entirely, on the
/// reading that nothing left to mirror means nothing left to keep — but the
/// source path is derived, and a derivation that comes out wrong is
/// indistinguishable from a deletion. The memory source encodes
/// `current_dir()`'s exact spelling, so launching from a case-variant cwd points
/// it at a directory that has never existed and takes the tracked notes with it.
/// Deleting content from the mirror still works the ordinary way: the source
/// directory exists and the file is gone from it.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst).context("could not create mirror destination dir")?;

    let mut keep = std::collections::HashSet::new();
    for entry in std::fs::read_dir(src).context("could not read mirror source dir")? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        keep.insert(entry.file_name());
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).context("could not copy mirror file")?;
        }
    }

    // Prune destination entries that vanished from the source.
    for entry in std::fs::read_dir(dst).context("could not read mirror dest dir")? {
        let entry = entry?;
        if keep.contains(&entry.file_name()) {
            continue;
        }
        let p = entry.path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p).context("could not prune stale mirror subdir")?;
        } else {
            std::fs::remove_file(&p).context("could not prune stale mirror file")?;
        }
    }
    Ok(())
}

/// Copy `src` over `dst` **without pruning**, and report the top-level names in
/// `dst` that `src` does not have.
///
/// This is `copy_dir_all`'s non-destructive half, and it exists because memory is
/// the one thing here with two writers on two machines. A pruning mirror is
/// correct while an env dir is the only source of truth; the moment a second
/// device commits notes of its own, prune means "delete whatever the other
/// machine wrote", silently, on the next launch. Union plus a named report is the
/// only merge that cannot lose a note. Deleting one for real stays possible — it
/// takes a `git rm` of the mirror copy, which is deliberate rather than a
/// side effect of launching.
///
/// A missing `src` leaves `dst` alone, for the same reason `copy_dir_all` does: a
/// derived source path that comes out wrong is indistinguishable from an empty one.
fn merge_dir(src: &Path, dst: &Path) -> Result<Vec<String>> {
    if !src.exists() {
        return Ok(vec![]);
    }
    std::fs::create_dir_all(dst).context("could not create merge destination dir")?;

    let mut from_src = std::collections::HashSet::new();
    for entry in std::fs::read_dir(src).context("could not read merge source dir")? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        from_src.insert(entry.file_name());
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            merge_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to).context("could not copy merged file")?;
        }
    }

    let mut extra = vec![];
    for entry in std::fs::read_dir(dst).context("could not read merge dest dir")? {
        let entry = entry?;
        if !from_src.contains(&entry.file_name()) {
            extra.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    extra.sort();
    Ok(extra)
}

/// Seed `.githooks/pre-commit` and point the clone at it.
///
/// Why aello does this at all: `/sync` mirrors an env's memory, persona and
/// handoff into the tracked `claude-internal/<name>/` folder and stages it by
/// path with **no check of what is in it**. Memory notes are exactly where a
/// session writes down a password it just used, and one of the repos this runs
/// in is public. Blocking a commit is the only point in that chain that can
/// refuse; a skill instruction cannot, because the skill is advice to an agent.
///
/// Three properties, each of which cost somebody a debugging session:
///
/// * **Written with LF regardless of checkout.** Hooks are run by `sh` and a
///   CRLF `pre-commit` fails to execute — and it has no extension, so a `*.sh`
///   attribute rule does not cover the copy in the target project. Normalising
///   here means the guard cannot be silently disabled by the line endings of
///   whoever cloned aello.
/// * **`core.hooksPath` is set on every placement, not once.** It is per-clone
///   local config and does not travel with a pull, so a fresh clone of a project
///   that has the file has no guard until something sets it again. Re-running it
///   is what heals that; a marker recording "already configured" would go stale
///   exactly when the repo is re-cloned. It is skipped when the setting already
///   points somewhere else, since that is a hook layout aello did not build.
/// * **Only aello's own copy is replaced.** The file carries a
///   `aello-pre-commit v<N>` marker on line 2. A copy with an older marker is
///   upgraded, so a widened pattern reaches projects scaffolded months ago; a
///   file *without* the marker is somebody's own hook and is left alone.
fn seed_pre_commit_hook(project: &Path) -> Result<()> {
    // Not a git repo yet (`github-setup` runs later, or never): the hook would
    // sit there unread and `git config` would fail. Nothing to do.
    if !project.join(".git").exists() {
        return Ok(());
    }
    let dir = project.join(".githooks");
    let path = dir.join("pre-commit");
    let ours = PRE_COMMIT_HOOK.replace("\r\n", "\n");

    let write = match read_existing(&path)? {
        None => true,
        Some(existing) => match pre_commit_version(&existing) {
            // Someone else's hook. Leave it; they own this path now.
            None => false,
            Some(v) => v < PRE_COMMIT_VERSION,
        },
    };
    if write {
        std::fs::create_dir_all(&dir).context("could not create .githooks dir")?;
        std::fs::write(&path, &ours).context("could not write .githooks/pre-commit")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }

    // Keep git from converting the hook to CRLF on a Windows checkout of the
    // *target* project. `.githooks/*` rather than `*.sh`: the file has no
    // extension, which is how this breaks silently.
    ensure_gitattributes_entry(project, ".githooks/* text eol=lf")?;

    let current = std::process::Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["config", "--local", "--get", "core.hooksPath"])
        .output();
    let configured = match &current {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => return Ok(()), // no git binary; the file is still seeded
    };
    if configured.is_empty() || configured == ".githooks" {
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(project)
            .args(["config", "--local", "core.hooksPath", ".githooks"])
            .status();
    }
    Ok(())
}

/// The `aello-pre-commit v<N>` marker on line 2 of our own hook, or `None` when
/// the file is not one of ours.
fn pre_commit_version(text: &str) -> Option<u32> {
    text.lines()
        .take(5)
        .find_map(|l| l.split("aello-pre-commit v").nth(1))
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().ok()
        })
}

/// Ensure `entry` exists as its own line in the project's `.gitattributes`.
/// Same contract as [`ensure_gitignore_entry`] — idempotent, append-only, and
/// it refuses rather than clobbering when the existing file cannot be read.
fn ensure_gitattributes_entry(project: &Path, entry: &str) -> Result<()> {
    let path = project.join(".gitattributes");
    let existing = read_existing(&path)?.unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(entry);
    out.push('\n');
    std::fs::write(&path, out).context("could not write .gitattributes")
}

/// Ensure `entry` exists as its own line in the project's `.gitignore`, creating
/// the file or appending as needed. Idempotent — a matching line (ignoring
/// surrounding whitespace) is never duplicated. Preserves existing content —
/// and fails rather than writing when the existing content cannot be read, since
/// the write below is a full-file rewrite (see [`read_existing`]).
pub fn ensure_gitignore_entry(project: &Path, entry: &str) -> Result<()> {
    let path = project.join(".gitignore");
    let existing = read_existing(&path)?.unwrap_or_default();
    // Treat a trailing-slash variant (`.claude-env-*/`) as already present so we
    // don't append a near-duplicate line.
    let norm = |l: &str| l.trim().trim_end_matches('/').to_string();
    if existing.lines().any(|l| norm(l) == norm(entry)) {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(entry);
    out.push('\n');
    std::fs::write(&path, out).context("could not write .gitignore")
}

/// settings.json for an isolated Claude env: subscription auth (no keys, no env
/// block), bypass permissions, the transcript hooks, a `Stop` hook that speaks
/// the response and a second `SessionEnd` group that hands the leased voice back
/// to the pool. Every env speaks; silence is `aello voice mute`, not placement.
pub fn settings_json(model: &str) -> String {
    let py = if cfg!(windows) { "python" } else { "python3" };
    // `$CLAUDE_CONFIG_DIR` — not a path to any checkout — is the whole point:
    // the hook travels with the env, so moving a repo can't silence it. The
    // quotes are escaped for JSON here, since this is assembled as text.
    let speak = format!("{py} \\\"$CLAUDE_CONFIG_DIR/hooks/speak.py\\\"");
    let stop =
        format!("\n    \"Stop\": [{{\"hooks\":[{{\"type\":\"command\",\"command\":\"{speak}\"}}]}}],");
    let voice_end =
        format!(",\n      {{\"hooks\":[{{\"type\":\"command\",\"command\":\"{speak}\"}}]}}");
    format!(
        r#"{{
  "model": {},
  "cleanupPeriodDays": {CLEANUP_PERIOD_DAYS},
  "skipDangerousModePermissionPrompt": true,
  "permissions": {{
    "defaultMode": "bypassPermissions"
  }},
  "statusLine": {{"type": "command", "command": "{STATUSLINE_COMMAND}"}},
  "hooks": {{{stop}
    "SessionStart": [{{"hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/session-start.py\""}}]}}],
    "UserPromptSubmit": [{{"hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/user-prompt-submit.py\""}}]}}],
    "PreToolUse": [{{"matcher":"{PLAN_TOOLS}","hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/pre-tool-use.py\""}}]}}],
    "PostCompact": [{{"hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/post-compact.py\""}}]}}],
    "SessionEnd": [
      {{"hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/session-end.py\""}}]}}{voice_end}
    ]
  }}
}}
"#,
        json_str(model),
    )
}

/// The `PreToolUse` matcher for the plan-mode block. A matcher (rather than an
/// unmatched group) is the difference between spawning Python twice a session
/// and spawning it on every Read — the hook is registered fleet-wide, so that
/// cost would be paid by every env on every tool call.
const PLAN_TOOLS: &str = "EnterPlanMode|ExitPlanMode";

/// The env-relative voice command — the only one aello considers its own. Any
/// other `speak.py` in settings.json was installed by hand against a checkout.
const OWNED_SPEAK: &str = "$CLAUDE_CONFIG_DIR/hooks/speak.py";

/// Register the voice hook in an existing `settings.json`. `speak.py` branches
/// on the event it's given, so one command serves both: `Stop` speaks the
/// response, `SessionEnd` returns the leased voice to the pool. Idempotent.
///
/// This also **migrates**: a hand-installed hook pointing at a checkout
/// (`python "C:/…/revoiced/speak.py"`) is replaced by the env-relative one. That
/// path is the problem the vendored hook exists to solve — leaving it would keep
/// every env coupled to one directory, and adding ours beside it would speak
/// each response twice.
///
/// There is no deregister branch: every env speaks. Silence is a runtime
/// setting the hook itself reads (`aello voice mute`), so an env is never made
/// quiet by rewriting its settings.
fn sync_voice_hooks(settings: &Path) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return Ok(());
    };
    let Ok(mut v) = parse_settings(settings, &text) else {
        return Ok(());
    };
    let py = if cfg!(windows) { "python" } else { "python3" };
    let Some(hooks) = v
        .as_object_mut()
        .and_then(|o| o.entry("hooks").or_insert_with(|| serde_json::json!({})).as_object_mut())
    else {
        return Ok(());
    };

    let owned = |g: &serde_json::Value| group_has_command(g, OWNED_SPEAK);
    // Anchored on a path boundary, not a bare substring. `contains("speak.py")`
    // also matched a user's own `my_speak.py` or `tools/fastspeak.pyx`, and the
    // retain below drops the whole GROUP — so an unrelated sibling command in it
    // went too. This runs on every `aello run`, not once at enable, so a hook
    // added later would have been removed on the next launch.
    let legacy = |g: &serde_json::Value| {
        !owned(g)
            && (group_has_command(g, "/speak.py")
                || group_has_command(g, r"\speak.py")
                || group_has_command(g, "\"speak.py")
                || group_has_command(g, " speak.py"))
    };

    let mut changed = false;
    for event in ["Stop", "SessionEnd"] {
        if let Some(serde_json::Value::Array(arr)) = hooks.get_mut(event) {
            let before = arr.len();
            arr.retain(|g| !legacy(g));
            changed |= arr.len() != before;
        }
        if hooks.get(event).is_some_and(|e| registers_command(e, OWNED_SPEAK)) {
            continue;
        }
        let group = serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": format!("{py} \"{OWNED_SPEAK}\""),
            }]
        });
        match hooks.get_mut(event) {
            Some(serde_json::Value::Array(arr)) => arr.push(group),
            // Absent is ours to create; a non-array is a value the user put
            // there by hand, and every sibling heal skips rather than clobber.
            None => {
                hooks.insert(event.to_string(), serde_json::json!([group]));
            }
            Some(_) => continue,
        }
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .context("could not update settings.json with the voice hook")
}

/// Self-heal the `model` key in an existing `settings.json`.
///
/// `settings.json` is the **only** channel carrying the model to Claude Code —
/// `launch` passes no `--model` — and it is written once, on first placement. So
/// without this, `aello edit <name> --model` updated `config.toml`, `.aello.toml`
/// and `aello list` while the env kept running the model it was placed with,
/// forever, and `cmd_edit` still printed "Changes apply on the next run".
///
/// A key-scoped merge, deliberately not a regenerate: a real env's settings.json
/// carries keys the user added by hand (`effortLevel`, `enabledPlugins`), and
/// rewriting the file from a template would silently delete them.
/// How long Claude Code keeps its own session transcripts. Its default is 30
/// days, and contextdb's SessionEnd archive records the transcript by **path**,
/// so at 30 days the reference silently stops resolving. Measured on 2026-08-03:
/// 15% of 265 archives already dangled, with a clean cliff at the 30-day mark
/// (6–14% dead under 30 days, 44% at 30–39). The archive now copies the
/// transcript too, so this is belt-and-braces — but it also keeps `--resume`
/// working on old sessions, which the copy does not.
/// How long Claude Code keeps its own session files.
///
/// This was **365** while a recorded path was the only archive: the transcript
/// had to outlive the pointer. It no longer is — SessionEnd copies the file and
/// verifies the copy — so the number flipped from "keep it long enough to
/// survive" to "get it out of the working tree fast". contextdb is the store;
/// Claude Code's copy is a temporary that a session with a handoff note deletes
/// outright, and this is the backstop for the ones that don't.
///
/// 10 days, not 3: an unarchived transcript is one whose session never reached
/// SessionEnd (killed terminal, reboot), and that is exactly the session worth
/// resuming. Ten days is how long that chance lasts.
const CLEANUP_PERIOD_DAYS: u32 = 10;

/// The values aello has written into `cleanupPeriodDays` itself. Anything else
/// is the user's and is left alone — but our own previous default has to be
/// updatable, or every env placed before this change keeps 365 forever and the
/// setting silently means nothing.
const OUR_CLEANUP_VALUES: &[u64] = &[365, 10];

/// Self-heal the retention setting into an env placed before it existed, and
/// migrate aello's own earlier default.
///
/// Absent → written. Holding a value aello itself wrote (`OUR_CLEANUP_VALUES`)
/// → updated. Anything else is the user's and is left alone, including a
/// deliberately short one. Without the migration branch every env placed before
/// this change would keep 365 forever, which is the value the whole design just
/// stopped needing — a setting that only applies to new envs is a setting the
/// fleet does not have.
fn ensure_cleanup_period(settings: &Path) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return Ok(());
    };
    let Ok(mut v) = parse_settings(settings, &text) else {
        return Ok(());
    };
    let Some(obj) = v.as_object_mut() else {
        return Ok(());
    };
    match obj.get("cleanupPeriodDays").and_then(|d| d.as_u64()) {
        Some(n) if n == CLEANUP_PERIOD_DAYS as u64 => return Ok(()),
        Some(n) if !OUR_CLEANUP_VALUES.contains(&n) => return Ok(()),
        Some(_) => {}
        None if obj.contains_key("cleanupPeriodDays") => return Ok(()), // non-numeric: theirs
        None => {}
    }
    obj.insert("cleanupPeriodDays".into(), CLEANUP_PERIOD_DAYS.into());
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .context("could not update settings.json with the transcript retention")
}

/// The statusline command aello considers its own. Registered by name rather
/// than by absolute path: `aello update` replaces the binary in place and a
/// recorded path would survive a reinstall that moved it, going silently dead
/// — a statusline that fails just doesn't render, so there is nothing to see.
/// `aello check` executes it for that reason.
///
/// No backslashes on purpose: Claude Code runs the statusLine command through
/// Git Bash on Windows, which eats unquoted backslashes as escapes.
pub const STATUSLINE_COMMAND: &str = "aello statusline";

/// Register the usage readout in an existing `settings.json`.
///
/// Only when there is no `statusLine` at all, or when the one there is ours: a
/// hand-written statusline is the user's, and a placement that replaced it
/// every run would be the "env dir is rewritten under you" complaint with a
/// new face. Removing the key is how you opt out, and `place` leaves it
/// removed only until the next run — so opting out for good means pointing it
/// at something else.
fn ensure_statusline(settings: &Path) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return Ok(());
    };
    let Ok(mut v) = parse_settings(settings, &text) else {
        return Ok(());
    };
    let Some(obj) = v.as_object_mut() else {
        return Ok(());
    };
    match obj.get("statusLine") {
        Some(s) => {
            let cmd = s["command"].as_str().unwrap_or_default();
            if !cmd.contains(STATUSLINE_COMMAND) {
                return Ok(()); // Somebody else's statusline. Leave it alone.
            }
            if cmd == STATUSLINE_COMMAND {
                return Ok(());
            }
        }
        None => {}
    }
    obj.insert(
        "statusLine".into(),
        serde_json::json!({"type": "command", "command": STATUSLINE_COMMAND}),
    );
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .context("could not update settings.json with the statusline")
}

fn ensure_model(settings: &Path, model: &str) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return Ok(());
    };
    let Ok(mut v) = parse_settings(settings, &text) else {
        return Ok(());
    };
    let Some(obj) = v.as_object_mut() else {
        return Ok(());
    };
    if obj.get("model").and_then(|m| m.as_str()) == Some(model) {
        return Ok(());
    }
    obj.insert("model".into(), serde_json::Value::String(model.to_string()));
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .context("could not update settings.json with the model")
}

/// Append the TL;DR instruction to the env's persona when it isn't already
/// there. The voice hook speaks that line and nothing else, so a persona that
/// never writes one makes the capability silent. Appends rather than rewrites —
/// the persona is the user's, and this only adds a section. When a blueprint has
/// no persona at all, the section becomes the whole file.
fn ensure_tldr_instruction(env_dir: &Path) -> Result<()> {
    // An env that injects the instruction per turn already has it covered, and
    // more reliably than the persona does: the persona is the file most likely
    // to be rewritten wholesale, so anything the voice depends on is safer
    // outside it. Appending here as well would put the sentence back into the
    // file that was deliberately cleared of it.
    if injects_tldr_per_turn(env_dir) {
        return Ok(());
    }
    let path = env_dir.join("CLAUDE.md");
    let existing = read_existing(&path)?.unwrap_or_default();
    if existing.contains("TL;DR") {
        return Ok(());
    }
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(crate::templates::VOICE_TLDR);
    std::fs::write(&path, out).context("could not add the TL;DR instruction to CLAUDE.md")
}

/// Where an accepted persona's generation is recorded: `<env>/persona.gen`,
/// holding one line like `gen1 2026-08-03`.
///
/// A sidecar rather than a config key or a field on the placed `.aello.toml`,
/// because both of those are rewritten from a struct on every `place` and would
/// drop it. `place` never touches this file, so it survives every later run.
/// Config says *whether* an env has a custom persona (`claude_md = "custom"`);
/// this says *which generation*, next to the file it describes.
pub const PERSONA_GEN_FILE: &str = "persona.gen";

/// Read an env's persona generation number, or 0 when it has none.
pub fn persona_generation(env_dir: &Path) -> u32 {
    let Ok(text) = std::fs::read_to_string(env_dir.join(PERSONA_GEN_FILE)) else {
        return 0;
    };
    text.split_whitespace()
        .next()
        .and_then(|t| t.strip_prefix("gen"))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Record that generation `gen` of this env's persona was accepted on `date`.
/// Overwrites: only the current generation matters, and the history that does
/// matter is in git via the `claude-internal/` mirror.
pub fn write_persona_generation(env_dir: &Path, gen: u32, date: &str) -> Result<()> {
    std::fs::write(env_dir.join(PERSONA_GEN_FILE), format!("gen{gen} {date}\n"))
        .context("could not record the persona generation")
}

/// Today's UTC date as `YYYY-MM-DD`.
///
/// Hand-rolled rather than pulling in a date crate for one string. This is
/// Howard Hinnant's `civil_from_days`: shift the epoch to 0000-03-01 so leap
/// day lands at the end of the cycle, then divide out 400/100/4-year eras.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs / 86_400 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March-based
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Accept a generated persona into an env: write it, bump the generation and
/// report the new number. The caller flips `claude_md` to `custom`.
///
/// Overwrites `CLAUDE.md` deliberately — this is the one operation that is
/// *supposed* to replace a persona, which is why it is a command and not part
/// of `place` (place never clobbers a persona, and must not start).
pub fn accept_persona(env_dir: &Path, content: &str) -> Result<(u32, String)> {
    if !env_dir.exists() {
        anyhow::bail!("no env dir at {} — place the blueprint there first", env_dir.display());
    }
    std::fs::write(env_dir.join("CLAUDE.md"), content)
        .context("could not write the new CLAUDE.md")?;
    let gen = persona_generation(env_dir) + 1;
    let date = today_utc();
    write_persona_generation(env_dir, gen, &date)?;
    Ok((gen, date))
}

/// True when this env carries the TL;DR instruction on a `UserPromptSubmit`
/// hook, so `place` should leave the persona alone.
///
/// Since the hook became bundled this is true of every placed env, so the
/// persona append below is a fallback rather than the normal path — it comes
/// back only if someone unregisters the hook by hand. Kept for exactly that
/// case: the voice is silent without the instruction somewhere.
fn injects_tldr_per_turn(env_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(env_dir.join("settings.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    registers_command(&v["hooks"]["UserPromptSubmit"], "user-prompt-submit.py")
}

/// Self-heal: ensure an existing `settings.json` registers one of aello's own
/// transcript hooks. `settings.json` is written only once (never clobbered), so
/// envs placed before a hook existed would otherwise never pick it up. Idempotent
/// — keyed on aello's own script name, not on the event key, so a third-party
/// hook on the same event doesn't block the heal; aello's group is appended
/// alongside it.
fn ensure_own_hook(settings: &Path, event: &str, script: &str) -> Result<()> {
    ensure_own_hook_matching(settings, event, script, None)
}

/// As `ensure_own_hook`, but registers the group under a `matcher`. `PreToolUse`
/// needs one: an unmatched group fires on every tool call, so the plan-mode
/// block would spawn Python for every Read in every env.
fn ensure_own_hook_matching(
    settings: &Path,
    event: &str,
    script: &str,
    matcher: Option<&str>,
) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return Ok(());
    };
    let Ok(mut v) = parse_settings(settings, &text) else {
        return Ok(());
    };
    let py = if cfg!(windows) { "python" } else { "python3" };
    let hooks = v
        .as_object_mut()
        .and_then(|o| o.entry("hooks").or_insert_with(|| serde_json::json!({})).as_object_mut());
    let Some(hooks) = hooks else { return Ok(()) };
    let mut group = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": format!("{py} \"$CLAUDE_CONFIG_DIR/hooks/{script}\""),
        }]
    });
    if let Some(m) = matcher {
        group["matcher"] = serde_json::Value::String(m.into());
    }
    match hooks.get_mut(event) {
        None => {
            hooks.insert(event.into(), serde_json::json!([group]));
        }
        Some(existing) => {
            if registers_command(existing, script) {
                return Ok(());
            }
            let Some(arr) = existing.as_array_mut() else { return Ok(()) };
            arr.push(group);
        }
    }
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .with_context(|| format!("could not update settings.json with the {event} hook"))
}

/// Parse a settings.json, saying so on stderr when it can't be read as JSON.
///
/// Every self-heal treats an unparseable file as "leave it alone" — the right
/// call, since the alternative is overwriting something the user is mid-edit on.
/// But doing it in silence meant a typo'd settings.json quietly cost you the
/// hooks with no hint of why, so the skip is now at least audible.
fn parse_settings(path: &Path, text: &str) -> Result<serde_json::Value> {
    serde_json::from_str(text).inspect_err(|e| {
        eprintln!(
            "warning: {} is not valid JSON ({e}) — leaving it untouched; \
             aello's hooks can't be kept current until it parses",
            path.display()
        );
    }).map_err(Into::into)
}

/// True when a settings.json hook-event value (an array of `{hooks:[{command}]}`
/// groups) already contains a command mentioning `needle`.
fn registers_command(event: &serde_json::Value, needle: &str) -> bool {
    event
        .as_array()
        .is_some_and(|groups| groups.iter().any(|g| group_has_command(g, needle)))
}

/// True when one `{hooks:[{command}]}` group runs a command mentioning `needle`.
fn group_has_command(group: &serde_json::Value, needle: &str) -> bool {
    group
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains(needle))
            })
        })
}

/// Minimal JSON string encoder for the model value.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_dir_naming() {
        let p = env_dir(Path::new("/proj"), "coder");
        assert!(p.ends_with(".claude-env-coder"));
    }

    #[test]
    fn settings_json_is_valid() {
        let s = settings_json("sonnet");
        let v: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
        assert_eq!(v["model"], "sonnet");
        assert_eq!(v["permissions"]["defaultMode"], "bypassPermissions");
        assert!(v["hooks"]["PostCompact"].is_array());
        assert!(v["hooks"]["SessionEnd"].is_array());
    }

    #[test]
    fn every_env_registers_the_statusline() {
        let v: serde_json::Value = serde_json::from_str(&settings_json("opus")).unwrap();
        assert_eq!(v["statusLine"]["type"], "command");
        assert_eq!(v["statusLine"]["command"], STATUSLINE_COMMAND);
        // Git Bash runs this on Windows and eats unquoted backslashes.
        assert!(!STATUSLINE_COMMAND.contains('\\'), "no backslashes in the command");
    }

    /// The self-heal: an env placed before the statusline existed adopts it,
    /// and one the user pointed somewhere else keeps their command.
    #[test]
    fn the_statusline_heals_in_but_never_replaces_the_users_own() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("settings.json");

        std::fs::write(&s, r#"{"model":"opus"}"#).unwrap();
        ensure_statusline(&s).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["statusLine"]["command"], STATUSLINE_COMMAND);
        assert_eq!(v["model"], "opus", "the rest of the file survives");

        std::fs::write(&s, r#"{"statusLine":{"type":"command","command":"my-own-thing"}}"#).unwrap();
        ensure_statusline(&s).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["statusLine"]["command"], "my-own-thing", "a hand-written statusline is theirs");
    }

    #[test]
    fn every_env_registers_the_stop_and_release_hooks() {
        let s = settings_json("sonnet");
        let v: serde_json::Value = serde_json::from_str(&s).expect("valid JSON");
        // Stop speaks the response; SessionEnd hands the leased voice back —
        // alongside aello's own transcript hook, which must survive.
        assert!(registers_command(&v["hooks"]["Stop"], "speak.py"));
        assert!(registers_command(&v["hooks"]["SessionEnd"], "speak.py"));
        assert!(registers_command(&v["hooks"]["SessionEnd"], "session-end.py"));
        // Pointed at the env, never at a checkout — the whole point of vendoring.
        assert!(s.contains("$CLAUDE_CONFIG_DIR/hooks/speak.py"));
    }

    #[test]
    fn place_seeds_session_end_hook_and_script() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        place(&env, &inst, None, &Capabilities::default()).unwrap();

        // Hook script lands in the env, alongside post-compact.py.
        assert!(env.join("hooks/session-end.py").exists());
        // settings.json registers it.
        let s = std::fs::read_to_string(env.join("settings.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert!(v["hooks"]["SessionEnd"].is_array());
    }

    /// contextdb records the transcript by path, and Claude Code deletes its own
    /// session files after `cleanupPeriodDays` (default 30). Measured 2026-08-03:
    /// 15% of 265 archives already pointed at nothing, with a clean cliff at the
    /// 30-day mark. Nothing errors when that happens — the archive just quietly
    /// stops being an archive.
    #[test]
    /// Retention is deliberately **shorter** than Claude Code's 30-day default,
    /// which is the opposite of what this asserted while a recorded path was
    /// the only archive. SessionEnd now copies the transcript and verifies the
    /// copy, so the original is a temporary; the setting exists to get it out
    /// of the working tree, not to keep it alive.
    #[test]
    fn settings_expire_claude_codes_own_transcripts_quickly() {
        let s = settings_json("opus");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let days = v["cleanupPeriodDays"].as_u64().expect("cleanupPeriodDays must be set");
        assert!(days < 30, "retention {days} is not shorter than Claude Code's default");
        assert!(
            days >= 7,
            "retention {days} is too short: a session that never reached SessionEnd is \
             unarchived, and this is the only window in which it can still be resumed"
        );
    }

    /// Our own previous default must migrate, or every env placed before this
    /// keeps 365 forever and the change reaches nothing already on disk.
    #[test]
    fn cleanup_period_migrates_aellos_own_earlier_default() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("settings.json");
        std::fs::write(&s, r#"{"model":"opus","cleanupPeriodDays":365}"#).unwrap();
        ensure_cleanup_period(&s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["cleanupPeriodDays"].as_u64(), Some(CLEANUP_PERIOD_DAYS as u64));
    }

    #[test]
    fn cleanup_period_self_heals_but_never_overrides_a_chosen_value() {
        let dir = tempfile::tempdir().unwrap();
        let s = dir.path().join("settings.json");

        // An env placed before this existed: the key is absent and gets filled in.
        std::fs::write(&s, r#"{"model":"opus"}"#).unwrap();
        ensure_cleanup_period(&s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["cleanupPeriodDays"].as_u64(), Some(CLEANUP_PERIOD_DAYS as u64));
        assert_eq!(v["model"].as_str(), Some("opus"), "the rest of settings.json must survive");

        // A value the user chose is theirs — including a deliberately short one.
        std::fs::write(&s, r#"{"model":"opus","cleanupPeriodDays":7}"#).unwrap();
        ensure_cleanup_period(&s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&s).unwrap()).unwrap();
        assert_eq!(v["cleanupPeriodDays"].as_u64(), Some(7), "a user's retention was overwritten");
    }

    /// SessionEnd does all the archiving in practice — PostCompact only fires on
    /// compaction, which a 1M-context session ended with /clear never reaches.
    /// So the transcript copy is the archive, not a nicety.
    #[test]
    fn session_end_hook_copies_the_transcript_it_records() {
        let s = SESSION_END_SCRIPT;
        assert!(s.contains("_transcript.jsonl"), "no transcript copy is written");
        assert!(s.contains("transcript_archived"), "the record must say whether the copy landed");
        // Streamed, not read into memory — these run to tens of MB at session exit.
        assert!(s.contains("src.read("), "the copy must stream rather than slurp");
        // Windows MAX_PATH: the encoded-cwd directory repeats the whole project
        // path, so a deep project pushes the transcript past 260 chars and the
        // copy silently degrades back to a pointer. Measured at 325 chars.
        assert!(s.contains(r"\\\\?\\"), "the copy must opt out of Windows MAX_PATH");
        // The handoff note is the part that exists nowhere else; a failed copy
        // must not take it down with it.
        assert!(
            s.find("archived = \"\"").is_some_and(|i| i > s.find("except Exception").unwrap()),
            "a failed transcript copy must fall through, not abort the record"
        );
    }

    /// The SessionStart hook is the only thing that tells a session it is running
    /// under aello at all — the env dir is gitignored, the persona is the user's
    /// and usually silent about it, and a project `CLAUDE.md` exists only for a
    /// maintainer. Losing this block would not fail anything; sessions would just
    /// quietly go back to hand-editing files the next launch overwrites, which is
    /// what happened before it existed. Pin the parts that carry the meaning.
    #[test]
    fn session_start_hook_announces_the_aello_environment() {
        let s = SESSION_START_SCRIPT;
        for needle in [
            "You are running under aello",
            // The env dir is rewritten every run, and `.aello-keep` is the escape.
            "aello run",
            ".aello-keep",
            // Skills are the user's to type — the rule an agent most often breaks.
            "never yours to run",
            // It must name *this* blueprint, not speak in the abstract.
            "{agent}",
        ] {
            assert!(s.contains(needle), "session-start.py no longer says {needle:?}");
        }
        // Emitted on every session, not only when a handoff or note is waiting —
        // the early `sys.exit(0)` used to skip it entirely.
        assert!(
            s.contains("context = standing"),
            "the standing block must be emitted even with no handoff/note to deliver"
        );
    }

    #[test]
    fn ensure_session_end_hook_self_heals_old_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        // An env placed before SessionEnd existed: PostCompact only.
        std::fs::write(
            &settings,
            r#"{"model":"opus","hooks":{"PostCompact":[{"hooks":[]}]}}"#,
        )
        .unwrap();

        ensure_own_hook(&settings, "SessionEnd", "session-end.py").unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        // SessionEnd inserted, PostCompact and model preserved.
        assert!(v["hooks"]["SessionEnd"].is_array());
        assert!(v["hooks"]["PostCompact"].is_array());
        assert_eq!(v["model"], "opus");

        // Idempotent: a second pass does not duplicate or alter it.
        let before = std::fs::read_to_string(&settings).unwrap();
        ensure_own_hook(&settings, "SessionEnd", "session-end.py").unwrap();
        assert_eq!(before, std::fs::read_to_string(&settings).unwrap());
    }

    #[test]
    fn ensure_session_end_hook_appends_beside_a_third_party_hook() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        // A SessionEnd hook the user added themselves: the key exists, but
        // aello's own hook does not.
        std::fs::write(
            &settings,
            r#"{"hooks":{"SessionEnd":[{"hooks":[{"type":"command","command":"python other.py"}]}]}}"#,
        )
        .unwrap();

        ensure_own_hook(&settings, "SessionEnd", "session-end.py").unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        let groups = v["hooks"]["SessionEnd"].as_array().unwrap();
        // Both survive: theirs first, aello's appended.
        assert_eq!(groups.len(), 2);
        assert!(registers_command(&v["hooks"]["SessionEnd"], "other.py"));
        assert!(registers_command(&v["hooks"]["SessionEnd"], "session-end.py"));

        // Idempotent: a second pass does not append again.
        let before = std::fs::read_to_string(&settings).unwrap();
        ensure_own_hook(&settings, "SessionEnd", "session-end.py").unwrap();
        assert_eq!(before, std::fs::read_to_string(&settings).unwrap());
    }

    #[test]
    fn every_env_seeds_all_five_scripts_and_the_tldr_instruction() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        // No capabilities at all: the voice is not one of them any more.
        let caps = Capabilities::default();

        place(&env, &inst, Some("# persona\n"), &caps).unwrap();

        // speak.py imports duck, focus and notify as siblings and shells out to
        // win_audio.ps1 next to it. A partial copy would break at runtime, not
        // at placement — and for focus/notify it does not break at all, it just
        // goes quiet, which is why all five are asserted here.
        assert!(env.join("hooks/speak.py").exists());
        assert!(env.join("hooks/duck.py").exists());
        assert!(env.join("hooks/focus.py").exists());
        assert!(env.join("hooks/notify.py").exists());
        assert!(env.join("hooks/win_audio.ps1").exists());
        // Something must ask for the line speak.py speaks, or the voice is
        // silent and the hook nags every turn. That is the per-turn hook now,
        // so the persona is left exactly as the blueprint wrote it.
        let persona = std::fs::read_to_string(env.join("CLAUDE.md")).unwrap();
        assert_eq!(persona, "# persona\n");
        assert!(env.join("hooks/user-prompt-submit.py").exists());
        assert!(USER_PROMPT_SUBMIT_SCRIPT.contains("TL;DR"));
        // The voice is not a /sync capability, so no skill is seeded.
        assert!(!env.join("skills/sync/SKILL.md").exists());
    }

    /// Half the drift check: the version the vendored `speak.py` claims must be
    /// the one `project.rs` records, so a re-vendor that forgets to move the
    /// constant fails here rather than shipping a copy nobody can date.
    #[test]
    fn the_recorded_hook_version_matches_the_vendored_script() {
        let line = SPEAK_SCRIPT
            .lines()
            .find(|l| l.starts_with("HOOK_VERSION"))
            .expect("vendored speak.py has no HOOK_VERSION");
        let vendored: u32 = line
            .split('=')
            .nth(1)
            .and_then(|v| v.trim().parse().ok())
            .expect("HOOK_VERSION is not an integer");
        assert_eq!(
            vendored, HOOK_VERSION,
            "re-vendored speak.py is version {vendored} but project.rs still records {HOOK_VERSION}"
        );
    }

    /// Digest of all five vendored hook files, in the order below, with CRLF
    /// normalised so a Windows checkout and Linux CI agree. Update it in the
    /// same commit as a re-vendor — and only together with `HOOK_VERSION`.
    const HOOK_FILES_DIGEST: &str =
        "d8aa37a6c158a6b31ca66308ceb3807fd9153690d1708f70bcd822e534142925";

    fn hook_files_digest() -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for script in [
            SPEAK_SCRIPT,
            DUCK_SCRIPT,
            FOCUS_SCRIPT,
            NOTIFY_SCRIPT,
            WIN_AUDIO_SCRIPT,
        ] {
            h.update(script.replace("\r\n", "\n").as_bytes());
        }
        format!("{:x}", h.finalize())
    }

    /// The other half. `HOOK_VERSION` lives only in `speak.py`, but upstream
    /// moves it whenever *any* of the five hook-path files changes — so the test
    /// above sees nothing when a re-vendor touches only `duck.py` or
    /// `win_audio.ps1`. This one hashes all five: any byte that moves fails it,
    /// and the fix is to check that `HOOK_VERSION` moved too before recording
    /// the new digest.
    #[test]
    fn the_recorded_digest_covers_all_five_vendored_hook_files() {
        let actual = hook_files_digest();
        assert_eq!(
            actual, HOOK_FILES_DIGEST,
            "a vendored hook file changed. Confirm upstream bumped HOOK_VERSION \
             (now {HOOK_VERSION} here), then record the new digest: {actual}"
        );
    }

    /// `speak.py` guards the `focus` and `notify` imports so a partial copy
    /// still speaks — which is exactly how a three-file vendor turned desktop
    /// notifications off everywhere without failing. Both must be bundled.
    #[test]
    fn the_optional_siblings_are_vendored_not_left_to_the_import_guard() {
        assert!(SPEAK_SCRIPT.contains("import focus as focusing"));
        assert!(SPEAK_SCRIPT.contains("import notify as notifying"));
        assert!(FOCUS_SCRIPT.contains("def terminal_window"));
        assert!(NOTIFY_SCRIPT.contains("def show("));
    }

    #[test]
    fn voice_hooks_self_heal_into_an_env_placed_before_they_existed() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        let caps = Capabilities::default();

        // An env as they existed before the voice was universal: a settings.json
        // that place() must not clobber, with no Stop hook in it.
        std::fs::create_dir_all(&env).unwrap();
        std::fs::write(
            env.join("settings.json"),
            r#"{"model":"opus","effortLevel":"high","hooks":{"SessionEnd":[
                 {"hooks":[{"type":"command","command":"python \"$CLAUDE_CONFIG_DIR/hooks/session-end.py\""}]}
               ]}}"#,
        )
        .unwrap();
        place(&env, &inst, Some("# persona\n"), &caps).unwrap();

        let read = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap()
        };
        let v = read();
        assert!(registers_command(&v["hooks"]["Stop"], "speak.py"));
        assert!(registers_command(&v["hooks"]["SessionEnd"], "speak.py"));
        // aello's own transcript hook is untouched, and so is a user-added key —
        // the heal is a merge, never a regenerate.
        assert!(registers_command(&v["hooks"]["SessionEnd"], "session-end.py"));
        assert_eq!(v["effortLevel"], "high");
        // The env picked up the TL;DR instruction it was placed without — via
        // the per-turn hook, which is healed in the same way the voice is.
        assert!(registers_command(&v["hooks"]["UserPromptSubmit"], "user-prompt-submit.py"));
        assert!(env.join("hooks/user-prompt-submit.py").exists());

        // Re-placing is idempotent — no duplicate hook groups.
        place(&env, &inst, Some("# persona\n"), &caps).unwrap();
        let v = read();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(v["hooks"]["SessionEnd"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn edit_model_reaches_an_already_placed_env() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let caps = Capabilities::default();

        // Placed on sonnet, then edited to opus (`aello edit coder --model opus`,
        // which rewrites the blueprint and re-places on the next run).
        place(&env, &Instance { name: "coder".into(), model: "sonnet".into(), mirror_root: None }, None, &caps)
            .unwrap();
        // A key the user added by hand: a regenerate would destroy it.
        let settings = env.join("settings.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        v.as_object_mut().unwrap().insert("effortLevel".into(), "high".into());
        std::fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        place(&env, &Instance { name: "coder".into(), model: "opus".into(), mirror_root: None }, None, &caps).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        // settings.json is the only channel carrying the model to Claude Code.
        assert_eq!(v["model"], "opus");
        // The hand-added key survived, and so did the hooks.
        assert_eq!(v["effortLevel"], "high");
        assert!(registers_command(&v["hooks"]["SessionEnd"], "session-end.py"));
        assert!(registers_command(&v["hooks"]["Stop"], "speak.py"));
    }

    #[test]
    fn a_foreign_hook_merely_containing_speak_py_survives() {
        // The predicate was an unanchored contains("speak.py"), and `retain`
        // drops the whole GROUP — so a user's own my_speak.py took an unrelated
        // sibling command with it, on every run rather than once at enable.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        std::fs::create_dir_all(&env).unwrap();
        std::fs::write(
            env.join("settings.json"),
            r#"{"hooks":{"Stop":[
                 {"hooks":[
                   {"type":"command","command":"python tools/my_speak.py"},
                   {"type":"command","command":"python unrelated.py"}
                 ]}
               ]}}"#,
        )
        .unwrap();

        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        place(&env, &inst, None, &Capabilities::default()).unwrap(); // and again

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        assert!(registers_command(&v["hooks"]["Stop"], "my_speak.py"), "foreign hook deleted");
        assert!(registers_command(&v["hooks"]["Stop"], "unrelated.py"), "sibling deleted");
        assert!(registers_command(&v["hooks"]["Stop"], OWNED_SPEAK), "ours not installed");
    }

    #[test]
    fn placement_migrates_a_hand_installed_absolute_path_hook() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        std::fs::create_dir_all(&env).unwrap();
        // An env as they exist today: the hook wired in by hand against a
        // checkout, plus an unrelated Stop hook that must survive untouched.
        std::fs::write(
            env.join("settings.json"),
            r#"{"hooks":{"Stop":[
                 {"hooks":[{"type":"command","command":"python \"C:/checkout/revoiced/speak.py\""}]},
                 {"hooks":[{"type":"command","command":"python notify.py"}]}
               ]}}"#,
        )
        .unwrap();

        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        place(&env, &inst, Some("# persona\n"), &Capabilities::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        // The absolute path is gone, replaced by the env-relative one — not
        // added beside it, which would speak every response twice.
        assert_eq!(stop.len(), 2);
        assert!(!v["hooks"]["Stop"].to_string().contains("C:/checkout"));
        assert!(registers_command(&v["hooks"]["Stop"], OWNED_SPEAK));
        // An unrelated hook is left alone.
        assert!(registers_command(&v["hooks"]["Stop"], "notify.py"));
    }

    #[test]
    fn place_seeds_sync_and_scaffolds_selected_files() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        let caps = Capabilities { changelog: true, github: true, ..Default::default() };

        place(&env, &inst, Some("# persona"), &caps).unwrap();

        // /sync skill seeded inside the env, reflecting caps.
        let skill = std::fs::read_to_string(env.join("skills/sync/SKILL.md")).unwrap();
        assert!(skill.contains("Commit + push"));
        assert!(skill.contains("CHANGELOG.md"));
        assert!(!skill.contains("README.md"));

        // Scaffolds land in the PROJECT dir, only for enabled caps.
        assert!(proj.path().join("CHANGELOG.md").exists());
        assert!(!proj.path().join("README.md").exists()); // readme not selected
        assert!(!proj.path().join("docs").exists()); // docs not selected
        assert!(env.join("CLAUDE.md").exists()); // global persona in the env
    }

    /// Placement is still driven by `Capabilities`, so the only thing standing
    /// between a contributor and the maintainer's files is `Role::caps()`. Assert
    /// it end-to-end through `place` rather than trusting the expansion: a
    /// contributor that scaffolds a README would overwrite nothing and look
    /// exactly like working.
    #[test]
    fn each_role_scaffolds_only_its_own_files() {
        use crate::models::Role;
        let cases = [
            (Role::Maintainer, true, true, true),
            (Role::Contributor, true, false, false),
            (Role::Standalone, false, false, false),
        ];
        for (role, changelog, readme, docs) in cases {
            let proj = tempfile::tempdir().unwrap();
            let env = env_dir(proj.path(), "r");
            let inst = Instance { name: "r".into(), model: "opus".into(), mirror_root: None };
            place(&env, &inst, None, &role.caps()).unwrap();

            let label = role.as_str();
            assert_eq!(proj.path().join("CHANGELOG.md").exists(), changelog, "{label} CHANGELOG");
            assert_eq!(proj.path().join("README.md").exists(), readme, "{label} README");
            assert_eq!(proj.path().join("docs").exists(), docs, "{label} docs/");
            // The project CLAUDE.md is the maintainer's alone.
            assert_eq!(
                proj.path().join("CLAUDE.md").exists(),
                role == Role::Maintainer,
                "{label} project CLAUDE.md"
            );
            // Standalone gets no /sync at all; the others do.
            assert_eq!(
                env.join("skills/sync/SKILL.md").exists(),
                role != Role::Standalone,
                "{label} /sync"
            );
            // Every role gets the three universal skills.
            for s in ["handoff", "note", "twosentences"] {
                assert!(env.join(format!("skills/{s}/SKILL.md")).exists(), "{label} /{s}");
            }
        }
    }

    /// A contributor must never be *told* to maintain prose it doesn't own —
    /// the generated skill is the whole instruction surface.
    #[test]
    fn contributor_sync_never_mentions_the_maintainers_files() {
        use crate::models::Role;
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "c");
        let inst = Instance { name: "c".into(), model: "opus".into(), mirror_root: None };
        place(&env, &inst, None, &Role::Contributor.caps()).unwrap();

        let skill = std::fs::read_to_string(env.join("skills/sync/SKILL.md")).unwrap();
        assert!(skill.contains("CHANGELOG.md"), "contributor should keep its changelog step");
        assert!(skill.contains("Commit + push"), "contributor should still commit");
        assert!(!skill.contains("README.md"), "contributor was told about the README");
        assert!(!skill.contains("docs/"), "contributor was told about docs/");
    }

    #[test]
    fn github_cap_gitignores_env_dirs_idempotently() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        let caps = Capabilities { github: true, ..Default::default() };
        let gi = proj.path().join(".gitignore");

        // Pre-existing .gitignore with unrelated content, no trailing newline.
        std::fs::write(&gi, "target/\n*.log").unwrap();

        // First placement appends the entry, preserving existing lines.
        place(&env, &inst, None, &caps).unwrap();
        let after_first = std::fs::read_to_string(&gi).unwrap();
        assert!(after_first.contains("target/"));
        assert!(after_first.contains("*.log"));
        assert_eq!(after_first.matches(".claude-env-*").count(), 1);

        // Second placement must NOT duplicate the entry.
        place(&env, &inst, None, &caps).unwrap();
        let after_second = std::fs::read_to_string(&gi).unwrap();
        assert_eq!(after_second.matches(".claude-env-*").count(), 1);
    }

    /// Both new `github` files land, and — the part worth pinning — the CI
    /// workflow decides the ecosystem **at run time**. Baking a guess in at
    /// placement seeds a workflow that fails forever in every repo of the other
    /// kind, and aello does not know a project's stack when it places into it.
    #[test]
    fn github_cap_seeds_ci_and_renovate_without_guessing_the_stack() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        place(&env, &inst, None, &Capabilities { github: true, ..Default::default() }).unwrap();

        let ci = std::fs::read_to_string(proj.path().join(".github/workflows/ci.yml")).unwrap();
        assert!(ci.contains("detect ecosystem"), "the stack must be detected in the job");
        assert!(ci.contains("pip-audit --strict"));
        assert!(ci.contains("npm audit"));
        // A repo with neither must say so rather than passing silently.
        assert!(ci.contains("no Python, Node or Rust manifest found"));
        assert!(ci.contains("cargo audit"));
        // `npm install` would resolve fresh and paper over a missing lockfile,
        // which is the finding, not something to work around. Match a *command*
        // — a substring search hits the comment explaining why it is not used,
        // exactly as a grep for `git add claude-internal` hits the line
        // forbidding it.
        assert!(ci.lines().any(|l| l.trim() == "npm ci"));
        assert!(
            !ci.lines().any(|l| l.trim().starts_with("npm install")),
            "no step may actually run `npm install`"
        );

        let rn = std::fs::read_to_string(proj.path().join(".github/renovate.json")).unwrap();
        assert!(rn.contains("\"automerge\": false"), "nothing automerges");
        serde_json::from_str::<serde_json::Value>(&rn).expect("renovate.json must be valid JSON");

        // Neither is regenerated over a project's own tuned copy.
        std::fs::write(proj.path().join(".github/renovate.json"), "{\"mine\":true}").unwrap();
        place(&env, &inst, None, &Capabilities { github: true, ..Default::default() }).unwrap();
        assert_eq!(
            std::fs::read_to_string(proj.path().join(".github/renovate.json")).unwrap(),
            "{\"mine\":true}"
        );
    }

    /// The whole point of the destination is that the mirror stops being written
    /// into the public project. A test that only checks the new path exists
    /// would pass while the old one was still being filled in too — and the old
    /// one is the one that publishes.
    #[test]
    fn a_mirror_destination_moves_the_mirror_out_of_the_project() {
        let proj = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let root = elsewhere.path().to_string_lossy().into_owned();

        let env = env_dir(proj.path(), "demo");
        let inst = Instance {
            name: "demo".into(),
            model: "haiku".into(),
            mirror_root: Some(root.clone()),
        };
        let caps = Capabilities { github: true, ..Default::default() };
        place(&env, &inst, None, &caps).unwrap();

        // The memory the mirror is meant to carry landed in the destination…
        let moved = elsewhere.path().join("demo").join("memory").join("MEMORY.md");
        assert!(moved.exists(), "the mirror must be written to the configured destination");
        // …and nothing was written into the public project.
        assert!(
            !proj.path().join("claude-internal").exists(),
            "the in-project mirror must not be written as well — that is the leak"
        );

        // And it is the same path the read-back half resolves, or a restore on
        // the second machine looks at an empty folder and reports success.
        assert_eq!(
            mirror_dir(proj.path(), "demo", Some(&root)),
            elsewhere.path().join("demo")
        );
    }

    /// The version constant and the marker inside the script are two halves of
    /// one fact. If they drift, a placed copy either never upgrades (marker
    /// ahead) or upgrades on every single placement (marker behind) — and both
    /// look like the feature working.
    #[test]
    fn the_pre_commit_marker_matches_the_recorded_version() {
        assert_eq!(
            pre_commit_version(PRE_COMMIT_HOOK),
            Some(PRE_COMMIT_VERSION),
            "the `aello-pre-commit v<N>` marker in src/pre_commit_hook.sh disagrees \
             with PRE_COMMIT_VERSION ({PRE_COMMIT_VERSION})"
        );
    }

    /// The hook is only a guard if it refuses. This runs the real script through
    /// `sh` against a real staged blob, because every property that could break
    /// it — CRLF, the anchored key pattern, the placeholder filter — is
    /// invisible to a test that only checks the file was written.
    #[test]
    #[cfg(windows)]
    fn the_seeded_pre_commit_hook_blocks_a_real_key_and_passes_clean_content() {
        let proj = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(proj.path())
                .args(args)
                .output()
                .expect("git")
        };
        if !run(&["init"]).status.success() {
            return; // no git available; nothing to assert
        }
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        place(&env, &inst, None, &Capabilities { github: true, ..Default::default() }).unwrap();

        let hook = proj.path().join(".githooks").join("pre-commit");
        let bytes = std::fs::read(&hook).unwrap();
        assert!(!bytes.contains(&b'\r'), "the hook must be LF — sh cannot run a CRLF script");

        // Driven through `git commit`, not by invoking `sh` directly: git finds
        // its own shell and honours `core.hooksPath`, so this also proves the
        // config side landed. Calling `sh` here fails on PATH under cargo and
        // would have proved only that the file parses.
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        assert_eq!(
            String::from_utf8_lossy(&run(&["config", "--get", "core.hooksPath"]).stdout).trim(),
            ".githooks",
            "placement must point the clone at .githooks or the hook never runs"
        );

        // Clean content passes. `-A` is what a real /sync does.
        std::fs::write(proj.path().join("ok.md"), "just some prose\n").unwrap();
        run(&["add", "-A"]);
        let ok = run(&["commit", "-m", "clean"]);
        assert!(
            ok.status.success(),
            "a clean commit must not be blocked: {}",
            String::from_utf8_lossy(&ok.stderr)
        );

        // A memory note carrying an armored key does not — this is the exact
        // path the note is about: /sync stages claude-internal/ blind.
        let note = proj.path().join("claude-internal/Demo/memory/leak.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "the server key:\n-----BEGIN OPENSSH PRIVATE KEY-----\nb3Blbn\n").unwrap();
        run(&["add", "claude-internal/Demo/memory/leak.md"]);
        let blocked = run(&["commit", "-m", "leak"]);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&blocked.stdout),
            String::from_utf8_lossy(&blocked.stderr)
        );
        assert!(!blocked.status.success(), "an armored private key must be blocked: {said}");
        assert!(said.contains("PRIVATE KEY"), "the refusal must name what it found: {said}");
    }

    /// A hook the user wrote is theirs. Ours is upgraded in place; anything
    /// without the marker is not touched, or aello silently eats a project's
    /// own commit checks on its next launch.
    #[test]
    fn placement_upgrades_its_own_pre_commit_hook_and_leaves_a_foreign_one_alone() {
        let proj = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join(".git")).unwrap();
        let dir = proj.path().join(".githooks");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pre-commit");

        // Someone else's hook survives untouched.
        std::fs::write(&path, "#!/bin/sh\n# my own checks\nexit 0\n").unwrap();
        seed_pre_commit_hook(proj.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "#!/bin/sh\n# my own checks\nexit 0\n");

        // An older copy of ours is replaced.
        std::fs::write(&path, "#!/bin/sh\n# aello-pre-commit v0 — stale\nexit 0\n").unwrap();
        seed_pre_commit_hook(proj.path()).unwrap();
        assert_eq!(pre_commit_version(&std::fs::read_to_string(&path).unwrap()), Some(PRE_COMMIT_VERSION));

        // The `.gitattributes` line is appended once, not per placement.
        seed_pre_commit_hook(proj.path()).unwrap();
        let ga = std::fs::read_to_string(proj.path().join(".gitattributes")).unwrap();
        assert_eq!(ga.matches(".githooks/* text eol=lf").count(), 1);
    }

    #[test]
    fn github_cap_scaffolds_release_hygiene() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        let caps = Capabilities { github: true, ..Default::default() };

        place(&env, &inst, None, &caps).unwrap();

        let ga = std::fs::read_to_string(proj.path().join(".gitattributes")).unwrap();
        assert!(ga.contains("text=auto"));
        let ver = std::fs::read_to_string(proj.path().join("VERSION")).unwrap();
        assert_eq!(ver.trim(), "0.1.0");
        let wf = std::fs::read_to_string(
            proj.path().join(".github/workflows/version.yml"),
        )
        .unwrap();
        assert!(wf.contains("bump patch in VERSION"));
        assert!(wf.contains("[skip ci]"));

        // A no-github blueprint seeds none of these in a fresh project.
        let fresh = tempfile::tempdir().unwrap();
        let fenv = env_dir(fresh.path(), "bare");
        place(&fenv, &Instance { name: "bare".into(), model: "haiku".into(), mirror_root: None }, None,
              &Capabilities { changelog: true, ..Default::default() }).unwrap();
        assert!(!fresh.path().join(".gitattributes").exists());
        assert!(!fresh.path().join("VERSION").exists());
        assert!(!fresh.path().join(".github").exists());
    }

    #[test]
    fn github_cap_seeds_tracked_claude_internal_mirror() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        let caps = Capabilities { github: true, ..Default::default() };

        place(&env, &inst, Some("# persona snapshot"), &caps).unwrap();

        // Mirror is namespaced per blueprint: claude-internal/<name>/...
        let ci = proj.path().join("claude-internal").join("demo");
        // Persona snapshot is renamed so it never auto-loads as a second CLAUDE.md.
        let persona = std::fs::read_to_string(ci.join("persona.CLAUDE.md")).unwrap();
        assert!(persona.contains("persona snapshot"));
        assert!(!ci.join("CLAUDE.md").exists());
        // Skills + memory mirrored one-way from the live env dir.
        assert!(ci.join("skills/sync/SKILL.md").exists());
        assert!(ci.join("memory/MEMORY.md").exists());
        // Tracked: the mirror is NOT covered by the .claude-env-* gitignore line.
        let gi = std::fs::read_to_string(proj.path().join(".gitignore")).unwrap();
        assert!(gi.lines().all(|l| !l.contains("claude-internal")));

        // A no-github blueprint seeds no claude-internal mirror.
        let fresh = tempfile::tempdir().unwrap();
        let fenv = env_dir(fresh.path(), "bare");
        place(&fenv, &Instance { name: "bare".into(), model: "haiku".into(), mirror_root: None }, Some("# p"),
              &Capabilities { changelog: true, ..Default::default() }).unwrap();
        assert!(!fresh.path().join("claude-internal").exists());
    }

    #[test]
    fn two_blueprints_in_one_repo_keep_separate_mirrors() {
        // Regression: a flat claude-internal/ let the 2nd placement clobber the
        // 1st's persona + sync skill and merge-corrupt memory. Per-blueprint
        // namespacing keeps both mirrors intact.
        let proj = tempfile::tempdir().unwrap();
        let caps = Capabilities { github: true, ..Default::default() };

        let env_a = env_dir(proj.path(), "core");
        place(&env_a, &Instance { name: "core".into(), model: "opus".into(), mirror_root: None },
              Some("# core persona"), &caps).unwrap();
        let env_b = env_dir(proj.path(), "frontend");
        place(&env_b, &Instance { name: "frontend".into(), model: "sonnet".into(), mirror_root: None },
              Some("# frontend persona"), &caps).unwrap();

        // Both mirrors coexist under their own namespace.
        let a = proj.path().join("claude-internal").join("core");
        let b = proj.path().join("claude-internal").join("frontend");
        let pa = std::fs::read_to_string(a.join("persona.CLAUDE.md")).unwrap();
        let pb = std::fs::read_to_string(b.join("persona.CLAUDE.md")).unwrap();
        assert!(pa.contains("core persona")); // not clobbered by frontend
        assert!(pb.contains("frontend persona"));
        // Each keeps its own sync skill + memory.
        assert!(a.join("skills/sync/SKILL.md").exists());
        assert!(b.join("skills/sync/SKILL.md").exists());
        assert!(a.join("memory/MEMORY.md").exists());
        assert!(b.join("memory/MEMORY.md").exists());
    }

    #[test]
    fn mirroring_never_deletes_a_memory_note_the_env_lacks() {
        // Memory used to be a one-way pruning sync, and this test asserted the
        // prune. That is correct only while one machine owns the env dir: a note
        // another device committed has no counterpart here, so the next launch
        // deleted it — silently, and the commit after that recorded the deletion.
        // The mirror keeps it now, and `aello restore` is how it gets adopted.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let caps = Capabilities { github: true, ..Default::default() };
        place(&env, &Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None }, Some("# p"), &caps).unwrap();

        let ci = proj.path().join("claude-internal").join("demo");
        assert!(ci.join("skills/sync/SKILL.md").exists());

        // Stand in for a note this env has never seen by writing it straight into
        // the mirror — which is what `git pull` does.
        std::fs::write(ci.join("memory").join("from-laptop.md"), "other machine\n").unwrap();
        place(&env, &Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None }, Some("# p"), &caps).unwrap();

        assert!(
            ci.join("memory/from-laptop.md").exists(),
            "a pulled note must survive a launch that never saw it"
        );
        assert!(ci.join("skills/sync/SKILL.md").exists(), "skills still mirrored");
        assert!(ci.join("memory/MEMORY.md").exists(), "memory still mirrored");
        assert!(ci.join("persona.CLAUDE.md").exists(), "persona still mirrored");
    }

    #[test]
    fn mirroring_still_prunes_a_skill_the_blueprint_no_longer_seeds() {
        // Skills stay a strict one-way mirror: they are generated from the role on
        // every place, so an orphan is stale output and not another machine's work.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let caps = Capabilities { github: true, ..Default::default() };
        place(&env, &Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None }, Some("# p"), &caps).unwrap();

        let ci = proj.path().join("claude-internal").join("demo");
        std::fs::create_dir_all(ci.join("skills").join("retired")).unwrap();
        std::fs::write(ci.join("skills").join("retired").join("SKILL.md"), "# old\n").unwrap();
        place(&env, &Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None }, Some("# p"), &caps).unwrap();

        assert!(!ci.join("skills/retired").exists(), "stale skill should be pruned");
        assert!(ci.join("skills/sync/SKILL.md").exists(), "live skills survive");
    }

    #[test]
    fn the_handoff_is_snapshotted_into_the_mirror_and_restored_from_it() {
        // The resume note is the one thing that never crossed to a second machine:
        // the root file is deleted on the next boot and gitignored in some repos,
        // and /sync was told in as many words never to commit it. The mirror copy
        // is the crossing, under a name `*.HANDOFF.md` does not match.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let caps = Capabilities { github: true, ..Default::default() };
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        place(&env, &inst, Some("# p"), &caps).unwrap();

        let root_note = proj.path().join("demo.HANDOFF.md");
        std::fs::write(&root_note, "# Handoff\nwhere I got to\n").unwrap();
        place(&env, &inst, Some("# p"), &caps).unwrap();

        let mirrored = proj.path().join("claude-internal").join("demo").join(MIRRORED_HANDOFF);
        assert!(mirrored.exists(), "the handoff must reach the mirror");
        assert!(
            !mirrored.file_name().unwrap().to_string_lossy().contains(".HANDOFF.md"),
            "the mirrored name must not match the *.HANDOFF.md ignore rule"
        );

        // Boot consumes the root file. The snapshot is what a second machine reads.
        std::fs::remove_file(&root_note).unwrap();
        place(&env, &inst, Some("# p"), &caps).unwrap();
        assert!(mirrored.exists(), "an absent root note must not delete the snapshot");

        let r = restore(proj.path(), &env, "demo", None).unwrap();
        assert!(r.handoff);
        assert_eq!(
            std::fs::read_to_string(&root_note).unwrap(),
            "# Handoff\nwhere I got to\n",
            "restore puts the note back where SessionStart reads it"
        );
    }

    #[test]
    fn restore_adopts_the_mirror_without_deleting_local_work() {
        // The inbound half. `place` only restores a *missing* env dir, so coming
        // home to an existing one left the pulled notes unread — and it must not
        // fix that by overwriting whatever this machine wrote in the meantime.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let caps = Capabilities { github: true, ..Default::default() };
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        place(&env, &inst, Some("# persona from here"), &caps).unwrap();

        let mem = memory_dir(&env, proj.path());
        std::fs::write(mem.join("local-only.md"), "written here, never synced\n").unwrap();

        // What a `git pull` leaves behind.
        let ci = proj.path().join("claude-internal").join("demo");
        std::fs::write(ci.join("memory").join("from-laptop.md"), "the other machine\n").unwrap();
        std::fs::write(ci.join("persona.CLAUDE.md"), "# a different persona\n").unwrap();

        let r = restore(proj.path(), &env, "demo", None).unwrap();

        assert!(mem.join("from-laptop.md").exists(), "the pulled note lands where memory is read");
        assert!(mem.join("local-only.md").exists(), "an unsynced local note survives");
        assert_eq!(
            std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(),
            "# persona from here",
            "the persona is never clobbered — it is reported instead"
        );
        assert!(r.persona_differs, "and the difference is reported");
        assert!(r.memory >= 2, "counts what the mirror holds: {}", r.memory);
    }

    #[test]
    fn restore_seeds_a_persona_only_when_the_env_has_none() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let ci = proj.path().join("claude-internal").join("demo");
        std::fs::create_dir_all(ci.join("memory")).unwrap();
        std::fs::write(ci.join("persona.CLAUDE.md"), "# the snapshot\n").unwrap();
        std::fs::create_dir_all(&env).unwrap();

        let r = restore(proj.path(), &env, "demo", None).unwrap();
        assert_eq!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(), "# the snapshot\n");
        assert!(!r.persona_differs, "seeding an absent persona is not a conflict");
    }

    #[test]
    fn a_crlf_checkout_is_not_a_persona_conflict() {
        // The cross-OS trap. `* text=auto` (which the github role scaffolds) stores
        // the snapshot LF and checks it out with the platform's newlines, while the
        // env's CLAUDE.md is never touched by git and keeps the writer's. Comparing
        // bytes told the user their persona had diverged, and to run `aello persona`
        // over it, when the two files were the same text — on the exact
        // Windows-to-Linux round trip `restore` is for. Measured on a real repo
        // before this landed.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let ci = proj.path().join("claude-internal").join("demo");
        std::fs::create_dir_all(ci.join("memory")).unwrap();
        std::fs::create_dir_all(&env).unwrap();

        let text = "# Persona\nline two\n";
        std::fs::write(env.join("CLAUDE.md"), text).unwrap();
        std::fs::write(ci.join("persona.CLAUDE.md"), text.replace('\n', "\r\n")).unwrap();

        let r = restore(proj.path(), &env, "demo", None).unwrap();
        assert!(!r.persona_differs, "newlines alone must not read as a divergence");

        // A real difference still does.
        std::fs::write(ci.join("persona.CLAUDE.md"), "# Persona\r\nsomething else\r\n").unwrap();
        let r = restore(proj.path(), &env, "demo", None).unwrap();
        assert!(r.persona_differs, "a genuine edit must still be reported");
    }

    #[test]
    fn restore_refuses_when_there_is_no_mirror() {
        // Reporting success against a mirror that was never written is the empty
        // result this project keeps getting wrong: a standalone blueprint has none.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        std::fs::create_dir_all(&env).unwrap();
        let err = restore(proj.path(), &env, "demo", None).unwrap_err();
        assert!(format!("{err}").contains("no mirror at"), "got: {err}");
    }

    #[test]
    fn a_restored_env_reads_memory_at_this_machines_path_spelling() {
        // The cross-device case: the mirror was committed by a machine whose
        // project path encodes differently (a Windows drive letter vs a POSIX
        // home). Both directions derive the component from the project path they
        // are handed, so neither carries the other machine's spelling.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let ci = proj.path().join("claude-internal").join("demo");
        std::fs::create_dir_all(ci.join("memory")).unwrap();
        std::fs::write(ci.join("memory").join("MEMORY.md"), "- index\n").unwrap();
        // A stale encoded dir from the other machine, as a clone would carry it.
        let foreign = env.join("projects").join("C--Users-someone-else-repo").join("memory");
        std::fs::create_dir_all(&foreign).unwrap();

        restore(proj.path(), &env, "demo", None).unwrap();

        let here = memory_dir(&env, proj.path());
        assert!(here.join("MEMORY.md").exists(), "restored into this machine's encoded dir");
        assert!(!foreign.join("MEMORY.md").exists(), "not into the other machine's");
    }

    #[test]
    fn dropping_the_github_cap_clears_the_tracked_mirror() {
        // Inside the `github` gate, `mirror_env_internal` was never called once
        // the cap went off — so the folder froze in git forever, still carrying a
        // github-flavoured /sync skill (git sections, the Bash tool) that the
        // blueprint no longer has. `remove --purge` was the only way out.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        let ci = proj.path().join("claude-internal").join("demo");

        place(&env, &inst, Some("# p"), &Capabilities { github: true, ..Default::default() }).unwrap();
        assert!(ci.join("skills/sync/SKILL.md").exists());

        // Drop github while something else stays on, so the blueprint still has
        // a /sync skill — it just must not be tracked any more. No *role*
        // produces this combination (every role with a cap has github), so this
        // exercises the placement layer directly: the mirror follows
        // `caps.github` alone, whatever else is set.
        place(&env, &inst, Some("# p"), &Capabilities { readme: true, ..Default::default() }).unwrap();
        assert!(!ci.exists(), "the mirror should not survive the cap being dropped");
        // The live env is untouched — the mirror is a copy, never the source.
        assert!(env.join("skills/sync/SKILL.md").exists());
        // A sibling blueprint's mirror is not collateral damage.
        let other = proj.path().join("claude-internal").join("other");
        std::fs::create_dir_all(&other).unwrap();
        place(&env, &inst, Some("# p"), &Capabilities { readme: true, ..Default::default() }).unwrap();
        assert!(other.exists(), "another blueprint's mirror must survive");
    }

    /// The mirror is tracked so a clone still has the blueprint's memory,
    /// skills and persona — and until this, the first `aello run` after a clone
    /// seeded a bare env and mirrored it straight over them.
    #[test]
    fn a_fresh_clone_is_restored_from_the_mirror_not_erased_by_it() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into(), mirror_root: None };
        let caps = Capabilities { github: true, ..Default::default() };
        let mirror = proj.path().join("claude-internal").join("demo");

        place(&env, &inst, Some("# seeded persona"), &caps).unwrap();

        // A session's worth of accumulation: a memory note, a hand-kept skill,
        // and a persona the user accepted.
        let mem = env
            .join("projects")
            .join(crate::sessions::encode_project_path(proj.path()))
            .join("memory");
        std::fs::write(mem.join("aello-overview.md"), "what aello is").unwrap();
        std::fs::create_dir_all(env.join("skills").join("regenerate")).unwrap();
        std::fs::write(env.join("skills").join("regenerate").join("SKILL.md"), "# custom").unwrap();
        std::fs::write(env.join("CLAUDE.md"), "# accepted persona gen1").unwrap();
        place(&env, &inst, Some("# seeded persona"), &caps).unwrap();
        assert!(mirror.join("memory").join("aello-overview.md").exists());

        // The clone: the env dir is gitignored, so it is simply not there.
        std::fs::remove_dir_all(&env).unwrap();
        place(&env, &inst, Some("# seeded persona"), &caps).unwrap();

        assert!(mem.join("aello-overview.md").exists(), "memory was not restored");
        assert!(
            env.join("skills").join("regenerate").join("SKILL.md").exists(),
            "a non-generated skill was not restored"
        );
        assert_eq!(
            std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(),
            "# accepted persona gen1",
            "the accepted persona was replaced by the seeded one"
        );
        // And the mirror still holds it — the restore ran before the sync back.
        assert!(mirror.join("memory").join("aello-overview.md").exists());
    }

    /// The memory source path is *derived* (it encodes `current_dir()`'s exact
    /// spelling), so "the directory isn't there" can mean the derivation was
    /// wrong rather than that the user deleted eleven notes. Prune only what a
    /// present source says is gone.
    #[test]
    fn a_missing_memory_source_leaves_the_mirror_alone() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        std::fs::create_dir_all(env.join("skills")).unwrap();
        let mirror_mem = proj.path().join("claude-internal").join("demo").join("memory");
        std::fs::create_dir_all(&mirror_mem).unwrap();
        std::fs::write(mirror_mem.join("MEMORY.md"), "- [a](a.md)").unwrap();

        // No `projects/<encoded>/memory` in the env at all.
        mirror_env_internal(proj.path(), &env, "demo", true, None).unwrap();

        assert!(
            mirror_mem.join("MEMORY.md").exists(),
            "an unfindable source pruned the tracked memory"
        );
    }

    /// Every placement ignores its env dir, whatever the role. This used to be
    /// `github`-only, and the default role is `standalone` — so the env that is
    /// most likely to hold a `.credentials.json` (no shared token configured)
    /// was the one nothing kept out of `git add -A`.
    #[test]
    fn every_role_gitignores_its_env_dir() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "haiku".into(), mirror_root: None };

        // standalone: no caps at all.
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        let gi = std::fs::read_to_string(proj.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".claude-env-*"), "standalone env dir was not ignored");
        // The rest of the github scaffolding stays gated.
        assert!(!proj.path().join(".gitattributes").exists());

        // Still exactly one line after a second placement.
        place(&env, &inst, None, &Capabilities { changelog: true, ..Default::default() }).unwrap();
        let gi = std::fs::read_to_string(proj.path().join(".gitignore")).unwrap();
        assert_eq!(gi.matches(".claude-env-*").count(), 1);
    }

    /// An unreadable `.gitignore` is not an empty one. `read_to_string` fails on
    /// any non-UTF-8 byte, and this function rewrites the whole file — so the
    /// old `unwrap_or_default()` replaced every rule the repo had with one line.
    #[test]
    fn an_unreadable_gitignore_is_refused_not_replaced() {
        let proj = tempfile::tempdir().unwrap();
        let gi = proj.path().join(".gitignore");
        // UTF-16LE *with the BOM*, which is what Notepad writes when told to.
        // The BOM is load-bearing here: BOM-less UTF-16 ASCII is byte-for-byte
        // valid UTF-8 (the high bytes are NULs), so it reads back as NUL-laced
        // text and appends without error — corrupted, but not caught. `FF FE`
        // is the byte pair that no UTF-8 string can contain.
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("target/\n*.log\n".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&gi, &utf16).unwrap();

        let err = ensure_gitignore_entry(proj.path(), ".claude-env-*").unwrap_err();
        assert!(err.to_string().contains(".gitignore"), "the error should name the file: {err}");
        assert_eq!(std::fs::read(&gi).unwrap(), utf16, "the user's .gitignore was rewritten");
    }

    #[test]
    fn place_seeds_starter_memory_and_never_clobbers_it() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        place(&env, &inst, None, &Capabilities::default()).unwrap();

        let mem = env
            .join("projects")
            .join(crate::sessions::encode_project_path(proj.path()))
            .join("memory");
        let index = mem.join("MEMORY.md");
        let ws = mem.join("working-style.md");

        // Fresh placement seeds the index + the bundled working-style memory.
        assert!(index.exists());
        assert!(ws.exists());
        assert!(std::fs::read_to_string(&index).unwrap().contains("working-style.md"));
        assert!(std::fs::read_to_string(&ws).unwrap().contains("does not read plans"));

        // A re-place over a user-edited MEMORY.md leaves it (and memory) untouched.
        std::fs::write(&index, "- my own memory\n").unwrap();
        std::fs::remove_file(&ws).unwrap();
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&index).unwrap(), "- my own memory\n");
        assert!(!ws.exists()); // not re-seeded while a MEMORY.md exists
    }

    #[test]
    fn rename_allows_a_case_only_change() {
        // On Windows/macOS the default filesystem is case-insensitive, so the
        // destination "already exists" — it *is* the source. The guard named the
        // source as the obstruction and the documented feature was unreachable on
        // both platforms aello ships binaries for.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        place(&env, &inst, Some("# p"), &Capabilities { github: true, ..Default::default() }).unwrap();

        assert!(rename_placed(proj.path(), crate::models::Agent::Claude, "coder", "Coder").unwrap());

        let renamed = env_dir(proj.path(), "Coder");
        assert!(renamed.join("settings.json").exists());
        assert_eq!(load_instance(&renamed).unwrap().name, "Coder");
        // No temp directory is left parked in the project.
        let leftovers: Vec<_> = std::fs::read_dir(proj.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".aello-rename-"))
            .collect();
        assert!(leftovers.is_empty(), "temp dir left behind: {leftovers:?}");
    }

    #[test]
    fn rename_carries_the_handoff_and_note_files() {
        // Both are addressed by blueprint name and both consumers key off the new
        // one, so a note left under the old name has no producer and no reader.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        place(&env, &Instance { name: "coder".into(), model: "opus".into(), mirror_root: None }, None, &Capabilities::default())
            .unwrap();
        std::fs::write(proj.path().join("coder.HANDOFF.md"), "resume me\n").unwrap();
        std::fs::write(proj.path().join("coder.NOTE.md"), "inbox\n").unwrap();

        rename_placed(proj.path(), crate::models::Agent::Claude, "coder", "reviewer").unwrap();

        assert!(!proj.path().join("coder.HANDOFF.md").exists());
        assert_eq!(
            std::fs::read_to_string(proj.path().join("reviewer.HANDOFF.md")).unwrap(),
            "resume me\n"
        );
        assert_eq!(
            std::fs::read_to_string(proj.path().join("reviewer.NOTE.md")).unwrap(),
            "inbox\n"
        );
    }

    #[test]
    fn rename_never_clobbers_another_envs_inbox() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        place(&env, &Instance { name: "coder".into(), model: "opus".into(), mirror_root: None }, None, &Capabilities::default())
            .unwrap();
        std::fs::write(proj.path().join("coder.NOTE.md"), "mine\n").unwrap();
        std::fs::write(proj.path().join("reviewer.NOTE.md"), "theirs\n").unwrap();

        rename_placed(proj.path(), crate::models::Agent::Claude, "coder", "reviewer").unwrap();

        // The occupied destination wins; the rename still succeeds.
        assert_eq!(
            std::fs::read_to_string(proj.path().join("reviewer.NOTE.md")).unwrap(),
            "theirs\n"
        );
    }

    #[test]
    fn place_registers_the_session_start_hook_and_script() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        // An env from before SessionStart existed: settings.json is never
        // clobbered, so the hook has to be healed into it.
        std::fs::create_dir_all(&env).unwrap();
        std::fs::write(env.join("settings.json"), r#"{"model":"opus"}"#).unwrap();
        place(&env, &inst, None, &Capabilities::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        assert!(registers_command(&v["hooks"]["SessionStart"], "session-start.py"));
        assert!(env.join("hooks/session-start.py").exists());
    }

    #[test]
    fn a_user_prompt_submit_hook_replaces_the_persona_tldr_section() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        // The hook is bundled, so a fresh env carries the instruction per turn
        // and the persona is left exactly as the blueprint wrote it.
        place(&env, &inst, Some("# Persona\n"), &Capabilities::default()).unwrap();
        assert_eq!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(), "# Persona\n");

        // Unregister it by hand and the persona append comes back — the voice
        // needs the instruction to exist somewhere, so this is the fallback.
        let settings = env.join("settings.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        v["hooks"].as_object_mut().unwrap().remove("UserPromptSubmit");
        std::fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        // ensure_own_hook would just heal it back, so assert on the helper that
        // actually decides: with no registration, the append happens.
        assert!(!injects_tldr_per_turn(&env));
        ensure_tldr_instruction(&env).unwrap();
        assert!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap().contains("TL;DR"));
    }

    #[test]
    fn accepting_a_persona_overwrites_it_and_bumps_the_generation() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        place(&env, &inst, Some("# stock\n"), &Capabilities::default()).unwrap();

        assert_eq!(persona_generation(&env), 0); // nothing accepted yet

        let (gen, date) = accept_persona(&env, "# generated\n").unwrap();
        assert_eq!(gen, 1);
        assert_eq!(date.len(), 10, "date should be YYYY-MM-DD, got {date:?}");
        assert_eq!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(), "# generated\n");
        assert_eq!(persona_generation(&env), 1);

        // A later placement must not put the template back over it. place() only
        // writes when absent, and `custom` resolves to None anyway — both belts.
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        assert_eq!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(), "# generated\n");

        // Generations accumulate rather than resetting.
        let (gen2, _) = accept_persona(&env, "# second\n").unwrap();
        assert_eq!(gen2, 2);
        assert_eq!(persona_generation(&env), 2);
    }

    #[test]
    fn today_utc_is_a_sane_iso_date() {
        let d = today_utc();
        let parts: Vec<&str> = d.split('-').collect();
        assert_eq!(parts.len(), 3, "{d}");
        let y: i32 = parts[0].parse().unwrap();
        let m: u32 = parts[1].parse().unwrap();
        let day: u32 = parts[2].parse().unwrap();
        assert!((2024..2100).contains(&y), "year {y} out of range");
        assert!((1..=12).contains(&m), "month {m}");
        assert!((1..=31).contains(&day), "day {day}");
    }

    #[test]
    fn place_registers_the_user_prompt_submit_hook_and_script() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        // An env placed before the hook existed: settings.json is never
        // clobbered, so the registration has to be healed into it.
        std::fs::create_dir_all(&env).unwrap();
        std::fs::write(env.join("settings.json"), r#"{"model":"opus"}"#).unwrap();
        place(&env, &inst, None, &Capabilities::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        assert!(registers_command(&v["hooks"]["UserPromptSubmit"], "user-prompt-submit.py"));
        assert!(env.join("hooks/user-prompt-submit.py").exists());
    }

    /// The plan-mode block, healed into an env placed before it existed. The
    /// matcher is asserted too: without it the hook fires on every tool call,
    /// which is a Python spawn per Read rather than two per session.
    #[test]
    fn place_registers_the_plan_mode_block_with_a_matcher() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        std::fs::create_dir_all(&env).unwrap();
        std::fs::write(env.join("settings.json"), r#"{"model":"opus"}"#).unwrap();
        place(&env, &inst, None, &Capabilities::default()).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        assert!(registers_command(&v["hooks"]["PreToolUse"], "pre-tool-use.py"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], PLAN_TOOLS);
        assert!(env.join("hooks/pre-tool-use.py").exists());

        // Idempotent: a second placement must not stack a duplicate group.
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(env.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    }

    /// A fresh env gets the block from `settings_json`, not from the heal path —
    /// the two write it independently, so both need asserting.
    #[test]
    fn fresh_settings_json_blocks_plan_mode() {
        let v: serde_json::Value = serde_json::from_str(&settings_json("opus")).unwrap();
        assert!(registers_command(&v["hooks"]["PreToolUse"], "pre-tool-use.py"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], PLAN_TOOLS);
        // Both plan tools must be in the matcher, or the block is half-open.
        for tool in ["EnterPlanMode", "ExitPlanMode"] {
            assert!(PLAN_TOOLS.contains(tool), "matcher no longer covers {tool}");
            assert!(
                PRE_TOOL_USE_SCRIPT.contains(tool),
                "pre-tool-use.py no longer denies {tool}"
            );
        }
        assert!(PRE_TOOL_USE_SCRIPT.contains("\"deny\""));
    }

    #[test]
    fn user_prompt_submit_hook_carries_the_response_rules() {
        // Injected on every prompt in every env, so the wording is the product.
        // Each rule is here because its absence was felt: padding, agreement
        // before evaluation, plans handed over instead of questions, an answer
        // that had to be read end to end to be acted on, and a missing TL;DR
        // that leaves the voice silent. Named without a count on purpose —
        // the rules have been four, then five, then four again in two days,
        // and the test renaming each time told nobody anything.
        // The instruction is a run of adjacent Python string literals, so a
        // phrase can straddle two source lines and be absent from the file as
        // written. Rejoin the seams first — otherwise reflowing a paragraph
        // fails the test with "no longer says", which reads as a dropped rule.
        let script = USER_PROMPT_SUBMIT_SCRIPT.replace("\r\n", "\n").replace("\"\n    \"", "");
        for needle in [
            "Be concise",
            "no preamble",
            "hedging",
            "praise or agreement",
            "soften a finding",
            "I don't know",
            "plan for approval",
            "never use plan mode",
            "concrete options",
            "prose to a few sentences",
            "3–4 numbered steps",
            "must stand alone",
            "Their actions, not yours",
            "TL;DR: <two to four sentences>",
            "on one line",
            "read aloud",
        ] {
            assert!(
                script.contains(needle),
                "user-prompt-submit.py no longer says {needle:?}"
            );
        }
    }

    #[test]
    fn a_kept_skill_survives_placement() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };
        let caps = Capabilities { github: true, ..Default::default() };

        // First placement generates the skills.
        place(&env, &inst, None, &caps).unwrap();

        // Hand-edit /sync and /note, and mark only /sync kept.
        let sync = env.join("skills/sync/SKILL.md");
        let note = env.join("skills/note/SKILL.md");
        std::fs::write(&sync, "# my VPS deploy sync\n").unwrap();
        std::fs::write(&note, "# my note\n").unwrap();
        std::fs::write(env.join("skills/sync").join(KEEP_MARKER), "").unwrap();

        place(&env, &inst, None, &caps).unwrap();

        // The marked one is untouched; the unmarked one is regenerated.
        assert_eq!(std::fs::read_to_string(&sync).unwrap(), "# my VPS deploy sync\n");
        assert_ne!(std::fs::read_to_string(&note).unwrap(), "# my note\n");
    }

    #[test]
    fn a_kept_sync_is_not_removed_when_caps_go_empty() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into(), mirror_root: None };

        place(&env, &inst, None, &Capabilities { github: true, ..Default::default() }).unwrap();
        let sync = env.join("skills/sync/SKILL.md");
        std::fs::write(&sync, "# hand-written\n").unwrap();
        std::fs::write(env.join("skills/sync").join(KEEP_MARKER), "").unwrap();

        // Dropping every cap normally deletes /sync — a kept one stays, since it
        // is not generated from caps in the first place.
        place(&env, &inst, None, &Capabilities::default()).unwrap();
        assert_eq!(std::fs::read_to_string(&sync).unwrap(), "# hand-written\n");
    }

    #[test]
    fn rename_moves_env_dir_mirror_and_updates_instance() {
        let proj = tempfile::tempdir().unwrap();
        let caps = Capabilities { github: true, ..Default::default() };
        let old_env = env_dir(proj.path(), "old");
        place(&old_env, &Instance { name: "old".into(), model: "opus".into(), mirror_root: None },
              Some("# p"), &caps).unwrap();
        assert!(old_env.exists());
        assert!(proj.path().join("claude-internal/old").exists());

        // Rename moves both the env dir and the tracked mirror.
        assert!(rename_placed(proj.path(), crate::models::Agent::Claude, "old", "new").unwrap());
        let new_env = env_dir(proj.path(), "new");
        assert!(!old_env.exists());
        assert!(new_env.exists());
        assert!(!proj.path().join("claude-internal/old").exists());
        assert!(proj.path().join("claude-internal/new").exists());

        // .aello.toml now names the new blueprint; the model is preserved.
        let inst = load_instance(&new_env).unwrap();
        assert_eq!(inst.name, "new");
        assert_eq!(inst.model, "opus");

        // A blueprint not placed in this project renames to a clean no-op.
        let fresh = tempfile::tempdir().unwrap();
        assert!(!rename_placed(fresh.path(), crate::models::Agent::Claude, "x", "y").unwrap());

        // A destination env dir that already exists is refused, not clobbered.
        let taken = env_dir(proj.path(), "taken");
        place(&taken, &Instance { name: "taken".into(), model: "haiku".into(), mirror_root: None },
              None, &caps).unwrap();
        assert!(rename_placed(proj.path(), crate::models::Agent::Claude, "new", "taken").is_err());
    }

    #[test]
    fn rename_mirror_collision_does_not_move_env_dir() {
        // Destination env dir is free but its mirror already exists: the rename
        // must fail WITHOUT half-moving the env dir (else config and disk would
        // diverge and `run <old>` would re-scaffold a fresh env).
        let proj = tempfile::tempdir().unwrap();
        let caps = Capabilities { github: true, ..Default::default() };
        let src_env = env_dir(proj.path(), "src");
        place(&src_env, &Instance { name: "src".into(), model: "opus".into(), mirror_root: None },
              Some("# p"), &caps).unwrap();
        // A stray mirror at the destination, with no matching env dir.
        std::fs::create_dir_all(proj.path().join("claude-internal/dest")).unwrap();

        assert!(rename_placed(proj.path(), crate::models::Agent::Claude, "src", "dest").is_err());
        // The source env dir is untouched — nothing half-moved.
        assert!(src_env.exists());
        assert!(!env_dir(proj.path(), "dest").exists());
        assert!(proj.path().join("claude-internal/src").exists());
    }

    #[test]
    fn gitignore_entry_dedups_trailing_slash_variant() {
        let proj = tempfile::tempdir().unwrap();
        std::fs::write(proj.path().join(".gitignore"), "node_modules\n.claude-env-*/\n").unwrap();
        ensure_gitignore_entry(proj.path(), ".claude-env-*").unwrap();
        let after = std::fs::read_to_string(proj.path().join(".gitignore")).unwrap();
        // The `.claude-env-*/` line already covers it — no near-duplicate added.
        assert_eq!(after.matches(".claude-env-*").count(), 1);
    }

    #[test]
    fn place_without_caps_seeds_no_sync_skill() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "sonnet".into(), mirror_root: None };

        place(&env, &inst, None, &Capabilities::default()).unwrap();

        assert!(!env.join("skills/sync/SKILL.md").exists());
        assert!(!proj.path().join("CHANGELOG.md").exists());
    }

    #[test]
    fn place_always_seeds_universal_skills_even_with_no_caps() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "sonnet".into(), mirror_root: None };

        // No caps at all — /sync is skipped, but /handoff and /twosentences are
        // universal.
        place(&env, &inst, None, &Capabilities::default()).unwrap();

        let handoff = env.join("skills/handoff/SKILL.md");
        assert!(handoff.exists());
        let s = std::fs::read_to_string(&handoff).unwrap();
        assert!(s.contains("name: handoff"));
        assert!(s.contains("bare.HANDOFF.md")); // filename prefixed with blueprint name

        let two = env.join("skills/twosentences/SKILL.md");
        assert!(two.exists());
        let t = std::fs::read_to_string(&two).unwrap();
        assert!(t.contains("name: twosentences"));
        assert!(t.contains("exactly two sentences"));

        let note = env.join("skills/note/SKILL.md");
        assert!(note.exists());
        let n = std::fs::read_to_string(&note).unwrap();
        assert!(n.contains("name: note"));
        assert!(n.contains("<target>.NOTE.md")); // leaves a note for another env
        assert!(n.contains("from bare")); // attributed to this blueprint
    }
}
