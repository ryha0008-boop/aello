//! Built-in CLAUDE.md persona templates, bundled into the binary.
//!
//! A blueprint's `claude_md` is one of three values — `coder` (a coding project
//! starts here), `none` (anything that isn't one, blank until it earns a
//! persona) and `custom` (the env's own CLAUDE.md, written by
//! `/regenerate-claude-md-accept`) — or a filesystem path, still accepted for
//! the blueprints that point at a persona file the user maintains.
//!
//! `resolve` turns that into the content to place, or `None` when the config is
//! not the source of the persona.

use crate::models::Capabilities;
use anyhow::{Context, Result};

const CODER: &str = include_str!("../templates/coder.md");

/// The value meaning "this env's own `CLAUDE.md` is the persona".
///
/// Set by `/regenerate-claude-md-accept` once a hand-written persona has been
/// accepted into an env. `resolve` returns nothing for it, so `place` writes no
/// persona and the file already in the env dir stands — which is what makes a
/// generated persona survive every later run.
pub const CUSTOM: &str = "custom";

/// The value meaning "no global persona", written explicitly rather than left
/// absent so a blank persona reads as a decision instead of an oversight. Every
/// non-IT project starts here and stays until it has earned a custom one.
pub const NONE: &str = "none";

/// Persona values the add flow offers.
///
/// Three, deliberately: a coding project starts on `coder`, anything else starts
/// blank, and both become `custom` the first time a persona is generated for
/// them. `sysadmin` was a fourth and was dropped — it was barely used and read
/// as coder with different nouns, so it earned neither the maintenance nor the
/// slot in the picker.
///
/// A **path** is still accepted by `resolve` and is not listed here: five
/// blueprints point at persona files the user maintains, three of them sharing
/// one file on purpose, and collapsing those to `custom` would freeze the copies
/// and silently end the sharing.
#[allow(dead_code)] // referenced only by the TUI's guard test
pub const BUILTINS: &[&str] = &[CODER_NAME, NONE, CUSTOM];

/// The one bundled template's name.
pub const CODER_NAME: &str = "coder";

/// Content of a built-in template by name, or None if not a builtin.
pub fn builtin(name: &str) -> Option<&'static str> {
    match name {
        CODER_NAME => Some(CODER),
        _ => None,
    }
}

/// Persona section appended when the `voice` capability is on. The TTS hook
/// speaks the trailing `TL;DR:` line and nothing else, so without this the hook
/// has nothing to say — it blocks the turn once asking for the line, then gives
/// up. Appended to whatever persona the blueprint uses (built-in or custom)
/// rather than baked into the templates, so non-voice blueprints stay unchanged.
pub const VOICE_TLDR: &str = r#"
## End every response with a TL;DR

End **every** response with a final line in exactly this form:

```
TL;DR: <two sentences>
```

Two sentences, no more. Say what happened and what it means or what's next —
the outcome, not the steps. No bullets, no bold, nothing after it.

This is not decoration: the voice hook speaks that line and nothing else, so it
is the only part of a response that gets heard. A response without it is
rejected and you will be asked to add one before the turn can end. The user
skims for keywords rather than reading in full, so the TL;DR is also the part
they actually read.
"#;

/// Resolve a blueprint's `claude_md` value to the persona content `place`
/// should write, or `None` when it should write nothing.
///
/// `None` covers the two cases where the config is not the source of the
/// persona: `none` (there is no persona) and `custom` (the env's own
/// `CLAUDE.md` is the persona, and overwriting it would destroy the generated
/// text). Both return `Ok(None)` rather than an error — this is the normal
/// steady state for most envs, not a fault.
pub fn resolve(claude_md: &str) -> Result<Option<String>> {
    if claude_md == CUSTOM || claude_md == NONE {
        return Ok(None);
    }
    if let Some(content) = builtin(claude_md) {
        return Ok(Some(content.to_string()));
    }
    std::fs::read_to_string(claude_md)
        .map(Some)
        .with_context(|| format!("claude_md '{claude_md}' is not a built-in template or a readable file"))
}

/// The manual-only banner, carried by every seeded skill.
///
/// `disable-model-invocation: true` in the frontmatter already stops the model
/// calling one of these as a tool. What it does not stop — and what actually
/// happened on 2026-08-03 — is an agent opening the file with `Read` and
/// carrying out the steps by hand, which is the same thing reached by a
/// different route: the user believed `/sync` had run when it had not. The
/// frontmatter closes the tool path; this closes the reading path.
fn manual_only(cmd: &str) -> String {
    format!(
        "> **Only the user runs this.** It happens when they type `/{cmd}`, and at no
> other time. If you are reading this file because *you* decided to — to see what
> it does, or to carry out its steps yourself — then stop and do neither.
> Following these instructions **is** running the skill, whichever route you took
> to them, and a checkpoint the user did not ask for is one they will believe
> happened when it did not. Say the skill exists and let them invoke it.

"
    )
}

/// Footer on every generated skill. All four are rewritten on each `place`, and
/// the person most likely to need that fact is whoever is editing one in place —
/// who, without this, finds out when their edit silently disappears.
fn keep_footer(skill: &str) -> String {
    format!(
        "
---

*aello regenerates this skill on every run, so edits made here are replaced. To
keep a version you have rewritten for this project, create an empty
`.aello-keep` file beside this one (`skills/{skill}/.aello-keep`) — aello then
leaves the skill alone, and will not delete it either. A kept skill no longer
tracks the blueprint's role; remove the marker to return to the generated
version.*
"
    )
}

/// Generate a `/sync` SKILL.md tailored to a blueprint's capabilities, so the
/// skill only covers what this blueprint actually maintains — a no-GitHub
/// blueprint gets no git/commit/push talk at all. `name` is the blueprint name,
/// used for the `Env:` commit trailer. Caller seeds it only when at least one
/// capability is enabled (`Capabilities::any`).
pub fn render_sync_skill(caps: &Capabilities, name: &str, mirror_root: Option<&str>) -> String {
    let manual = manual_only("sync");
    let tools = if caps.github {
        "Bash, Read, Edit, Write, Grep, Glob"
    } else {
        "Read, Edit, Write, Grep, Glob"
    };
    let tail = if caps.github { ", then commit and push" } else { "" };

    let mut s = format!(
        "---
name: sync
description: Checkpoint the project — reconcile the docs this blueprint maintains against the current code{tail}. Invoke manually with /sync.
disable-model-invocation: true
allowed-tools: {tools}
---

# /sync — project checkpoint

{manual}When invoked, reconcile the docs this project maintains so they match the current code{tail}. Invoking this skill is your authorization to do so.
"
    );

    if caps.github {
        s.push_str(
            "
## Repo health
- Run `git rev-parse --is-inside-work-tree`. If this is not a git repo, tell the user and stop — this blueprint expects one.
- Check for an `origin` remote (`git remote get-url origin`). If there is none, warn and offer to create one with `gh repo create` — do NOT create it without explicit confirmation.
- Report the current branch (warn on detached HEAD), `git fetch` (best-effort), then report ahead/behind vs the upstream.
- Show a short `git status` summary.

## Dependency hygiene
The rule, once, so it is not re-derived per project: **ranges in the manifest, exact versions in the lock, deploys install from the lock and never resolve, nothing automerges, and a lock is verified by installing it — not by reading it.**

- **Assert a lock exists; do not create one.** Python → `uv.lock`, or a `requirements.txt` that was *compiled* (exact pins, ideally hashes) rather than hand-written. Node → a committed `package-lock.json`. A manifest with no lock **is the finding** — report it and stop there. Compiling a lock changes what installs, and on a live system that is a deploy, which is not something a checkpoint gets to do unasked.
- **A hand-written `requirements.txt` listing only direct imports is the same finding**, even though a file exists. Everything transitive is then unpinned and resolves to whatever is newest that day. Measured in one repo on 2026-08-11: a **beta** release of a signing library had installed itself into the code path that signs every order, and nobody chose it. Check whether the transitive set is pinned, not whether the file is present.
- **Check the lock against what is actually deployed** where you can reach it. A pin read off a developer's desktop can be *older* than the server, in which case installing the manifest moves production backwards on every run — that also happened in the same repo.
- If `.github/workflows/ci.yml` is absent, say so: without it the tests and the audit run only where somebody remembers to type them, which is never the server.
- **Stage `.github/` explicitly when you touch it.** A workflow that exists on disk and was never committed does nothing, and it hides well: untracked because unstaged, skipped because untracked. Confirm with `git status --porcelain .github/` before you finish rather than assuming.
- Renovate does nothing until its GitHub App is installed, which is the user's step and not yours. If `.github/renovate.json` exists, say whether you can see any Renovate PRs or a dependency dashboard issue; absent evidence, report it as **seeded, not confirmed running**.
",
        );
    }

    let mut roles: Vec<&str> = Vec::new();
    if caps.project_md {
        roles.push("- **CLAUDE.md** (project root) — project-specific instructions and context for this codebase. Keep it accurate as the project evolves. This is the *project* CLAUDE.md, separate from the global persona.");
    }
    if caps.readme {
        roles.push("- **README.md** — user-facing entry point: what the project is, install, usage, and the command/feature reference. Must reflect current behavior.");
    }
    if caps.changelog {
        roles.push("- **CHANGELOG.md** — version history of user-facing changes. Add new entries under `[Unreleased]` (create it if missing). Match the file's existing style.");
    }
    if caps.docs {
        roles.push("- **docs/** — deeper, topic-by-topic reference docs. Keep each page consistent with actual behavior; don't just duplicate the README.");
    }
    // Memory is reconciled before any doc (and before the github mirror below)
    // so the checkpoint captures what this env learned this session.
    s.push_str(
        "
## Reconcile memory, then docs
**Memory first** — before any doc, refresh this env's memory so the checkpoint (and the mirror below, if any) captures what you've learned this session. Review `MEMORY.md` and the per-fact files in this env's memory dir: add new facts, correct stale ones, prune what's wrong, and keep the one-line `MEMORY.md` index in sync. Report: memory updated / already-fresh.

**Finding the memory dir — do not construct the path by hand.** It is `$CLAUDE_CONFIG_DIR/projects/<encoded-cwd>/memory/`, where `<encoded-cwd>` is this project's path with **every non-alphanumeric character folded to `-`**. Guessing that encoding is a reliable way to create a second, empty memory dir that nothing reads. List `$CLAUDE_CONFIG_DIR/projects/` and use the directory that is there (normally exactly one).
",
    );
    if !roles.is_empty() {
        s.push_str(
            "
Then, for each doc file below that exists, compare it against the current code and recent commits, then make it accurate. This is a **two-way reconcile, not append-only**: add what's missing, **correct what's now wrong**, and **delete what no longer applies**. Report per file: updated / already-fresh / skipped (absent).

",
        );
        s.push_str(&roles.join("\n"));
        s.push('\n');
    }

    if caps.github {
        s.push_str(&format!(
            "
## Mirror this env's internal config (tracked)
Version-control this env's internal config by mirroring it into the tracked `claude-internal/{name}/` folder at the repo root, so the skills, commands, memory, and persona that live in the gitignored `.claude-env-{name}/` dir are captured in git. The folder is **namespaced per blueprint** (`claude-internal/{name}/`) so multiple blueprints sharing this repo don't clobber each other's mirror. The live env dir stays the **single source of truth** — this is a **one-way copy** from it, refreshing only what changed.
- Self-heal first: `mkdir -p claude-internal/{name}/skills claude-internal/{name}/memory claude-internal/{name}/commands` — an env placed before this step won't have the folder yet, so create it.
- Mirror `.claude-env-{name}/skills/` → `claude-internal/{name}/skills/`. **Copy the whole directory, not the skills you know about** — a hand-added or vendored skill is exactly the one nobody remembers to list, and a mirror missing it looks complete.
- Mirror `.claude-env-{name}/commands/` → `claude-internal/{name}/commands/`, under the same never-delete rule as memory below. aello writes no command, so every one of them is hand-written and the mirror is the only copy that crosses machines.
- Mirror this env's memory dir (`.claude-env-{name}/projects/<this-project>/memory/`) → `claude-internal/{name}/memory/`. **Add and overwrite; never delete a mirror note that has no counterpart in the env dir** — on a repo worked from two machines that note is the other machine's, and dropping it here is how it disappears. Deleting one for real is a deliberate `git rm`.
- Snapshot `.claude-env-{name}/CLAUDE.md` → `claude-internal/{name}/persona.CLAUDE.md` — **keep this exact name**, never `CLAUDE.md`, so Claude Code does not auto-load the snapshot as a second persona.
- If `{name}.HANDOFF.md` exists at the project root, snapshot it → `claude-internal/{name}/handoff.md`. **Lowercase `handoff.md`, and this copy IS committed** — it is the only way the resume note reaches another machine, since the root file is gitignored in some repos and deleted on the next boot in all of them. So run `/handoff` **before** `/sync`, not after: a note written afterwards misses this step and stays on this machine. Absent means nothing was written this session — leave the existing snapshot alone rather than deleting it.
- **Read what you are about to publish, before you stage it.** This folder is your memory and your handoff, and it is the one part of this commit that nobody wrote for a reader. Memory notes are exactly where a session writes down a credential it just used — a provider API key, an OAuth token, an SSH key, a database password, the contents of a `.env`. Grep the mirror for them (`sk-ant-`, `ghp_`, `AKIA`, `BEGIN .* PRIVATE KEY`, `password`, `api_key`, `token`) and **read the hits**. If one is real key material: **stop, do not stage, do not commit**, remove it from the env's memory note as well as the mirror, and tell the user which note it was in. Do not note it and carry on — a commit is published the moment it is pushed, and rewriting history is not a fix once it is.
- Identifiers are not the concern — IPs, hostnames, machine paths and domains are all fine — credentials and passwords are.
{destination}- A `pre-commit` hook in `.githooks/` refuses the commit if key material reaches the index anyway; the check above is so you find it *before* you have written it into a commit message and a mirror. **Never pass `--no-verify`.** If the hook fires, it found something real — read what it named.
",
            destination = match mirror_root {
                // The mirror goes to another repo's working tree, because this
                // one is public. Staging it here would publish exactly what the
                // destination exists to keep private, so the skill must not
                // contain the in-project `git add` line at all.
                Some(root) => format!(
                    "- **This blueprint's mirror does NOT live in this repo.** It goes to `{root}/{name}/`, which is a working tree of a *separate, private* repo — this project is public, and the mirror is your memory. **Do not `git add claude-internal/` here**; if that folder exists in this repo it is a leftover and should be `git rm -r --cached`'d and gitignored.\n\
                     - Commit and push it in the destination repo, as its own step: `git -C {root} add {name}` → `git -C {root} commit -m \"memory: <what changed>\"` → `git -C {root} pull --rebase` → `git -C {root} push`. If the destination does not exist or is not a git repo, **stop and tell the user** — do not fall back to mirroring into this repo, which is the failure this setting exists to prevent.\n"
                ),
                // The default. The check is the safe-by-default half: a mirror
                // in a public repo is a publish, and nothing else in the chain
                // would notice.
                None => format!(
                    "- **Check this repo's visibility before staging: `gh repo view --json visibility`.** If it comes back `PUBLIC`, **stop**. Publishing this env's memory, persona and handoff to the world is a decision the user makes, not a side effect of `/sync`. Tell them, and point at `aello edit {name} --mirror-dir <path>`, which sends the mirror to a working tree of a private repo instead and leaves this repo's code public. If `gh` is missing or the repo has no GitHub remote, say which and treat it as unanswered rather than assuming private.\n\
                     - Stage it by explicit path: `git add claude-internal/{name}`. This folder is tracked on purpose — it is *not* covered by the `.claude-env-*` gitignore line.\n"
                ),
            }
        ));
        s.push_str(&format!(
            "
## Commit + push
- Stage **only the files you created or modified in this session**, plus any docs you reconciled above — by explicit path (e.g. `git add path/a path/b`). **Never `git add -A` / `git add .`** — a blanket stage sweeps unrelated untracked files (other tooling's scaffolding, another env's in-flight work) into your commit. Run `git status` first; unstage anything you didn't touch. Then commit with a clear message summarizing what changed.
- **Never stage the transient env files.** `<blueprint>.HANDOFF.md` (a resume note to yourself) and `<blueprint>.NOTE.md` (another env's inbox) live at the project root, are **created during the session**, and are **deleted on the next boot** by the SessionStart hook. The rule above would otherwise sweep them in — a handoff you wrote this session *is* a file you created this session. They are deliberately not gitignored, because they are meant to be visible while they exist. Leave them untracked, and never commit one. This is about the files **at the project root** — `claude-internal/{name}/handoff.md`, the snapshot the mirror step takes, is tracked on purpose and does get staged.
- **End every commit message with a trailer line `Env: {name}`** (after a blank line) so the commit records which aello blueprint made it. Your git author identity is already set to this blueprint; the trailer makes it visible in the message body too.
- **After committing, before pushing, run `git pull --rebase origin <current-branch>`** to integrate any commits the remote gained since you last fetched — another machine, another blueprint working in this repo, or CI auto-committing a version bump. This replays your commit on top so the push is a fast-forward — skipping it leaves you a commit behind and the *next* `/sync` push gets rejected.
- Push to `origin` on the current branch. If the push fails for a missing upstream, set it: `git push -u origin <branch>`.
- Report the final state: branch, commit sha, push result, and the remote URL.

Use normal prose for commit messages. Don't skip hooks or force-push unless the user explicitly asks.
"
        ));
    }

    s.push_str(&keep_footer("sync"));
    s
}

/// Generate the `/handoff` SKILL.md. Unlike `/sync`, this is **universal** —
/// seeded for every blueprint regardless of capabilities — because a clean
/// session handoff is useful even for a blueprint that maintains no docs. At
/// session end it writes a self-contained `<name>.HANDOFF.md` resume note so the
/// next session picks up seamlessly after a full `/clear` (which, unlike a
/// compact, leaves no summary behind). The filename is prefixed with the
/// blueprint `name` so co-located blueprints don't clobber each other's handoff.
pub fn render_handoff_skill(name: &str) -> String {
    let keep = keep_footer("handoff");
    let manual = manual_only("handoff");
    format!(
        "---
name: handoff
description: Write a self-contained {name}.HANDOFF.md resume note so the next session continues seamlessly after a /clear. Invoke manually with /handoff.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /handoff — session resume note

{manual}When invoked, write a `{name}.HANDOFF.md` at the project root that lets the
**next** session resume this work with **zero prior context**. Invoking this
skill is your authorization to do so.

The filename is prefixed with this blueprint's name (`{name}`) so multiple
blueprints sharing one repo each keep their own handoff without clobbering each
other. Write exactly `{name}.HANDOFF.md`, no other name.

A handoff is not a compact: after a `/clear` there is no conversation summary to
fall back on, so `{name}.HANDOFF.md` must be **fully self-contained**. Assume the
reader boots fresh, has never seen this conversation, and reads only this file
plus the pointers it names.

`{name}.HANDOFF.md` is **transient and untracked** — it is read on boot, then
deleted. Begin the file with a one-line banner: `> Transient resume note ({name}). Read on boot, then delete.`

Write these sections, in order:

1. **Read first** — point the next session at its durable context before
   anything else: the env persona (`$CLAUDE_CONFIG_DIR/CLAUDE.md`) and the
   memory index (`$CLAUDE_CONFIG_DIR/projects/<this-project>/memory/MEMORY.md`).
   Tell it to read those before acting on this note.
2. **Shipped this session** — what actually changed, with commit shas (run
   `git log --oneline` for the recent ones) and a one-line summary each. Note
   anything committed-but-not-pushed or staged-but-not-committed.
3. **Open threads / next steps** — what is in flight, what was deferred, and the
   concrete next action. Be specific enough to act on without re-deriving it.
4. **Gotchas** — traps the next session would otherwise hit: failing/flaky
   tests, environment quirks, decisions made and why, paths that matter.

Keep it tight and skimmable. Then tell the user the note is written and remind
them it is deleted on next boot.

**The file at the project root is never committed.** It is untracked on purpose
and gone by the next boot, so if you run `/sync` after this, leave
`{name}.HANDOFF.md` out of the staging list — it is a file you created this
session, and that rule does not cover it.

What *is* committed is the copy `/sync`'s mirror step snapshots to
`claude-internal/{name}/handoff.md`, and that copy is how this note reaches a
second machine working the same repo. It only happens if the snapshot is taken
after you write — so the order is **`/handoff` first, then `/sync`**. Mention that
to the user if they are about to switch machines.
{keep}"
    )
}

/// Generate the `/note` SKILL.md. Also **universal** — seeded for every
/// blueprint. Unlike `/handoff` (a note to *yourself* for your next session),
/// `/note` leaves a note for **another** environment sharing this repo: when this
/// blueprint touches something the other env owns, or hits a problem on the
/// other side, it records what it was doing and what the other env must fix. The
/// target env is passed as the skill argument; `name` is this (authoring)
/// blueprint, woven in so the note is attributed.
pub fn render_note_skill(name: &str) -> String {
    let keep = keep_footer("note");
    let manual = manual_only("note");
    format!(
        "---
name: note
description: Leave a note for another aello environment about something it needs to fix. Invoke manually with /note <env-name>.
disable-model-invocation: true
allowed-tools: Write, Read, Bash
---

# /note — leave a note for another environment

{manual}When invoked, leave a note addressed to **another** aello environment. The
argument is that environment's name (e.g. `/note frontend`). Invoking this skill
is your authorization to write the note.

This is **not** a handoff. `/handoff` is a note to *yourself* for your next
session; `/note` is a message to a *different* environment — you touched
something it owns, or hit a problem on its side, and it needs to act.

You are the **`{name}`** environment. The note goes to `<target>.NOTE.md` at the
**target's** project root. That file is the target env's inbox — its SessionStart
hook reads the note on boot, delivers it, and deletes the file.

Steps:
1. Take the target env name from the argument; if none was given, ask which
   environment the note is for and stop. Use the blueprint's **canonical
   casing** (`RevoicedMainDev`, not `revoicedmaindev`) — the hook looks for
   exactly `<Name>.NOTE.md`, so wrong casing is a silent dead letter on a
   case-sensitive filesystem.
2. **Work out the target's project root — it is not always this repo.** Look for
   `.claude-env-<target>/` here first. If it is not here, the target env lives in
   a different repo and the note belongs at **that** repo's root: its
   SessionStart hook only ever reads its own project root, so a note left here
   would never be delivered. Locate that repo (ask the user for the path if you
   cannot) rather than writing a note nobody will read. Only if you cannot
   establish where the target lives should you treat the name as a typo and
   check with the user.
3. **Overwrite** `<target>.NOTE.md` at that project root with a single, current
   note — do not append to or preserve any old note. The target reads a note as
   soon as you leave it, so only the latest matters; a fresh note supersedes
   whatever was there.
4. Write, in this order:
   - A one-line banner: `> Note for the <target> env from {name}. Read it, act on it, then delete this file.`
   - A `## from {name} — <timestamp>` heading (get the timestamp with `date`).
   - **What I was doing** — the task that led here.
   - **The problem** — what is broken or blocked on the target's side, concretely.
   - **What you need to fix** — the specific change or decision you need from the
     target env, naming the files/paths involved.
   Keep it tight and actionable; the target boots without your context.

Then tell the user the note was written, naming the **full path** — so a note
sent into another repo is obviously that, and not mistaken for one left here.
{keep}"
    )
}

/// Generate the `/twosentences` SKILL.md. Like `/handoff` this is **universal**
/// — seeded for every blueprint regardless of capabilities. It condenses the
/// previous assistant response into exactly two sentences; a pure text task, so
/// it needs no tools.
pub fn render_twosentences_skill() -> String {
    let manual = manual_only("twosentences");
    let body = format!("---
name: twosentences
description: Summarize your previous response in exactly two sentences. Invoke manually with /twosentences.
disable-model-invocation: true
allowed-tools:
---

# /twosentences — two-sentence summary

{manual}When invoked, condense your **previous response** (the most recent assistant
message before this invocation) into **exactly two sentences**.

Output only those two sentences — no preamble, no heading, no bullets, no code,
nothing else. Keep the key facts and the outcome; drop detail, caveats, and
step-by-step explanation.
");
    format!("{body}{}", keep_footer("twosentences"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_resolve() {
        assert!(resolve("coder").unwrap().unwrap().contains("coding agent"));
    }

    /// `custom` and `none` must resolve to "write nothing", not to an error and
    /// not to empty content. `custom` is the load-bearing one: it means the
    /// env's own CLAUDE.md is the persona, so anything that made `place` write
    /// here would overwrite a generated persona with a template.
    #[test]
    fn custom_and_none_resolve_to_no_content() {
        assert_eq!(resolve(CUSTOM).unwrap(), None);
        assert_eq!(resolve(NONE).unwrap(), None);
    }

    #[test]
    fn unknown_name_is_path_error() {
        assert!(resolve("definitely-not-a-file-or-builtin").is_err());
    }

    #[test]
    fn sync_skill_omits_git_when_no_github() {
        let caps = Capabilities { project_md: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder", None);
        assert!(s.contains("CLAUDE.md"));
        assert!(!s.contains("git "));
        assert!(!s.contains("commit and push"));
        assert!(!s.contains("Env: coder")); // no commit trailer without github
        assert!(s.contains("Memory first")); // memory reconcile renders for every blueprint
        assert!(!s.contains("claude-internal")); // mirror is github-only
        assert!(s.contains("allowed-tools: Read, Edit, Write, Grep, Glob"));
    }

    #[test]
    fn sync_skill_includes_git_and_only_selected_docs() {
        let caps = Capabilities { github: true, changelog: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder", None);
        assert!(s.contains("Repo health"));
        assert!(s.contains("Commit + push"));
        assert!(s.contains("git pull --rebase origin")); // rebase before push, so the next push fast-forwards
        assert!(s.contains("Env: coder")); // per-blueprint commit trailer
        assert!(s.contains("CHANGELOG.md"));
        assert!(!s.contains("README.md"));
        assert!(s.contains("allowed-tools: Bash,"));
    }

    #[test]
    fn sync_skill_reconciles_memory_before_docs() {
        let caps = Capabilities { changelog: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder", None);
        let mem = s.find("Memory first").expect("memory step present");
        let doc = s.find("CHANGELOG.md").expect("doc role present");
        assert!(mem < doc, "memory must be reconciled before the docs");
    }

    #[test]
    fn handoff_skill_is_self_contained_and_manual() {
        let s = render_handoff_skill("coder");
        assert!(s.contains("name: handoff"));
        assert!(s.contains("disable-model-invocation: true"));
        assert!(s.contains("allowed-tools: Write, Read, Bash"));
        assert!(s.contains("coder.HANDOFF.md")); // filename prefixed with blueprint name
        assert!(s.contains("zero prior context")); // self-contained, no compact summary
        assert!(s.contains("Read on boot, then delete")); // transient
        assert!(s.contains("commit shas"));
        assert!(s.contains("coder")); // blueprint name woven in
    }

    #[test]
    fn note_skill_targets_another_env_and_is_manual() {
        let s = render_note_skill("core");
        assert!(s.contains("name: note"));
        assert!(s.contains("disable-model-invocation: true"));
        assert!(s.contains("allowed-tools: Write, Read, Bash"));
        assert!(s.contains("another")); // addressed to a different environment
        assert!(s.contains("<target>.NOTE.md")); // target-keyed inbox file
        assert!(s.contains("Overwrite")); // one current note, read immediately
        assert!(s.contains("from core")); // attributed to the authoring blueprint
        assert!(s.contains("not** a handoff")); // distinct from /handoff
    }

    #[test]
    fn note_skill_handles_a_target_in_another_repo() {
        let s = render_note_skill("core");
        // The target's SessionStart hook only reads its own project root, so a
        // cross-repo note left here is a dead letter — the skill has to say so.
        assert!(s.contains("not always this repo"));
        assert!(s.contains("canonical"));
        // "append" was the old wording and contradicted the Overwrite step.
        assert!(!s.contains("append a note"));
    }

    #[test]
    fn sync_skill_excludes_the_transient_env_files_from_staging() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "coder", None);
        // "stage what you created this session" would otherwise sweep in the
        // handoff/note files, which are transient and deleted on next boot.
        assert!(s.contains("HANDOFF.md"));
        assert!(s.contains("NOTE.md"));
        assert!(s.contains("Never stage the transient env files"));
    }

    #[test]
    fn every_generated_skill_is_marked_user_only() {
        let caps = Capabilities { github: true, ..Default::default() };
        for (s, cmd) in [
            (render_sync_skill(&caps, "coder", None), "sync"),
            (render_handoff_skill("coder"), "handoff"),
            (render_note_skill("coder"), "note"),
            (render_twosentences_skill(), "twosentences"),
        ] {
            // The frontmatter flag closes the tool path...
            assert!(s.contains("disable-model-invocation: true"), "{cmd} is model-invocable");
            // ...and the banner closes the read-it-and-do-it-anyway path.
            assert!(s.contains("**Only the user runs this.**"), "{cmd} lacks the banner");
            assert!(s.contains(&format!("they type `/{cmd}`")), "{cmd} banner names the wrong command");
        }
    }

    #[test]
    fn every_generated_skill_documents_the_keep_marker() {
        let caps = Capabilities { github: true, ..Default::default() };
        for (s, dir) in [
            (render_sync_skill(&caps, "coder", None), "sync"),
            (render_handoff_skill("coder"), "handoff"),
            (render_note_skill("coder"), "note"),
            (render_twosentences_skill(), "twosentences"),
        ] {
            assert!(s.contains(&format!("skills/{dir}/.aello-keep")), "{dir} lacks the marker");
        }
    }

    #[test]
    fn twosentences_skill_is_universal_and_manual() {
        let s = render_twosentences_skill();
        assert!(s.contains("name: twosentences"));
        assert!(s.contains("disable-model-invocation: true"));
        assert!(s.contains("exactly two sentences"));
        assert!(s.contains("previous response"));
    }

    #[test]
    fn sync_skill_mirrors_internal_before_commit() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", None);
        // Mirror step names the per-blueprint tracked folder, the env source,
        // the renamed persona snapshot, and self-heals the folder.
        assert!(s.contains("claude-internal/reviewer/")); // namespaced per blueprint
        assert!(s.contains("claude-internal/reviewer/persona.CLAUDE.md"));
        assert!(s.contains(".claude-env-reviewer/skills/")); // env dir is source of truth
        assert!(s.contains("mkdir -p claude-internal/reviewer")); // self-heal already-placed envs
        // The mirror is staged before the commit step runs.
        let mirror = s.find("Mirror this env's internal config").expect("mirror step");
        let commit = s.find("## Commit + push").expect("commit step");
        assert!(mirror < commit, "mirror must be staged before commit");
    }

    /// The mirror step used to stage `claude-internal/` blind, and the whole
    /// safety story was the sentence "this folder is tracked on purpose". It is
    /// a session's own memory — the one place a password gets written down in
    /// passing — and on this repo it goes to a public remote. The check has to
    /// come *before* the stage, and it has to say refuse rather than warn: a
    /// finding reported after the push is not a finding.
    #[test]
    fn sync_reads_the_mirror_before_it_publishes_it() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", None);

        let check = s.find("before you stage it").expect("mirror must be reviewed before staging");
        let stage = s.find("git add claude-internal/reviewer").expect("stage step");
        assert!(check < stage, "the credential check must come before the stage");

        assert!(s.contains("do not stage, do not commit"), "must refuse, not warn");
        assert!(s.contains("Never pass `--no-verify`"));
        // Scoped to what actually matters. An identifier scrubber produces a
        // report nobody reads, and a report nobody reads protects nothing.
        assert!(s.contains("BEGIN .* PRIVATE KEY"));
        assert!(
            s.contains("IPs, hostnames, machine paths and domains are all fine"),
            "the scope has to be stated or this grows into a scrubber"
        );
    }

    /// The policy is stated in the skill rather than the persona on purpose: a
    /// persona is written once and never clobbered, so an edit there reaches no
    /// existing env, while `place` rewrites this skill on every run. The note
    /// that asked for this said "put it in the blueprint persona" — that is the
    /// one place it could not have worked.
    #[test]
    fn sync_states_the_dependency_policy_and_refuses_to_generate_a_lock() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", None);

        assert!(s.contains("## Dependency hygiene"));
        assert!(s.contains("ranges in the manifest, exact versions in the lock"));
        assert!(s.contains("verified by installing it"));
        // Report, never generate: compiling a lock changes what installs.
        assert!(s.contains("Assert a lock exists; do not create one"));
        // A present-but-hand-written manifest is the same finding as none. This
        // is the half a "does the file exist" check misses entirely.
        assert!(s.contains("listing only direct imports is the same finding"));
        // The trap that made a seeded workflow do nothing for weeks.
        assert!(s.contains("git status --porcelain .github/"));
        // And Renovate cannot be reported as running by anyone but the user.
        assert!(s.contains("seeded, not confirmed running"));

        // A blueprint with no git duties gets none of it — same rule as the
        // rest of repo health.
        let none = render_sync_skill(&Capabilities { changelog: true, ..Default::default() }, "x", None);
        assert!(!none.contains("## Dependency hygiene"));
    }

    /// The default is safe-by-default rather than merely configurable: a mirror
    /// staged into a public repo publishes the env's memory, and nothing else in
    /// the chain would ever notice. `/sync` has to check and stop.
    #[test]
    fn sync_stops_before_publishing_a_mirror_into_a_public_repo() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", None);
        assert!(s.contains("gh repo view --json visibility"));
        assert!(s.contains("If it comes back `PUBLIC`, **stop**"));
        // It must name the way out, or the only available action is to give up.
        assert!(s.contains("aello edit reviewer --mirror-dir"));
        // And it must not treat "cannot tell" as "private".
        assert!(s.contains("treat it as unanswered rather than assuming private"));
    }

    /// With a destination set, the in-project `git add claude-internal/…` line
    /// must be **gone**, not merely accompanied by a warning. Leaving it in is
    /// how the mirror ends up in both places — and the public one is the one
    /// that matters.
    #[test]
    fn a_configured_mirror_destination_replaces_the_in_project_stage() {
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", Some("/srv/private-mirror"));

        assert!(
            !s.contains("git add claude-internal/reviewer"),
            "the in-project stage must not survive alongside a destination"
        );
        // …and the section says so outright. Note that a bare grep for
        // `git add claude-internal` still matches — on this prohibition — so
        // that is not a usable check on a rendered skill.
        assert!(s.contains("Do not `git add claude-internal/` here"));
        assert!(s.contains("git -C /srv/private-mirror add reviewer"));
        assert!(s.contains("git -C /srv/private-mirror push"));
        // A missing destination must not silently fall back to publishing here.
        assert!(
            s.contains("do not fall back to mirroring into this repo"),
            "a fallback is the exact failure the destination exists to prevent"
        );
        // The public-repo check belongs to the default branch only — with a
        // destination configured, this repo being public is the expected case.
        assert!(!s.contains("If it comes back `PUBLIC`, **stop**"));
    }

    #[test]
    fn sync_carries_the_handoff_across_machines_without_committing_the_root_file() {
        // Two rules that read as contradictions unless both are spelled out: the
        // root `<name>.HANDOFF.md` is never staged, and the mirror's `handoff.md`
        // always is. Without the second the note cannot reach a second machine at
        // all — which is how it worked until now.
        let caps = Capabilities { github: true, ..Default::default() };
        let s = render_sync_skill(&caps, "reviewer", None);
        assert!(s.contains("claude-internal/reviewer/handoff.md"));
        assert!(s.contains("reviewer.HANDOFF.md"));
        // The ordering is the whole flow: a note written after /sync misses the snapshot.
        assert!(s.contains("`/handoff` **before** `/sync`"), "must state the order");
        // And memory must not prune, or the other machine's notes die on a launch.
        assert!(
            s.contains("never delete a mirror note that has no counterpart in the env dir"),
            "the union rule must be explicit"
        );
    }

    #[test]
    fn the_handoff_skill_scopes_never_committed_to_the_root_file() {
        // It used to say `<name>.HANDOFF.md` is "never committed", full stop. Once
        // /sync snapshots it into the mirror that is false as written, and two
        // seeded skills contradicting each other is worse than either being wrong.
        let s = render_handoff_skill("reviewer");
        assert!(s.contains("The file at the project root is never committed"));
        assert!(s.contains("claude-internal/reviewer/handoff.md"));
        assert!(
            s.contains("`/handoff` first, then `/sync`"),
            "the order has to be stated where the note is written, not only in /sync"
        );
    }
}
