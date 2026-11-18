//! Claude Code hook command for Agent Room.
//!
//! Claude Code runs this binary on every hook event and passes the event as JSON on
//! stdin. It must be fast and must never disturb Claude Code: it always exits 0 and
//! prints nothing unless it is answering a permission request.
//!
//!     room-hook              forward the event to the Agent Room app (PLAN.md 3.2)
//!     room-hook --channel    MCP channel server: passes messages the user types in the
//!                            app into this terminal session (PLAN.md M8b)
//!     room-hook --status     for the user's status line command: forwards the account's
//!                            5-hour and 7-day usage to the app (PLAN.md M10). Prints
//!                            nothing, so it can sit beside any status line script
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

use room_proto::{HookEnvelope, PermissionReply, SOCKET_NAME, reply_timeout};

mod channel;

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
        Some("--channel") => channel::serve(),
        Some("--status") => status(&read_stdin()?),
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

/// Forwards the usage limits in a status line's input, if it has any.
fn status(raw: &str) -> Result<(), String> {
    let input: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("status JSON: {e}"))?;
    let Some(limits) = room_proto::limits::Limits::from_status_line(&input) else { return Ok(()) };
    let Ok(mut stream) = UnixStream::connect(state_dir()?.join(SOCKET_NAME)) else { return Ok(()) };
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| format!("socket timeout: {e}"))?;
    let mut line = serde_json::to_string(&room_proto::limits::LimitsReport { limits }).map_err(|e| format!("encode: {e}"))?;
    line.push('\n');
    stream.write_all(line.as_bytes()).map_err(|e| format!("send: {e}"))
}

/// Sends the event to the app; for permission requests prints the user's answer.
fn forward(raw: &str) -> Result<(), String> {
    let mut hook: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("hook JSON: {e}"))?;
    if let Some(output) = hold(&hook) {
        return writeln!(io::stdout().lock(), "{output}").map_err(|e| format!("stdout: {e}"));
    }
    mark_session(&mut hook);
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
        .set_read_timeout(Some(reply_timeout(&envelope.hook)))
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
    writeln!(stdout, "{}", reply.hook_output(&envelope.hook)).map_err(|e| format!("stdout: {e}"))
}

/// How often a paused hook looks at its hold file, and checks that the app still runs.
const HOLD_POLL: Duration = Duration::from_millis(250);
const APP_CHECK: Duration = Duration::from_secs(2);

/// Pause and stop from the app (`room_proto::hold`). Before a tool runs: wait while the
/// session is paused, and once it is stopped deny the tool and end the turn. Alone,
/// `continue: false` lets the tool run first, and no `Stop` hook follows, so the app is
/// told the turn ended here. The tool event reaches the app only when the tool really
/// goes ahead. A new prompt clears a stop, so it never ends a later turn. A paused hook
/// lets go when the app is gone, so Claude never waits on nobody.
fn hold(hook: &serde_json::Value) -> Option<serde_json::Value> {
    let output = held(hook)?;
    let mut ended = serde_json::json!({ "hook_event_name": "Stop", "last_assistant_message": room_proto::hold::STOP_REASON });
    for key in ["session_id", "transcript_path", "cwd"] {
        if let Some(value) = hook.get(key) {
            ended[key] = value.clone();
        }
    }
    if let Ok(mut stream) = UnixStream::connect(state_dir().ok()?.join(SOCKET_NAME))
        && let Ok(mut line) = serde_json::to_string(&HookEnvelope::new(ended))
    {
        line.push('\n');
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
        let _ = stream.write_all(line.as_bytes());
    }
    Some(output)
}

fn held(hook: &serde_json::Value) -> Option<serde_json::Value> {
    use room_proto::hold::{self, Hold};
    let session = hook.get("session_id")?.as_str()?;
    match hook.get("hook_event_name")?.as_str()? {
        "UserPromptSubmit" => {
            if hold::read(session) == Some(Hold::Stop) {
                let _ = hold::write(session, None);
            }
            None
        }
        "PreToolUse" => {
            let mut checked: Option<std::time::Instant> = None;
            loop {
                let held = hold::read(session)?;
                // A hold left by an app that is gone means nothing.
                if checked.is_none_or(|at| at.elapsed() >= APP_CHECK) {
                    UnixStream::connect(state_dir().ok()?.join(SOCKET_NAME)).ok()?;
                    checked = Some(std::time::Instant::now());
                }
                match held {
                    Hold::Stop => {
                        return Some(serde_json::json!({
                            "continue": false,
                            "stopReason": hold::STOP_REASON,
                            "hookSpecificOutput": {
                                "hookEventName": "PreToolUse",
                                "permissionDecision": "deny",
                                "permissionDecisionReason": hold::STOP_REASON,
                            },
                        }));
                    }
                    Hold::Pause => std::thread::sleep(HOLD_POLL),
                }
            }
        }
        _ => None,
    }
}

/// Sessions started with `claude -p` by other tools (e.g. another plugin's Stop hook
/// that writes a summary) must not take over the room or close Claude's answer
/// (PLAN.md M8a status). Claude Code doesn't say so in the payload, so look at the
/// `claude` process that runs this hook; its pid also tells the app which channel
/// belongs to the session (PLAN.md M8b). Only for the events that can switch the room,
/// since it runs `ps`. Sessions the app starts itself set `AGENT_ROOM_CHAT` and count as
/// interactive.
fn mark_session(hook: &mut serde_json::Value) {
    let event = hook.get("hook_event_name").and_then(serde_json::Value::as_str);
    if !matches!(event, Some("SessionStart" | "UserPromptSubmit")) || std::env::var_os("AGENT_ROOM_CHAT").is_some() {
        return;
    }
    let Some((pid, args)) = find_claude(std::os::unix::process::parent_id()) else { return };
    let Some(object) = hook.as_object_mut() else { return };
    object.insert(room_proto::CLAUDE_PID_FIELD.to_owned(), pid.into());
    if args.iter().any(|arg| matches!(arg.as_str(), "-p" | "--print")) {
        object.insert(room_proto::HEADLESS_FIELD.to_owned(), serde_json::Value::Bool(true));
    }
}

/// Walks up from `pid` (through the plugin's shell shim, if any) to the first `claude`
/// process: its pid and arguments.
pub(crate) fn find_claude(mut pid: u32) -> Option<(u32, Vec<String>)> {
    for _ in 0..4 {
        let output = std::process::Command::new("ps").args(["-o", "ppid=,args=", "-p", &pid.to_string()]).output().ok()?;
        let line = String::from_utf8_lossy(&output.stdout);
        let mut fields = line.split_whitespace();
        let parent = fields.next()?.parse::<u32>().ok()?;
        let args: Vec<&str> = fields.collect();
        if is_claude(&args) {
            return Some((pid, args.into_iter().map(str::to_owned).collect()));
        }
        if parent <= 1 {
            return None;
        }
        pid = parent;
    }
    None
}

/// `claude …` or `node …/claude …`.
fn is_claude(args: &[&str]) -> bool {
    args.iter().take(2).any(|arg| Path::new(arg).file_name().is_some_and(|name| name == "claude"))
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
    fn recognises_claude_processes() {
        assert!(is_claude(&["claude", "-p"]));
        assert!(is_claude(&["/opt/homebrew/bin/claude"]));
        assert!(is_claude(&["node", "/usr/local/bin/claude", "--print"]));
        assert!(!is_claude(&["/bin/sh", "/x/plugin/libexec/room-hook"]));
        assert!(!is_claude(&["zsh", "-c", "claude -p"]));
    }

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
