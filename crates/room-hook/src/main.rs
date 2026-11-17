//! Claude Code hook command for Agent Room.
//!
//! Claude Code runs this binary on every hook event and passes the event as JSON on
//! stdin. It must be fast and must never disturb Claude Code: it always exits 0 and
//! prints nothing unless it is answering a permission request (not implemented yet).
//!
//! Milestone 1 only has log mode:
//!
//!     room-hook --log [path]
//!
//! appends `{"ts_ms":<unix ms>,"hook":<stdin>}` as one line to `path`, by default
//! `~/.agent-room/spike/events.jsonl`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_DIR: &str = ".agent-room";

fn main() -> ExitCode {
    if let Err(err) = run() {
        log_error(&err);
    }
    // Always succeed: a non-zero exit shows a "hook error" notice in Claude Code.
    ExitCode::SUCCESS
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--log") => {
            let path = match args.next() {
                Some(p) => PathBuf::from(p),
                None => state_dir()?.join("spike").join("events.jsonl"),
            };
            let mut input = String::new();
            io::stdin()
                .read_to_string(&mut input)
                .map_err(|e| format!("read stdin: {e}"))?;
            append_event(&path, &input)
        }
        Some(other) => Err(format!("unknown argument: {other}")),
        // Real mode (socket to the app) arrives in milestone 2; until then do nothing.
        None => Ok(()),
    }
}

fn state_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(STATE_DIR))
}

/// Wraps the raw hook JSON with a timestamp and appends it as a single line.
fn append_event(path: &Path, raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty stdin".into());
    }
    let line = format!("{{\"ts_ms\":{},\"hook\":{}}}\n", now_ms(), one_line(raw));
    let mut file = open_private_append(path)?;
    // Several hooks can fire at once (parallel tools, sub-agents): lock so lines never mix.
    file.lock().map_err(|e| format!("lock {}: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// Raw newlines can only appear between JSON tokens, never inside strings, so
/// replacing them with spaces keeps valid JSON valid.
fn one_line(raw: &str) -> String {
    raw.replace(['\n', '\r'], " ")
}

/// Opens `path` for appending; creates missing parent dirs as 0700 and the file as 0600,
/// because hook payloads can contain secrets.
fn open_private_append(path: &Path) -> Result<File, String> {
    if let Some(parent) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Best effort: if even this fails there is nowhere left to report to.
fn log_error(err: &str) {
    let Ok(dir) = state_dir() else { return };
    if let Ok(mut file) = open_private_append(&dir.join("hook-errors.log")) {
        let _ = writeln!(file, "{} room-hook: {err}", now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_strips_newlines_between_tokens() {
        assert_eq!(one_line("{\n  \"a\": 1\r\n}"), "{   \"a\": 1  }");
    }

    #[test]
    fn append_event_writes_one_wrapped_line_per_call() {
        let dir = std::env::temp_dir().join(format!("room-hook-test-{}", std::process::id()));
        let path = dir.join("nested").join("events.jsonl");
        append_event(&path, "{\"hook_event_name\":\"Stop\"}\n").unwrap();
        append_event(&path, "{\n\"hook_event_name\":\"SessionEnd\"\n}").unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("{\"ts_ms\":"));
        assert!(lines[0].ends_with(",\"hook\":{\"hook_event_name\":\"Stop\"}}"));
        assert!(lines[1].contains("\"SessionEnd\""));

        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn append_event_rejects_empty_input() {
        let path = std::env::temp_dir().join("room-hook-never-created.jsonl");
        assert!(append_event(&path, "  \n").is_err());
        assert!(!path.exists());
    }
}
