//! Per-env token accounting, read back out of Claude Code transcripts.
//!
//! Every assistant message in a transcript carries a `usage` object with the
//! four counts that matter — `input_tokens`, `output_tokens`,
//! `cache_creation_input_tokens`, `cache_read_input_tokens` — plus the model
//! that produced it. contextdb is laid out `<root>/<project>/<blueprint>/`, so
//! grouping by env is free and needs no new hook: this reads what SessionEnd
//! already archived.
//!
//! **Dedup by `message.id` is not a nicety, it is the whole correctness story.**
//! Claude Code writes one transcript record per *content block*, and every
//! record for a message repeats that message's full `usage`. Measured here on a
//! real transcript: 266 usage-bearing records for 173 distinct messages, so a
//! naive sum overstated output by 68% (218,607 vs 129,877). The same dedup also
//! absorbs a session archived twice (17 of the 122 archives on this machine) and
//! the overlap between an archive and the still-live transcript it was copied
//! from — which is what makes scanning both sources safe.
//!
//! Costs are **list API rates**, applied to a subscription's usage: the figure
//! answers "what would this have cost on the API", not "what were you billed".
//! A model with no entry in `RATES` is never silently priced at zero — its
//! tokens are counted into `unpriced` and the model id is named in the output,
//! because a cost that quietly excludes half the traffic is worse than no cost
//! at all.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Rate-limit window length. Claude's usage window is 5 hours from the first
/// message of a block.
pub const WINDOW_SECS: i64 = 5 * 3600;

/// List price per million tokens, `(input, output)`.
///
/// Matched by longest prefix, so a dated id (`claude-haiku-4-5-20251001`) hits
/// its family entry. Cache multipliers are uniform across models and applied to
/// the input rate: 1.25x for a 5-minute write, 2x for a 1-hour write, 0.1x for
/// a read.
///
/// Sonnet 5 is listed at its standard $3/$15 rather than the $2/$10
/// introductory rate that runs to 2026-08-31 — a price that expires on a date
/// would make this table quietly wrong the morning after, and a binary already
/// installed elsewhere would keep reporting the old number.
const RATES: &[(&str, f64, f64)] = &[
    ("claude-fable-5", 10.0, 50.0),
    ("claude-mythos-5", 10.0, 50.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-opus-4", 5.0, 25.0),
    ("claude-opus-3", 15.0, 75.0),
    ("claude-3-opus", 15.0, 75.0),
    ("claude-sonnet-5", 3.0, 15.0),
    ("claude-sonnet-4", 3.0, 15.0),
    ("claude-3-7-sonnet", 3.0, 15.0),
    ("claude-3-5-sonnet", 3.0, 15.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.8, 4.0),
    ("claude-3-haiku", 0.25, 1.25),
];

const CACHE_WRITE_5M: f64 = 1.25;
const CACHE_WRITE_1H: f64 = 2.0;
const CACHE_READ: f64 = 0.1;

fn rates(model: &str) -> Option<(f64, f64)> {
    RATES
        .iter()
        .filter(|(prefix, _, _)| model.starts_with(prefix))
        .max_by_key(|(prefix, _, _)| prefix.len())
        .map(|&(_, i, o)| (i, o))
}

/// The four counts, kept apart because they price differently by a factor of 20.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
}

impl Usage {
    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
    }

    /// Every token that passed through, cached or not. This is the figure the
    /// 5-hour window is measured in.
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write_5m + self.cache_write_1h + self.cache_read
    }

    pub fn cache_write(&self) -> u64 {
        self.cache_write_5m + self.cache_write_1h
    }

    /// Dollar cost at list rates, or `None` when the model has no entry — the
    /// caller reports that rather than folding it in as zero.
    pub fn cost(&self, model: &str) -> Option<f64> {
        let (i, o) = rates(model)?;
        Some(
            self.input as f64 / 1e6 * i
                + self.output as f64 / 1e6 * o
                + self.cache_write_5m as f64 / 1e6 * i * CACHE_WRITE_5M
                + self.cache_write_1h as f64 / 1e6 * i * CACHE_WRITE_1H
                + self.cache_read as f64 / 1e6 * i * CACHE_READ,
        )
    }
}

/// One deduplicated assistant message.
#[derive(Clone, Debug)]
pub struct Record {
    pub ts: i64,
    pub model: String,
    pub blueprint: String,
    pub project: String,
    pub session: String,
    /// The git branch checked out when the message was sent. Absent on records
    /// older than the field, and `HEAD` on a detached checkout — both are kept
    /// as they are rather than folded into `main`.
    pub branch: String,
    /// Reasoning effort, when the record carries it. `None` is its own bucket:
    /// the field only appears on newer records, and folding an absent value
    /// into the commonest one would invent a number.
    pub effort: Option<String>,
    pub usage: Usage,
}

/// What the sessions were *doing*, as opposed to what they spent.
///
/// Collected in the same pass as usage, because the transcripts are already
/// being read and a second walk over a gigabyte to count tool calls would be
/// pure waste. Every counter is deduplicated on the id its own record carries
/// (`uuid` for an event, the `toolu_…` id for a tool call) rather than on
/// `message.id`: an archived session can be read twice, a live env overlaps its
/// own archive, and one message is written as several records — one per content
/// block — so a message-level key would drop most of the tool calls.
#[derive(Default, Clone, Debug)]
pub struct Activity {
    /// One entry per completed turn, in milliseconds. Claude Code times its own
    /// turns and writes a `turn_duration` system record, so this is measured
    /// wall clock rather than a difference between timestamps that happens to
    /// include however long the user was away.
    pub turn_ms: Vec<u64>,
    /// When turns happen: hour of day (UTC) and weekday (0 = Monday).
    pub turn_hour: [u64; 24],
    pub turn_weekday: [u64; 7],
    /// Tool calls by name, deduplicated by the tool-use id.
    pub tools: BTreeMap<String, u64>,
    /// Which skill the harness attributed an assistant message to — the only
    /// evidence that a seeded skill was actually *run* rather than merely
    /// seeded. Counted two ways because they answer different questions: the
    /// messages are how much work the skill did, the sessions are how often it
    /// was reached for.
    pub skills: BTreeMap<String, u64>,
    pub skill_sessions: BTreeMap<String, BTreeSet<String>>,
    /// Basename → edits. The path is dropped deliberately: the same file is
    /// edited from two checkouts of one repo and they are the same file.
    pub files: BTreeMap<String, u64>,
    /// First word of a shell command, lowercased.
    pub shell: BTreeMap<String, u64>,
    /// What the *harness* pushed into the conversation, by kind: task
    /// reminders, hook output, skill listings, deferred-tool deltas. This is
    /// context nobody typed, and it is the only way to see what it costs.
    pub attachments: BTreeMap<String, Injected>,
    /// Every injection, in the order read. Priced at the end of the scan, once
    /// the records that carry them forward are all known.
    pub injections: Vec<InjectionEvent>,
    /// `[Request interrupted by user]` markers — the times a turn was stopped
    /// mid-flight, which is the closest thing to a "wrong direction" signal.
    pub interrupts: u64,
    /// Prompts queued while a turn was running, and queued prompts withdrawn
    /// before they ran.
    pub queued: u64,
    pub unqueued: u64,
}

/// One kind of harness-injected context: how often, and how big.
///
/// `chars` is the serialized payload, and the token figure derived from it is an
/// **estimate** — a quarter of a character count, the usual rule of thumb. The
/// transcript records what was injected but never how many tokens it became, so
/// this is the honest ceiling on what can be said: it is not read off a `usage`
/// field and must not be printed as though it were.
#[derive(Default, Clone, Copy, Debug)]
pub struct Injected {
    pub count: u64,
    pub chars: u64,
    /// What it cost to put these tokens into the context once, at the rate of
    /// the request that paid for it.
    pub write_cost: f64,
    /// What it cost to carry them through every later request in the same
    /// session. This is the number that matters: an injection is written once
    /// and re-read for the rest of the session.
    pub read_cost: f64,
}

impl Injected {
    /// Rough tokens: characters ÷ 4. An estimate, labelled as one everywhere it
    /// is shown.
    pub fn est_tokens(&self) -> u64 {
        self.chars / 4
    }

    pub fn cost(&self) -> f64 {
        self.write_cost + self.read_cost
    }

    /// How many times over the write price this actually cost. 1.0 would mean
    /// the injection was never re-read; anything above it is the session
    /// carrying the tokens forward.
    pub fn multiplier(&self) -> f64 {
        if self.write_cost <= 0.0 {
            return 0.0;
        }
        self.cost() / self.write_cost
    }
}

/// A readable name for an injection kind. The stored key keeps Claude Code's
/// own spelling so it can be matched back to a record; only the display is
/// shortened, and only for the two long hook prefixes.
pub fn injection_label(kind: &str) -> String {
    match kind.split_once('/') {
        Some(("hook_additional_context", ev)) => format!("hook: {ev}"),
        Some(("hook_success", ev)) => format!("hook: {ev} (2nd copy)"),
        _ => kind.to_string(),
    }
}

/// One injection, kept until the scan can price it — that needs every record in
/// the session, which is only known once all the files have been read.
#[derive(Clone, Debug)]
pub struct InjectionEvent {
    pub session: String,
    pub ts: i64,
    pub kind: String,
    pub chars: u64,
}

impl Activity {
    /// Harness-injected context, costliest first. Ranked by money rather than
    /// by token count: a payload injected early in a long session is re-read
    /// hundreds of times, and one injected at the end is not, so the two
    /// orderings disagree.
    pub fn injected_ranking(&self) -> Vec<(String, Injected)> {
        let mut v: Vec<(String, Injected)> =
            self.attachments.iter().map(|(k, i)| (k.clone(), *i)).collect();
        v.sort_by(|a, b| {
            b.1.cost().total_cmp(&a.1.cost()).then_with(|| b.1.chars.cmp(&a.1.chars))
        });
        v
    }

    pub fn injected_total(&self) -> Injected {
        let mut t = Injected::default();
        for i in self.attachments.values() {
            t.count += i.count;
            t.chars += i.chars;
            t.write_cost += i.write_cost;
            t.read_cost += i.read_cost;
        }
        t
    }

    pub fn turns(&self) -> u64 {
        self.turn_ms.len() as u64
    }

    /// Median turn, in seconds. The mean is far higher and far less useful — a
    /// handful of 90-minute turns drag it away from anything typical.
    pub fn median_turn_secs(&self) -> f64 {
        percentile_secs(&self.turn_ms, 0.5)
    }

    pub fn p90_turn_secs(&self) -> f64 {
        percentile_secs(&self.turn_ms, 0.9)
    }

    pub fn longest_turn_secs(&self) -> f64 {
        self.turn_ms.iter().max().copied().unwrap_or(0) as f64 / 1000.0
    }

    /// Hours actually spent inside turns. **Not** elapsed session time: the
    /// gaps between turns are the user reading, typing and being elsewhere, and
    /// counting them would turn a lunch break into work.
    pub fn turn_hours(&self) -> f64 {
        self.turn_ms.iter().sum::<u64>() as f64 / 3_600_000.0
    }

    pub fn tool_calls(&self) -> u64 {
        self.tools.values().sum()
    }

    /// Tool calls per turn — how much of a turn is the agent working versus
    /// answering.
    pub fn tools_per_turn(&self) -> f64 {
        match self.turns() {
            0 => 0.0,
            n => self.tool_calls() as f64 / n as f64,
        }
    }

    pub fn busiest_weekday(&self) -> Option<usize> {
        (0..7).max_by_key(|&d| self.turn_weekday[d]).filter(|&d| self.turn_weekday[d] > 0)
    }

    /// Skills ranked by how many sessions reached for them: name, sessions,
    /// assistant messages.
    pub fn skill_ranking(&self) -> Vec<(String, u64, u64)> {
        let mut v: Vec<(String, u64, u64)> = self
            .skills
            .iter()
            .map(|(k, msgs)| {
                let s = self.skill_sessions.get(k).map(|s| s.len() as u64).unwrap_or(0);
                (k.clone(), s, *msgs)
            })
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.cmp(&a.2)).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// Top `n` entries of a counter, biggest first, ties broken by name so the
    /// output is stable between runs.
    pub fn top(map: &BTreeMap<String, u64>, n: usize) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = map.iter().map(|(k, c)| (k.clone(), *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.truncate(n);
        v
    }
}

fn percentile_secs(ms: &[u64], q: f64) -> f64 {
    if ms.is_empty() {
        return 0.0;
    }
    let mut v = ms.to_vec();
    v.sort_unstable();
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i] as f64 / 1000.0
}

/// A priced roll-up: totals plus whatever couldn't be priced.
#[derive(Default, Clone, Debug)]
pub struct Priced {
    pub usage: Usage,
    pub cost: f64,
    /// Tokens on models with no rate entry. Excluded from `cost`, never hidden.
    pub unpriced_tokens: u64,
    /// Per-model split, so a mixed env shows where the money went.
    pub by_model: BTreeMap<String, Usage>,
}

impl Priced {
    pub fn push(&mut self, model: &str, u: &Usage) {
        self.usage.add(u);
        match u.cost(model) {
            Some(c) => self.cost += c,
            None => self.unpriced_tokens += u.total(),
        }
        self.by_model.entry(model.to_string()).or_default().add(u);
    }
}

#[derive(Clone, Debug)]
pub struct SessionRoll {
    pub id: String,
    pub project: String,
    pub first: i64,
    pub last: i64,
    pub messages: u64,
    pub priced: Priced,
}

#[derive(Clone, Debug)]
pub struct EnvRoll {
    pub blueprint: String,
    /// Projects this blueprint has been used in (contextdb groups by project).
    pub projects: BTreeSet<String>,
    pub priced: Priced,
    pub sessions: Vec<SessionRoll>,
    pub first: i64,
    pub last: i64,
}

/// One 5-hour rate-limit block.
#[derive(Clone, Debug)]
pub struct Block {
    pub start: i64,
    pub last: i64,
    pub priced: Priced,
    /// Per-blueprint split of this block — the quota is shared across envs, so
    /// the interesting question is which env is eating it.
    pub by_env: BTreeMap<String, Usage>,
}

impl Block {
    pub fn end(&self) -> i64 {
        self.start + WINDOW_SECS
    }
}

/// Everything one scan produced.
pub struct Report {
    /// Every deduplicated message, oldest first. Kept rather than dropped
    /// because a caller may need a window the 5-hour blocks can't express —
    /// the statusline sums "since the plan's own window opened", and a block
    /// straddling that instant would be counted whole.
    pub records: Vec<Record>,
    /// What the sessions were doing, from the same pass over the same files.
    pub activity: Activity,
    pub envs: Vec<EnvRoll>,
    /// Every 5-hour block across all envs, oldest first. The quota is
    /// machine-wide (one shared subscription token), so blocks are global.
    pub blocks: Vec<Block>,
    /// Model ids with no rate entry, so an unpriced total can be explained.
    pub unknown_models: BTreeSet<String>,
    pub files_scanned: usize,
    /// Archived sessions that hold **only a pointer** to a transcript that no
    /// longer exists, so they contribute nothing to any total here. Reported
    /// rather than swallowed: 269 of 415 archives on the machine this was
    /// written on, which is months of history reading as zero.
    pub pointer_only: usize,
    pub messages: usize,
    /// Records seen before dedup — the gap against `messages` is the
    /// content-block duplication this module exists to strip.
    pub raw_records: usize,
}

impl Report {
    pub fn total(&self) -> Priced {
        let mut p = Priced::default();
        for e in &self.envs {
            p.usage.add(&e.priced.usage);
            p.cost += e.priced.cost;
            p.unpriced_tokens += e.priced.unpriced_tokens;
            for (m, u) in &e.priced.by_model {
                p.by_model.entry(m.clone()).or_default().add(u);
            }
        }
        p
    }

    /// The block containing `now`, if the most recent block is still open.
    pub fn current_block(&self, now: i64) -> Option<&Block> {
        self.blocks.last().filter(|b| now < b.end())
    }

    /// Largest block ever recorded, in total tokens. Used as the denominator
    /// for "% of window" — **the actual subscription quota is not present in
    /// any transcript**, so the only honest ceiling is the biggest 5-hour block
    /// this machine has produced before.
    pub fn peak_block_tokens(&self) -> u64 {
        self.blocks.iter().map(|b| b.priced.usage.total()).max().unwrap_or(0)
    }
}

// ── Scanning ────────────────────────────────────────────────────────────────

/// A directory of transcripts and the blueprint/project they belong to.
pub struct Source {
    pub dir: PathBuf,
    pub blueprint: String,
    pub project: String,
}

/// Archived transcripts under `<contextdb>/<project>/<blueprint>/`.
///
/// Returns an empty list when the root is missing rather than erroring: a fresh
/// machine has no contextdb yet, and that is "nothing recorded", not a fault.
pub fn contextdb_sources(root: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else { return out };
    for p in projects.flatten() {
        if !p.path().is_dir() {
            continue;
        }
        let project = p.file_name().to_string_lossy().into_owned();
        let Ok(bps) = std::fs::read_dir(p.path()) else { continue };
        for b in bps.flatten() {
            if !b.path().is_dir() {
                continue;
            }
            out.push(Source {
                blueprint: b.file_name().to_string_lossy().into_owned(),
                dir: b.path(),
                project: project.clone(),
            });
        }
    }
    out
}

/// Live (not-yet-archived) transcripts inside a placed env: every
/// `<env>/projects/*/` directory.
///
/// All of them, not just the encoded cwd — a case-variant launch directory
/// produces a second encoded folder, and the sessions inside it are real spend.
pub fn live_sources(env_dir: &Path, blueprint: &str, project: &str) -> Vec<Source> {
    let root = env_dir.join("projects");
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| Source {
            dir: e.path(),
            blueprint: blueprint.to_string(),
            project: project.to_string(),
        })
        .collect()
}

/// Every transcript directory worth reading: all of contextdb, plus the live
/// transcripts of blueprints placed in `cwd`.
///
/// contextdb is global and durable, but only holds *ended* sessions — the one
/// you are sitting in right now is not there yet. The live env dirs close that
/// gap for the current project, which is the only project whose env dirs we can
/// locate (contextdb records a project's folder name, not its path). Sessions
/// running elsewhere on this machine are therefore missing until they end;
/// that limit is real and the CLI says so.
///
/// Cline blueprints are skipped — a Cline env produces no Claude Code
/// transcripts, so it would report a silent zero rather than "not applicable".
pub fn collect_sources(cwd: &Path) -> Vec<Source> {
    let cfg = crate::config::load().unwrap_or_default();
    let mut sources = contextdb_sources(&crate::config::contextdb_dir(&cfg));

    let project = cwd
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".into());
    for b in &cfg.blueprints {
        if b.agent != crate::models::Agent::Claude {
            continue;
        }
        let env = b.agent.env_dir(cwd, &b.name);
        if env.exists() {
            sources.extend(live_sources(&env, &b.name, &project));
        }
    }
    sources
}

/// Read every transcript under `sources`, deduplicate, and roll up.
pub fn scan(sources: &[Source]) -> Report {
    let mut seen: HashSet<String> = HashSet::new();
    let mut seen_events: HashSet<String> = HashSet::new();
    let mut activity = Activity::default();
    let mut records: Vec<Record> = Vec::new();
    let mut files = 0usize;
    let mut raw = 0usize;
    let mut pointer_only = 0usize;

    for src in sources {
        let Ok(rd) = std::fs::read_dir(&src.dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            // The SessionEnd sidecar (`*_end.jsonl`) holds the handoff note and
            // a transcript pointer, never usage. Skipping it by name saves
            // reading 391 files here; anything else with a .jsonl extension is
            // still read, so a rename can't silently drop real data.
            if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("_end.jsonl")) {
                // …but count the ones with no transcript beside them. Those
                // sessions ended, were archived, and contribute **zero
                // tokens**, because SessionEnd only recorded a path to a
                // transcript Claude Code has since deleted. A total computed
                // over them is not wrong by a rounding error; it is missing
                // whole months, and nothing about the output would say so.
                let copied = PathBuf::from(
                    path.to_string_lossy().replace("_end.jsonl", "_transcript.jsonl"),
                );
                if !copied.exists() {
                    pointer_only += 1;
                }
                continue;
            }
            files += 1;
            raw += read_transcript(
                &path,
                src,
                &mut seen,
                &mut records,
                &mut activity,
                &mut seen_events,
            );
        }
    }

    records.sort_by_key(|r| r.ts);
    price_injections(&mut activity, &records);
    let messages = records.len();
    let blocks = build_blocks(&records);
    let (envs, unknown_models) = roll_envs(&records);

    Report {
        records,
        activity,
        envs,
        blocks,
        unknown_models,
        files_scanned: files,
        pointer_only,
        messages,
        raw_records: raw,
    }
}

/// Parse one transcript. Returns the number of usage-bearing records seen
/// (before dedup), so the caller can report how much duplication was stripped.
fn read_transcript(
    path: &Path,
    src: &Source,
    seen: &mut HashSet<String>,
    out: &mut Vec<Record>,
    act: &mut Activity,
    seen_events: &mut HashSet<String>,
) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else { return 0 };
    let mut raw = 0usize;
    for line in text.lines() {
        // Cheap gates: most lines are tool results with nothing we want, and
        // JSON-parsing a gigabyte of them would dominate the run. Each gate is
        // a substring no other record shape carries, and the line is parsed
        // once for whichever of them hit.
        let has_usage = line.contains("\"usage\"");
        // `"tool_use"` and not `tool_use_id` — the closing quote is what keeps
        // a tool *result* out of the tool-call count.
        let has_tool = line.contains("\"tool_use\"");
        let has_event = line.contains("turn_duration")
            || line.contains("attributionSkill")
            || line.contains("[Request interrupted by user")
            || line.contains("queue-operation")
            || line.contains("\"attachment\"");
        if !(has_usage || has_tool || has_event) {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };

        if has_tool || has_event {
            read_activity(&v, act, seen_events);
        }
        if !has_usage {
            continue;
        }

        let msg = &v["message"];
        let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else { continue };
        raw += 1;

        let Some(id) = msg["id"].as_str() else { continue };
        if !seen.insert(id.to_string()) {
            continue; // Same message, another content block — or a second archive.
        }

        let ts = v["timestamp"].as_str().and_then(parse_iso8601).unwrap_or(0);
        let model = msg["model"].as_str().unwrap_or("unknown").to_string();
        let session = v["sessionId"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());

        out.push(Record {
            ts,
            model,
            blueprint: src.blueprint.clone(),
            project: src.project.clone(),
            session,
            branch: v["gitBranch"].as_str().unwrap_or_default().to_string(),
            effort: v["effort"].as_str().map(str::to_string),
            usage: parse_usage(usage),
        });
    }
    raw
}

/// Price every injection against the requests that actually carried it.
///
/// **This is the whole point of the section.** An injection is not a one-off
/// charge: it is written into the context once and then re-read by every later
/// request in the same session, so its real price is
/// `tokens × (2 × input)` once, plus `tokens × 0.1 × input` per request after
/// it. Measured on this machine the median injection is followed by **75**
/// further requests, which turns a 400-token per-turn hook into something like
/// six times its apparent cost — and reading the token count alone gets that
/// wrong in the alarming direction, since 3.8M tokens sounds like a fortune and
/// is not.
///
/// Rates come from the model of each carrying request rather than a constant,
/// so a session that switched models is priced as it happened.
fn price_injections(act: &mut Activity, records: &[Record]) {
    if act.injections.is_empty() {
        return;
    }
    // Per session: the requests, in order, and the suffix sum of their input
    // rates — so pricing one injection is a binary search, not a scan.
    let mut by_session: HashMap<&str, Vec<(i64, f64)>> = HashMap::new();
    for r in records {
        let rate = rates(&r.model).map(|(i, _)| i).unwrap_or(0.0);
        by_session.entry(r.session.as_str()).or_default().push((r.ts, rate));
    }
    let mut suffix: HashMap<&str, Vec<f64>> = HashMap::new();
    for (s, reqs) in &by_session {
        let mut sums = vec![0.0; reqs.len() + 1];
        for i in (0..reqs.len()).rev() {
            sums[i] = sums[i + 1] + reqs[i].1;
        }
        suffix.insert(s, sums);
    }

    for ev in &act.injections {
        let Some(reqs) = by_session.get(ev.session.as_str()) else { continue };
        let idx = reqs.partition_point(|(ts, _)| *ts <= ev.ts);
        if idx >= reqs.len() {
            continue; // Nothing followed it, so nothing ever paid for it.
        }
        let tokens = (ev.chars / 4) as f64 / 1e6;
        let sums = &suffix[ev.session.as_str()];
        let e = act.attachments.entry(ev.kind.clone()).or_default();
        // The first request after the injection writes it into the cache; every
        // request after that re-reads it.
        e.write_cost += tokens * reqs[idx].1 * CACHE_WRITE_1H;
        e.read_cost += tokens * (sums[idx] - reqs[idx].1) * CACHE_READ;
    }
}

/// Pull the non-usage signal out of one already-parsed record.
///
/// Every branch deduplicates on an id the record carries, because the same
/// session is read more than once — a live env overlaps its own archive, and 17
/// of 122 archives on the machine this was written on are one session archived
/// twice.
fn read_activity(
    v: &serde_json::Value,
    act: &mut Activity,
    seen: &mut HashSet<String>,
) -> Option<()> {
    let uuid = v["uuid"].as_str();
    let mut fresh = |key: &str| -> bool { seen.insert(key.to_string()) };

    match v["type"].as_str() {
        // Claude Code times its own turns. Prefer that to a difference between
        // timestamps, which would silently include however long the user spent
        // reading the last answer.
        Some("system") if v["subtype"].as_str() == Some("turn_duration") => {
            let ms = v["durationMs"].as_u64()?;
            if uuid.is_some_and(&mut fresh) {
                act.turn_ms.push(ms);
                if let Some(ts) = v["timestamp"].as_str().and_then(parse_iso8601) {
                    act.turn_hour[(ts.rem_euclid(86400) / 3600).clamp(0, 23) as usize] += 1;
                    // 1970-01-01 was a Thursday, which is index 3 counting from
                    // Monday.
                    act.turn_weekday[(ts.div_euclid(86400) + 3).rem_euclid(7) as usize] += 1;
                }
            }
            return Some(());
        }
        // A queue record carries **no uuid** — keying on one silently counted
        // zero of 665, which looked exactly like a user who never queues.
        // Session + timestamp + operation is unique enough to dedup a
        // twice-archived session, which is all the key is for.
        Some("queue-operation") => {
            let op = v["operation"].as_str()?;
            let key = format!(
                "q:{}:{}:{op}",
                v["sessionId"].as_str().unwrap_or(""),
                v["timestamp"].as_str().unwrap_or(""),
            );
            if fresh(&key) {
                match op {
                    "enqueue" => act.queued += 1,
                    "remove" => act.unqueued += 1,
                    _ => {}
                }
            }
            return Some(());
        }
        // An attachment *does* carry a uuid — checked, after the queue record
        // above turned out not to.
        Some("attachment") => {
            let a = v.get("attachment")?;
            // A hook attachment names the event that produced it, so the split
            // between "the per-turn rules" and "the session banner" needs no
            // matching on their text — which would go quietly wrong the next
            // time the wording changes, and it has changed once already.
            let kind = match (a["type"].as_str(), a["hookEvent"].as_str()) {
                (Some(t), Some(ev)) => format!("{t}/{ev}"),
                (Some(t), None) => t.to_string(),
                _ => "unknown".to_string(),
            };
            if uuid.is_some_and(&mut fresh) {
                let chars = serde_json::to_string(a).map(|s| s.len() as u64).unwrap_or(0);
                let e = act.attachments.entry(kind.clone()).or_default();
                e.count += 1;
                e.chars += chars;
                if let (Some(s), Some(ts)) =
                    (v["sessionId"].as_str(), v["timestamp"].as_str().and_then(parse_iso8601))
                {
                    act.injections.push(InjectionEvent {
                        session: s.to_string(),
                        ts,
                        kind,
                        chars,
                    });
                }
            }
            return Some(());
        }
        _ => {}
    }

    if let (Some(skill), Some(id)) = (v["attributionSkill"].as_str(), uuid) {
        if fresh(id) {
            *act.skills.entry(skill.to_string()).or_default() += 1;
            if let Some(s) = v["sessionId"].as_str() {
                act.skill_sessions
                    .entry(skill.to_string())
                    .or_default()
                    .insert(s.to_string());
            }
        }
    }

    let content = &v["message"]["content"];
    // An interrupt lands either as a bare string or as a text block, depending
    // on whether anything else was attached to the same user turn.
    if content.as_str().is_some_and(|t| t.contains(INTERRUPT_MARKER))
        && uuid.is_some_and(&mut fresh)
    {
        act.interrupts += 1;
    }
    let content = content.as_array()?;
    for b in content {
        match b["type"].as_str() {
            Some("tool_use") => {
                // The tool-use id, not the message id: one message is written
                // as several records, one per content block, so keying on the
                // message would keep the first block and drop every tool call.
                let Some(id) = b["id"].as_str() else { continue };
                if !fresh(id) {
                    continue;
                }
                let name = b["name"].as_str().unwrap_or("unknown");
                *act.tools.entry(name.to_string()).or_default() += 1;
                let input = &b["input"];
                if matches!(name, "Edit" | "Write" | "NotebookEdit") {
                    if let Some(p) = input["file_path"].as_str() {
                        let base = p.rsplit(['/', '\\']).next().unwrap_or(p);
                        *act.files.entry(base.to_string()).or_default() += 1;
                    }
                }
                if matches!(name, "Bash" | "PowerShell") {
                    if let Some(c) = input["command"].as_str() {
                        if let Some(w) = c.split_whitespace().next() {
                            *act.shell.entry(w.to_lowercase()).or_default() += 1;
                        }
                    }
                }
            }
            // An interrupt is written as a plain text block on a user record.
            Some("text") => {
                if b["text"].as_str().is_some_and(|t| t.contains(INTERRUPT_MARKER))
                    && uuid.is_some_and(&mut fresh)
                {
                    act.interrupts += 1;
                }
            }
            _ => {}
        }
    }
    Some(())
}

/// What Claude Code writes into the transcript when a turn is stopped.
const INTERRUPT_MARKER: &str = "[Request interrupted by user";

pub fn parse_usage(u: &serde_json::Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    let total_write = n("cache_creation_input_tokens");
    // `cache_creation` splits the write across TTL buckets, which price 1.25x
    // vs 2x. When it's absent (older transcripts) everything goes to the 5m
    // bucket — that's the API default TTL, so it's the right guess, and it
    // under-states rather than inflates.
    let (w5, w1) = match u.get("cache_creation").filter(|c| c.is_object()) {
        Some(c) => {
            let a = c["ephemeral_5m_input_tokens"].as_u64().unwrap_or(0);
            let b = c["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0);
            if a + b == 0 && total_write > 0 { (total_write, 0) } else { (a, b) }
        }
        None => (total_write, 0),
    };
    Usage {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_write_5m: w5,
        cache_write_1h: w1,
        cache_read: n("cache_read_input_tokens"),
    }
}

/// Group sorted records into 5-hour blocks.
///
/// A block starts at the containing hour of its first message and runs 5 hours.
/// A gap of a full window with no traffic also closes a block — otherwise an
/// overnight pause and the next morning's work land in one "block" that never
/// existed as a rate-limit window.
fn build_blocks(records: &[Record]) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for r in records {
        let fits = blocks.last().is_some_and(|b| {
            r.ts < b.start + WINDOW_SECS && r.ts - b.last < WINDOW_SECS
        });
        if !fits {
            blocks.push(Block {
                start: floor_hour(r.ts),
                last: r.ts,
                priced: Priced::default(),
                by_env: BTreeMap::new(),
            });
        }
        let b = blocks.last_mut().expect("just pushed when empty");
        b.last = r.ts;
        b.priced.push(&r.model, &r.usage);
        b.by_env.entry(r.blueprint.clone()).or_default().add(&r.usage);
    }
    blocks
}

fn floor_hour(ts: i64) -> i64 {
    ts - ts.rem_euclid(3600)
}

fn roll_envs(records: &[Record]) -> (Vec<EnvRoll>, BTreeSet<String>) {
    let mut by_env: BTreeMap<String, EnvRoll> = BTreeMap::new();
    let mut by_session: BTreeMap<(String, String), SessionRoll> = BTreeMap::new();
    let mut unknown = BTreeSet::new();

    for r in records {
        if rates(&r.model).is_none() {
            unknown.insert(r.model.clone());
        }
        let env = by_env.entry(r.blueprint.clone()).or_insert_with(|| EnvRoll {
            blueprint: r.blueprint.clone(),
            projects: BTreeSet::new(),
            priced: Priced::default(),
            sessions: Vec::new(),
            first: r.ts,
            last: r.ts,
        });
        env.projects.insert(r.project.clone());
        env.priced.push(&r.model, &r.usage);
        env.first = env.first.min(r.ts);
        env.last = env.last.max(r.ts);

        let s = by_session
            .entry((r.blueprint.clone(), r.session.clone()))
            .or_insert_with(|| SessionRoll {
                id: r.session.clone(),
                project: r.project.clone(),
                first: r.ts,
                last: r.ts,
                messages: 0,
                priced: Priced::default(),
            });
        s.messages += 1;
        s.first = s.first.min(r.ts);
        s.last = s.last.max(r.ts);
        s.priced.push(&r.model, &r.usage);
    }

    for ((bp, _), s) in by_session {
        if let Some(e) = by_env.get_mut(&bp) {
            e.sessions.push(s);
        }
    }
    let mut envs: Vec<EnvRoll> = by_env.into_values().collect();
    for e in &mut envs {
        e.sessions.sort_by(|a, b| b.last.cmp(&a.last)); // newest first
    }
    envs.sort_by(|a, b| b.priced.usage.total().cmp(&a.priced.usage.total()));
    (envs, unknown)
}

// ── Statistics ──────────────────────────────────────────────────────────────

/// One project's rollup. contextdb groups by a project's **folder name**, not
/// its path, so two checkouts of the same repo in different directories are one
/// project here — which is usually what you want and is worth knowing when it
/// isn't.
#[derive(Clone, Debug)]
pub struct ProjectRoll {
    pub project: String,
    pub sessions: u64,
    pub messages: u64,
    /// Which blueprints have worked here. A project worked by three envs is a
    /// different thing from one worked by a single env.
    pub blueprints: BTreeSet<String>,
    pub priced: Priced,
    pub first: i64,
    pub last: i64,
}

impl ProjectRoll {
    /// Tokens **per session** — the "how hungry is this project" number.
    ///
    /// Tokens ÷ sessions, not sessions ÷ tokens: the latter is a number like
    /// 0.0000001 that ranks identically and reads like nothing. A project with
    /// few long sessions ranks above one with many short ones even when the
    /// second has spent more in total, which is the point — this measures the
    /// cost of *engaging* with a project, not how much it has been used.
    pub fn per_session(&self) -> u64 {
        if self.sessions == 0 {
            return 0;
        }
        self.priced.usage.total() / self.sessions
    }

    pub fn cost_per_session(&self) -> f64 {
        if self.sessions == 0 {
            return 0.0;
        }
        self.priced.cost / self.sessions as f64
    }
}

/// One UTC day.
#[derive(Clone, Copy, Debug, Default)]
pub struct DayRoll {
    /// Midnight UTC of that day, epoch seconds.
    pub day: i64,
    pub tokens: u64,
    pub cost: f64,
    pub messages: u64,
}

/// What each bucket actually cost, as opposed to how many tokens it holds.
/// The two are wildly different — cache read is most of the tokens and a
/// minority of the money — and printing only the token share misleads.
#[derive(Clone, Copy, Debug, Default)]
pub struct BucketCost {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

impl BucketCost {
    pub fn total(&self) -> f64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

#[derive(Debug)]
pub struct Stats {
    /// Ranked by tokens per session, hungriest first.
    pub projects: Vec<ProjectRoll>,
    /// Contiguous days, oldest first — including days with nothing on them,
    /// because a gap is a fact and a chart that closes the gap invents work.
    pub daily: Vec<DayRoll>,
    /// Tokens by hour of day, **UTC** (no timezone crate, and guessing an
    /// offset would silently shift every bar).
    pub hourly: [u64; 24],
    pub sessions: u64,
    pub bucket_cost: BucketCost,
    /// Span of recorded history, in days.
    pub span_days: i64,
    /// Spend per git branch, costliest first. `(unrecorded)` collects records
    /// older than the field rather than hiding them; `HEAD` is a detached
    /// checkout and is left as itself.
    pub branches: Vec<Slice>,
    /// Spend per reasoning effort, costliest first. `(unrecorded)` is its own
    /// row — the field appears only on newer records and folding it into `high`
    /// would invent the number.
    pub efforts: Vec<Slice>,
    /// Per-model daily totals over the charted window, so a migration between
    /// models is visible as it happened rather than as one blended average.
    pub model_daily: Vec<(String, Vec<u64>)>,
    /// First and last day each model was seen, over all of history.
    pub model_span: Vec<(String, i64, i64)>,
}

/// A named slice of the total: one branch, one effort level, one anything.
#[derive(Clone, Debug)]
pub struct Slice {
    pub name: String,
    pub sessions: u64,
    pub messages: u64,
    pub priced: Priced,
}

impl Stats {
    /// Mean cost per day over the recorded span — not over the charted window,
    /// which is a different and smaller number.
    pub fn cost_per_day(&self, total_cost: f64) -> f64 {
        if self.span_days <= 0 {
            return total_cost;
        }
        total_cost / self.span_days as f64
    }

    pub fn busiest_day(&self) -> Option<&DayRoll> {
        self.daily.iter().max_by_key(|d| d.tokens)
    }

    pub fn peak_hour(&self) -> Option<usize> {
        (0..24).max_by_key(|&h| self.hourly[h]).filter(|&h| self.hourly[h] > 0)
    }
}

/// Roll a scan up into the shapes the TUI charts. `window_days` bounds the
/// daily series only; every other figure is over all of history.
pub fn stats(report: &Report, window_days: i64) -> Stats {
    let mut projects: BTreeMap<String, ProjectRoll> = BTreeMap::new();
    let mut project_sessions: HashSet<(String, String)> = HashSet::new();
    let mut all_sessions: HashSet<String> = HashSet::new();
    let mut by_day: BTreeMap<i64, DayRoll> = BTreeMap::new();
    let mut hourly = [0u64; 24];
    let mut bucket_cost = BucketCost::default();

    for r in &report.records {
        let p = projects.entry(r.project.clone()).or_insert_with(|| ProjectRoll {
            project: r.project.clone(),
            sessions: 0,
            messages: 0,
            blueprints: BTreeSet::new(),
            priced: Priced::default(),
            first: r.ts,
            last: r.ts,
        });
        p.messages += 1;
        p.blueprints.insert(r.blueprint.clone());
        p.priced.push(&r.model, &r.usage);
        p.first = p.first.min(r.ts);
        p.last = p.last.max(r.ts);
        if project_sessions.insert((r.project.clone(), r.session.clone())) {
            p.sessions += 1;
        }
        all_sessions.insert(r.session.clone());

        let cost = r.usage.cost(&r.model).unwrap_or(0.0);
        let day = r.ts.div_euclid(86400) * 86400;
        let d = by_day.entry(day).or_insert(DayRoll { day, ..Default::default() });
        d.tokens += r.usage.total();
        d.cost += cost;
        d.messages += 1;

        hourly[(r.ts.rem_euclid(86400) / 3600).clamp(0, 23) as usize] += r.usage.total();

        if let Some((i, o)) = rates(&r.model) {
            bucket_cost.input += r.usage.input as f64 / 1e6 * i;
            bucket_cost.output += r.usage.output as f64 / 1e6 * o;
            bucket_cost.cache_write += r.usage.cache_write_5m as f64 / 1e6 * i * CACHE_WRITE_5M
                + r.usage.cache_write_1h as f64 / 1e6 * i * CACHE_WRITE_1H;
            bucket_cost.cache_read += r.usage.cache_read as f64 / 1e6 * i * CACHE_READ;
        }
    }

    // Fill the gaps: a day with no work is a zero bar, not a missing one.
    let daily = match (by_day.keys().next(), by_day.keys().next_back()) {
        (Some(&first), Some(&last)) => {
            let start = first.max(last - (window_days - 1).max(0) * 86400);
            let mut out = Vec::new();
            let mut d = start;
            while d <= last {
                out.push(by_day.get(&d).copied().unwrap_or(DayRoll { day: d, ..Default::default() }));
                d += 86400;
            }
            out
        }
        _ => Vec::new(),
    };

    let span_days = match (report.records.first(), report.records.last()) {
        (Some(a), Some(b)) => ((b.ts - a.ts) / 86400).max(1),
        _ => 0,
    };

    let mut projects: Vec<ProjectRoll> = projects.into_values().collect();
    projects.sort_by(|a, b| {
        b.per_session().cmp(&a.per_session()).then_with(|| b.project.cmp(&a.project))
    });

    let branches = slice_by(&report.records, |r| {
        if r.branch.is_empty() { UNRECORDED } else { &r.branch }
    });
    let efforts = slice_by(&report.records, |r| r.effort.as_deref().unwrap_or(UNRECORDED));
    let (model_daily, model_span) = model_timeline(&report.records, &daily);

    Stats {
        projects,
        daily,
        hourly,
        sessions: all_sessions.len() as u64,
        bucket_cost,
        span_days,
        branches,
        efforts,
        model_daily,
        model_span,
    }
}

/// The bucket an absent field goes in. Never folded into the commonest value:
/// `effort` and `gitBranch` only exist on newer records, and a silent merge
/// would report a number nothing measured.
pub const UNRECORDED: &str = "(unrecorded)";

/// Roll records into named slices by whatever `key` returns, costliest first.
fn slice_by(records: &[Record], key: impl Fn(&Record) -> &str) -> Vec<Slice> {
    let mut map: BTreeMap<String, Slice> = BTreeMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for r in records {
        let k = key(r).to_string();
        let s = map.entry(k.clone()).or_insert_with(|| Slice {
            name: k.clone(),
            sessions: 0,
            messages: 0,
            priced: Priced::default(),
        });
        s.messages += 1;
        s.priced.push(&r.model, &r.usage);
        if seen.insert((k, r.session.clone())) {
            s.sessions += 1;
        }
    }
    let mut out: Vec<Slice> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.priced.cost.total_cmp(&a.priced.cost).then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Per-model tokens per charted day, plus each model's first and last day over
/// all of history.
///
/// Only the models that actually appear in the charted window get a series —
/// otherwise a migration chart carries a row of zeroes for every model ever
/// used, which is the same visual weight as a model still in service.
fn model_timeline(
    records: &[Record],
    daily: &[DayRoll],
) -> (Vec<(String, Vec<u64>)>, Vec<(String, i64, i64)>) {
    let mut span: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for r in records {
        let day = r.ts.div_euclid(86400) * 86400;
        let e = span.entry(r.model.clone()).or_insert((day, day));
        e.0 = e.0.min(day);
        e.1 = e.1.max(day);
    }
    let mut model_span: Vec<(String, i64, i64)> =
        span.iter().map(|(m, (a, b))| (m.clone(), *a, *b)).collect();
    model_span.sort_by_key(|(_, a, _)| *a);

    let Some(first_day) = daily.first().map(|d| d.day) else {
        return (Vec::new(), model_span);
    };
    let mut series: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for r in records {
        let day = r.ts.div_euclid(86400) * 86400;
        if day < first_day {
            continue;
        }
        let i = ((day - first_day) / 86400) as usize;
        if i >= daily.len() {
            continue;
        }
        series
            .entry(r.model.clone())
            .or_insert_with(|| vec![0; daily.len()])[i] += r.usage.total();
    }
    // Drop a series that is all zeroes in the window. `<synthetic>` records
    // carry a usage object with nothing in it, and an all-blank sparkline reads
    // as a broken chart rather than as "no tokens" — the span table below still
    // lists the model, so nothing is hidden.
    let mut model_daily: Vec<(String, Vec<u64>)> =
        series.into_iter().filter(|(_, v)| v.iter().sum::<u64>() > 0).collect();
    model_daily.sort_by_key(|(_, v)| std::cmp::Reverse(v.iter().sum::<u64>()));
    (model_daily, model_span)
}

// ── Formatting helpers (shared by the CLI table and the TUI tab) ────────────

/// `2026-08-09T18:15:28.917Z` → epoch seconds. Returns `None` on anything that
/// isn't that shape, so a malformed line lands in the epoch-0 bucket visibly
/// rather than shifting a block by a random amount.
pub fn parse_iso8601(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + sec)
}

/// Howard Hinnant's civil-date algorithm — the same one `sessions::format_utc`
/// inverts, kept here so the two agree on the epoch.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn fmt_time(ts: i64) -> String {
    if ts <= 0 {
        return "—".into();
    }
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
    crate::sessions::format_utc(t)
}

/// Compact token count: `412M`, `18.2M`, `23.7k`, `812`.
pub fn fmt_tokens(n: u64) -> String {
    let f = n as f64;
    if f >= 1e9 {
        format!("{:.2}B", f / 1e9)
    } else if f >= 1e7 {
        format!("{:.0}M", f / 1e6)
    } else if f >= 1e6 {
        format!("{:.2}M", f / 1e6)
    } else if f >= 1e4 {
        format!("{:.0}k", f / 1e3)
    } else if f >= 1e3 {
        format!("{:.1}k", f / 1e3)
    } else {
        n.to_string()
    }
}

pub fn fmt_cost(c: f64) -> String {
    if c >= 1000.0 {
        format!("${c:.0}")
    } else if c >= 1.0 {
        format!("${c:.2}")
    } else {
        format!("${c:.3}")
    }
}

/// Eight levels of a vertical bar — one character per column, so 30 days or 24
/// hours fit a line each without a widget or a second screen.
const SPARK: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// A sparkline over `values`, one character per value.
pub fn spark(values: &[u64]) -> String {
    let max = values.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return " ".repeat(values.len());
    }
    values
        .iter()
        .map(|&v| {
            // A non-zero day must never render as blank — that reads as "no
            // work", which is a different fact.
            let idx = if v == 0 { 0 } else { (v as f64 / max as f64 * 8.0).ceil().max(1.0) as usize };
            SPARK[idx.min(8)]
        })
        .collect()
}

/// A duration where the seconds matter. `fmt_duration` has minute resolution,
/// which renders a typical 90-second turn as "1m" and a 20-second one as "0m".
pub fn fmt_secs(secs: f64) -> String {
    let s = secs.max(0.0).round() as i64;
    match (s / 3600, (s % 3600) / 60, s % 60) {
        (0, 0, sec) => format!("{sec}s"),
        (0, m, sec) => format!("{m}m{sec:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

pub fn fmt_duration(secs: i64) -> String {
    let s = secs.max(0);
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

/// Seconds since the Unix epoch, now.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(ts: i64, bp: &str, sess: &str, model: &str, u: Usage) -> Record {
        Record {
            ts,
            model: model.into(),
            blueprint: bp.into(),
            project: "proj".into(),
            session: sess.into(),
            branch: "main".into(),
            effort: Some("high".into()),
            usage: u,
        }
    }

    #[test]
    fn parses_the_transcript_timestamp_format() {
        // Ground truth: a real record from this machine's contextdb.
        let ts = parse_iso8601("2026-08-09T18:15:28.917Z").unwrap();
        assert_eq!(crate::sessions::format_utc(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64)
        ), "2026-08-09 18:15");
        // Epoch itself, and a leap day, to pin the civil-date arithmetic.
        assert_eq!(parse_iso8601("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso8601("2024-02-29T12:00:00Z"), Some(1709208000));
        assert_eq!(parse_iso8601("not a timestamp"), None);
    }

    /// The bug this module exists to prevent: one message, several content
    /// blocks, each repeating the full usage. Counting records instead of
    /// messages overstated output by 68% on a real transcript.
    #[test]
    fn duplicate_message_ids_are_counted_once() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        let line = |id: &str| {
            format!(
                r#"{{"timestamp":"2026-08-09T18:15:28.917Z","sessionId":"s1","type":"assistant","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":2,"output_tokens":100,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
            )
        };
        // Same id three times (three content blocks), then a distinct message.
        std::fs::write(
            &f,
            format!("{}\n{}\n{}\n{}\n", line("msg_a"), line("msg_a"), line("msg_a"), line("msg_b")),
        )
        .unwrap();

        let src = Source {
            dir: dir.path().to_path_buf(),
            blueprint: "Bp".into(),
            project: "proj".into(),
        };
        let r = scan(std::slice::from_ref(&src));
        assert_eq!(r.raw_records, 4, "all four records should be seen");
        assert_eq!(r.messages, 2, "but only two distinct messages counted");
        assert_eq!(r.envs[0].priced.usage.output, 200);

        // And scanning the same directory twice (an archive plus the live copy
        // it was made from) must not double anything.
        let again = scan(&[
            Source { dir: dir.path().into(), blueprint: "Bp".into(), project: "proj".into() },
            Source { dir: dir.path().into(), blueprint: "Bp".into(), project: "proj".into() },
        ]);
        assert_eq!(again.envs[0].priced.usage.output, 200);
    }

    /// Activity has to be keyed on the *block*, not the message. Claude Code
    /// writes one record per content block and repeats `message.id` on each, so
    /// a message-level key keeps the first block — usually `thinking` — and
    /// drops every tool call the turn made.
    #[test]
    fn tool_calls_are_counted_per_block_not_per_message() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        let block = |uuid: &str, body: &str| {
            format!(
                r#"{{"timestamp":"2026-08-09T18:15:28.917Z","sessionId":"s1","uuid":"{uuid}","type":"assistant","message":{{"id":"msg_a","model":"claude-opus-5","usage":{{"input_tokens":2,"output_tokens":100}},"content":[{body}]}}}}"#
            )
        };
        std::fs::write(
            &f,
            format!(
                "{}\n{}\n{}\n",
                block("u1", r#"{"type":"thinking","thinking":"…"}"#),
                block("u2", r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"git status"}}"#),
                block("u3", r#"{"type":"tool_use","id":"toolu_2","name":"Edit","input":{"file_path":"C:\\repo\\src\\main.rs"}}"#),
            ),
        )
        .unwrap();

        let src = Source {
            dir: dir.path().to_path_buf(),
            blueprint: "Bp".into(),
            project: "proj".into(),
        };
        let a = scan(std::slice::from_ref(&src)).activity;
        assert_eq!(a.tools.get("Bash"), Some(&1));
        assert_eq!(a.tools.get("Edit"), Some(&1));
        assert_eq!(a.shell.get("git"), Some(&1));
        // Basename only — the same file edited from two checkouts is one file.
        assert_eq!(a.files.get("main.rs"), Some(&1));

        // Reading the same directory twice (an archive plus the live copy it
        // came from) must not double the tool calls either.
        let twice = scan(&[
            Source { dir: dir.path().into(), blueprint: "Bp".into(), project: "proj".into() },
            Source { dir: dir.path().into(), blueprint: "Bp".into(), project: "proj".into() },
        ]);
        assert_eq!(twice.activity.tools.get("Bash"), Some(&1));
    }

    /// A queue record carries no `uuid`. Keying the dedup on one counted zero
    /// of 665 real records — a silent zero indistinguishable from a user who
    /// never queues a prompt, which is exactly how this repo goes wrong.
    #[test]
    fn events_without_a_uuid_are_still_counted() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        std::fs::write(
            &f,
            concat!(
                r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-01T13:14:31.767Z","sessionId":"s1","content":"do the thing"}"#,
                "\n",
                r#"{"type":"queue-operation","operation":"remove","timestamp":"2026-08-01T13:14:37.093Z","sessionId":"s1","content":"do the thing"}"#,
                "\n",
                r#"{"type":"system","subtype":"turn_duration","uuid":"u1","durationMs":93728,"timestamp":"2026-08-01T13:20:00.000Z","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"user","uuid":"u2","timestamp":"2026-08-01T13:21:00.000Z","sessionId":"s1","message":{"role":"user","content":"[Request interrupted by user]"}}"#,
                "\n",
                r#"{"type":"assistant","uuid":"u3","attributionSkill":"sync","timestamp":"2026-08-01T13:22:00.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-opus-5","usage":{"output_tokens":1}}}"#,
                "\n",
            ),
        )
        .unwrap();

        let src = Source {
            dir: dir.path().to_path_buf(),
            blueprint: "Bp".into(),
            project: "proj".into(),
        };
        let a = scan(std::slice::from_ref(&src)).activity;
        assert_eq!(a.queued, 1, "an enqueue with no uuid must still count");
        assert_eq!(a.unqueued, 1);
        assert_eq!(a.turns(), 1);
        assert_eq!(a.median_turn_secs(), 93.728);
        assert_eq!(a.interrupts, 1);
        assert_eq!(a.skill_ranking(), vec![("sync".to_string(), 1, 1)]);
        // 2026-08-01 was a Saturday; the histogram is Monday-first.
        assert_eq!(a.turn_weekday[5], 1, "turn_weekday: {:?}", a.turn_weekday);
        assert_eq!(a.turn_hour[13], 1);
    }

    /// `effort` and `gitBranch` only exist on newer records. Folding an absent
    /// value into the commonest one reports a number nothing measured, so it
    /// gets its own bucket and the page says so.
    #[test]
    fn an_absent_field_gets_its_own_bucket_rather_than_the_commonest_one() {
        let u = Usage { output: 100, ..Default::default() };
        let mut a = rec(1_000, "A", "s1", "claude-opus-5", u);
        a.branch = String::new();
        a.effort = None;
        let mut b = rec(2_000, "A", "s2", "claude-opus-5", u);
        b.branch = "feature".into();
        b.effort = Some("medium".into());
        let s = stats(&report_of(vec![a, b]), 30);

        let names: Vec<&str> = s.branches.iter().map(|x| x.name.as_str()).collect();
        assert!(names.contains(&UNRECORDED), "branches: {names:?}");
        assert!(names.contains(&"feature"), "branches: {names:?}");
        let efforts: Vec<&str> = s.efforts.iter().map(|x| x.name.as_str()).collect();
        assert!(efforts.contains(&UNRECORDED), "efforts: {efforts:?}");
        assert!(efforts.contains(&"medium"), "efforts: {efforts:?}");
        // Every record lands in exactly one bucket of each split.
        assert_eq!(s.branches.iter().map(|x| x.messages).sum::<u64>(), 2);
        assert_eq!(s.efforts.iter().map(|x| x.messages).sum::<u64>(), 2);
    }

    /// A model with no tokens in the charted window must not draw an all-blank
    /// sparkline — that reads as a broken chart. It still appears in the
    /// first/last-seen list, so nothing is hidden.
    #[test]
    fn a_model_with_no_tokens_in_the_window_is_listed_but_not_charted() {
        let day = 86_400;
        let real = rec(30 * day, "A", "s1", "claude-opus-5", Usage { output: 10, ..Default::default() });
        let empty = rec(30 * day, "A", "s1", "<synthetic>", Usage::default());
        let s = stats(&report_of(vec![real, empty]), 30);
        let charted: Vec<&str> = s.model_daily.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(charted, vec!["claude-opus-5"], "charted: {charted:?}");
        assert_eq!(s.model_span.len(), 2, "but both are listed with their span");
    }

    /// Attachments are the harness talking to itself. They carry a uuid — unlike
    /// the queue records above — so the same key works, and reading a directory
    /// twice must not double the payload.
    #[test]
    fn injected_context_is_measured_in_characters_and_counted_once() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        std::fs::write(
            &f,
            concat!(
                r#"{"type":"attachment","uuid":"a1","timestamp":"2026-08-01T13:00:00.000Z","sessionId":"s1","attachment":{"type":"task_reminder","content":[],"itemCount":0}}"#,
                "\n",
                r#"{"type":"attachment","uuid":"a2","timestamp":"2026-08-01T13:01:00.000Z","sessionId":"s1","attachment":{"type":"hook_additional_context","content":"be concise"}}"#,
                "\n",
            ),
        )
        .unwrap();
        let src =
            Source { dir: dir.path().to_path_buf(), blueprint: "Bp".into(), project: "proj".into() };

        let a = scan(std::slice::from_ref(&src)).activity;
        let total = a.injected_total();
        assert_eq!(total.count, 2);
        assert!(total.chars > 0, "the payload must be measured, not just counted");
        assert_eq!(a.attachments.len(), 2);
        // Biggest payload first — a rare large injection costs more than many
        // tiny ones, so the ranking is by size, not by count.
        assert_eq!(a.injected_ranking()[0].0, "hook_additional_context");

        let dup = |d: &Path| Source {
            dir: d.to_path_buf(),
            blueprint: "Bp".into(),
            project: "proj".into(),
        };
        let twice = scan(&[dup(dir.path()), dup(dir.path())]).activity;
        assert_eq!(twice.injected_total().chars, total.chars, "read twice, counted once");
    }

    /// The point of the injection section: a payload is written into the
    /// context once and then re-read by every later request in the session, so
    /// its real price is a multiple of its apparent one. Reading the token
    /// count alone gets this wrong in the alarming direction.
    #[test]
    fn an_injection_is_priced_against_every_request_that_follows_it() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        // 400 characters of payload, then three requests that carry it.
        let filler = "x".repeat(400);
        let req = |min: u32, id: &str| {
            format!(
                r#"{{"timestamp":"2026-08-01T13:{min:02}:00.000Z","sessionId":"s1","type":"assistant","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"output_tokens":1}}}}}}"#
            )
        };
        std::fs::write(
            &f,
            format!(
                "{}\n{}\n{}\n{}\n",
                format!(
                    r#"{{"type":"attachment","uuid":"a1","timestamp":"2026-08-01T13:00:00.000Z","sessionId":"s1","attachment":{{"type":"hook_additional_context","hookEvent":"UserPromptSubmit","content":"{filler}"}}}}"#
                ),
                req(1, "m1"),
                req(2, "m2"),
                req(3, "m3"),
            ),
        )
        .unwrap();
        let src =
            Source { dir: dir.path().to_path_buf(), blueprint: "Bp".into(), project: "proj".into() };
        let a = scan(std::slice::from_ref(&src)).activity;

        // The hook event names the row, so no matching on the hook's wording —
        // which has changed once already and would have gone silently wrong.
        let i = a.attachments.get("hook_additional_context/UserPromptSubmit").expect("row");
        assert_eq!(i.count, 1);
        let tokens = i.est_tokens() as f64 / 1e6;
        let (rate, _) = rates("claude-opus-5").unwrap();
        // One write at 2x input, then TWO re-reads at 0.1x — three requests
        // follow, and the first of them is the one that writes it.
        assert!((i.write_cost - tokens * rate * CACHE_WRITE_1H).abs() < 1e-12, "{i:?}");
        assert!((i.read_cost - tokens * rate * 2.0 * CACHE_READ).abs() < 1e-12, "{i:?}");
        assert!((i.multiplier() - 1.1).abs() < 0.001, "multiplier was {}", i.multiplier());
    }

    /// An injection nothing followed was never carried by any request, so it
    /// costs nothing — priced at zero rather than at the write price of a
    /// request that does not exist.
    #[test]
    fn an_injection_with_no_request_after_it_costs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("20260809_120000_abc_transcript.jsonl");
        std::fs::write(
            &f,
            concat!(
                r#"{"timestamp":"2026-08-01T13:00:00.000Z","sessionId":"s1","type":"assistant","message":{"id":"m1","model":"claude-opus-5","usage":{"output_tokens":1}}}"#,
                "\n",
                r#"{"type":"attachment","uuid":"a1","timestamp":"2026-08-01T13:05:00.000Z","sessionId":"s1","attachment":{"type":"task_reminder","content":[]}}"#,
                "\n",
            ),
        )
        .unwrap();
        let src =
            Source { dir: dir.path().to_path_buf(), blueprint: "Bp".into(), project: "proj".into() };
        let a = scan(std::slice::from_ref(&src)).activity;
        let i = a.attachments.get("task_reminder").expect("row");
        assert_eq!(i.count, 1, "it still happened, and is still counted");
        assert_eq!(i.cost(), 0.0, "but nothing ever paid to carry it");
    }

    #[test]
    fn turn_lengths_keep_their_seconds() {
        // fmt_duration would render every one of these as "0m" or "1m".
        assert_eq!(fmt_secs(9.0), "9s");
        assert_eq!(fmt_secs(93.728), "1m34s");
        assert_eq!(fmt_secs(5462.0), "1h31m");
    }

    #[test]
    fn cache_write_splits_by_ttl_and_falls_back_to_5m() {
        let with_split = serde_json::json!({
            "input_tokens": 2, "output_tokens": 3,
            "cache_creation_input_tokens": 100,
            "cache_read_input_tokens": 7,
            "cache_creation": {"ephemeral_5m_input_tokens": 40, "ephemeral_1h_input_tokens": 60}
        });
        let u = parse_usage(&with_split);
        assert_eq!((u.cache_write_5m, u.cache_write_1h), (40, 60));

        // Older transcripts have no `cache_creation` object at all.
        let no_split = serde_json::json!({
            "input_tokens": 2, "output_tokens": 3,
            "cache_creation_input_tokens": 100, "cache_read_input_tokens": 7
        });
        let u = parse_usage(&no_split);
        assert_eq!((u.cache_write_5m, u.cache_write_1h), (100, 0));
        assert_eq!(u.total(), 2 + 3 + 100 + 7);
    }

    #[test]
    fn prices_each_bucket_at_its_own_multiplier() {
        let u = Usage {
            input: 1_000_000,
            output: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            cache_read: 1_000_000,
        };
        // Opus 5: $5 in / $25 out → 5 + 25 + 6.25 + 10 + 0.5
        let c = u.cost("claude-opus-5").unwrap();
        assert!((c - 46.75).abs() < 1e-9, "got {c}");
        // A dated id must still match its family by longest prefix.
        assert_eq!(rates("claude-haiku-4-5-20251001"), Some((1.0, 5.0)));
        // `claude-opus-5` must not be swallowed by the shorter `claude-opus-4`.
        assert_eq!(rates("claude-opus-5"), Some((5.0, 25.0)));
    }

    /// An unrecognised model must never price as free — that is the silent-zero
    /// shape this repo keeps getting bitten by. Its tokens are quarantined and
    /// its id is surfaced.
    #[test]
    fn an_unknown_model_is_quarantined_not_zeroed() {
        let u = Usage { output: 500, ..Default::default() };
        assert_eq!(u.cost("claude-opus-99"), None);

        let recs = vec![rec(100, "Bp", "s", "claude-opus-99", u)];
        let (envs, unknown) = roll_envs(&recs);
        assert_eq!(envs[0].priced.cost, 0.0);
        assert_eq!(envs[0].priced.unpriced_tokens, 500);
        assert!(unknown.contains("claude-opus-99"));
    }

    #[test]
    fn blocks_close_on_the_window_and_on_a_long_gap() {
        let u = Usage { output: 10, ..Default::default() };
        let base = 10 * 3600; // a clean hour boundary
        let recs = vec![
            rec(base + 60, "A", "s1", "claude-opus-5", u),
            rec(base + 7200, "B", "s2", "claude-opus-5", u), // +2h, same block
            rec(base + WINDOW_SECS + 60, "A", "s3", "claude-opus-5", u), // past 5h → new
            // A gap longer than a window closes the block even though the next
            // message would still fall inside a 5h span of the block start.
            rec(base + WINDOW_SECS + 60 + WINDOW_SECS + 1, "A", "s4", "claude-opus-5", u),
        ];
        let blocks = build_blocks(&recs);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].start, base, "block starts at the containing hour");
        assert_eq!(blocks[0].priced.usage.output, 20);
        // The shared-quota view: who spent it inside this block.
        assert_eq!(blocks[0].by_env.len(), 2);
        assert_eq!(blocks[0].by_env["A"].output, 10);
    }

    #[test]
    fn sessions_roll_up_under_their_env_newest_first() {
        let u = Usage { output: 10, ..Default::default() };
        let recs = vec![
            rec(1000, "A", "old", "claude-opus-5", u),
            rec(9000, "A", "new", "claude-opus-5", u),
            rec(9100, "A", "new", "claude-opus-5", u),
        ];
        let (envs, _) = roll_envs(&recs);
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].sessions.len(), 2);
        assert_eq!(envs[0].sessions[0].id, "new");
        assert_eq!(envs[0].sessions[0].messages, 2);
        assert_eq!(envs[0].priced.usage.output, 30);
    }

    // ── Statistics ──────────────────────────────────────────────────────────

    fn rec_in(ts: i64, project: &str, sess: &str, u: Usage) -> Record {
        Record {
            ts,
            model: "claude-opus-5".into(),
            blueprint: "A".into(),
            project: project.into(),
            session: sess.into(),
            branch: "main".into(),
            effort: Some("high".into()),
            usage: u,
        }
    }

    fn report_of(records: Vec<Record>) -> Report {
        Report {
            records,
            activity: Activity::default(),
            envs: Vec::new(),
            blocks: Vec::new(),
            unknown_models: BTreeSet::new(),
            files_scanned: 0,
            pointer_only: 0,
            messages: 0,
            raw_records: 0,
        }
    }

    /// The ranking is tokens **per session**, so a project with one huge
    /// session outranks one with many small ones even when the second has
    /// spent more overall. That inversion is the whole point of the metric,
    /// so it is the thing pinned.
    #[test]
    fn projects_rank_by_tokens_per_session_not_by_total() {
        let big = Usage { output: 900, ..Default::default() };
        let small = Usage { output: 100, ..Default::default() };
        let r = report_of(vec![
            rec_in(1000, "hungry", "h1", big),
            // "busy" spends more in total (1000 > 900) across four sessions.
            rec_in(1000, "busy", "b1", small),
            rec_in(2000, "busy", "b2", small),
            rec_in(3000, "busy", "b3", small),
            rec_in(4000, "busy", "b4", small),
            rec_in(5000, "busy", "b5", small),
            rec_in(6000, "busy", "b6", small),
            rec_in(7000, "busy", "b7", small),
            rec_in(8000, "busy", "b8", small),
            rec_in(9000, "busy", "b9", small),
            rec_in(9500, "busy", "b10", small),
        ]);
        let s = stats(&r, 30);

        assert_eq!(s.projects[0].project, "hungry");
        assert_eq!(s.projects[0].sessions, 1);
        assert_eq!(s.projects[0].per_session(), 900);
        assert_eq!(s.projects[1].project, "busy");
        assert_eq!(s.projects[1].sessions, 10);
        assert_eq!(s.projects[1].per_session(), 100);
        assert!(
            s.projects[1].priced.usage.total() > s.projects[0].priced.usage.total(),
            "the lower-ranked project really did spend more in total"
        );
        assert_eq!(s.sessions, 11, "sessions are counted across every project");
    }

    /// A day with no work is a zero bar, not a missing one — otherwise a chart
    /// closes the gap and invents a week of steady activity that never
    /// happened.
    #[test]
    fn the_daily_series_keeps_its_empty_days() {
        let u = Usage { output: 10, ..Default::default() };
        let day = 86400;
        let big = Usage { output: 99, ..Default::default() };
        let s = stats(&report_of(vec![rec_in(day * 10, "p", "a", big), rec_in(day * 14, "p", "b", u)]), 30);
        assert_eq!(s.daily.len(), 5, "day 10 through day 14 inclusive");
        assert_eq!(s.daily[0].tokens, 99);
        assert_eq!(s.daily[1].tokens, 0);
        assert_eq!(s.daily[4].tokens, 10);
        assert_eq!(s.busiest_day().map(|d| d.day), Some(day * 10));
    }

    #[test]
    fn the_daily_window_is_bounded_but_history_is_not() {
        let u = Usage { output: 10, ..Default::default() };
        let day = 86400;
        let recs = (0..40).map(|i| rec_in(day * (100 + i), "p", &format!("s{i}"), u)).collect();
        let s = stats(&report_of(recs), 30);
        assert_eq!(s.daily.len(), 30, "the chart is bounded");
        assert_eq!(s.sessions, 40, "the totals are not");
        assert_eq!(s.projects[0].priced.usage.output, 400);
    }

    /// Tokens and money are different shapes: cache read dominates one and not
    /// the other. Reporting only the token split misleads, so the cost split
    /// is computed per bucket at each bucket's own multiplier.
    #[test]
    fn the_cost_split_is_not_the_token_split() {
        let u = Usage { output: 1_000_000, cache_read: 100_000_000, ..Default::default() };
        let s = stats(&report_of(vec![rec_in(0, "p", "a", u)]), 30);
        let c = s.bucket_cost;
        // Opus 5: $5/M in, $25/M out; a read is 0.1x input.
        assert!((c.output - 25.0).abs() < 1e-6, "output {}", c.output);
        assert!((c.cache_read - 50.0).abs() < 1e-6, "cache read {}", c.cache_read);
        let token_share = u.cache_read as f64 / u.total() as f64;
        let cost_share = c.cache_read / c.total();
        assert!(token_share > 0.99, "reads are ~all the tokens: {token_share}");
        assert!(cost_share < 0.7, "and nothing like all the money: {cost_share}");
    }

    #[test]
    fn hours_bucket_in_utc_and_name_their_peak() {
        let u = Usage { output: 10, ..Default::default() };
        let s = stats(
            &report_of(vec![
                rec_in(15 * 3600, "p", "a", u),
                rec_in(86400 + 15 * 3600 + 59, "p", "b", u),
                rec_in(3 * 3600, "p", "c", u),
            ]),
            30,
        );
        assert_eq!(s.hourly[15], 20);
        assert_eq!(s.hourly[3], 10);
        assert_eq!(s.peak_hour(), Some(15));
    }

    /// An archive whose transcript was never copied (or has since been
    /// deleted) contributes nothing — and that is indistinguishable from a
    /// quiet week unless it is counted and said out loud. 269 of 415 archives
    /// on the machine this was written on.
    #[test]
    fn archives_with_no_transcript_beside_them_are_counted_and_not_swallowed() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("proj").join("A");
        std::fs::create_dir_all(&d).unwrap();

        // One session archived with its transcript…
        std::fs::write(d.join("20260810_100000_aaaa_end.jsonl"), "{}").unwrap();
        std::fs::write(
            d.join("20260810_100000_aaaa_transcript.jsonl"),
            r#"{"timestamp":"2026-08-10T10:00:00.000Z","sessionId":"aaaa","message":{"id":"m1","model":"claude-opus-5","usage":{"output_tokens":10}}}"#,
        )
        .unwrap();
        // …and two archived as a pointer only.
        std::fs::write(d.join("20260701_100000_bbbb_end.jsonl"), "{}").unwrap();
        std::fs::write(d.join("20260702_100000_cccc_end.jsonl"), "{}").unwrap();

        let sources =
            vec![Source { dir: d, blueprint: "A".into(), project: "proj".into() }];
        let report = scan(&sources);

        assert_eq!(report.pointer_only, 2, "both pointer-only archives are counted");
        assert_eq!(report.messages, 1, "and only the real transcript contributes usage");
        assert_eq!(report.files_scanned, 1, "the sidecars are not read");
    }

    /// An empty scan must not divide by zero or claim a peak that isn't there.
    #[test]
    fn an_empty_scan_produces_an_empty_but_valid_stat_block() {
        let s = stats(&report_of(Vec::new()), 30);
        assert!(s.projects.is_empty());
        assert!(s.daily.is_empty());
        assert_eq!(s.sessions, 0);
        assert_eq!(s.peak_hour(), None);
        assert!(s.busiest_day().is_none());
        assert_eq!(s.cost_per_day(12.0), 12.0, "no span means the total stands as the rate");
    }
}
