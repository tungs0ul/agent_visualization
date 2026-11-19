//! Tokens per session (V2.md phase 6, kept small for a widget): read from Claude
//! Code's session transcript after each turn, only the part written since the last
//! read. Each assistant message's `usage` counts once (a message's content blocks
//! repeat it on several lines).

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Seek, SeekFrom};

use bevy::prelude::Resource;
use room_proto::limits::Limits;

/// Tokens a session used so far.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tokens {
    /// Everything sent to the model, cache reads and writes included.
    pub input: u64,
    pub output: u64,
}

#[derive(Default)]
struct Reading {
    offset: u64,
    seen: HashSet<String>,
    tokens: Tokens,
    /// The context size: everything the latest message sent to the model.
    context: u64,
    /// When Claude Code last compacted the conversation (the tank drains).
    compacted: Option<std::time::Instant>,
    /// Compaction started (PreCompact) and hasn't finished (PostCompact) yet.
    compacting: bool,
}

#[derive(Resource, Default)]
pub struct Usage {
    sessions: HashMap<String, Reading>,
    /// The account's 5-hour and 7-day usage (PLAN.md M10), shared by all sessions.
    limits: Limits,
}

impl Usage {
    /// A newer report of the usage limits, from a status line or an app session.
    pub fn report_limits(&mut self, limits: Limits) {
        self.limits.merge(limits);
    }

    /// The limits as last reported, without windows that have reset since.
    pub fn limits(&self) -> Limits {
        self.limits.current(unix_now())
    }

    pub fn tokens(&self, session: &str) -> Option<Tokens> {
        self.sessions.get(session).map(|reading| reading.tokens).filter(|tokens| tokens.input + tokens.output > 0)
    }

    /// The context's size (tokens) and when it was last compacted.
    pub fn context(&self, session: &str) -> Option<(u64, Option<std::time::Instant>)> {
        self.sessions.get(session).map(|reading| (reading.context, reading.compacted)).filter(|(size, at)| *size > 0 || at.is_some())
    }

    /// Claude Code starts compacting; the tank empties once it is done.
    pub fn compacting(&mut self, session: &str) {
        self.sessions.entry(session.to_owned()).or_default().compacting = true;
    }

    pub fn is_compacting(&self, session: &str) -> bool {
        self.sessions.get(session).is_some_and(|reading| reading.compacting)
    }

    /// Claude Code compacted the conversation: the context starts small again.
    pub fn compacted(&mut self, session: &str) {
        let reading = self.sessions.entry(session.to_owned()).or_default();
        reading.compacting = false;
        reading.context = 0;
        reading.compacted = Some(std::time::Instant::now());
    }

    /// Reads what the transcript gained since last time.
    pub fn update(&mut self, session: &str, transcript: &str) {
        let reading = self.sessions.entry(session.to_owned()).or_default();
        let Ok(mut file) = std::fs::File::open(transcript) else { return };
        if file.seek(SeekFrom::Start(reading.offset)).is_err() {
            return;
        }
        let mut lines = BufReader::new(file);
        let mut line = String::new();
        // Only whole lines: a line still being written is read next time.
        while lines.read_line(&mut line).is_ok_and(|read| read > 0) {
            if !line.ends_with('\n') {
                break;
            }
            reading.offset += line.len() as u64;
            count(reading, &line);
            line.clear();
        }
    }
}

fn count(reading: &mut Reading, line: &str) {
    if !line.contains("\"usage\"") {
        return;
    }
    let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else { return };
    let message = &entry["message"];
    let usage = &message["usage"];
    if !usage.is_object() {
        return;
    }
    if let Some(id) = message["id"].as_str()
        && !reading.seen.insert(id.to_owned())
    {
        return;
    }
    let field = |name: &str| usage[name].as_u64().unwrap_or(0);
    let sent = field("input_tokens") + field("cache_creation_input_tokens") + field("cache_read_input_tokens");
    reading.tokens.input += sent;
    reading.context = sent;
    reading.tokens.output += field("output_tokens");
}

/// The key a sub-agent's tokens are kept under, beside its session's.
pub fn agent_key(session: &str, agent: &str) -> String {
    format!("{session}/{agent}")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The header line for the limits, "5h 23 % · week 70 %", its hover text with the
/// reset times, and the fuller window's percent (for the colour).
pub fn limits_line(limits: Limits) -> Option<(String, String, f32)> {
    let now = unix_now();
    let windows = [("5h", "5-hour", limits.five_hour), ("week", "Weekly", limits.seven_day)];
    let known: Vec<_> = windows.into_iter().filter_map(|(short, long, window)| Some((short, long, window?))).collect();
    let fullest = known.iter().map(|(_, _, window)| window.used).reduce(f32::max)?;
    let line = known.iter().map(|(short, _, window)| format!("{short} {:.0} %", window.used)).collect::<Vec<_>>().join(" · ");
    let hover = known
        .iter()
        .map(|(_, long, window)| format!("{long} limit: {:.0} % used, resets in {}", window.used, until(window.resets_at.saturating_sub(now))))
        .collect::<Vec<_>>()
        .join("\n");
    Some((line, hover, fullest))
}

/// "3 d 4 h", "2 h 10 min", "5 min".
fn until(secs: u64) -> String {
    let (days, hours, minutes) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    match (days, hours) {
        (1.., _) => format!("{days} d {hours} h"),
        (0, 1..) => format!("{hours} h {minutes} min"),
        _ => format!("{} min", minutes.max(1)),
    }
}

/// The context window of a model: 1M tokens for the `[1m]` variants, else 200k.
pub fn window(model: Option<&str>) -> u64 {
    if model.is_some_and(|model| model.contains("[1m]")) { 1_000_000 } else { 200_000 }
}

/// "1.2M", "34k", "512".
pub fn short(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.0}k", count as f64 / 1e3),
        _ => format!("{:.1}M", count as f64 / 1e6),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn counts_each_message_once_and_only_new_lines() {
        let dir = std::env::temp_dir().join(format!("usage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let message = |id: &str| {
            format!(
                "{{\"message\":{{\"id\":\"{id}\",\"usage\":{{\"input_tokens\":2,\"cache_read_input_tokens\":100,\"output_tokens\":10}}}}}}\n"
            )
        };
        std::fs::write(&path, message("a") + &message("a") + "{\"type\":\"user\"}\n").unwrap();
        let mut usage = Usage::default();
        usage.update("s", path.to_str().unwrap());
        assert_eq!(usage.tokens("s"), Some(Tokens { input: 102, output: 10 }));

        // The next turn appends; only the new message counts, and a half-written
        // line waits for the next read.
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(file, "{}{{\"message\":{{\"id\":\"c\"", message("b")).unwrap();
        usage.update("s", path.to_str().unwrap());
        assert_eq!(usage.tokens("s"), Some(Tokens { input: 204, output: 20 }));
        // The context is what the latest message sent; compaction empties it.
        assert_eq!(usage.context("s").map(|(size, _)| size), Some(102));
        usage.compacted("s");
        assert_eq!(usage.context("s").map(|(size, _)| size), Some(0));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(short(1_234_567), "1.2M");
        assert_eq!(short(34_000), "34k");
    }

    #[test]
    fn limits_show_what_is_known_and_keep_older_windows() {
        use room_proto::limits::Window;
        let mut usage = Usage::default();
        assert_eq!(limits_line(usage.limits()), None);
        let later = unix_now() + 2 * 3_600 + 600;
        usage.report_limits(Limits { five_hour: Some(Window { used: 23.4, resets_at: later }), seven_day: None });
        usage.report_limits(Limits { five_hour: None, seven_day: Some(Window { used: 70.0, resets_at: later }) });
        let (line, hover, fullest) = limits_line(usage.limits()).unwrap();
        assert_eq!(line, "5h 23 % · week 70 %");
        assert!(hover.starts_with("5-hour limit: 23 % used, resets in 2 h 10 min"), "{hover}");
        assert_eq!(fullest, 70.0);
        // A window that has reset is gone until the next report.
        usage.report_limits(Limits { five_hour: Some(Window { used: 99.0, resets_at: 1 }), seven_day: None });
        assert_eq!(limits_line(usage.limits()).unwrap().0, "week 70 %");
    }
}
