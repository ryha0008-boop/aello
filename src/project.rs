//! Placing a blueprint into a project: the env dir, its `.aello.toml`,
//! `settings.json`, optional CLAUDE.md, and the PostCompact hook script.

use crate::models::{Capabilities, Instance};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const POST_COMPACT_SCRIPT: &str = include_str!("hooks_post_compact.py");
const SESSION_END_SCRIPT: &str = include_str!("hooks_session_end.py");
const SESSION_START_SCRIPT: &str = include_str!("hooks_session_start.py");

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

/// The `HOOK_VERSION` of the vendored copy above. Upstream bumps its constant
/// whenever one of the five hook-path files changes, so comparing the two is
/// how this copy learns it has fallen behind — see the test at the bottom of
/// this file, which fails if a re-vendor moves the scripts without moving this.
///
/// A version, not a commit sha: revoiced's CI commits a `VERSION` bump on every
/// push to main, so local work rebases onto that and every unpushed sha is
/// rewritten. A recorded sha goes stale by itself; a recorded version cannot.
/// Surfaced by `aello voice status`, so checking a machine does not mean
/// finding an env dir and running Python in it.
pub const HOOK_VERSION: u32 = 8;

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

/// Env dir for a blueprint inside a project — `project/.claude-env-<name>`.
pub fn env_dir(project: &Path, name: &str) -> PathBuf {
    project.join(format!(".claude-env-{name}"))
}

/// Move a placed blueprint's on-disk artifacts when it's renamed: the env dir
/// `.claude-env-<old>` → `.claude-env-<new>`, the `name` in its `.aello.toml`,
/// and the tracked `claude-internal/<old>/` mirror → `<new>/`. Returns true when
/// the env dir was present (i.e. the blueprint is placed in this project);
/// false is a clean no-op for a blueprint that isn't placed here. Errors if a
/// destination already exists, so a rename never clobbers another env. Skills
/// and mirror content that embed the old name are refreshed on the next `run`.
pub fn rename_placed(project: &Path, old: &str, new: &str) -> Result<bool> {
    let old_env = env_dir(project, old);
    if !old_env.exists() {
        return Ok(false);
    }
    let new_env = env_dir(project, new);
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

/// Mark the env as onboarded so interactive `claude` skips its first-run
/// wizard (theme/login) and goes straight in — auth is handled by the shared
/// token. Merges `hasCompletedOnboarding: true` into `.claude.json`.
pub fn mark_onboarded(env_dir: &Path) -> Result<()> {
    let path = env_dir.join(".claude.json");
    let mut v: serde_json::Value = match std::fs::read_to_string(&path) {
        // Claude Code owns this file too; if it exists but we can't parse it,
        // leave it untouched rather than overwrite real state with `{}`.
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        },
        Err(_) => serde_json::json!({}),
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
        // Same for the voice hook, so an env placed before voice was universal
        // starts speaking on its next run.
        sync_voice_hooks(&settings)?;
        // And the model, so `aello edit <name> --model` actually reaches the env.
        ensure_model(&settings, &inst.model)?;
        // Stop Claude Code deleting the transcripts contextdb points at.
        ensure_cleanup_period(&settings)?;
    }

    // Global persona — set once, never clobbered (the user may have edited it).
    if let Some(content) = claude_md {
        let path = env_dir.join("CLAUDE.md");
        if !path.exists() {
            std::fs::write(&path, content).context("could not write CLAUDE.md")?;
        }
    }

    // The voice hook speaks the trailing TL;DR line, so the persona has to ask
    // for one. Appended (never clobbering) so an existing persona keeps its text.
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
        std::fs::write(&skill, crate::templates::render_sync_skill(caps, &inst.name))
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
    scaffold_project(project, env_dir, &inst.name, caps)?;

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
    if caps.github {
        // Keep env dirs (and the credentials inside them) out of the repo.
        ensure_gitignore_entry(project, ".claude-env-*")?;
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
    mirror_env_internal(project, env_dir, blueprint, caps.github)?;
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
) -> Result<()> {
    let dest = project.join("claude-internal").join(blueprint);
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
    let mem = env_dir
        .join("projects")
        .join(crate::sessions::encode_project_path(project))
        .join("memory");
    copy_dir_all(&mem, &dest.join("memory"))
        .context("could not mirror memory into claude-internal")?;
    let persona = env_dir.join("CLAUDE.md");
    if persona.exists() {
        std::fs::create_dir_all(&dest).context("could not create claude-internal dir")?;
        std::fs::copy(&persona, dest.join("persona.CLAUDE.md"))
            .context("could not snapshot persona into claude-internal")?;
    }
    Ok(())
}

/// One-way *sync* of `src` into `dst`: copy every regular file/subdir from `src`,
/// then delete anything in `dst` that no longer exists in `src`. Pruning keeps
/// the tracked mirror from accumulating orphaned files — a deleted memory note,
/// or a skill the blueprint no longer seeds — which a copy-only mirror would keep
/// committing forever. (Dropping the `github` cap is handled a level up, in
/// `mirror_env_internal`, which removes the folder outright rather than diffing
/// it.) Symlinks are skipped: the env is the
/// single source of truth and must not pull foreign content into git. A missing
/// `src` prunes `dst` entirely (nothing left to mirror).
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        if dst.exists() {
            std::fs::remove_dir_all(dst).context("could not prune stale mirror dir")?;
        }
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

/// Ensure `entry` exists as its own line in the project's `.gitignore`, creating
/// the file or appending as needed. Idempotent — a matching line (ignoring
/// surrounding whitespace) is never duplicated. Preserves existing content.
fn ensure_gitignore_entry(project: &Path, entry: &str) -> Result<()> {
    let path = project.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
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
  "hooks": {{{stop}
    "SessionStart": [{{"hooks":[{{"type":"command","command":"{py} \"$CLAUDE_CONFIG_DIR/hooks/session-start.py\""}}]}}],
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
const CLEANUP_PERIOD_DAYS: u32 = 365;

/// Self-heal the retention setting into an env placed before it existed. Only
/// fills it in when **absent** — a value the user chose themselves is theirs,
/// including a deliberately short one.
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
    if obj.contains_key("cleanupPeriodDays") {
        return Ok(());
    }
    obj.insert("cleanupPeriodDays".into(), CLEANUP_PERIOD_DAYS.into());
    std::fs::write(settings, serde_json::to_string_pretty(&v)?)
        .context("could not update settings.json with the transcript retention")
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
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
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

/// True when this env carries the TL;DR instruction on a `UserPromptSubmit`
/// hook, so `place` should leave the persona alone.
///
/// Not something aello scaffolds — it is opt-in, registered by hand in that
/// env's `settings.json`. The point is that the instruction survives a persona
/// rewritten from scratch, which the appended copy does not.
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
    let group = serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": format!("{py} \"$CLAUDE_CONFIG_DIR/hooks/{script}\""),
        }]
    });
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
        let inst = Instance { name: "coder".into(), model: "opus".into() };

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
    fn settings_keep_transcripts_past_claude_codes_default_retention() {
        let s = settings_json("opus");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let days = v["cleanupPeriodDays"].as_u64().expect("cleanupPeriodDays must be set");
        assert!(days > 30, "retention {days} is not longer than Claude Code's 30-day default");
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
        let inst = Instance { name: "coder".into(), model: "opus".into() };
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
        // The persona must ask for the line the hook speaks.
        let persona = std::fs::read_to_string(env.join("CLAUDE.md")).unwrap();
        assert!(persona.starts_with("# persona"));
        assert!(persona.contains("TL;DR"));
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
        "6a66082553fedaa3c1bb69e14517b336257fd06e83accc7a7f52756343d028e5";

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
        let inst = Instance { name: "coder".into(), model: "opus".into() };
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
        // The persona picked up the instruction it was placed without.
        assert!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap().contains("TL;DR"));

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
        place(&env, &Instance { name: "coder".into(), model: "sonnet".into() }, None, &caps)
            .unwrap();
        // A key the user added by hand: a regenerate would destroy it.
        let settings = env.join("settings.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        v.as_object_mut().unwrap().insert("effortLevel".into(), "high".into());
        std::fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        place(&env, &Instance { name: "coder".into(), model: "opus".into() }, None, &caps).unwrap();

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

        let inst = Instance { name: "coder".into(), model: "opus".into() };
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

        let inst = Instance { name: "coder".into(), model: "opus".into() };
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
        let inst = Instance { name: "coder".into(), model: "opus".into() };
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
            let inst = Instance { name: "r".into(), model: "opus".into() };
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
        let inst = Instance { name: "c".into(), model: "opus".into() };
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
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
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

    #[test]
    fn github_cap_scaffolds_release_hygiene() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
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
        place(&fenv, &Instance { name: "bare".into(), model: "haiku".into() }, None,
              &Capabilities { changelog: true, ..Default::default() }).unwrap();
        assert!(!fresh.path().join(".gitattributes").exists());
        assert!(!fresh.path().join("VERSION").exists());
        assert!(!fresh.path().join(".github").exists());
    }

    #[test]
    fn github_cap_seeds_tracked_claude_internal_mirror() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
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
        place(&fenv, &Instance { name: "bare".into(), model: "haiku".into() }, Some("# p"),
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
        place(&env_a, &Instance { name: "core".into(), model: "opus".into() },
              Some("# core persona"), &caps).unwrap();
        let env_b = env_dir(proj.path(), "frontend");
        place(&env_b, &Instance { name: "frontend".into(), model: "sonnet".into() },
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
    fn mirror_prunes_files_deleted_from_the_env() {
        // The mirror is a one-way sync: a file removed from the env must not
        // linger in the tracked claude-internal/ folder.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let caps = Capabilities { github: true, ..Default::default() };
        place(&env, &Instance { name: "demo".into(), model: "haiku".into() }, Some("# p"), &caps).unwrap();

        let ci = proj.path().join("claude-internal").join("demo");
        assert!(ci.join("skills/sync/SKILL.md").exists());

        // Delete a memory note from the live env, then re-place. (A memory note
        // is the reachable case: the earlier version of this test hand-deleted
        // the /sync skill, a state production can't reach — the skill is only
        // removed when no caps are left, which implies github is off, which used
        // to mean the mirror never ran at all.)
        let mem = env
            .join("projects")
            .join(crate::sessions::encode_project_path(proj.path()))
            .join("memory");
        std::fs::write(mem.join("scratch.md"), "temporary\n").unwrap();
        place(&env, &Instance { name: "demo".into(), model: "haiku".into() }, Some("# p"), &caps).unwrap();
        assert!(ci.join("memory/scratch.md").exists(), "new note reaches the mirror");

        std::fs::remove_file(mem.join("scratch.md")).unwrap();
        place(&env, &Instance { name: "demo".into(), model: "haiku".into() }, Some("# p"), &caps).unwrap();

        // The orphaned copy is gone; the rest of the mirror survives.
        assert!(!ci.join("memory/scratch.md").exists(), "stale note should be pruned");
        assert!(ci.join("skills/sync/SKILL.md").exists(), "skills still mirrored");
        assert!(ci.join("memory/MEMORY.md").exists(), "memory still mirrored");
        assert!(ci.join("persona.CLAUDE.md").exists(), "persona still mirrored");
    }

    #[test]
    fn dropping_the_github_cap_clears_the_tracked_mirror() {
        // Inside the `github` gate, `mirror_env_internal` was never called once
        // the cap went off — so the folder froze in git forever, still carrying a
        // github-flavoured /sync skill (git sections, the Bash tool) that the
        // blueprint no longer has. `remove --purge` was the only way out.
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "demo");
        let inst = Instance { name: "demo".into(), model: "haiku".into() };
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

    #[test]
    fn no_github_cap_writes_no_gitignore() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "haiku".into() };
        let caps = Capabilities { changelog: true, ..Default::default() };

        place(&env, &inst, None, &caps).unwrap();
        assert!(!proj.path().join(".gitignore").exists());
    }

    #[test]
    fn place_seeds_starter_memory_and_never_clobbers_it() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into() };

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
        let inst = Instance { name: "coder".into(), model: "opus".into() };
        place(&env, &inst, Some("# p"), &Capabilities { github: true, ..Default::default() }).unwrap();

        assert!(rename_placed(proj.path(), "coder", "Coder").unwrap());

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
        place(&env, &Instance { name: "coder".into(), model: "opus".into() }, None, &Capabilities::default())
            .unwrap();
        std::fs::write(proj.path().join("coder.HANDOFF.md"), "resume me\n").unwrap();
        std::fs::write(proj.path().join("coder.NOTE.md"), "inbox\n").unwrap();

        rename_placed(proj.path(), "coder", "reviewer").unwrap();

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
        place(&env, &Instance { name: "coder".into(), model: "opus".into() }, None, &Capabilities::default())
            .unwrap();
        std::fs::write(proj.path().join("coder.NOTE.md"), "mine\n").unwrap();
        std::fs::write(proj.path().join("reviewer.NOTE.md"), "theirs\n").unwrap();

        rename_placed(proj.path(), "coder", "reviewer").unwrap();

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
        let inst = Instance { name: "coder".into(), model: "opus".into() };

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
        let inst = Instance { name: "coder".into(), model: "opus".into() };

        // Without the hook, the section is appended to a persona lacking it.
        place(&env, &inst, Some("# Persona\n"), &Capabilities::default()).unwrap();
        assert!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap().contains("TL;DR"));

        // With it registered, a persona cleared of the section stays cleared.
        std::fs::write(env.join("CLAUDE.md"), "# Persona\n").unwrap();
        let settings = env.join("settings.json");
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        v["hooks"]["UserPromptSubmit"] = serde_json::json!([{
            "hooks": [{"type": "command",
                       "command": "python \"$CLAUDE_CONFIG_DIR/hooks/user-prompt-submit.py\""}]
        }]);
        std::fs::write(&settings, serde_json::to_string_pretty(&v).unwrap()).unwrap();

        place(&env, &inst, Some("# Persona\n"), &Capabilities::default()).unwrap();
        assert_eq!(std::fs::read_to_string(env.join("CLAUDE.md")).unwrap(), "# Persona\n");
    }

    #[test]
    fn a_kept_skill_survives_placement() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "coder");
        let inst = Instance { name: "coder".into(), model: "opus".into() };
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
        let inst = Instance { name: "coder".into(), model: "opus".into() };

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
        place(&old_env, &Instance { name: "old".into(), model: "opus".into() },
              Some("# p"), &caps).unwrap();
        assert!(old_env.exists());
        assert!(proj.path().join("claude-internal/old").exists());

        // Rename moves both the env dir and the tracked mirror.
        assert!(rename_placed(proj.path(), "old", "new").unwrap());
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
        assert!(!rename_placed(fresh.path(), "x", "y").unwrap());

        // A destination env dir that already exists is refused, not clobbered.
        let taken = env_dir(proj.path(), "taken");
        place(&taken, &Instance { name: "taken".into(), model: "haiku".into() },
              None, &caps).unwrap();
        assert!(rename_placed(proj.path(), "new", "taken").is_err());
    }

    #[test]
    fn rename_mirror_collision_does_not_move_env_dir() {
        // Destination env dir is free but its mirror already exists: the rename
        // must fail WITHOUT half-moving the env dir (else config and disk would
        // diverge and `run <old>` would re-scaffold a fresh env).
        let proj = tempfile::tempdir().unwrap();
        let caps = Capabilities { github: true, ..Default::default() };
        let src_env = env_dir(proj.path(), "src");
        place(&src_env, &Instance { name: "src".into(), model: "opus".into() },
              Some("# p"), &caps).unwrap();
        // A stray mirror at the destination, with no matching env dir.
        std::fs::create_dir_all(proj.path().join("claude-internal/dest")).unwrap();

        assert!(rename_placed(proj.path(), "src", "dest").is_err());
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
        let inst = Instance { name: "bare".into(), model: "sonnet".into() };

        place(&env, &inst, None, &Capabilities::default()).unwrap();

        assert!(!env.join("skills/sync/SKILL.md").exists());
        assert!(!proj.path().join("CHANGELOG.md").exists());
    }

    #[test]
    fn place_always_seeds_universal_skills_even_with_no_caps() {
        let proj = tempfile::tempdir().unwrap();
        let env = env_dir(proj.path(), "bare");
        let inst = Instance { name: "bare".into(), model: "sonnet".into() };

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
