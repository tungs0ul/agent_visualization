//! Claude Code hook command for Agent Room.
//!
//! Claude Code runs this binary on every hook event and passes the event as JSON on
//! stdin. It must be fast and must never disturb Claude Code: it always exits 0 and
//! prints nothing unless it is answering a permission request.
//!
//!     room-hook              forward the event to the Agent Room app (PLAN.md 3.2)
//!     room-hook --log [path] append `{"ts_ms":<unix ms>,"hook":<stdin>}` as one line to
//!                            `path`, by default `~/.agent-room/spike/events.jsonl`
//!
//! Forward mode: if the app is not running, exit silently. For `PermissionRequest`, wait
//! for the user's answer and print it; on timeout or any error print nothing, so Claude
//! Code shows its normal terminal prompt. It never answers "allow" on its own.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use room_proto::{HookEnvelope, PermissionReply, SOCKET_NAME};

const DEFAULT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(1);

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
            append_event(&path, &read_stdin()?)
        }
        Some(other) => Err(format!("unknown argument: {other}")),
        None => forward(&read_stdin()?),
    }
}

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("read stdin: {e}"))?;
    Ok(input)
}

fn state_dir() -> Result<PathBuf, String> {
    room_proto::state_dir().ok_or_else(|| "HOME is not set".to_owned())
}

/// Sends the event to the app; for permission requests prints the user's answer.
fn forward(raw: &str) -> Result<(), String> {
    let hook: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("hook JSON: {e}"))?;
    let envelope = HookEnvelope::new(hook);

    // App not running (no socket, or a stale one): nothing to do.
    let Ok(mut stream) = UnixStream::connect(state_dir()?.join(SOCKET_NAME)) else {
        return Ok(());
    };
    stream
        .set_write_timeout(Some(WRITE_TIMEOUT))
        .map_err(|e| format!("socket timeout: {e}"))?;
    let mut line = serde_json::to_string(&envelope).map_err(|e| format!("encode: {e}"))?;
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(|e| format!("send: {e}"))?;
    if !envelope.needs_reply {
        return Ok(());
    }

    stream
        .set_read_timeout(Some(permission_timeout()))
        .map_err(|e| format!("socket timeout: {e}"))?;
    let mut reply = String::new();
    match BufReader::new(&stream).read_line(&mut reply) {
        // The user did not answer in time: let Claude Code ask in the terminal.
        Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return Ok(()),
        Err(e) => return Err(format!("receive: {e}")),
        // App closed the connection without answering (quit, or answered elsewhere).
        Ok(0) => return Ok(()),
        Ok(_) => {}
    }
    let reply: PermissionReply = serde_json::from_str(reply.trim()).map_err(|e| format!("reply JSON: {e}"))?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{}", reply.hook_output()).map_err(|e| format!("stdout: {e}"))
}

/// `AGENT_ROOM_PERMISSION_TIMEOUT` in seconds, default 60. Must stay well below the
/// hook timeout Claude Code applies (600 s by default).
fn permission_timeout() -> Duration {
    std::env::var("AGENT_ROOM_PERMISSION_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .map_or(DEFAULT_PERMISSION_TIMEOUT, Duration::from_secs)
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
