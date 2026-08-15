# Changelog

## [Unreleased]

### Added
- **`.aello-env` — declare which secrets a project needs, and keep them out of
  plaintext.** A committed file listing bare variable names, one per line. The
  values come from an external secret store that launches aello
  (`vault.ps1 run -NoCapture … -- aello run <bp>`); aello never resolves a secret
  and never holds one. Environment pass-through already worked, so this adds no
  injection — it adds the three things injection alone leaves broken.

  **A declared name that is not set stops the launch.** Otherwise you get an
  agent that works until it first needs the key, and the measured response to
  that is the user pasting the key into the chat — which is how one OpenRouter
  key reached twelve transcript records. A present-but-empty variable counts as
  missing, because a variable set to nothing reads as configured everywhere and
  silences the check written to catch it.

  **A line containing `=` is refused with an error.** That is the point of the
  format, not a style rule: a `KEY=value` file eventually holds a real key in a
  project directory, while bare names make that structurally impossible — which
  is what lets the file be committed, so a fresh clone learns what a project
  needs without anyone carrying a value across. A malformed name is an error too,
  never a silent skip.

  **Secrets no longer leak sideways between projects.** Agents run `aello` from
  inside an aello env routinely, so project A's keys would otherwise ride into
  project B's session while B declares none. Each launch publishes its own list
  as `AELLO_DECLARED`; a nested run strips every inherited name its project does
  not declare.

- **aello's own credentials can leave `config.toml`.** `AELLO_OAUTH_TOKEN` and
  `AELLO_CLINE_API_KEY` take precedence over `oauth_token` and
  `[cline].api_key`, so both can live in the secret store and be deleted from the
  config file — where they sit in plaintext, and where a transcript scan found
  the token dumped into sessions six times.

  Deliberately **not** `CLAUDE_CODE_OAUTH_TOKEN`: that name is stripped from
  every child by the credential scrub, so a store-supplied one was deleted before
  it could be used, and every env silently fell back to an interactive login.
  Measured. The scrub is not weakened — it exists so an agent running `aello`
  inside an env cannot authenticate as whoever owns the ambient variable.

  `aello login` now warns when the store is already supplying the credential it
  is about to write, because `login` and `edit` serialize the whole config to
  disk: without the warning the key moves into the store and aello quietly copies
  it back out in plaintext. It warns rather than refuses, since a login is also
  how you replace a credential you have lost.

- `docs/vault.md`, in `aello docs`, the TUI reader and the site.

### Fixed
- **Custom slash commands now cross machines.** `<env>/commands/*.md` — your own
  `/whatever` commands — were the one part of an env the `claude-internal/`
  mirror never carried: `aello run` mirrored skills, memory, persona and the
  resume note, and `aello restore` read the same four back. A command written on
  one machine simply did not exist on the other, and nothing said so. They are
  mirrored and restored now, in both directions.

  They are a **union**, like memory, and not a pruning copy like skills. aello
  generates a skill from the role, so a mirror-only skill is stale output worth
  deleting; it generates no command at all, so a mirror-only command is the other
  machine's hand-written work and the mirror is its only copy. A launch that
  finds one names it and points at `aello restore`, the same way it already did
  for memory notes.

  Found while planning a two-machine test on a real env: it had three commands
  and its mirror had one, put there by hand months after the fact.

### Added
- **`.aello-nomirror`** — an empty marker beside a skill's `SKILL.md` keeps that
  skill out of the tracked `claude-internal/` mirror. For a skill vendored from
  someone else's repo: it has its own upstream, and mirroring it commits that
  project into this repo's history. Marking one that is already mirrored removes
  it on the next run, so the marker is also how you take it back out.

  Separate from `.aello-keep`, which stops *regeneration* — a skill can want
  either, both or neither, and one marker meaning both makes the other unsayable.
  Written because a repo had recorded exactly this decision in a commit message
  and the next launch silently reverted it.

### Added
- **Four more ways to split the same spend**, on both the TUI stats page and
  `aello tokens --stats`: **by git branch**, **by reasoning effort**, a
  **per-model timeline** over the charted window, and **context nobody typed** —
  what the harness itself injected (task reminders, hook output, skill and agent
  listings, nested memory).

  An absent `effort` or `gitBranch` gets its own `(unrecorded)` row instead of
  being folded into the commonest value; both fields only exist on newer
  records, and here that row is **18.8% of all cost**. A split with one row is
  dropped, because "100% on main" is not a finding.

  The model timeline makes a migration read as a handover rather than one
  blended average. A model with no tokens in the window is listed with its
  first/last-seen dates but not charted — an all-blank sparkline reads as a
  broken chart.

  **Injections are priced, not just counted, and the price is the finding.** An
  injection is written into the context once and then re-read by every later
  request in its session — median **75** further requests here — so it costs
  `tokens × 2 × input` once plus `tokens × 0.1 × input` per request after that.
  The table shows write, re-read, total and the multiplier: 9,029 injections,
  ~3.82M tokens, **~$243 (4.2% of all spend), of which $205 is re-reads and only
  $38 is writes**. aello's own two hooks are $23.34 (the per-turn rules) and
  $16.38 (SessionStart) of that. Rates come from the model of each carrying
  request, so a session that switched models is priced as it happened.

  Rows are keyed by the record's own `hookEvent` rather than by matching the
  hook's text — aello's wording has already changed once and 53 injections carry
  the old phrasing, which a text signature would have dropped silently.

  The token figure is **characters ÷ 4 and labelled an estimate everywhere it
  appears**: the transcript records what was injected and never what it
  tokenised to. And a SessionStart hook is **recorded twice** (`hook_success`
  plus `hook_additional_context`); both rows are shown with the duplicate
  labelled `(2nd copy)`, rather than summed into a number twice the truth.

- **The statistics page now reports what the sessions *did*, not only what they
  spent** — `S` in the TUI and `aello tokens --stats` both gain it, from the
  same scan, with no new hook and retroactively over all of history.

  Turns (count, median, p90, longest, and hours spent *inside* turns), tool mix,
  skills actually run, most-edited files, shell verbs, turns by weekday, and the
  two friction signals: interrupted turns and queued-then-withdrawn prompts.
  Turn length comes from Claude Code's own `turn_duration` records, so it is
  measured wall clock rather than a gap between timestamps that would count the
  time you spent reading.

  Skills are counted from `attributionSkill`, which is the only evidence a
  seeded skill was actually **run** rather than merely seeded — `/handoff` in
  213 sessions here, `/sync` in 207, `/twosentences` in 19.

  Two dedup keys were load-bearing and neither was obvious: tool calls key on
  the `toolu_…` id rather than `message.id`, because one message is written as
  several records and a message-level key keeps the `thinking` block and drops
  every tool call; and queue records carry **no uuid at all**, so keying on one
  counted 0 of 665 — a silent zero that reads exactly like a user who never
  queues. Both are pinned by tests.

### Fixed
- **A long report piped into `head` no longer ends in a panic dump.**
  `println!` panics when its pipe is gone, so `aello tokens --stats | head` could
  die with `failed printing to stdout: The pipe is being closed` — a normal way
  to read a long report, reported as a crash. That one panic now exits quietly;
  every other panic keeps the default hook and its backtrace. It is a race (the
  writer has to still be writing when the reader exits), so it reproduced about
  one run in three before the fix and zero times in twelve after.

### Changed
- **contextdb is now the only place a transcript lives.** SessionEnd copies the
  transcript, **verifies the copy byte-for-byte with sha256**, and then deletes
  Claude Code's original — but only when the session also wrote a `/handoff`
  note. Transcript retention (`cleanupPeriodDays`) drops from **365 to 10 days**
  and now *migrates* in existing envs rather than only filling an absent key, so
  the change reaches envs already on disk instead of only new ones.

  **The handoff note is the gate, and not as a proxy for "ended cleanly".**
  Deleting the original costs `--resume` for that session and nothing else,
  because the archive holds every byte. A handoff note *is* the continuity — it
  is written to be read by the next session — so once it exists the transcript's
  resume value is already spent. No note means the session may still be worth
  resuming, so its transcript stays and the 10-day timer takes it later.
  Measured across 372 archives: 257 (69%) carried a note, and the `SessionEnd`
  trigger alone (`prompt_input_exit` 183, `clear` 151, `other` 38) does not
  separate them.

  Ten days rather than three: an unarchived transcript belongs to a session that
  never reached SessionEnd at all — a killed terminal, a reboot — which is
  exactly the session worth resuming. Ten days is how long that chance lasts.

  Every outcome is recorded in the archive: `transcript_verified` is `sha256`,
  `MISMATCH` or the exception type, and `original_deleted` is `session-end`,
  `failed: <Error>` or empty. Measured all three paths against the real hook —
  note present → original gone; no note → original kept; file held open by
  another process → copy still written, note still captured, exit 0, and
  `failed: PermissionError` on the record rather than silence.

### Added
- **A statistics page over the token scan — `S` on the TUI tokens tab, or
  `aello tokens --stats`.** Projects ranked by **tokens ÷ sessions** (how
  expensive it is to *engage* with a project, so one huge session outranks
  twenty small ones), each bucket's token share beside its cost share, a 30-day
  sparkline that keeps its empty days, and an hour-of-day histogram labelled
  UTC because there is no timezone crate and a guessed offset would shift every
  bar silently.

  Measured while building it: cache read is **98.3% of tokens and 69.1% of
  cost**, output 0.4% of tokens and 12.4% of cost — the two splits are printed
  together because reading only the token one is how "cache is cheap" becomes a
  wrong conclusion.

- **`aello tokens` now says what it could not count.** Two disclosures, both for
  numbers that otherwise read as a quiet period rather than as missing data:
  **269 of 415** archived sessions on the machine this was built on held only a
  *pointer* to a transcript Claude Code has since deleted (SessionEnd only
  started copying the file later), so they contributed zero tokens and are now
  counted and named — for 226 of them the original was still on disk, and
  copying it moved that machine's true total from 3.94B tokens to **7.41B**; and
  the live half of the scan only reaches the current directory's envs — running
  from `~` rather than the project dropped 430M tokens and 31 days of span here,
  moving `$/day` from $72 to $315.

- **A usage readout under the prompt, in the session itself.** Every env now
  registers `aello statusline` as its Claude Code `statusLine`, which renders
  two rows on every conversation update — ceilings on top, spend beneath:

  ```
  204k·$9.95 │ 5h·42%·34M·1h57m │ 7d·20%·527M·5d18h
  this·6.57M·$4.08 │ sess·20M·$14.14 │ prjt·926M·$638.27
  ```

  Context tokens and session cost, then each **plan window** as percentage,
  tokens and time to reset; below, tokens and list-rate cost for **the last
  turn, this turn, this session and this project**. Colour is the part read at a
  glance: context red, money green, a window green under 80% and red over it,
  and the whole spend row red once a window is spent.

  The two plan percentages come from the payload Claude Code hands the
  statusline, and they exist **nowhere else** — no transcript carries them,
  which is why `aello tokens` measures its 5-hour window against this machine's
  own peak block instead. The token counts come from the transcripts, deduped by
  `message.id` exactly as `aello tokens` does; the session figure reproduces
  `aello tokens --sessions` to the cent on a real session. A missing field drops
  its segment rather than printing a confident `0%`, so an API-key session (no
  `rate_limits`) simply shows fewer segments.

  A turn is split on **user prompts**, which is not the same as records with
  `"role":"user"`: measured over every transcript in this project, 4,539 of
  4,895 such records are tool results, 22 are `[Request interrupted by user]`
  markers, and 21 are real prompts carrying a pasted image. Counting an
  interrupt as a boundary would insert an empty turn and blank out "last turn"
  at exactly the moment it is worth reading.

  The project-wide total is cached for 180 seconds because the statusline
  re-runs up to three times a second and the scan takes ~0.8s; everything
  narrower than the project is re-read from disk on every render. Existing envs
  adopt the registration on their next `aello run`, and a hand-written
  `statusLine` is never replaced. `aello check` now **executes** it with a real
  payload — a statusline that fails renders nothing and reports nowhere, so
  "registered" is not evidence that it works.
- **Documented the Renovate failure mode that looks exactly like success:
  Mend's onboarding defaults to "Scan Only", which sets Renovate to `silent`.**
  It runs jobs on schedule and creates no PRs, no issues and no dependency
  dashboard, so a correctly installed Renovate and one never installed produce
  identical evidence — nothing. Measured across 33 repos here. `docs/roles.md`
  now says installing the App is necessary and not sufficient, and
  `docs/troubleshooting.md` gained a section with the fix and the reminder to
  read the job log rather than the config: the first guess here blamed the
  seeded weekly schedule, and the job log disproved it in one look.
- **`aello check [path]` / `aello check --all` — verify a repo's aello
  integrations by proving each one rather than reading a file.** It executes the
  voice hook to ask its version, fires the `pre-commit` hook at a real staged
  key, reads CI's last actual run, and judges a lockfile on whether the
  *transitive* set is pinned. Renovate is reported as **seeded, not confirmed
  running** unless a PR or dependency dashboard proves the App is installed, and
  an env mirror tracked in a repo GitHub reports as `PUBLIC` is a FAIL. `--all`
  sweeps every repo holding a placed env; `--json` prints the report; exit code
  is 1 on any failure. Aliased as `toolcheck`.

  **Every check is shaped so that "silently absent" cannot read as "fine"** — a
  hook git never runs because `core.hooksPath` is unset, a workflow that exists
  on disk and was never committed, and a manifest listing only direct imports all
  look correct in a file listing. Where evidence cannot be obtained the row is
  WARN and says so; an inability to test is never a pass.

  The `pre-commit` check stages its canary into a **throwaway index** and runs
  the hook via `git hook run`, never `git commit` — a checker that commits is one
  that lands a canary commit on the single repo where the guard is broken, which
  is the exact case it exists to find.
- **`aello tokens` and a `T` tab in the TUI — token usage and estimated cost per
  env.** Input / output / cache-write / cache-read kept apart, because they price
  as much as 20x apart; per-model split, per-session breakdown (`--sessions`), and
  `--json`. It needs nothing enabled and works retroactively: the source is the
  transcripts contextdb has been archiving all along, so history recorded before
  this existed is already counted. Live sessions in the current directory are read
  too, so the session you are sitting in shows up before it ends; sessions in other
  projects appear once they do (contextdb records a project's folder name, not its
  path, so their env dirs can't be located from elsewhere).

  **Deduplicating by `message.id` is the whole correctness story, not a detail.**
  Claude Code writes one transcript record per *content block* and repeats the
  message's full `usage` on each, so summing records roughly doubles the answer —
  measured on a real transcript here, 266 usage-bearing records for 173 distinct
  messages, overstating output by 68% (218,607 vs 129,877). The same dedup absorbs
  a session archived twice (17 of 122 archives on this machine) and the overlap
  between an archive and the live transcript it was copied from, which is what
  makes reading both sources safe at all. The CLI prints how many duplicates it
  collapsed; on a real archive that number being zero means the dedup broke, not
  that the run was clean.

  Two claims the output refuses to overstate. **Cost is an estimate at list API
  rates, never a bill** — an env runs on a subscription, where no per-token charge
  exists — and a model with no rate entry is *quarantined into an `unpriced` total
  and named*, never silently priced at zero, which is the failure shape this repo
  keeps hitting. **The 5-hour percentage is against this machine's own peak block,
  not a plan quota**: the subscription's limit appears in no transcript, so aello
  cannot read it and does not guess, and the label says which ceiling it used.
  Blocks are computed across every env because the quota is machine-wide (one
  shared token), with a per-env split inside the current block so you can see which
  env is eating it. Verified against real data: the cost arithmetic reproduces
  independently to the cent, and the dedup matches a hand measurement of the same
  transcript. See `docs/tokens.md`.

  Re-verified against the published rate card, and the docs now say three things
  the numbers otherwise invite you to get wrong. **The token split is not the cost
  split** — cache read is 98.4% of tokens but 70% of cost, against 18% for cache
  writes and 13% for output — and **cache is not uniformly cheap**: a read is 0.1x
  input but a 1-hour write is *2x* it, so reads win on volume rather than unit
  price. **Every cache write measured here is the 1h bucket and none is 5m**, which
  makes the "default to 5m when `cache_creation` is absent" fallback load-bearing:
  silently taking it would understate the fleet by $145.63, not by a rounding
  error. And **there is no cache storage ceiling** to account for — nothing meters
  stored cache; read tokens bill per request, which is why one re-read prefix
  accrues billions of read tokens without anything being stored at that size.
  `docs/tokens.md` also now records the one rate entry known to be wrong and left
  in deliberately: `claude-opus-4` prices Opus 4.0/4.1 at $5/$25 when they are
  $15/$75, harmless while no such transcript exists.
- **`aello restore <name>` — work one blueprint from two machines.** The tracked
  `claude-internal/<name>/` mirror already carried a blueprint's memory, skills
  and persona into git, and `aello run` seeded a fresh clone from it. The return
  trip was missing: on the machine that already had an env dir, pulling the other
  machine's commits changed nothing the agent could see, because a live env is
  deliberately never contradicted by a snapshot. `restore` is that direction, run
  on demand. It is additive — memory and skills are merged, so an unsynced local
  note survives, and a persona that differs from the snapshot is reported and left
  alone rather than replaced (`aello persona` is still the only command that
  overwrites one) — compared as text, not bytes, because the scaffolded
  `.gitattributes` normalizes newlines in the tracked snapshot but not in the
  gitignored env dir, so a byte compare called two identical files a divergence on
  any machine whose checkout disagreed with the writer. Works across operating
  systems: the `<encoded-cwd>` component of the memory path is derived from each
  machine's own project path rather than carried in the mirror. The full loop is in
  `docs/workflows.md`.

- **The `/handoff` resume note now reaches other machines.** `/sync`'s mirror step
  snapshots `<name>.HANDOFF.md` to `claude-internal/<name>/handoff.md`, which is
  committed; a clone or an `aello restore` puts it back at the project root where
  the SessionStart hook delivers and deletes it. The file at the project root is
  still never committed — it is gitignored in some repos and deleted on the next
  boot in all of them, which is exactly why nothing but a snapshot could cross.
  **This makes the order matter: `/handoff` before `/sync`.** A note written after
  the snapshot is taken stays on the machine that wrote it, so both skills now say
  so and the documented session loop puts `/handoff` first.

### Fixed
- **A launch no longer deletes memory notes another machine committed.** Mirroring
  memory into `claude-internal/` was a pruning one-way sync, which is correct only
  while one machine owns the env dir. Pull a second machine's notes and the next
  `aello run` deleted every one of them from the mirror — silently, and the next
  commit recorded the deletion, leaving git history as the only copy. Memory is
  now a union: the mirror only ever gains notes, and a launch that finds notes it
  has no counterpart for prints their names and points at `aello restore`.
  Deleting one for real is a deliberate `git rm` of the mirror copy. Skills are
  unaffected and still pruned — they are regenerated from the role on every
  placement, so an orphan there is stale output rather than someone's work.
  Reproduced against the released binary before the fix: one launch, one deleted
  note, staged.
- **aello can now drive the Cline CLI as well as Claude Code.** `aello add
  <name> --agent cline` creates a Cline blueprint; `aello run` places and
  launches it exactly as it does a Claude one. The two are split completely and
  deliberately: a Claude env is `.claude-env-<name>` configured by
  `CLAUDE_CONFIG_DIR`, a Cline env is `.cline-env-<name>` configured by
  `--config`/`--data-dir` flags, and everything Cline-specific lives in
  `cline.rs` so neither can acquire the other's assumptions. Existing
  blueprints need no migration — a config with no `agent` key loads as Claude,
  which is what it was.

- **`aello login` now asks which agent you mean**, or takes `--agent
  claude|cline`. They are separate accounts with separate billing and one is
  never inferred from the other: Claude keeps the shared subscription token,
  Cline stores a provider id, key and model under `[cline]` in `config.toml`,
  shared by every Cline env the same way.

  **A Cline env is metered.** Every turn costs money per token at your
  provider, unlike a Claude env. Its env dir is therefore gitignored
  unconditionally — not gated on the role the way the Claude line is — because
  it holds the API key in plaintext, and an unignored one is a credential a
  single `git add -A` away from a public repo.

  A Cline env is also quieter than a Claude one, and this is not an omission:
  Cline has **no voice, no per-turn rules injection and no transcript
  capture**. Measured — of `TaskStart`, `TaskComplete`, `SessionShutdown`,
  `UserPromptSubmit` and `PostToolUse`, only `TaskStart` fires, only from
  `<config>/hooks/` (a `--hooks-dir` copy fired nothing at all), and its payload
  carries identifiers with no prompt and no response. There is nothing to speak
  from. The four response rules instead ship as a Cline rules file, which is
  the one channel measured working.

- **A Cline blueprint is now creatable from the TUI, and gets a persona, the
  four commands and a memory.** `a` opens an agent picker first — Claude Code or
  Cline — because everything after it differs: a Cline blueprint takes a
  free-text provider model id where a Claude one takes a curated alias. Editing
  never offers the picker, since the agents share nothing on disk and switching
  one would strand its env.

  A Cline env now gets the **same persona templates** (`--claude-md coder`
  works), written to its rules dir rather than a `CLAUDE.md` Cline ignores —
  and switching to `none` *removes* it, which matters because rules apply on
  every request. It gets **`/sync`, `/handoff`, `/note` and `/twosentences`**,
  and a **memory directory**, seeded once and never overwritten.

  Two things about how those work, because both are weaker than the Claude
  equivalent and saying so is the point. **Cline has no user-defined slash
  commands** — measured, with a workflow and a skill in every candidate
  location; `/canary` came back as prose, and the only slash commands the binary
  has are connector built-ins. So the commands are *routed* from the rules file,
  which names each one against its `SKILL.md` path. And **Cline has no memory
  system at all**, so aello supplies one the same way: a rule, re-sent every
  request, telling the agent to read the index and write what it learns. Both
  are instructions the agent follows rather than hooks aello enforces.

  `/sync` for Cline has **no git step**: it reconciles the memory and the
  project's `AGENTS.md` (Cline's project-`CLAUDE.md` equivalent) and stops.

### Added
- **Dependency hygiene is now something aello establishes, not something each
  project rediscovers.** A repo that grew organically has its tests and its
  dependency audit running only where somebody remembers to type them — the
  developer's desktop, never the server. Three pieces, seeded for roles with git
  duties:

  - **`.github/workflows/ci.yml`** — tests plus `pip-audit` / `npm audit` on
    every push and PR. **Stack-agnostic like `version.yml`, but by detecting the
    ecosystem at run time rather than at seed time**: aello does not know a
    project's stack when it places into it, and a guess baked in at placement
    seeds a workflow that fails forever in every repo of the other kind. A repo
    with neither manifest reports that there was nothing to do, rather than
    passing silently. The audit **fails the build** — an advisory that only
    prints is one nobody reads.
  - **`.github/renovate.json`** — grouped minor/patch weekly, majors on their own
    PR, security updates off-schedule, **nothing automerged**, editing the
    manifest and never a generated lock. Placement says once that it does nothing
    until the Renovate **GitHub App** is installed, which aello cannot do —
    reporting a seeded file as "configured" is the kind of claim this project
    keeps having to undo.
  - **A `/sync` step that asserts a lock exists and refuses to create one.**
    Compiling a lock changes what installs, and on a live system that is a
    deploy — not something a checkpoint does unasked.

  **A hand-written `requirements.txt` listing only direct imports is the same
  finding as having no lock at all**, which a "does the file exist" check misses
  entirely. Measured in one 14-month-old project on 2026-08-11: everything
  transitive was unpinned, and a **beta** release of a signing library had
  installed itself into the code path that signs every order. In the same repo a
  pin read off a developer's desktop was older than the server, so installing the
  manifest moved production backwards on every run. Adding CI there found two
  more bugs on its first two runs — a suite silently collecting 384 of 420 tests
  while still printing OK, and four tests that passed only by reading a gitignored
  file present on one machine.

  The policy is stated in the generated `/sync` rather than in the persona,
  deliberately: a persona is written once and never clobbered, so an edit there
  reaches no existing env, while `place` rewrites the skill on every run.

- **`/sync`'s mirror has a destination now — `aello edit <name> --mirror-dir
  <path>`.** The mirror is an env's memory, persona and handoff, and in a public
  repo staging it is a publish rather than a backup. Deleting it is not the fix:
  being in git is exactly what makes an env restorable from a second machine. So
  the destination moves — point it at a working tree of a **private** repo and
  the product stays public while the memory does not.

  It takes a **path to an existing git working tree**, not a URL: aello never
  clones or pushes on your behalf, `/sync` is what commits. `edit` rejects a
  missing directory or a non-repo up front, because a mirror writing into a plain
  folder is indistinguishable from one that worked — files appear, nothing is
  ever committed, and the memory silently stops crossing machines. `--mirror-dir
  -` clears it. The `<blueprint>/` component is still appended, so one
  destination can hold several blueprints.

  With a destination set the generated `/sync` **drops the in-project `git add
  claude-internal/…` line entirely** rather than warning next to it, and grows a
  commit-and-push step against the destination repo. It does **not** fall back to
  mirroring into the project when the destination is missing — it stops and says
  so, since that fallback is the leak the setting exists to prevent.

  Without a destination, `/sync` now runs `gh repo view --json visibility` before
  staging and **stops if the repo is public**, naming `--mirror-dir` as the way
  forward; "cannot tell" is reported as unanswered rather than assumed private.
  That is the safe-by-default half — a future public repo is covered without
  anyone having to remember. `#[serde(default)]` on both new fields is the whole
  migration: every existing config and `.aello.toml` loads with the in-project
  mirror it already had.

- **A `pre-commit` hook is now seeded alongside the other `github` scaffolding,
  and `/sync` reads the mirror before it publishes it.** `/sync` mirrors an env's
  memory, persona and handoff into the tracked `claude-internal/<name>/` folder
  and stages it **by path** — so nothing in that chain ever read what was in it,
  and the whole safety story was the sentence "this folder is tracked on
  purpose". Memory notes are exactly where a session writes down a credential it
  just used, and aello's own repo is public with 21 tracked mirror files, so for
  that one the mirror is a publish rather than a backup. (Nothing has leaked: an
  account-wide scan of 51 repos over full history found zero private keys, zero
  real `.env` files and zero live provider keys. The problem is that nothing
  prevented the next one.)

  `.githooks/pre-commit` blocks armored private keys, PuTTY keys, real `.env`
  files (`.env.example` passes), certificate/keystore bundles,
  `.netrc`/`.pgpass`/`.htpasswd`, port-knock sequences, and non-placeholder
  provider API keys. `git config core.hooksPath .githooks` is re-run **on every
  placement**, because it is per-clone local config that does not travel with a
  pull — a fresh clone otherwise has the hook file and no guard, which is the
  failure that looks most like success. A repo already pointing `core.hooksPath`
  elsewhere is left alone.

  **It deliberately says nothing about IP addresses, hostnames, machine paths or
  domains.** Those are identifiers, not secrets; flagging them produces a report
  that gets skimmed once and then bypassed with `--no-verify`, at which point the
  real check is gone with it. Narrow is what keeps it enforceable.

  The file carries an `aello-pre-commit v<N>` marker: an older copy of *ours* is
  upgraded on the next placement so a widened pattern reaches projects scaffolded
  months ago, while a `pre-commit` without the marker is somebody's own hook and
  is never touched. Written with LF regardless of checkout, with `.githooks/*
  text eol=lf` appended to the project's `.gitattributes` — hooks are run by `sh`
  and a CRLF one fails to execute, and the file has no extension so a `*.sh` rule
  misses it. Verified by driving a real `git commit` through the seeded hook: a
  clean commit passes, a memory note carrying an armored key is refused and the
  refusal names what it found.

### Changed
- **Voice hooks re-vendored at `HOOK_VERSION` 24, five bumps at once (19 → 24).**
  `speak.py`, `duck.py`, `focus.py` and `notify.py` all moved; `win_audio.ps1` is
  byte-identical and is re-vendored anyway, because "only some files changed" is
  how a partial vendor gets rationalised. Every env picks it up on its next
  `aello run`. The four that matter before you deploy:

  - **A line that never played said nothing.** Both of the player's pipes are
    `DEVNULL` and its exit code was discarded, while `record()` had already
    written the turn to history as spoken — so a player exiting 1 in silence and
    one reading the line out were the same thing to every reader. It lands in
    the shared state as `play_error`, and **`aello voice status` now prints it**
    as a `playback` line, the same treatment `telegram_error` got at 18 and for
    the same reason: this command reads the same file as `speak.py --status` and
    would otherwise be the one saying nothing is wrong.
  - **`--sweep` from an incomplete copy answered "clean" without looking.** It
    reaches for two `duck.py` functions the guarded-import stub never carried,
    and the blanket `except` swallowed the `AttributeError`: two lines, exit 0,
    the check never reached, output-identical to a healthy machine. So any sweep
    reading taken from a partial copy below 24 is worthless in either direction.
  - **One malformed key in `state.json` stopped every hook on the machine.**
    `setdefault` cannot repair a key that is present with the wrong type;
    measured with `{"leases": null}`, the integrity check passed, the lease scan
    raised, and the top-level handler turned it into `sys.exit(0)` — silence in
    every env with nothing said anywhere. Worth knowing here rather than only
    upstream, because **aello writes to that file too**.
  - **`duck.json`'s lock did not say who owned it.** It stole a stale lock by
    stat-then-unlink with nothing tying the file it inspected to the file it
    deleted, so two waiters that both judged it stale both won. Two holders of
    `duck.json` is not a lost update — it is the only record of what normal was,
    so it is permanent volume loss.

  Also across 20–24: the `state.json` durability set (20) stops dropping writes
  silently and stops reading a *denied* file as an empty one; `focus.py` (21) no
  longer adopts any titled window when matching by pid, which could send the
  user's prompt to `explorer.exe`; the sweep's live-session repair (22) matched
  **0 of 49** stored entries against 6 live sessions and now matches 27, and it
  no longer switches itself off whenever `duck.json` merely exists; and `tg_env`
  (23) strips whitespace, without which a `TELEGRAM_BOT_TOKEN` carrying a
  trailing space made `urllib` raise `InvalidURL` quoting **the whole URL, token
  included**, into `state.json` where `--status` prints it.

- **`docs/cline.md` no longer implies a Cline env touches nothing outside
  itself.** An isolated run does write one file into the shared tree —
  `~/.cline/cli-node-extra-ca-certs.pem`, a node CA bundle — whatever `--config`
  and `--data-dir` say. Measured by deleting it and re-running: it came back
  byte-identical, while the run's sessions, database, logs and credential all
  stayed inside the env dir and the shared tree's session count did not move. No
  credential or per-env state crosses over, so nothing changes about how the
  isolation is used; the claim was simply stronger than the evidence, and a
  stated rationale that has quietly become false is worse than none. The page
  also now records that the launch path can be exercised end to end with a
  deliberately invalid key, which is the cheapest way to check a new machine is
  wired up without spending anything at a provider.

- **The scroll reveal is slow enough to see.** On the landing page it appeared
  to snap into place on refresh, and the duration token was not the reason —
  `--brand-animation-easing-default` is an expo-out that completes **90% of the
  movement in the first third** of its duration, so the captured 0.6s was
  perceived as roughly 200ms. Raising it to 1.1s and widening the travel from
  0.9375rem to `--base-size-28` (1.75rem) puts the bulk of the movement between
  390ms and 950ms — measured off the live page over DevTools, sampling computed
  opacity and transform, rather than inferred from the token.

  `--brand-animation-duration-default` drives the reveal and nothing else, so
  the change is contained. `/design-system` now states the perceived duration
  beside the nominal one and reads both **out of the tokens** — the first draft
  of that sentence hardcoded "0.6s" and was wrong within the hour, which is the
  entire argument for generating the page.

- **The site palette is now orange on near-black.** The captured GitHub
  green-and-blue is gone: one warm accent family over neutral greys, with
  nothing set to pure `#000` — it flattens every border and shadow drawn on top
  of it. The token *count* is unchanged and so is the *structure* (names, type
  scale, spacing, radii, motion, all still from the capture); only the colour
  values moved. Five tokens that named a hue they no longer hold were renamed
  with them — `--color-decorative-indigo` → `--color-decorative-orange-mid`,
  `-purple-soft` → `-orange-soft`, `-purple-mid` → `--color-decorative-rust`,
  and `--color-canvas-green-{subtle,dark}` → `--color-canvas-accent-{subtle,dark}`
  — because a token called `indigo` holding an orange is worse than no name.

  Every value was measured rather than eyeballed. The old palette shipped one
  pair below WCAG AA (`--color-fg-subtle` on `--color-canvas-subtle`, 2.79:1)
  and one button state that put white text on a fill at 4.07:1; both are fixed,
  and no pair fails AA now. The logo's three frames are separated by
  *lightness* rather than hue, because three oranges of equal lightness are not
  tellable apart — which the first candidate triad proved at 1.15:1 between two
  of them.

### Added
- **The site has a `/design-system` page**, generated from `site/app/globals.css`
  at build time rather than transcribed beside it — swatches, type specimens
  with the line-height and weight that travel with each step, easing curves
  drawn from their `cubic-bezier`, the spacing ramp, and a usage count per
  token so the blast radius of changing one is visible before you change it.
  Nine of the 99 tokens have no reference anywhere; the page says so and says
  why (the palette is transcribed from the captured system wholesale), because
  the useful fact is that editing one of them changes nothing on the page.

  **Three rules are now enforced by `next build`, not documented and hoped for.**
  A literal colour outside `globals.css` fails the build with the file, the
  line and the fix; so does an `@media` width that is not one of the documented
  breakpoints; so do unbalanced comment markers inside the `:root` block. All
  three were verified by introducing a violation and watching the build fail — a
  guard nobody has seen fire is indistinguishable from one that cannot, and the
  first version of the third guard *was* that: it compared parsed tokens against
  declared tokens, which can never disagree because both sides share the same
  comment model. Marker balance is what actually distinguishes the two states.

  The breakpoint rule paid for itself immediately: `app/docs/docs.module.css`
  carried a lone `64rem` where the rest of the site turns at `63.25rem`, so
  there was a 0.75rem band in which the docs page had already dropped its table
  of contents while every other section was still wide. The comment rule earned
  itself the same way — a multi-line note added to `globals.css` silently
  swallowed the declaration after it, six tokens disappeared, and the page
  rendered 93 of 99 as though that were the design system.

  **The usage counts exclude the page itself**, and each token names the files
  that reference it. Counting the design-system stylesheet meant the page cited
  itself as evidence a token was used — `--color-decorative-orange-soft` and
  `--borderRadius-large` both read as live when their only user was the swatch
  describing them. Three tokens moved to inert once that stopped. The file list
  exists because a bare "11×" raises the question it is meant to settle: the
  Motion group in particular reads as a lot of animation, when all but three of
  those references are hover and focus transitions.

  The Motion group also carries a notice that appears **only** for a visitor
  whose browser asks for reduced motion, saying so and naming the Windows
  setting — because for them every duration specimen on the page is frozen and
  the site's scroll reveal, waveform and transitions are all switched off. It is
  CSS-only, gated on the same media query that does the disabling, so the two
  cannot disagree.

  Contrast is measured and reported on the page but deliberately not enforced,
  since the palette is reproduced as extracted and a failing pair is fixed at
  the call site. One pair fails AA today: `--color-fg-subtle` on
  `--color-canvas-subtle`, 2.79:1.

### Changed
- **The landing page says there are two agents.** It described aello as
  "isolated Claude Code environments" throughout — title, hero, footer,
  metadata — and never mentioned Cline at all, so the one public page a new
  user lands on contradicted the README beside it. It now leads with agent
  environments, carries a *Two agents* section covering the split env dirs, the
  separate logins and the metered key, and stops claiming every env speaks.
  `docs/cline.md` was also absent from the site's reading order, which sorted it
  last on a page whose comment claims to mirror `docs.rs::ORDER`; it now sits
  after `voice` in both, and in the README's doc list.

- **Voice hooks re-vendored at `HOOK_VERSION` 19.** `speak.py --status` now
  reports volume repairs over a **24-hour window** instead of the last 50
  history entries, with a count and — when there is nothing to report — how far
  back the file itself reaches. A window measured in entries is a window whose
  length depends on how busy the fleet is: 39 envs append to one
  `history.jsonl`, so "none in the last 50 turns" said nothing about time.
  Nothing on the hook path moved; only `speak.py` changed. The version is
  bumped anyway, because aello's digest test covers all five files byte-for-byte
  and cannot tell a CLI-only change from a hook-path one — an unbumped change
  fails it with no version to explain the mismatch.

### Fixed
- **`aello login --agent cline` no longer echoes the API key as you type it.**
  It was read with a plain `read_line`, so the key was displayed and captured by
  `tmux`, `script` and asciinema — while the Claude token on the sibling path is
  deliberately scrubbed from stdout for exactly that reason. Piped input still
  works and still reads from the pipe.

- **The five bundled hooks decode their own stdin as UTF-8.** `json.load(sys.stdin)`
  decodes with the console code page on Windows, so every non-ASCII string in the
  payload is corrupted. Measured (cp1252, Python 3.14): it does **not** raise —
  stdin's error handler is `surrogateescape` — so a CJK character in a path comes
  back as mojibake plus a lone surrogate, and a `transcript_path` decoded that way
  no longer opens (FileNotFoundError), which is the SessionEnd archive silently
  degrading back to a pointer. The JSON structure and ASCII fields are unaffected,
  so the plan-mode denial and the response-rule injection were never at risk;
  those two are hardening. `speak.py` has read bytes for this reason for a long
  time and the other five now match it.

- **`aello voice mute` flushes the shared state file before renaming it.**
  `rename` is atomic with respect to the name, not to bytes still in the OS
  cache, so a power loss published a `state.json` that was present, current, and
  truncated — which the Python side then reads as corrupt and silently drops
  every write to, permanently.

- **`install.sh` verifies the download against `SHA256SUMS`.** The release has
  published the manifest all along and `aello update` has always checked it, so
  the `curl | sh` line the README leads with was the least verified way to
  install aello. It catches a corrupted or truncated download; the manifest
  travels the same channel as the binary, so it is not tamper protection.

- **`aello login` redacts any line containing a token, not just a bare one.**
  Redaction shared its predicate with the parser, which requires a
  whitespace-delimited word starting with `sk-ant-` — so `{"token":"sk-ant-…"}`
  and `CLAUDE_CODE_OAUTH_TOKEN=sk-ant-…` were echoed to aello's own stdout in
  the clear, which `aello login | tee`, CI logs and tmux capture then keep. The
  same cases are where parsing fails, so the user was then asked to paste a
  year-long credential that had just been logged. Redaction is a plain substring
  search now, deliberately wider than extraction.

- **A launched agent no longer inherits the shell's credentials.** Agents run
  `aello` from inside an aello env, so an ambient `CLAUDE_CODE_OAUTH_TOKEN` is
  routine — and with no token configured, aello printed "Claude will prompt
  login" while the env quietly authenticated as whoever owns that variable.
  Worse on the Cline side, where the `claude-code` provider would pick up the
  shared subscription token in place of the per-env metered key. Both launch
  paths now strip `CLAUDE_CODE_OAUTH_TOKEN`, `ANTHROPIC_API_KEY` and
  `ANTHROPIC_AUTH_TOKEN` from the child before setting whatever aello chose.

- **`aello remove` and `aello login --agent cline` re-read `config.toml` before
  saving it.** Both loaded the config, then blocked on prompts with no time
  bound, then wrote that stale snapshot back — so an `aello login` finished in
  another terminal while the prompt sat there was silently discarded, token and
  all. `aello init` already documented this rule and followed it; these two were
  the outliers.

- **Commands that name an env dir now ask the blueprint which agent it is.**
  Five of them built `.claude-env-<name>` regardless, so on a Cline blueprint
  they operated on a path that does not exist and said nothing: `remove --purge`
  deleted nothing (leaving `.cline-env-<name>` — which holds the provider key in
  plaintext — on disk with its config entry gone), `edit --rename` left the env
  behind so the next run scaffolded a fresh one, and in the TUI a placed Cline
  env was hidden by the "placed here" filter, reported "NO SESSIONS", and its
  delete confirmation claimed nothing was left behind. One `Agent::env_dir`
  helper now serves all five.

- **A Cline blueprint set to `custom` no longer has its persona deleted every
  run.** `custom` and `none` both resolve to "aello writes no persona text", but
  they mean opposite things — `custom` means the env's own copy is authoritative.
  Placement cleared `persona.md` for both, and since Cline rules are re-sent every
  request, nothing else would ever have put it back.

- **`aello persona` re-validates the name it read from config, and refuses a
  Cline blueprint** rather than writing a `CLAUDE.md` that Cline does not read.

- **A fresh clone no longer loses everything `claude-internal/` was tracked to
  keep.** The mirror is written one way, env → tracked folder, *with prune* —
  and the env dir is the one thing a clone is guaranteed not to have. So the
  first `aello run` on a second machine seeded a bare env and then deleted every
  tracked memory note and hand-kept skill that the bare env did not have.
  (Measured on this repository: 11 tracked memory notes and 6 skills against the
  2 and 4 a fresh placement seeds.) `place` now restores a missing env dir *from*
  the mirror before it seeds anything, which is the direction tracking the folder
  always implied.

  A missing mirror *source* also no longer prunes: the memory path is derived
  from the launch directory's exact spelling, and a derivation that comes out
  wrong is indistinguishable from a deletion.

- **Every env dir is gitignored, whatever the role.** The `.claude-env-*` line
  was written only for `github` blueprints, on the reasoning that a Claude env
  holds no secret — but with no shared token configured, Claude Code writes its
  own `.credentials.json` inside it, and `standalone` (the *default* role) never
  got the line. The Cline side has always written its line unconditionally; both
  do now.

- **`aello github-setup` asks before it writes, and checks what it stages.** The
  confirmation prompt came *after* `git init` and the blanket `git add -A`
  `Initial commit`, so answering "n" cancelled nothing that had happened.
  It now confirms first, writes both ignore lines before staging, and refuses to
  commit if anything staged is inside an agent env dir or is a
  `.credentials.json` — an already-tracked file ignores `.gitignore` entirely,
  and this commit is one `gh repo create --public` away from the internet.

- **An unreadable file is no longer treated as an empty one.** `.gitignore`,
  `.claude.json` and the env persona were each read with
  `read_to_string(…).unwrap_or_default()` and then written back in full, so any
  non-UTF-8 byte or a Windows sharing violation replaced the user's file with
  aello's one line. Only `NotFound` defaults now; every other IO error fails
  loudly. (Same distinction `config::load` already draws — the fix existed in
  one file and had not been generalized.)

- **`aello edit --model` no longer applies Claude's model rules to a Cline
  blueprint.** Setting `openai/gpt-oss-120b` was rejected with "use an alias
  (opus, sonnet, haiku)" — a Claude-shaped error on a blueprint that has nothing
  to do with Claude. `add` already had this right; `edit` did not.

- **`aello run <cline-blueprint> -p "hi"` now explains itself.** Cline rejects
  any one-word prompt as a possible subcommand — *"Unknown command or unquoted
  prompt"* — which reads like a quoting mistake and is not one. Every test
  before this used a sentence, which is why it went unnoticed.

- **The Cline credential is installed with `cline auth`, not by writing
  `providers.json`.** Hand-writing that file half-worked, which is worse than
  failing: the env placed, the run launched, the provider was reached, and the
  error came back looking like a bad key — while what had actually happened is
  that **Cline rewrote the file on its next run and dropped the `apiKey`
  outright**, so the request carried no credential at all. A key written by
  `cline auth` survives that same run untouched.

- **`HOOK_VERSION` 18: the voice hook now puts back application volumes its
  own restore could never reach.** The restore works from the live audio
  session list, so an application that goes quiet and then *exits* is dropped
  from it and stays lowered with no record that it ever was — and a reboot
  mid-turn does that to everything at once (measured upstream: the duck wrote
  at 11:15:39, the machine went down at 11:16:05, three applications came back
  at 15%). Worse, one that returns carries a new process id, so its stored 0.15
  is read as its normal and the next duck lands on 0.0225. From 15 the hook
  reads the volumes **Windows has persisted** — the only view that outlives the
  session, the process and the reboot — at the start of each turn, and repairs
  what is still down. `REVOICED_SWEEP` is `0` (off), `signature` (only what the
  current duck level could have produced), or anything else for the default,
  which claims every stored volume between 0 and full. The wide default closes
  a hole the narrow rule has: the signature is computed from the duck level *as
  it is now*, so changing it orphans every value the old one left. An exact `0`
  is never touched in either mode. `speak.py --sweep` shows what a copy can see
  and repairs it; `speak.py --status` summarises the last 50 turns.

- **`aello voice status` now reports a failed Telegram send.** 18 records one
  in the shared `state.json` as `telegram_error` — a key revoiced owns and
  aello only reads, cleared by the next send that works. Before it, a timeout,
  a revoked token, a wrong chat id and an API `ok:false` all produced exactly
  nothing: no history entry, no stderr, no retry, and the line still spoken
  locally, so nothing about the session looked wrong. It lives on the state
  file rather than the history entry because the entry is already written by
  the time the send is attempted, and rewriting the history on the hook path
  once per turn in every env is not worth it.

### Fixed
- **`REVOICED_TELEGRAM` set to an empty value now switches Telegram off**
  (upstream 17), as its documentation always said. `""` was compared against
  `"0"` and read as *on*, so a blueprint that set the variable to nothing opted
  in. Use `0` to opt out on any version; the obvious test agrees with the bug,
  since PowerShell's `$env:X = ''` deletes the variable rather than emptying it
  and so measures the absent case.

- **`HOOK_VERSION` 14: turning Telegram on now reaches sessions that are
  already open.** At 13 the three variables only worked for a terminal started
  after they were set — Windows never pushes a new User-scope variable into a
  running process, so every session already on screen kept sending nothing and
  said nothing about it. 14 falls back to the persisted `HKCU\Environment`
  value when a name is **absent** from the process environment, so no relaunch
  is needed. It only fires on absent, never on present: `REVOICED_TELEGRAM=0`
  in a blueprint still wins, and so does an explicitly empty value, so a
  per-project opt-out survives a machine-wide default. `speak.py --status` now
  names the source (`set`, `set at User scope, picked up from there`, or
  `not set`), which is also the only way to tell 13 from 14 — and only from a
  shell that never inherited the variables, since a fresh one cannot
  distinguish them.

- **The voice hook is re-vendored at `HOOK_VERSION` 13, which adds a Telegram
  sender.** Upstream now delivers the spoken `TL;DR:` line and its mp3 to a
  Telegram chat alongside the local playback, so a response reaches you when
  you are away from the machine. It is opt-in and off unless three environment
  variables are set: `REVOICED_TELEGRAM=1`, `TELEGRAM_BOT_TOKEN` and
  `TELEGRAM_CHAT_ID`. Nothing changes for an env that sets none of them.

  Only `speak.py` moved between 11 and 13 — `duck.py`, `focus.py`, `notify.py`
  and `win_audio.ps1` are byte-identical — but all five are re-vendored as a
  unit anyway, because the guard that makes a partial copy survive is also what
  makes it silent. Unlike the 8 → 10 → 11 sequence, a copy left on 11 is not
  damaging, just deaf to Telegram: the audio-session bug was fixed at 11 and
  nothing here touches `duck.py`. `aello voice status` reports the vendored
  version, and a placed copy answers `python <env>/hooks/speak.py
  --hook-version` before any optional import, so a partial copy still tells you
  the truth.

- **Every response now ends with one block: the `TL;DR:` line, with 3–4
  numbered next steps beneath it.** The per-turn `UserPromptSubmit` hook's last
  rule now fixes the whole shape of an answer: a few sentences of prose, then
  one closing block — a summary of two to four sentences with the actions you
  take next directly under it, and nothing after those. A long reply can be
  skipped entirely and still acted on, which is the point, since a wall of
  findings is not an instruction. The steps are dropped when nothing is waiting
  on you; the `TL;DR:` line never is. ~300 tokens a turn, up from ~190.

  Summary and steps shipped as two separate rules and lasted about an hour:
  the summary said a thing, the steps repeated it, and you had to reconcile two
  closing sections. Merged, the spoken line introduces the list it sits on top
  of, and it grew from two sentences to two-to-four to absorb what the steps
  used to duplicate. The ordering is not taste — `speak.py` matches the last
  `TL;DR:` line and reads to end of line, so the summary must stay on one line
  (a wrapped one is spoken with its tail silently cut off) while numbered steps
  beneath it match nothing. Verified against the real `extract_tldr`.

  The steps have to **stand alone**, and saying so is load-bearing: the first
  wording produced correct steps underneath an essay that still had to be read
  to make sense of them — the exact failure the rule exists to prevent. It now
  states the assumption outright (the user skips every word above the steps, so
  no step may say "as described above") and the concise rule caps the prose at a
  few sentences, with anything that matters moved into a step rather than a
  paragraph. The two only work as a pair.

  The no-plans rule needed rewording to sit beside it: it used to ban "numbered
  proposals", which is exactly what the new rule asks for. It now bans laying
  out what the agent intends to do and waiting for sign-off. The distinction is
  whose hands the actions are in — a plan is the agent's next moves held for
  approval, the steps are yours.

### Fixed
- **The ducking fix at 10 was incomplete, and 10 is still damaging.** The
  vendored voice hooks move to `HOOK_VERSION` 11. Upstream keyed its record of
  the pre-duck volume on the process id, and **one process owns several audio
  sessions** — a browser runs a media stream and a notification stream under one
  pid, and `GetAllSessions()` yields a control for each. The second reading
  overwrote the first while both were being lowered, so the restore raised one
  and left the other down with its record already deleted: the same 1.0 → 0.15 →
  0.0225 → floor ratchet as at 8, just needing two sessions to trigger. Measured
  on this machine *after* the 41-env re-vendor at 10: five applications at
  exactly 0.15 from one duck. Records are keyed on `InstanceIdentifier` now,
  with the pid kept for the liveness check, and version 11 still honours the
  pid-keyed records that copies on 10 left behind rather than stranding the
  applications they describe. `restore()` also no longer runs unlocked when the
  lock is merely contended, which let the station's poll delete a record while a
  worker was still inside the lowering loop.

  Also at 11: `import duck` in `speak.py` is guarded like `focus` and `notify`,
  so a partial vendor reports through `MISSING` instead of raising at module
  scope — that failure killed `--hook-version` and `--status` too, leaving an
  env silent with every diagnostic built for it dead as well. The lock file
  carries an owner token (a holder that was legitimately stolen from used to
  delete its successor's live lock), `state.tmp` is per-process, and a
  `state.json` that cannot be parsed is never written over.

- **`aello voice` no longer erases the voice pool when the state file is
  corrupt.** `read_state` treated an unparseable `state.json` exactly like a
  missing one — empty defaults — and the next `mute`/`unmute`/`M` wrote those
  back, taking every preset and lease with them. Absent still reads as empty;
  present-but-unparseable now fails with the path and leaves the bytes alone for
  the hook to recover from. Found by reading upstream's fix for the same bug on
  the Python side, since both processes write this one file.

- **Ducked audio comes back up.** The vendored voice hooks move from
  `HOOK_VERSION` 8 to 10, which carries an upstream fix for a bug that
  permanently lowered the machine's per-application volumes. `duck.py` deleted
  its record of the original volumes whenever it could not enumerate audio
  sessions — which was every poll from a second thread, since comtypes
  initialises COM per thread — so the next duck read an already-lowered volume
  as normal and lowered it again. Windows keeps per-application volume in the
  registry against the executable's path, so this outlived the session, the
  process and the reboot: applications ended up at 0.15, then 0.0225, then the
  0.01 floor, with nothing on disk recording what normal had been.

  Upstream also takes a lock on the record, writes it *before* lowering
  anything, and keeps entries it could not restore instead of dropping the whole
  store. Measured here before the re-vendor: one application sitting at 0.15
  with no `duck.json` beside it. Existing envs adopt the fix on their next
  `aello run` — until then their copy is still the one doing the damage.

  Also in the bump: a `spoken` lifetime counter that revoiced keeps in the
  shared `state.json` (aello's keys are untouched), and a `REVOICED_HISTORY`
  default of 1000 rather than 200, so the shared data directory grows to roughly
  92 MB of audio instead of 18.5 MB.

### Added
- **No plans, anywhere.** The per-turn `UserPromptSubmit` hook gains a fourth
  rule — never present a plan for approval, never use plan mode; ask a short
  question or do the work, and where the choice is genuinely yours, offer
  concrete options to pick from. A plan handed over for sign-off goes unread,
  and the round trip buys nothing a question wouldn't.

  It ships with an enforcing half: a bundled `PreToolUse` hook matching
  `EnterPlanMode|ExitPlanMode` that denies both, so plan mode is unavailable
  rather than discouraged. Neither half is redundant — the hook stops the tool,
  and only the injected text stops a numbered proposal written as ordinary
  prose, which is what a plan usually looks like. The matcher is load-bearing:
  an unmatched `PreToolUse` group runs on *every* tool call, a Python spawn per
  `Read`.

  Verified as far as it can be, and no further: a `PreToolUse` deny does block a
  tool and return its reason (measured against `Read`), and the matcher does
  scope it (a `Glob` in the same run never reached the hook). Whether the two
  plan tools emit a `PreToolUse` event at all is **unproven** — `claude -p`
  never calls `ExitPlanMode` under `--permission-mode plan`, so print mode
  cannot answer it. A denial therefore appends to `plan-blocked.log` beside the
  script; the first line to appear settles it, and an empty log across envs that
  have wanted to plan means the text is doing all the work.

  Both reach existing envs on their next `run`, like the other hooks.

### Added
- **`aello persona <name> --from <file>`** — installs an agreed global persona
  into a placed env: replaces its `CLAUDE.md`, sets `claude_md = "custom"` so
  aello stops seeding a template over it, and bumps the generation recorded in
  `<env>/persona.gen` (`gen1 2026-08-03`). The only command that overwrites a
  persona; `run` still never does.

### Changed
- **The persona choice is now three values: `coder`, `none`, `custom`.** A
  coding project starts on `coder`, anything else on `none`, and both become
  `custom` the first time a generated persona is accepted — after which the
  env's own `CLAUDE.md` is authoritative. Existing configs migrate on load: a
  missing `claude_md` becomes an explicit `none`, so a blank persona reads as a
  decision rather than an oversight.

  A **path** is still accepted for the blueprints that point at a persona file
  you maintain, including the ones sharing a single file between several envs.

### Removed
- **The `sysadmin` persona template.** Barely used, and close enough to `coder`
  to not earn its own slot. The one blueprint on it migrates to `custom`: its
  env already holds the text, since aello never overwrites a persona, so nothing
  is lost — and calling it `coder` would have claimed text that env doesn't have.

### Added
- **Every env now carries three response rules, injected on every prompt.** A
  bundled `UserPromptSubmit` hook asks for concise answers (no preamble, filler
  or hedging), rules out sycophancy (no opening praise or agreement, no
  validating an unchecked premise, no softening a finding — say plainly when the
  user is wrong, and say "I don't know" when that's the answer), and requires the
  trailing `TL;DR:` line the voice speaks. ~150 tokens per turn.

  Per turn rather than per session because style decays: an instruction given at
  turn one is buried by turn eighty. On a hook rather than in the persona because
  the persona is written once and never clobbered, so editing it would reach no
  existing env — while `place()` rewrites the hook script every run, so all
  existing envs adopt this on their next launch with no backfill.

  The TL;DR instruction moves here from the persona as a result. Envs already
  carrying that section keep it (the persona is yours; aello won't edit it), so
  they simply have it in both places. Unregister the hook by hand and the persona
  append comes back, so the voice can't go silent for want of an instruction.

### Removed
- **`AUDIT-2026-07-08.md` is no longer in the repo.** Audits are working notes
  and were ruled out of git, and `AUDIT-*.md` was added to `.gitignore` — but the
  file was already tracked, and `.gitignore` has no effect on a tracked file, so
  it stayed published the whole time. Untracked now (the local copy is kept and
  archived outside the repo).
- **The dead `skills/sync/SKILL.md` at the repo root**, a hand-written `/sync`
  from before `templates::render_sync_skill` existed. Nothing has read it since;
  the generated skill is per blueprint and lives in the env dir.

### Fixed
- Stale `--github` / `--project-md` / "capability checklist" references in
  `CONTRIBUTING.md`, `docs/concepts.md`, `scripts/demo-recording.md`, the PR
  template and two source comments — all describing flags removed in 0.2.0.
- The release process in `CLAUDE.md` and `docs/development.md` still said the
  bump job always adds a patch. It hasn't since the change that made `v0.2.0`
  possible: a version with no tag yet is published exactly as written.

### Added
- **Every env now launches with `--thinking-display summarized`**, so each turn's
  reasoning is captured as a readable summary instead of an empty block. The
  API's default is `omitted`: thinking blocks arrive with empty text and only an
  opaque signature, which meant contextdb archived a full record of what was done
  and nothing of what was thought — 2,842 thinking blocks across 53 transcripts,
  every one empty. `display` controls visibility only; thinking happens and is
  billed identically, so this costs nothing. The raw chain of thought is never
  returned by any model — a summary is the ceiling. Override for one run with
  `aello run <name> -- --thinking-display omitted`.
- The README links the live docs site and carries a docs badge.
- **`docs/upgrading.md`** (`aello docs upgrading`) — the page to point an
  environment at the first time it meets 0.2: what migrates itself, the one case
  that changes behaviour, the removed flags, and the pre-0.2-binary hazard.
- **The SessionStart hook now opens every session by saying it is running under
  aello** — which blueprint, that the env dir is rewritten on every run
  (`.aello-keep` to pin a skill), that the seeded skills are the user's to type,
  and that commits are attributed automatically. Nothing else told a session any
  of this: the env dir is gitignored, the persona belongs to the user and usually
  doesn't mention aello, and a project `CLAUDE.md` only exists for a maintainer —
  so agents edited files the next launch silently overwrote. It lives on the hook
  rather than in the persona because `place()` rewrites the hook every run, so it
  reaches an env placed months ago without touching anything the user owns.
  ~257 tokens per session.

### Fixed
- **contextdb archived transcripts by *path*, and the path stopped resolving.**
  Claude Code deletes its own session files after `cleanupPeriodDays` (default
  **30**), and the env dir holding them is gitignored and removed by
  `aello remove --purge`. An audit on 2026-08-03 found **15% of 265 archives
  already pointed at nothing**, with a clean cliff at the 30-day mark — 6–14%
  dead under 30 days, 44% at 30–39. Nothing ever errored; the archive quietly
  stopped being one.

  SessionEnd now **copies** the transcript to `<ts>_<session>_transcript.jsonl`
  beside the record and reports the result in a new `transcript_archived` field,
  and placement sets `cleanupPeriodDays` to 365 — self-healed into existing envs
  only when the key is absent, so a value you chose is left alone. Expect
  contextdb to grow: transcripts are 1.3 MB at the median and tens of MB at the
  tail.

  The copy also opts out of Windows' 260-character `MAX_PATH` (`\?\` prefix).
  The encoded-cwd directory repeats the whole project path, so a deep project
  pushes the transcript past the limit and the copy fails — measured at 325
  chars, where the archive quietly degraded back to a pointer.

  The same audit confirmed **PostCompact is dormant, not broken** — it fires only
  on compaction, which a 1M-context session ended with `/clear` never reaches
  (265 SessionEnd records to zero PostCompact ones for any aello blueprint). It
  stays seeded for workflows that do compact.

## [0.2.0]

First stable line. The blueprint interface is settled: a blueprint is a name, a
model, a persona and a **role**, and that shape is what the rest of the tool is
built on. Existing configs migrate themselves — see below.

### Changed
- **The five capability flags are now three roles.** `maintainer` (owns the
  project `CLAUDE.md`, `CHANGELOG.md`, `docs/`, `README.md` and git),
  `contributor` (commits, pushes, and logs its own change in `CHANGELOG.md` —
  never touches the other three) and `standalone` (no `/sync` at all). One flag
  replaces ten: `aello add --role <role>` and `aello edit --role <role>`;
  `--project-md` / `--github` / `--changelog` / `--docs` / `--readme` and their
  `--no-` counterparts are **removed**, and the TUI's capability checklist is a
  role picker. `aello list`'s last column is now `ROLE`.

  The flags were never independent in practice. Grouped **by repo** — the unit
  that matters — every multi-blueprint project has exactly one blueprint holding
  everything and the rest holding only git duties; 32 of the 32 combinations
  nobody used were surface area with no behaviour behind it. Grouped fleet-wide
  they look all-or-nothing instead, which is the reading that nearly got the
  distinction deleted altogether: it is the whole multi-agent point and it is now
  the middle role rather than an accident of five checkboxes.

  **Existing configs migrate themselves** on the next load, with nothing to run:
  anything maintaining prose (`project_md`, `docs`, `readme`) becomes a
  maintainer, anything left holding only git becomes a contributor, nothing
  enabled becomes standalone. The old `[blueprints.caps]` table is read once and
  dropped the next time aello saves. The one behaviour change: a blueprint that
  had *some but not all* of the prose capabilities is now a maintainer, so
  `/sync` covers the files it previously skipped — `aello edit <name> --role
  contributor` if that isn't what you want.
- `docs/capabilities.md` is now `docs/roles.md` (`aello docs roles`).

### Added
- **A documentation site.** `/docs` on the landing page is generated from this
  repo's `docs/` directory at build time — the same files the binary embeds — so
  there is no second copy to drift. Sidebar of pages, per-page contents, and
  `*.md` cross-links rewritten to routes. Deployed to GitHub Pages by
  `.github/workflows/pages.yml` on any push to `main` that touches `site/**` or
  `docs/**`. The landing page gains a Workflows section linking into it, and its
  capability cards are now role cards.
- **Four new reference docs**, bundled into the binary like the rest — readable
  with `aello docs <name>`, in the TUI reader (`?`), and on the docs site:
  - `workflows.md` — task-shaped walkthroughs: your first env, two agents in one
    repo, the work → `/sync` → `/handoff` → `/clear` loop, resuming, updating an
    already-placed env, renaming, GitHub setup, removal. Ends with the
    conventions for adding another one.
  - `skills.md` — the four seeded skills in detail: why they're manual-only
    twice over, why they're regenerated on every run, and `.aello-keep`.
  - `development.md` — working on aello itself: the test-the-deployed-copy rule,
    re-vendoring the voice hook, the release pipeline and its sharp edges.
  - `troubleshooting.md` — failure modes and what they actually mean, starting
    with where everything lives on disk.
- **Voice hook re-vendored at `HOOK_VERSION = 8` — history now records what you
  asked, not just what was answered.** Upstream `revoiced` reads it from the
  transcript the hook already opens to find the `TL;DR:` line, so there is no new
  hook event, no new file, and no `settings.json` change. Two upstream env vars,
  both defaulting to on: `REVOICED_PROMPTS=0` disables capture outright and
  `REVOICED_PROMPT_MAX` (4000) truncates a pasted log; aello sets neither and
  passes the upstream defaults through. Existing envs pick it up on their next
  `run`, and `python <env>/hooks/speak.py --hook-version` reports it — printing
  before any optional import, so even a partial copy answers.

  It landed across versions 6, 7 and 8 in one evening, both later bumps fixing
  bugs found from this side by running upstream's own `user_prompts()` against a
  live transcript **from a vendored copy** rather than the revoiced checkout: 7
  stopped `<command-args>` being discarded as wrapper noise (a slash command with
  arguments recorded as a bare `/note`, and `/loop 5m /foo` as `/loop`), and 8
  stopped an edited-then-sent message being recorded twice (the transcript holds
  both the short draft and the long final, and the dedup only caught exact
  repeats, not prefixes). Only `speak.py` moved in all three.

### Fixed
- **The README is for users again, and `docs/` is for developers.** The README had
  filled up with detail that only makes sense once you've read the source — the
  voice section alone ran to Windows toast-identity registration, per-OS state
  directories and five-file vendoring, most of it a near-duplicate of a `docs/`
  page that covered the same ground better. That section is now what a user needs
  (it speaks, here's how to silence it, here's what to install), Concepts lost the
  transcript path templates and hook internals, and the mechanism moved to a new
  **`docs/voice.md`** — its own page, since the voice explicitly isn't a
  capability and was sitting inside `capabilities.md` anyway. New pages under
  `docs/` need no code change: they appear in `aello docs` and the TUI reader on
  their own.
- **The seeded skills now say they are yours to run, not the agent's.**
  `disable-model-invocation: true` stops the model calling one as a *tool*, but
  nothing stopped an agent opening the `SKILL.md` with `Read` and carrying out
  the steps by hand — which produces the worse outcome, because you believe a
  checkpoint ran when it did not. Every generated skill now opens with a banner
  saying that following the instructions **is** running the skill, whichever
  route you took to them. The frontmatter closes the tool path; the banner closes
  the reading path.
- **`place` no longer re-adds the TL;DR section to a persona that has moved it
  elsewhere.** The instruction that makes the voice work lived in the global
  persona — the one file most likely to be rewritten wholesale. An env can now
  carry it on a `UserPromptSubmit` hook instead (registered by hand in that env's
  `settings.json`, injected on every turn), and `place` leaves the persona alone
  when it sees that hook. Without it the append is unchanged, so nothing moves
  for an env that has not opted in.
- **`/sync` no longer tells you to commit the transient env files.** Its staging
  rule is "stage only what you created or modified this session" — and a
  `<blueprint>.HANDOFF.md` written by `/handoff` is exactly that, so the rule as
  written swept in a file that is untracked on purpose and deleted on the next
  boot. `/sync` now excludes `*.HANDOFF.md` and `*.NOTE.md` explicitly, and
  `/handoff` says so on its own side too.
- **`/note` handles a target env in another repo.** It assumed the target shared
  this repo: it sanity-checked `.claude-env-<target>` at *this* project root,
  reported a missing one as a probable typo, and then wrote the note here anyway
  — where the target's SessionStart hook, which only reads its own project root,
  would never see it. It now resolves the target's project root first and writes
  there, treats "cannot locate it" as the only typo case, and reports the full
  path it wrote. It also asks for the blueprint's canonical casing (the hook
  matches `<Name>.NOTE.md` exactly, so wrong casing is a silent dead letter on a
  case-sensitive filesystem), and no longer says "append a note" one paragraph
  above the step that says to overwrite.
- **`/sync` stopped guessing the memory directory.** It named
  `projects/<this-project>/memory/`, inviting a hand-built path; Claude encodes
  the cwd by folding every non-alphanumeric character to `-`, so a guess creates
  a second empty memory dir nothing reads. It now says to list
  `$CLAUDE_CONFIG_DIR/projects/` and use what is there.
- **`/sync`'s pre-push rebase no longer cites a CI that may not exist.** The
  reason given was aello's own `release: vX [skip ci]` auto-bump, which only runs
  if the scaffolded `.github/` was actually committed. The advice is right for
  any repo touched from more than one place; the rationale now says that instead.

### Added
- **`.aello-keep` — pin a hand-edited skill.** The four seeded skills (`/sync`,
  `/handoff`, `/note`, `/twosentences`) are rewritten on every run, so a project
  that needed a different workflow — a `/sync` that also deploys, say — silently
  lost its version on the next `aello run`, and the `claude-internal/` mirror
  then committed the generated one over it. An empty `.aello-keep` beside a
  skill (`.claude-env-<name>/skills/sync/.aello-keep`) makes `place` leave it
  alone: not regenerated, and not deleted if the blueprint later drops all its
  capabilities. Per env dir, so the same blueprint used in another project still
  gets the generated skill; every other file in the env keeps self-healing.
- **`--voice` capability — spoken responses.** A blueprint with `--voice` gets a
  `Stop` hook that reads each response's trailing `TL;DR:` line aloud through a
  free Edge neural voice, and a `SessionEnd` hook that returns the voice it
  leased. The hook (and its two helper files) is copied **into the env** and
  registered as `$CLAUDE_CONFIG_DIR/hooks/speak.py`, so it never points at a
  checkout elsewhere on disk — a newly placed env speaks with no hand-editing,
  and moving an unrelated directory can't silence it. The persona gains a section
  asking for the TL;DR line the hook speaks (appended, never clobbered, so
  enabling voice on an existing env works). State — voice pool, per-session
  leases, mutes — is machine-wide, so concurrent envs each get a different voice
  and queue behind one playback lock instead of talking over each other.
  Available as `--voice` / `--no-voice` on `add` and `edit`, in the `init`
  wizard, and as a row in the TUI capability checklist. Enabling it on an env
  whose `Stop` hook was wired up by hand against a checkout **replaces** that
  entry rather than adding beside it, so migrating doesn't speak every response
  twice; unrelated hooks are left alone.
- **`aello voice`** — the off switch: `mute` (optionally `--project`), `unmute`,
  `stop` (cut off the current sentence), and `status`. It writes the shared state
  directly, so it needs neither Python nor a placed env and works from any
  directory — which is exactly where you are when a machine you didn't expect to
  talk starts talking.
- **Prebuilt macOS binaries** — `aello-aarch64-macos` (Apple Silicon) and
  `aello-x86_64-macos` (Intel) now ship with every release, so macOS installs
  with the same one-liner as Linux and `aello update` works there. The binaries
  are unsigned; the installer strips the quarantine attribute for you, and the
  README documents the manual `xattr -d com.apple.quarantine` step.
- **`M` mutes the voice from the TUI** — the same machine-wide switch as
  `aello voice mute`, on a single key, so you don't have to leave the TUI (or
  find a script path) when a machine you didn't expect to talk starts talking.
  It silences every env and cuts off the sentence already playing; while muted
  the footer reads `VOICE: MUTED`, so a quiet machine isn't mistaken for a
  broken hook.
- **A landing page**, in `site/` — a static Next.js build covering the install
  one-liner, the three-command quick start, the capability table, and the
  universal skills. Nothing in the CLI depends on it; `npm run build` in `site/`
  emits plain HTML in `site/out/` for any static host.

### Changed
- **Every env speaks — the voice is no longer a capability.** `--voice` and
  `--no-voice` are gone from `add` and `edit`, the row is gone from the TUI
  checklist and the `init` wizard, and every placed env now gets the speak hooks
  and the TL;DR persona section unconditionally. Choosing it per blueprint bought
  nothing, and the moment you actually want silence — a machine that has started
  talking — is a runtime decision: `aello voice mute` (or `M` in the TUI) covers
  every env at once and is reversible in one keystroke. Existing envs pick the
  hooks up on their next `aello run`, and a `voice = …` left in your config is
  ignored on load and dropped on the next save.
- **The voice hooks are re-vendored from upstream `revoiced`** (`a86023a`). A
  voice-enabled env now gets **its own voice per environment** rather than per
  working directory: several blueprints sharing one repo no longer sound alike,
  and an agent that works in a subfolder stays the same speaker instead of
  becoming a second project. Identity is read from `CLAUDE_CONFIG_DIR`, which
  aello already exports, and falls back to the old directory-keyed behaviour when
  it is absent. Also picked up: em dashes in a `TL;DR:` are no longer spoken as
  mojibake on Windows, and a mute set while the hook was mid-write is no longer
  silently dropped. Still three files — upstream's optional `focus`/`notify`
  siblings are station-side and deliberately not vendored.
- Releases are now **versioned**. Every build publishes an immutable `vX.Y.Z`
  release (binaries + `SHA256SUMS`) alongside the existing rolling `latest` tag,
  so you can pin or roll back to a specific version and package managers have a
  stable URL to point at. `install.sh` and already-installed binaries keep using
  the rolling tag exactly as before — nothing to change on your side.
- `aello remove <name>` now **prompts for confirmation** before deleting a
  blueprint (`--yes` skips the prompt). A new `--purge` flag also deletes the
  placed `.claude-env-<name>/` env dir and its `claude-internal/<name>/` mirror
  in the current project; without it the on-disk dirs are left as-is and you're
  told they remain.
- The `/handoff` note is now written to `<blueprint>.HANDOFF.md` (prefixed with
  the blueprint name) instead of a shared `HANDOFF.md`, so multiple blueprints in
  one repo each keep their own handoff without overwriting each other. The
  SessionEnd hook archives the matching per-blueprint file.

### Fixed
- **Desktop notifications now work on a machine that has never run the revoiced
  station.** Windows drops a toast sent under an unregistered AppUserModelID with
  no error at all, and only the station registered one — so on such a machine
  every env was silently notification-less, the same failure as the incomplete
  vendor one layer out. Launching an env now runs the vendored
  `notify.py --register` (no test toast, idempotent). At `HOOK_VERSION = 5` that
  claims the toast identity and leaves the `revoiced:` protocol to a copy with an
  `action.py` to serve it — an env has none, so it can't take a working toast
  button away from the station.
- **"Skip this one" now skips that one.** A toast stays in Windows' notification
  centre for three days, and the skip link named no turn — so pressing the button
  on an old notification cut off whatever happened to be speaking at that moment,
  which across dozens of envs is usually another env's line. Re-vendored the voice
  hook at `HOOK_VERSION = 4`: the URI carries the turn id and the station refuses
  a stale one. Vendored at `HOOK_VERSION = 5`; existing envs pick it up on their
  next `run`. The drift check now
  covers **all five** vendored files rather than only `speak.py` — `HOOK_VERSION`
  lives in `speak.py` alone, so a re-vendor touching just `duck.py` or
  `win_audio.ps1` used to slip past it.
- **Desktop notifications work again — the voice hook was vendored incomplete.**
  `speak.py` imports four siblings; aello copied two of them. The other two,
  `focus.py` and `notify.py`, are imported behind a `try`/`ImportError` guard, so
  nothing failed: every env spoke normally while `notify`'s stub reported "not
  shown" and **no desktop notification was ever raised, in any env**, with
  nothing anywhere saying so. Placement now writes all five files
  (`speak.py`, `duck.py`, `focus.py`, `notify.py`, `win_audio.ps1`), so any
  existing env picks them up on its next `run`. `focus.py` also restores the
  window tracking that lease reaping and the toast's **Go to terminal** button
  depend on.
- **A vendored copy that has fallen behind is now visible.** `speak.py` carries a
  `HOOK_VERSION`, bumped upstream whenever one of the five hook-path files
  changes; aello records the version it vendored, a unit test fails if a
  re-vendor moves one without the other, and `aello voice status` prints it
  alongside the mute state. `python <env>/hooks/speak.py --status` prints what
  that env actually runs, so the two can be compared. The upstream re-vendor also
  brings a hard timeout on a stuck player (one wedged process used to hold the
  machine-wide speaker lock and silence every env) and a `win_audio.ps1` that no
  longer drops a line when the audio duration resolves slowly.
- **`aello update` no longer re-downloads and reinstalls a version you already
  have.** It never compared versions, so every run pulled the whole multi-MB
  asset and rewrote the running binary — which on Windows means renaming the live
  exe aside, and hard-fails when a second aello is running. `--force` reinstalls
  deliberately. A failed release-API fetch is now an error instead of a silent
  success, so `aello update && …` can't carry on as though it were current; all
  three network calls have timeouts (a stalled server used to hang it forever);
  and the Windows replace stages the new binary before moving the old one aside,
  so a failed write can't leave a truncated exe at the install path.
- **A blueprint whose persona file has moved or been deleted now fails on `run`
  instead of launching without it.** `add` and `edit` both reject an unusable
  persona, but `run` warned to stderr — moments before Claude's alternate screen
  wiped it — and carried on with no persona at all.
- **`--model " opus "` no longer reaches `settings.json` verbatim.** The value was
  validated as trimmed and lowercased, then the raw string was stored.
- Over-long blueprint names are rejected with a clear message instead of failing
  later inside `create_dir_all` with a raw OS error.
- `aello init` re-reads the config after its prompts, so a token captured by an
  `aello login` in another terminal while you were answering is no longer
  discarded.
- **A `Stop` hook of your own whose command merely contains `speak.py`** (say
  `tools/my_speak.py`) is no longer deleted on every `aello run` — the match is
  anchored on a path boundary now. It took any unrelated command in the same
  group with it.
- **`aello voice mute --project` no longer cuts off another project's audio.** The
  stop token is machine-wide; only a global mute should use it.
- Concurrent writes to the shared voice state can no longer corrupt it: aello and
  the hook were staging through the same fixed temp filename.
- **The TUI docs reader (`?`) can reach the end of a page.** The scroll cap
  under-counted wrapped rows, making the last lines of longer docs unreachable;
  `Home`/`End` (and `g`) now jump to either end.
- The TUI no longer leaves your terminal in raw mode on the alternate screen if
  it fails to start after switching screens.
- The TUI reports when a delete leaves the env dir and `claude-internal/` mirror
  behind, when an edit's target vanished mid-flow (it used to claim success), and
  why creating a contextdb folder failed instead of silently closing the box.
- Editing a blueprint in the TUI no longer discards a full `claude-*` model id or
  a custom persona path just because you scrolled the picker and came back.
- `aello edit --rename` now says that only the current project was updated, since
  a blueprint placed elsewhere keeps its old env dir there.
- `aello login` distinguishes `claude setup-token` failing from its output being
  unparseable, instead of inviting you to paste a token that was never issued.
- **`claude` installed via npm is found on Windows.** `claude.cmd` is a shim, and
  only `claude.exe` was ever looked for, so a working install reported "claude is
  not installed".
- The transcript hooks no longer crash with a traceback when the contextdb
  directory can't be created.
- **The `/handoff` note is now actually read on boot, and then deleted.** The
  skill has always told the agent the note is "read on boot, then deleted" and
  stamped a banner saying so, but nothing ever read it — there was no
  `SessionStart` hook, so the file sat at the project root dirtying `git status`
  and being re-archived verbatim by every later session. A new `SessionStart`
  hook delivers it (and `<name>.NOTE.md`, the cross-env inbox from `/note`) into
  the session, then removes the file. Deleting is safe because `SessionEnd`
  already archived the content to contextdb.
- **`aello edit <name> --rename` accepts a case-only change** (`coder` →
  `Coder`). On Windows and macOS the destination "already exists" because on a
  case-insensitive filesystem it *is* the source, so the rename bailed while
  naming the source as the obstruction — the feature was unreachable on both
  platforms aello ships binaries for. The case-flip is now routed through a temp
  name.
- **`--rename` carries the `<name>.HANDOFF.md` and `<name>.NOTE.md` files with
  it.** Both are addressed by blueprint name and both consumers key off the new
  one, so a pending resume note or cross-env inbox was left orphaned under a name
  nothing would ever look for again. An existing file at the destination is never
  clobbered.
- **The tracked `claude-internal/<name>/` mirror is cleared when the `github`
  capability is dropped.** It was only ever written from inside the `github`
  branch, so turning the cap off froze the folder in git forever — still carrying
  a git-flavoured `/sync` skill the blueprint no longer had — with
  `aello remove --purge` the only way to clear it. Another blueprint's mirror in
  the same repo is left untouched.
- A hook event whose value in `settings.json` isn't an array is now left alone
  rather than overwritten, matching every other self-heal, and a `settings.json`
  that doesn't parse as JSON now says so on stderr instead of skipping in silence.
- **`aello edit <name> --model` now actually reaches an env that has already been
  placed.** `settings.json` is the only thing that tells Claude Code which model
  to use, and it was written only when it didn't already exist — so editing the
  model updated `config.toml`, `.aello.toml` and `aello list` while the env kept
  running the model it was first placed with, indefinitely, and `edit` still
  reported "Changes apply on the next `aello run`". The model is now merged into
  the existing file on placement, key by key, so hand-added settings like
  `effortLevel` and `enabledPlugins` survive untouched.
- **`Ctrl`+letter no longer triggers the plain-letter command in the TUI.** Key
  presses were matched on the letter alone, ignoring modifiers, so **`Ctrl+U`
  started an unprompted self-update** that replaced the running binary,
  `Ctrl+S` silently wrote the contextdb path to disk, and `Ctrl+D` opened the
  delete modal. `Ctrl+C` now quits from every mode instead of doing whatever the
  bare letter did, and a chord typed into a name field is ignored rather than
  inserting the letter.
- **Uppercase keys work.** The footer advertises `[F] [S] [A] …` and the delete
  modal says `[Y] CONFIRM`, but only lowercase was bound — so pressing Shift+Y as
  instructed, or using any command with Caps Lock on, did nothing at all and gave
  no feedback. Command keys are now case-insensitive. Text entry still keeps its
  case, so capitalised blueprint and folder names are unaffected.
- The SessionEnd self-heal no longer skips envs that already have a **third-party
  `SessionEnd` hook**. It bailed whenever the `SessionEnd` key existed at all, so
  adding any hook of your own permanently blocked aello from installing its own
  transcript-archiving hook — envs placed before the feature existed would
  silently never gain it. The check is now keyed on aello's own command, and its
  hook group is appended alongside yours.

### Docs
- `docs/capabilities.md` gains a "when it doesn't speak" section for `--voice`:
  check `aello voice status` for a mute, then read the hook's `history.jsonl` —
  a real voice name means synthesis worked, `system fallback voice` means
  `edge-tts` wasn't found, and no entry at all means the hook never ran or the
  response had no `TL;DR:` line.
- `docs/concepts.md` now warns that a project-level `<project>/.claude/settings.json`
  is **silently ignored** under aello (`CLAUDE_CONFIG_DIR` points at the env dir),
  and says to use `<project>/.claude-env-<name>/settings.json` instead.
- Added a dedicated **macOS (build from source)** install section to the README
  (`cargo install --git …`, Rust-toolchain prerequisite, and the caveat that
  `aello update` only ships Linux/Windows binaries so macOS updates by rebuilding).

### Security
- `aello update` now verifies the downloaded binary's **SHA-256** against a
  `SHA256SUMS` asset published with the release before installing it, so a
  hijacked release asset or a TLS-intercepted download can't silently replace
  your binary. Releases without the manifest still update (the check is skipped
  with a note). The download is also now size-capped (128 MiB) so a malicious
  endpoint can't stream unbounded data and OOM the process.
- A hand-edited blueprint name in `config.toml` is now re-validated before it's
  used to build filesystem paths. `validate_name` only ran on config *writes*;
  read paths interpolated the stored name straight into `.claude-env-<name>` /
  `claude-internal/<name>`, so a name like `../../evil` could escape the project
  dir when placing (or, under `remove --purge`, deleting). `run` and
  `remove --purge` now re-gate the name at the sink.
- `aello login` / `aello init` no longer echo the token line to their own
  stdout. The capture loop tees `claude setup-token`'s output so the auth URL
  shows on a headless box, but it now redacts the line carrying the `sk-ant-…`
  token — previously running login under any stdout capture (`| tee`, a CI job
  log, `script`, tmux) persisted the long-lived token in cleartext in that log.
- On Unix, `config.toml` (which holds the plaintext, non-rotating OAuth token)
  and its directory are now written owner-only (`0600`/`0700`) instead of the
  default world-readable `0644`, so another local user or a low-privilege
  process can no longer read the token. No-op on Windows.

### Fixed
- The TUI now restores your terminal (raw mode, alternate screen, cursor) if it
  panics, instead of leaving the shell unusable, and undoes raw mode if it can't
  enter the alternate screen. The session-resume list also no longer risks a
  panic truncating a non-ASCII session id (it now truncates by character), and
  the in-app docs reader's scroll uses saturating arithmetic. A stale `/sync`
  skill that can't be removed when a blueprint drops all capabilities now
  surfaces the error instead of being silently re-committed.
- The `github` cap no longer appends a near-duplicate `.gitignore` line when a
  `.claude-env-*/` (trailing-slash) entry already exists. And aello no longer
  alphabetically reorders the keys in Claude-owned JSON (`.claude.json`,
  `settings.json`) when it reads and rewrites them, keeping git diffs clean.
- `aello edit --rename` is now transactional. It previously renamed the env dir
  first and only then checked whether the `claude-internal/<new>/` mirror
  collided — so a collision (or any fs error) left the env dir already moved but
  the config not saved, and `run <old>` re-scaffolded a fresh env, orphaning the
  renamed one. Both destinations are now pre-checked before any move, and a
  failed mirror move rolls the env-dir move back.
- **Editing a blueprint in the TUI (`E`) no longer downgrades its model or
  drops a custom persona.** The curated pickers can't represent a full
  `claude-*` model id, the `default` alias, or a custom persona path, so opening
  a CLI-configured blueprint and saving — even just to toggle one capability —
  used to rewrite its model to `opus` and its persona to `none`. The edit flow
  now preserves the original model/persona unless you actually change that
  picker, and shows a `KEEPING = …` hint when the stored value is off-list.
- **Config/token loss on a transient read error is prevented.** `config::load()`
  previously turned *any* I/O error (not just "file missing") into an empty
  default `Config`; since every command is `load → mutate → save`, one momentary
  file lock — routine on Windows from OneDrive, an AV scanner, or the Search
  Indexer holding `config.toml` — made the next `save()` overwrite it with an
  empty default, destroying every blueprint and the non-rotating OAuth token. It
  now defaults **only** when the file genuinely doesn't exist and propagates all
  other errors so the command aborts instead of clobbering your config.
- Blueprint names are now restricted to **ASCII** alphanumerics (plus `-`/`_`).
  Names like `café` or full-width characters previously slipped past
  `validate_name` yet made fragile, cross-platform-hostile `.claude-env-<name>/`
  directory names; the error message already promised "letters, digits".
- Blueprint names that are **Windows reserved device names** (`CON`, `PRN`,
  `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, any case) are now rejected at
  creation. The `github` cap creates a bare `claude-internal/<name>/` component,
  which Windows refuses for these names — previously the error surfaced only
  late, as an opaque OS failure during placement.
- Adding (or renaming to) a blueprint whose name differs from an existing one
  **only in case** (e.g. `Coder` vs `coder`) is now rejected. Both map to the
  same `.claude-env-<name>/` dir on Windows/macOS default filesystems, so
  running one after the other silently clobbered the other's state.
- Project paths containing an underscore or space are now encoded correctly for
  session/memory lookup. Claude Code folds **every** non-alphanumeric character
  to `-` (so `…\human_behavior` → `…-human-behavior`), but aello only folded
  `\ / : .` — leaving `_`/spaces intact pointed seeded memory and `--resume` at
  a directory Claude never reads, a silent no-op for those projects.
- `aello init` now aborts on end-of-input instead of silently accepting every
  default, so a non-interactive or closed stdin can no longer auto-create a
  blueprint you never confirmed. `--model` also rejects a bare `claude-`.
- Login-token capture is more robust to changes in `claude setup-token` output:
  it scans from the end (the token is printed after the auth URL) and trims
  surrounding quotes/punctuation so a trailing character can't truncate it.
- The in-app docs reader (`?` in the TUI) can now scroll to the bottom of long
  docs — the scroll limit accounts for line wrapping instead of stopping at the
  unwrapped line count, which cut off every doc whose lines wrap.
- The tracked `claude-internal/<blueprint>/` mirror is now a true one-way sync:
  files deleted or renamed in the env (for example the `sync` skill after the
  `github` cap is dropped) are pruned from the mirror instead of lingering in
  git forever, and symlinks in the env are skipped rather than followed.
- `aello update` now rejects an implausibly small download (a truncated transfer
  or an HTML error page) instead of replacing the binary with it and bricking the
  install.
- `config.toml` is now written atomically (temp file + rename), so an
  interrupted save can no longer truncate it and lose the stored login token.
- Session resume (`--resume`) and seeded starter memory now work when the
  project path contains a `.`. aello's project-directory encoding now maps `.`
  to `-` exactly like Claude Code, so it no longer points resume and the starter
  memory at a directory Claude never reads.

### Added
- **`install.sh`** — a `curl -fsSL … | sh` one-line installer for Linux x86_64.
  Detects OS/arch, downloads the matching asset from the rolling `latest` release
  into `~/.local/bin` (override with `AELLO_BIN_DIR`), guards against a truncated
  download, and prints a PATH hint; unsupported platforms exit with a
  build-from-source pointer rather than a partial install.
- **`aello edit <name> --rename <new>`** — renames a blueprint. The new name is
  validated and rejected if it's already taken; if the blueprint is placed in the
  current project, its `.claude-env-<name>/` env dir and `claude-internal/<name>/`
  mirror are moved to the new name and the placed instance still launches.
- **`aello completions <shell>`** — prints a shell completion script (bash, zsh,
  fish, powershell, elvish) to stdout, generated from the CLI definition so it
  stays in sync. See the README for how to load it per shell.
- **`/note` skill** — seeded for *every* blueprint (like `/handoff` and
  `/twosentences`). `/note <env-name>` leaves a note for **another** environment
  sharing the repo: it writes what you were doing, the problem, and what that env
  needs to fix to `<env-name>.NOTE.md` at the repo root (the target env's inbox),
  which the target reads and then deletes. A fresh note overwrites the last, and
  each is attributed to the authoring blueprint. Unlike `/handoff` (a note to
  yourself), this is a message across environments — for when two blueprints
  split one project and one hits something on the other's side.
- **Open-source project foundation.** aello is now dual licensed under MIT and
  Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `Cargo.toml` `license` field), with
  full crate metadata (`repository`, `homepage`, `keywords`, `categories`) for
  crates.io. Added `CONTRIBUTING.md` (dev loop + conventions, pointing to
  `CLAUDE.md` for architecture), GitHub issue forms (bug report / feature
  request) and a pull-request template, and Contributing/License sections in the
  README.

### Fixed
- `aello github-setup` now always lands its bootstrap "Initial commit" even on a
  machine with no global git `user.name`/`user.email`. Previously `git commit`
  aborted with *"Author identity unknown"* on a fresh repo. The bootstrap commit
  now falls back to a synthetic `aello <aello@aello.local>` identity (injected
  per-invocation via `git -c`, mirroring aello's per-env attribution) only when
  no identity is configured — an existing git identity is used unchanged and
  nothing is written to the user's git config.

### Added
- **SessionEnd hook — contextdb now captures `/clear` and plain-exit sessions.**
  Previously the only transcript hook was PostCompact, which fires only on
  compaction; a session ended with `/clear` (or a plain exit) never compacts, so
  its context never reached contextdb — a `/clear`-heavy workflow recorded
  nothing. aello now also seeds a **SessionEnd** hook that, on the main session
  ending, archives the `/handoff` note (`HANDOFF.md`, otherwise deleted on next
  boot) plus a pointer to the full transcript, to
  `<contextdb>/<project>/<blueprint>/<ts>_<session>_end.jsonl`. It skips subagent
  session-ends so the tree isn't flooded. Existing envs self-heal the hook into
  their `settings.json` on the next `aello run` (a user-edited settings file is
  preserved; the hook is only inserted when absent).
- **TUI registry now filters to blueprints placed in the current directory.**
  Launch `aello` in a project and the list shows only the blueprints whose env
  is already placed there (`.claude-env-<name>/` exists) — so a per-project
  blueprint workflow stays uncluttered. Press `F` to toggle showing all
  blueprints; if none are placed here yet, the full list shows as before. The
  registry title and footer count reflect the active filter (`PLACED HERE · N OF M`).
- `/twosentences` — a new **universal** skill (seeded for every blueprint, like
  `/handoff`, regardless of capabilities). Invoke it manually to condense your
  previous response into exactly two sentences. Lands in every env on the next
  `aello run`.
- **In-app docs reader.** Press `?` in the TUI for a full-screen reference
  reader, or run `aello docs` (lists the docs) / `aello docs <name>` (prints one)
  from the CLI. The reader renders the repo's `docs/` (lightly styled markdown:
  headings, bullets, code, inline `code`/**bold**/links) with `↑/↓` to scroll and
  `Tab`/`←→` to switch docs. The docs are embedded into the binary at compile
  time, so `docs/` is the single source of truth — adding a `.md` there makes it
  appear in the reader with no code change. Ships a new user-facing
  `docs/migrate.md` (migrating an existing repo onto aello: the validated flow +
  the gotchas, chiefly that `/sync` won't bootstrap the CI scaffolding for you).
- `/handoff` — a new **universal** skill (seeded for every blueprint regardless
  of capabilities, unlike `/sync`). At session end it writes a self-contained
  `HANDOFF.md` resume note at the project root so the next session continues
  seamlessly after a full `/clear` (which, unlike a compact, leaves no summary).
  The note captures read-first pointers (env persona + memory), what shipped
  this session with commit shas, open threads / next steps, and gotchas —
  assuming the next session boots with zero prior context. Transient and
  untracked: read on boot, then deleted. Manual-only (`disable-model-invocation`).
- `/sync` now version-controls each env's **internal config**, not just project
  docs. A tracked `claude-internal/<blueprint>/` folder at the repo root is a
  one-way mirror of the gitignored `.claude-env-<name>/` dir:
  `claude-internal/<name>/skills/`, `claude-internal/<name>/memory/`, and
  `claude-internal/<name>/persona.CLAUDE.md` (a snapshot of the env persona,
  renamed so Claude Code never auto-loads it). The mirror is **namespaced per
  blueprint** so multiple blueprints sharing one repo don't clobber each other's
  config. The live env dir stays the single source of truth. Placement seeds the
  folder (tracked — not gitignored), and the github `/sync` step self-heals it
  (`mkdir -p`) so already-placed envs adopt it, mirrors the env into it, and
  stages it by explicit path before committing. Re-place a blueprint
  (`aello run`) to pick it up.

### Documentation
- Documented the **`VERSION` single-source-of-truth** convention for `github`-cap
  projects: derive any other version stamp (badge, `package.json` field,
  `version.ts`, …) from `VERSION` at build time and **gitignore the derived
  artifact** — never stamp a version into a second tracked file. Because the
  github cap auto-bumps `VERSION` on every push and `/sync` correctly stages only
  what the agent touched (never `git add -A`), a duplicated stamp drifts on every
  CI bump and strands dirty forever. The fix is structural (derive + gitignore),
  not a `/sync` carve-out. Added to `docs/capabilities.md` and `docs/concepts.md`
  with the `env-console` precedent. Docs only — no code change.

### Changed
- `/sync` reconcile order: memory is now refreshed **first**, before the other
  docs, so the checkpoint (and the new `claude-internal/` mirror) captures what
  the env learned this session.
- `/sync` skill (github blueprints): the commit step now runs
  `git pull --rebase origin <branch>` **after committing, before pushing**, so
  it integrates the release CI's auto-bump commit and the push fast-forwards.
  Previously each `/sync` left local one commit behind, and the next push was
  rejected until a manual rebase. Re-place a blueprint (`aello run`) to pick up
  the new skill text.

## [0.1.34]

### Changed
- Reworked the bundled starter working-style memory: it now captures that the
  user doesn't read plans — surface concrete decisions to choose from ("which
  of these?") and ask short, ask often — replacing the old go-slow / verify
  wording. Affects newly placed envs (existing memories are never clobbered).

## [0.1.33]

### Added
- Fresh placements now seed a starter memory so a new env boots with the
  user's working-style note already loaded in `/context`: a bundled
  `working-style.md` memory plus a one-line `MEMORY.md` index pointing at it,
  under `projects/<encoded-cwd>/memory/`. Seeded only when there is no
  `MEMORY.md` yet — a re-place over an established memory leaves it untouched.

## [0.1.32]

### Changed
- `/sync` skill: the commit step now stages **only the files the blueprint
  created or modified this session** (by explicit path) instead of `git add -A`.
  A blanket stage swept unrelated untracked files — other tooling's scaffolding
  or another env's in-flight work — into a blueprint's commit. Re-place a
  blueprint (`aello run`) to regenerate its `SKILL.md` from the new template.

## [0.1.30]

### Added
- `aello edit <name>` — change an existing blueprint's model, persona, or
  capabilities in place. Capability flags are tri-state: `--github` enables,
  `--no-github` disables, omitting both leaves it unchanged. Changes apply on
  the next `aello run`; the global persona in an already-placed env is never
  re-clobbered.
- TUI: `E` edits the selected blueprint through the same guided steps as add,
  pre-filled with its current model, persona, and capabilities (name fixed).

## [0.1.26]

### Added
- The `github` capability now also scaffolds `.gitattributes` (`* text=auto`,
  CRLF normalization), a generic `VERSION` file, and a stack-agnostic
  `.github/workflows/version.yml` that patch-bumps `VERSION` on every push to
  `main` and commits it back with `[skip ci]`. All seeded only if absent.
- `aello github-setup` — drives GitHub repo creation for the current project:
  prechecks `gh` auth, makes an initial commit if needed, then `gh repo create`
  (private by default; `--public`), sets `origin`, and pushes. `--name`, `--yes`.
- `aello init` — first-run wizard: logs in if there's no shared token, then walks
  you through creating your first blueprint (name, model, persona, capabilities).

## [0.1.23]

### Added
- `README.md` (install, concepts, command + capability reference) and `docs/`
  (`concepts.md`, `capabilities.md`).

## [0.1.20]

### Added
- Built-in CLAUDE.md persona templates `coder` and `sysadmin`. `--claude-md coder`
  resolves to a bundled template; any other value is still treated as a file path.
- Per-blueprint capabilities (`--project-md`, `--github`, `--changelog`, `--docs`,
  `--readme`), selectable on `aello add` and via a checklist in the TUI add flow.
  On `run`, each enabled capability scaffolds its file (CHANGELOG/README/docs/,
  project CLAUDE.md) if missing and adds its section to a generated `/sync` skill.
- `/sync` is now generated per blueprint from its capabilities — a no-GitHub
  blueprint gets no git/commit/push sections. `list` shows a `SYNC` column.
- Per-env git attribution: `run` sets `GIT_AUTHOR_*`/`GIT_COMMITTER_*` to the
  blueprint identity (`<name> <name@aello.local>`), and the GitHub `/sync` section
  appends an `Env: <name>` commit trailer — so `git blame`/`git log` reveal which
  blueprint made each change.
- With the `github` capability, `run` seeds/appends a `.claude-env-*` line to the
  project's `.gitignore` (idempotent), so env dirs and their credentials are never
  committed.

## [0.1.19]

### Fixed
- `aello login` now streams `claude setup-token` output live, so the auth URL is
  visible on headless machines (e.g. a VPS with no browser). Previously stdout was
  piped to capture the token, which swallowed the URL and made login appear to hang.
