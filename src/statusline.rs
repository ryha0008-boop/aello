//! `aello statusline` — the usage readout Claude Code renders under the prompt.
//!
//! Claude Code runs a `statusLine` command on every conversation update and
//! renders its stdout (multi-line is kept, blank lines are dropped). It hands
//! the command a JSON payload on stdin, and that payload is the only place two
//! of these numbers exist at all: `rate_limits.five_hour` and
//! `rate_limits.seven_day` are the *subscription's own* utilisation, sent back
//! by the API in `anthropic-ratelimit-unified-*` headers. Nothing in a
//! transcript carries them — `tokens.rs` says so, and measures its 5-hour
//! window against this machine's peak block instead precisely because of it.
//! So the plan percentages come from the payload and the token counts come from
//! the transcripts; neither source can answer the other's question.
//!
//! Measured against Claude Code 2.1.228 (the schema is documented inside the
//! binary, and a probe statusline captured a live payload to confirm it).
//!
//! Everything here must survive a payload that is missing anything: an API-key
//! session has no `rate_limits` at all, and a session with no messages yet has
//! no `context_window.used_percentage`. A missing field drops its segment
//! rather than printing a zero, because a confident `0%` is the shape of wrong
//! this repo keeps hitting.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::tokens::{self, Priced, Usage};

/// How long a project-wide scan is reused. The scan is ~1s over hundreds of
/// megabytes and the statusline re-runs up to three times a second, so it
/// cannot run per render. This is a cache of a *derivation*, not a record that
/// something was done: it is keyed on nothing but age, re-derives from the
/// transcripts themselves, and the worst staleness is a total that lags by a
/// few minutes on a figure that moves by ~0.01% a turn. The session and turn
/// counts beside it are re-read from disk every single render.
const PROJECT_TTL_SECS: i64 = 180;

const CACHE_FILE: &str = "statusline-cache.json";

pub fn run() -> Result<()> {
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let v: serde_json::Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or(serde_json::Value::Null);

    let facts = gather(&v, tokens::now());
    println!("{}", render(&facts));
    Ok(())
}

// ── Facts ───────────────────────────────────────────────────────────────────

/// Everything the two lines are built from, so rendering is testable without a
/// transcript on disk or a live session.
#[derive(Default, Debug)]
pub struct Facts {
    pub model: String,
    pub effort: Option<String>,
    pub ctx_used_pct: Option<f64>,
    pub ctx_tokens: u64,
    pub ctx_size: u64,
    /// Claude Code's own running cost for this session — exact, and free.
    pub session_cost: Option<f64>,
    /// (percent used, epoch seconds when it resets)
    pub five_hour: Option<(f64, i64)>,
    pub seven_day: Option<(f64, i64)>,
    pub turn: Priced,
    pub prev_turn: Priced,
    pub session: Priced,
    pub project: Priced,
    /// Tokens spent inside the plan's current 5-hour and 7-day windows —
    /// summed over the interval the payload's percentage is measured against.
    /// Machine-wide, since the quota is.
    pub five_hour_tokens: u64,
    pub seven_day_tokens: u64,
    /// Set when the project total came from cache, so it can say how old it is.
    pub project_age: i64,
    pub now: i64,
}

fn gather(v: &serde_json::Value, now: i64) -> Facts {
    let mut f = Facts { now, ..Default::default() };

    f.model = v["model"]["display_name"].as_str().unwrap_or("").to_string();
    f.effort = v["effort"]["level"].as_str().map(str::to_string);
    f.ctx_used_pct = v["context_window"]["used_percentage"].as_f64();
    f.ctx_tokens = v["context_window"]["total_input_tokens"].as_u64().unwrap_or(0);
    f.ctx_size = v["context_window"]["context_window_size"].as_u64().unwrap_or(0);
    f.session_cost = v["cost"]["total_cost_usd"].as_f64();

    let limit = |k: &str| -> Option<(f64, i64)> {
        let l = v["rate_limits"].get(k)?;
        Some((l["used_percentage"].as_f64()?, l["resets_at"].as_i64().unwrap_or(0)))
    };
    f.five_hour = limit("five_hour");
    f.seven_day = limit("seven_day");

    if let Some(t) = v["transcript_path"].as_str() {
        let s = read_session(Path::new(t));
        f.turn = s.turn;
        f.prev_turn = s.prev;
        f.session = s.session;

        if let Some(cwd) = v["cwd"].as_str() {
            // The plan's *own* window bounds, not aello's inferred blocks: the
            // percentage on screen is measured against these, so the token
            // count beside it has to be summed over the same interval or the
            // two halves of one segment disagree.
            let w = Windows {
                five_start: f.five_hour.map(|(_, r)| r - FIVE_HOUR_SECS).unwrap_or(0),
                seven_start: f.seven_day.map(|(_, r)| r - SEVEN_DAY_SECS).unwrap_or(0),
            };
            let t = totals(Path::new(cwd), env_dir_of(Path::new(t)).as_deref(), now, w);
            f.project = t.project;
            f.five_hour_tokens = t.five_hour;
            f.seven_day_tokens = t.seven_day;
            f.project_age = t.age;
        }
    }
    f
}

// ── The current session's transcript ────────────────────────────────────────

#[derive(Default)]
struct Session {
    /// The turn in progress (everything since the last real user prompt).
    turn: Priced,
    /// The turn before it — the answer to "what did that last one cost me?".
    prev: Priced,
    session: Priced,
}

/// Walk one transcript, splitting it into turns.
///
/// A turn starts at a **user prompt**, which is not the same thing as a record
/// with `"role":"user"`: every tool result is written back as one too, and a
/// subagent's whole conversation is interleaved into the same file with
/// `isSidechain: true`. Counting those as boundaries would report a "turn" of
/// one tool call. Sidechain *usage* still counts — a subagent spends the same
/// quota — it just doesn't start a turn.
///
/// Dedup is by `message.id`, for the reason `tokens.rs` exists: Claude Code
/// writes one record per content block and repeats the whole `usage` on each,
/// so summing records roughly doubles the answer.
fn read_session(path: &Path) -> Session {
    let Ok(text) = std::fs::read_to_string(path) else { return Session::default() };

    let mut seen: HashSet<String> = HashSet::new();
    let mut turns: Vec<Priced> = vec![Priced::default()];
    let mut session = Priced::default();

    for line in text.lines() {
        if is_user_prompt(line) {
            turns.push(Priced::default());
            continue;
        }
        // Cheap gate: tool results are the bulk of a transcript by bytes and
        // carry no usage, so they are never parsed.
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let msg = &v["message"];
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else { continue };
        let Some(id) = msg["id"].as_str() else { continue };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let model = msg["model"].as_str().unwrap_or("unknown");
        let usage = tokens::parse_usage(u);
        session.push(model, &usage);
        if let Some(t) = turns.last_mut() {
            t.push(model, &usage);
        }
    }

    let turn = turns.pop().unwrap_or_default();
    let prev = turns.pop().unwrap_or_default();
    Session { turn, prev, session }
}

/// Does this line start a new turn?
///
/// The substring is a **gate, not the test**. Transcripts reach tens of
/// megabytes and are mostly tool results, so parsing every line is not free;
/// but the shapes below cannot be told apart by substring, so every line that
/// trips the gate is then really parsed. (Text that merely *quotes* a
/// transcript can't trip it: JSON escapes the inner quotes, so the bare
/// `"type":"user"` never appears inside a string value. That is measured, not
/// assumed — `quoting_a_transcript_does_not_trip_the_gate` pins it.)
fn is_user_prompt(line: &str) -> bool {
    if !line.contains(r#""type":"user""#) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(line).is_ok_and(|v| is_prompt_record(&v))
}

/// A typed prompt, as opposed to everything else Claude Code writes back as a
/// `user` record. Measured over every transcript in this project (4,895 such
/// records): 4,539 tool results, 313 plain-string prompts, 21 prompts carrying
/// a pasted image, and 22 text-only arrays that are all `[Request interrupted
/// by user]` markers.
///
/// So content that is a **string** is a prompt, and an **array** is a prompt
/// only when it holds something other than text and tool results — an image or
/// a document. Counting an interrupt marker as a boundary would insert an
/// empty turn between the interrupted answer and the next prompt, and "last
/// turn" would then read as nothing at exactly the moment it is interesting.
fn is_prompt_record(v: &serde_json::Value) -> bool {
    if v["type"] != "user"
        || v["isSidechain"].as_bool().unwrap_or(false)
        || v["isMeta"].as_bool().unwrap_or(false)
    {
        return false;
    }
    match &v["message"]["content"] {
        serde_json::Value::String(_) => true,
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .any(|b| !matches!(b["type"].as_str(), Some("text") | Some("tool_result") | None)),
        _ => false,
    }
}

// ── The project total ───────────────────────────────────────────────────────

/// `<env>/projects/<encoded-cwd>/<session>.jsonl` → `<env>`.
fn env_dir_of(transcript: &Path) -> Option<PathBuf> {
    Some(transcript.parent()?.parent()?.parent()?.to_path_buf())
}

const FIVE_HOUR_SECS: i64 = 5 * 3600;
const SEVEN_DAY_SECS: i64 = 7 * 86400;

/// The plan's window bounds, taken from the payload's reset times.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Windows {
    five_start: i64,
    seven_start: i64,
}

#[derive(Default)]
struct Totals {
    project: Priced,
    five_hour: u64,
    seven_day: u64,
    age: i64,
}

/// One scan, three answers: what this *project* has ever cost, and how many
/// tokens went through the plan's current 5-hour and 7-day windows.
///
/// The window counts are **machine-wide**, because the quota is — every env
/// shares one subscription token. They can only under-report: a session running
/// in another project right now is not readable until it ends (contextdb
/// records a project's folder name, not its path), which is the same limit
/// `aello tokens` documents.
///
/// Cached, because the scan is ~1s and the statusline renders up to three
/// times a second. The window bounds are part of the cache key: when a window
/// resets the old sum is not stale, it is *wrong*, and it would sit there for
/// the rest of the TTL saying the new window opened full.
fn totals(cwd: &Path, env_dir: Option<&Path>, now: i64, w: Windows) -> Totals {
    let cache = env_dir.map(|d| d.join(CACHE_FILE));

    if let Some(c) = cache.as_deref().and_then(read_cache) {
        let age = now - c.at;
        if age >= 0 && age < PROJECT_TTL_SECS && c.windows == w {
            return Totals { project: c.project, five_hour: c.five, seven_day: c.seven, age };
        }
    }

    let project = cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let report = tokens::scan(&tokens::collect_sources(cwd));

    let mut t = Totals::default();
    for r in &report.records {
        if r.project == project {
            t.project.push(&r.model, &r.usage);
        }
        if w.five_start > 0 && r.ts >= w.five_start {
            t.five_hour += r.usage.total();
        }
        if w.seven_start > 0 && r.ts >= w.seven_start {
            t.seven_day += r.usage.total();
        }
    }

    if let Some(p) = cache {
        write_cache(&p, now, &t, w);
    }
    t
}

struct Cache {
    at: i64,
    project: Priced,
    five: u64,
    seven: u64,
    windows: Windows,
}

fn read_cache(path: &Path) -> Option<Cache> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    let mut project = Priced::default();
    project.usage = Usage {
        input: n("input"),
        output: n("output"),
        cache_write_5m: n("cache_write_5m"),
        cache_write_1h: n("cache_write_1h"),
        cache_read: n("cache_read"),
    };
    project.cost = v["cost"].as_f64().unwrap_or(0.0);
    project.unpriced_tokens = n("unpriced_tokens");
    Some(Cache {
        at: v["at"].as_i64()?,
        project,
        five: n("five_hour"),
        seven: n("seven_day"),
        windows: Windows {
            five_start: v["five_start"].as_i64().unwrap_or(0),
            seven_start: v["seven_start"].as_i64().unwrap_or(0),
        },
    })
}

/// Staged through a per-pid temp file: several statuslines can be in flight at
/// once (the render fires up to three times a second), and a half-written cache
/// read back as zeros would print a project total that silently collapsed.
fn write_cache(path: &Path, now: i64, t: &Totals, w: Windows) {
    let u = &t.project.usage;
    let body = serde_json::json!({
        "at": now,
        "input": u.input,
        "output": u.output,
        "cache_write_5m": u.cache_write_5m,
        "cache_write_1h": u.cache_write_1h,
        "cache_read": u.cache_read,
        "cost": t.project.cost,
        "unpriced_tokens": t.project.unpriced_tokens,
        "five_hour": t.five_hour,
        "seven_day": t.seven_day,
        "five_start": w.five_start,
        "seven_start": w.seven_start,
    });
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, body.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

// ── Rendering ───────────────────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
/// Truecolor, matching the TUI's palette (`tui.rs`) so aello looks like one
/// thing: kinetic orange on near-black, matrix green for money.
const WHITE: &str = "\x1b[38;2;229;226;225m";
const ORANGE: &str = "\x1b[38;2;255;102;0m";
const GREEN: &str = "\x1b[38;2;74;255;138m";
const RED: &str = "\x1b[38;2;255;77;77m";

/// Between groups. Inside a group the separator is a bare `·`, which is what
/// keeps a group readable as one thing in a narrow window.
const SEP: &str = " │ ";

/// A limit at or above this reads as red rather than green.
const HOT_PCT: f64 = 80.0;

pub fn render(f: &Facts) -> String {
    let mut rows: Vec<String> = Vec::new();

    // Row 1 — the ceilings: context now, then the plan's two windows.
    let mut top: Vec<String> = Vec::new();
    if f.ctx_tokens > 0 {
        let cost = match f.session_cost {
            Some(c) => format!("{WHITE}·{GREEN}{}", tokens::fmt_cost(c)),
            None => String::new(),
        };
        top.push(format!("{RED}{}{cost}", tokens::fmt_tokens(f.ctx_tokens)));
    }
    if let Some((p, r)) = f.five_hour {
        top.push(window("5h", p, f.five_hour_tokens, r, f.now));
    }
    if let Some((p, r)) = f.seven_day {
        top.push(window("7d", p, f.seven_day_tokens, r, f.now));
    }
    if !top.is_empty() {
        rows.push(format!("{WHITE}{}", top.join(&format!("{WHITE}{SEP}"))));
    }

    // Row 2 — what has actually been spent, narrowest scope first. Orange
    // throughout, except the money; the whole row turns red once a plan window
    // is spent, since at that point the spend is the thing to look at.
    let over = [f.five_hour, f.seven_day].iter().flatten().any(|(p, _)| *p >= 100.0);
    let ink = if over { RED } else { ORANGE };
    let money = if over { RED } else { GREEN };
    let mut spend: Vec<String> = Vec::new();
    for (label, p) in [
        ("last", &f.prev_turn),
        ("this", &f.turn),
        ("sess", &f.session),
        ("prjt", &f.project),
    ] {
        if p.usage.total() == 0 {
            continue;
        }
        spend.push(format!(
            "{ink}{label}·{}·{money}{}",
            tokens::fmt_tokens(p.usage.total()),
            tokens::fmt_cost(p.cost)
        ));
    }
    if !spend.is_empty() {
        rows.push(format!("{ink}{}", spend.join(&format!("{ink}{SEP}"))));
    }

    if rows.is_empty() { String::new() } else { format!("{}{RESET}", rows.join("\n")) }
}

/// `5h·42%·50M·31m` — the plan's percentage, the tokens that went into that
/// same window, and how long until it resets. Green under the line, red over
/// it, so the colour is the part read at a glance.
fn window(label: &str, pct: f64, tokens_used: u64, resets_at: i64, now: i64) -> String {
    let ink = if pct >= HOT_PCT { RED } else { GREEN };
    let mut s = format!("{ink}{label}·{pct:.0}%");
    if tokens_used > 0 {
        s.push_str(&format!("·{}", tokens::fmt_tokens(tokens_used)));
    }
    if resets_at > 0 {
        s.push_str(&format!("·{}", fmt_until(resets_at - now)));
    }
    s
}

/// `5d6h` / `4h12m` / `38m` / `now`.
fn fmt_until(secs: i64) -> String {
    if secs <= 0 {
        return "now".into();
    }
    let (d, h, m) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        let mut f = Facts {
            model: "Opus 5".into(),
            effort: Some("high".into()),
            ctx_used_pct: Some(12.0),
            ctx_tokens: 117_345,
            ctx_size: 1_000_000,
            session_cost: Some(1.96),
            five_hour: Some((33.0, 1_000_000 + 3600)),
            seven_day: Some((19.0, 1_000_000 + 86400 * 2)),
            now: 1_000_000,
            ..Default::default()
        };
        f.session.push("claude-opus-5", &Usage { output: 1000, ..Default::default() });
        f
    }

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Two rows: ceilings on top, spend beneath. The model and the effort are
    /// deliberately absent — the session already knows what it is running.
    #[test]
    fn renders_two_rows() {
        let out = strip(&render(&facts()));
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows.len(), 2, "expected two rows, got {out:?}");
        assert_eq!(rows[0], "117k·$1.96 │ 5h·33%·1h00m │ 7d·19%·2d0h", "{out}");
        assert_eq!(rows[1], "sess·1.0k·$0.025", "{out}");
        assert!(!out.contains("Opus"), "the model is not shown: {out}");
        assert!(!out.contains("high"), "the effort is not shown: {out}");
        assert!(!out.contains('█'), "no bars: {out}");
    }

    /// An API-key session has no `rate_limits`, and a session with nothing
    /// spent yet has no counts. Neither may print a confident zero.
    #[test]
    fn a_missing_limit_drops_its_segment_rather_than_reporting_zero() {
        let mut f = facts();
        f.five_hour = None;
        f.seven_day = None;
        let out = strip(&render(&f));
        assert!(!out.contains("5h"), "{out}");
        assert!(!out.contains("7d"), "{out}");
        assert!(!out.contains("0%"), "{out}");
        assert!(out.contains("117k"), "the context is still there: {out}");
    }

    /// Colour is the part read at a glance, so it is asserted, not eyeballed:
    /// context red, money green, a limit green under the line and red over it.
    #[test]
    fn colour_says_what_the_number_means() {
        let f = facts();
        let out = render(&f);
        assert!(out.contains(&format!("{RED}117k")), "context is red: {out:?}");
        assert!(out.contains(&format!("{GREEN}$1.96")), "money is green: {out:?}");
        assert!(out.contains(&format!("{GREEN}5h")), "a cool limit is green: {out:?}");

        let mut hot = facts();
        hot.five_hour = Some((92.0, 1_000_000 + 3600));
        let out = render(&hot);
        assert!(out.contains(&format!("{RED}5h")), "a hot limit is red: {out:?}");
        assert!(out.contains(&format!("{ORANGE}sess")), "spend is orange under the limit: {out:?}");
    }

    /// Past the limit the spend row is what matters, so the whole row switches
    /// to red — including the money, which is green everywhere else.
    #[test]
    fn the_spend_row_turns_red_once_a_window_is_spent() {
        let mut f = facts();
        f.seven_day = Some((100.0, 1_000_000 + 86400));
        let out = render(&f);
        assert!(out.contains(&format!("{RED}sess")), "{out:?}");
        assert!(!out.contains(&format!("{ORANGE}sess")), "{out:?}");
    }

    /// The token count beside a percentage must be summed over the window that
    /// percentage is measured against, or one segment contradicts itself.
    #[test]
    fn a_window_shows_its_own_tokens() {
        let mut f = facts();
        f.five_hour_tokens = 50_000_000;
        f.seven_day_tokens = 103_000_000;
        let out = strip(&render(&f));
        assert!(out.contains("5h·33%·50M·1h00m"), "{out}");
        assert!(out.contains("7d·19%·103M·2d0h"), "{out}");
    }

    #[test]
    fn an_empty_payload_prints_nothing() {
        let f = gather(&serde_json::Value::Null, 0);
        assert_eq!(render(&f), "");
    }

    /// The whole payload contract in one place: every field this reads is one
    /// Claude Code documents, spelled as it spells it.
    #[test]
    fn reads_the_documented_payload_shape() {
        let v = serde_json::json!({
            "model": {"id": "claude-opus-5", "display_name": "Opus 5"},
            "effort": {"level": "high"},
            "cost": {"total_cost_usd": 2.5},
            "context_window": {
                "total_input_tokens": 117345,
                "context_window_size": 1000000,
                "used_percentage": 12
            },
            "rate_limits": {
                "five_hour": {"used_percentage": 33, "resets_at": 1786503600},
                "seven_day": {"used_percentage": 19, "resets_at": 1786989600}
            }
        });
        let f = gather(&v, 1_786_500_000);
        assert_eq!(f.model, "Opus 5");
        assert_eq!(f.effort.as_deref(), Some("high"));
        assert_eq!(f.session_cost, Some(2.5));
        assert_eq!(f.ctx_used_pct, Some(12.0));
        assert_eq!(f.five_hour.map(|(p, _)| p), Some(33.0));
        assert_eq!(f.seven_day.map(|(p, _)| p), Some(19.0));
    }

    /// Turn boundaries are user *prompts*. A tool result and a subagent's
    /// messages carry `"type":"user"` too, and treating either as a boundary
    /// reports a "turn" that is one tool call. Every case here was taken from
    /// a real transcript on this machine.
    #[test]
    fn turn_boundaries_are_user_prompts_only() {
        assert!(is_user_prompt(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#));
        assert!(
            is_user_prompt(
                r#"{"type":"user","message":{"content":[{"type":"image"},{"type":"text","text":"look"}]}}"#
            ),
            "a prompt with a pasted image is still a prompt"
        );
        assert!(!is_user_prompt(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok"}]}}"#
        ));
        assert!(
            !is_user_prompt(
                r#"{"type":"user","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#
            ),
            "an interrupt marker is not a new prompt"
        );
        assert!(!is_user_prompt(r#"{"type":"user","isSidechain":true,"message":{}}"#));
        assert!(!is_user_prompt(r#"{"type":"user","isMeta":true,"message":{}}"#));
        assert!(!is_user_prompt(r#"{"type":"assistant","message":{"usage":{}}}"#));
    }

    /// Text that quotes a transcript cannot trip the gate — JSON escapes the
    /// inner quotes, so the bare substring never appears inside a string
    /// value. Worth pinning: the gate is the only thing standing between this
    /// and parsing every line of a 7 MB file, and "a tool result quoting a
    /// transcript" is the obvious reason to think it is unsafe.
    #[test]
    fn quoting_a_transcript_does_not_trip_the_gate() {
        let line = r#"{"type":"assistant","message":{"id":"x","model":"claude-opus-5","content":[{"type":"text","text":"the record reads {\"type\":\"user\"}"}],"usage":{"output_tokens":5}}}"#;
        assert!(!line.contains(r#""type":"user""#), "escaped quotes, so no bare substring");
        assert!(!is_user_prompt(line));
    }

    /// A tool result *does* carry the bare substring — it is a real `user`
    /// record. The parse behind the gate is what rejects it.
    #[test]
    fn a_tool_result_trips_the_gate_and_is_rejected_by_the_parse() {
        let line = r#"{"parentUuid":"a","isSidechain":false,"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#;
        assert!(line.contains(r#""type":"user""#), "the gate must trip");
        assert!(!is_user_prompt(line), "and the parse must reject it");
    }

    /// Two turns, and the repeated `usage` on a second content block counted
    /// once — the dedup `tokens.rs` exists for, applied per turn.
    #[test]
    fn splits_turns_and_counts_a_repeated_message_once() {
        let dir = std::env::temp_dir().join(format!("aello-sl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let msg = |id: &str, out: u64| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"output_tokens":{out}}}}}}}"#
            )
        };
        let text = [
            r#"{"type":"user","message":{"role":"user","content":"one"}}"#.to_string(),
            msg("a", 100),
            msg("a", 100), // same message, second content block
            r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#.to_string(),
            msg("b", 50),
            r#"{"type":"user","message":{"role":"user","content":"two"}}"#.to_string(),
            msg("c", 7),
        ]
        .join("\n");
        std::fs::write(&path, text).unwrap();

        let s = read_session(&path);
        assert_eq!(s.turn.usage.output, 7, "current turn");
        assert_eq!(s.prev.usage.output, 150, "previous turn (tool result is not a boundary)");
        assert_eq!(s.session.usage.output, 157, "session total, deduped");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cache_round_trips() {
        let dir = std::env::temp_dir().join(format!("aello-slc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CACHE_FILE);
        let mut t = Totals { five_hour: 50_000_000, seven_day: 103_000_000, ..Default::default() };
        t.project
            .push("claude-opus-5", &Usage { output: 12_345, cache_read: 999, ..Default::default() });
        let w = Windows { five_start: 900_000, seven_start: 400_000 };
        write_cache(&path, 42, &t, w);

        let back = read_cache(&path).expect("cache reads back");
        assert_eq!(back.at, 42);
        assert_eq!(back.project.usage.output, 12_345);
        assert_eq!(back.project.usage.cache_read, 999);
        assert!((back.project.cost - t.project.cost).abs() < 1e-9);
        assert_eq!(back.five, 50_000_000);
        assert_eq!(back.seven, 103_000_000);
        // The window bounds are the cache key: a reset must invalidate the
        // sums rather than leave them reading as the new window's spend.
        assert!(back.windows == w, "the window bounds round-trip");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn until_reads_in_the_unit_that_matters() {
        assert_eq!(fmt_until(-5), "now");
        assert_eq!(fmt_until(60 * 38), "38m");
        assert_eq!(fmt_until(3600 * 4 + 60 * 12), "4h12m");
        assert_eq!(fmt_until(86400 * 5 + 3600 * 6), "5d6h");
    }

}
