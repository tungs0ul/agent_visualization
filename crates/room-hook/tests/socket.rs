//! Runs the real `room-hook` binary against a fake app socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use room_proto::HookEnvelope;

const PERMISSION: &str = r#"{"session_id":"s","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;
const PRE_TOOL: &str = r#"{"session_id":"s","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{}}"#;

/// A short, unique state dir (Unix socket paths are limited to ~104 bytes on macOS),
/// removed again when the test ends.
struct StateDir(PathBuf);

impl std::ops::Deref for StateDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for StateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn state_dir() -> StateDir {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("rh-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    StateDir(dir)
}

/// macOS scans a freshly built binary on its first launch, which can take longer than the
/// timing limits below. Launch it once, untimed, before measuring.
fn warm_up() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = state_dir();
        run_hook(&dir, PRE_TOOL);
    });
}

fn run_hook(dir: &Path, stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_room-hook"))
        .env("AGENT_ROOM_DIR", dir)
        .env("AGENT_ROOM_PERMISSION_TIMEOUT", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// Accepts one connection, returns the envelope it received, and answers with `reply`
/// (or stays silent until the hook gives up when `reply` is `None`).
fn fake_app(dir: &Path, reply: Option<&'static str>) -> JoinHandle<HookEnvelope> {
    let listener = UnixListener::bind(dir.join("room.sock")).unwrap();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let mut stream = reader.into_inner();
        match reply {
            // Fails with a broken pipe when the hook did not wait, which is fine.
            Some(reply) => {
                let _ = writeln!(stream, "{reply}");
            }
            // Hold the connection open past the hook's 1 s timeout.
            None => thread::sleep(Duration::from_millis(1500)),
        }
        serde_json::from_str(&line).unwrap()
    })
}

fn assert_silent_success(output: &Output) {
    assert!(output.status.success(), "exit status {:?}", output.status);
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn app_not_running_is_silent_and_fast() {
    warm_up();
    let dir = state_dir();
    let started = Instant::now();
    let output = run_hook(&dir, PERMISSION);
    assert_silent_success(&output);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!dir.join("hook-errors.log").exists());
}

#[test]
fn regular_event_is_forwarded_without_waiting() {
    warm_up();
    let dir = state_dir();
    let app = fake_app(&dir, None);
    let started = Instant::now();
    let output = run_hook(&dir, PRE_TOOL);
    assert!(started.elapsed() < Duration::from_millis(500), "hook waited for a reply");
    assert_silent_success(&output);
    let envelope = app.join().unwrap();
    assert!(!envelope.needs_reply);
    assert_eq!(envelope.hook["tool_name"], "Read");
}

#[test]
fn allow_reply_prints_allow_decision() {
    let dir = state_dir();
    let app = fake_app(&dir, Some(r#"{"decision":"allow"}"#));
    let output = run_hook(&dir, PERMISSION);
    assert!(app.join().unwrap().needs_reply);
    let printed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["hookSpecificOutput"]["hookEventName"], "PermissionRequest");
    assert_eq!(printed["hookSpecificOutput"]["decision"]["behavior"], "allow");
}

#[test]
fn deny_reply_prints_deny_with_message() {
    let dir = state_dir();
    let app = fake_app(&dir, Some(r#"{"decision":"deny","message":"Not in prod"}"#));
    let output = run_hook(&dir, PERMISSION);
    app.join().unwrap();
    let printed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["hookSpecificOutput"]["decision"]["behavior"], "deny");
    assert_eq!(printed["hookSpecificOutput"]["decision"]["message"], "Not in prod");
}

#[test]
fn no_answer_falls_back_to_terminal_prompt() {
    let dir = state_dir();
    let app = fake_app(&dir, None);
    let output = run_hook(&dir, PERMISSION);
    assert_silent_success(&output);
    app.join().unwrap();
}

#[test]
fn garbage_reply_falls_back_and_is_logged() {
    let dir = state_dir();
    let app = fake_app(&dir, Some("yes please"));
    let output = run_hook(&dir, PERMISSION);
    assert_silent_success(&output);
    app.join().unwrap();
    let log = std::fs::read_to_string(dir.join("hook-errors.log")).unwrap();
    assert!(log.contains("reply JSON"), "{log}");
}

#[test]
fn invalid_hook_json_is_silent_and_logged() {
    let dir = state_dir();
    let output = run_hook(&dir, "not json");
    assert_silent_success(&output);
    assert!(std::fs::read_to_string(dir.join("hook-errors.log")).unwrap().contains("hook JSON"));
}
