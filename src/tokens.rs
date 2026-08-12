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

use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    pub usage: Usage,
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
            raw += read_transcript(&path, src, &mut seen, &mut records);
        }
    }

    records.sort_by_key(|r| r.ts);
    let messages = records.len();
    let blocks = build_blocks(&records);
    let (envs, unknown_models) = roll_envs(&records);

    Report {
        records,
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
) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else { return 0 };
    let mut raw = 0usize;
    for line in text.lines() {
        // Cheap gate: most lines are user turns and tool results with no usage
        // at all, and JSON-parsing 322 MB of them would dominate the run.
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
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
            usage: parse_usage(usage),
        });
    }
    raw
}

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

    Stats { projects, daily, hourly, sessions: all_sessions.len() as u64, bucket_cost, span_days }
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
            usage: u,
        }
    }

    fn report_of(records: Vec<Record>) -> Report {
        Report {
            records,
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
