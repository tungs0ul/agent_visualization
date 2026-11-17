//! `room-hook --channel`: a Claude Code channel (PLAN.md M8b).
//!
//! Claude Code starts this as the plugin's MCP server and talks JSON-RPC over stdin and
//! stdout. It declares the `claude/channel` capability, connects to the Agent Room app
//! (`ChannelHello`) and turns every line the app sends into a
//! `notifications/claude/channel` event, which Claude handles like a prompt typed in the
//! terminal. The app only sends what the user typed and sent there.
//!
//! Claude Code only listens when the session was started with
//! `claude --dangerously-load-development-channels plugin:agent-room@agent-room` (custom
//! channels aren't on the research-preview allowlist). Without it the server answers
//! MCP but stays away from the app, so the app never offers to send to that session.

use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use room_proto::{ChannelHello, ChannelInfo, ChannelMessage, SOCKET_NAME};
use serde_json::{Value, json};

/// How often to look for the app again while it isn't running.
const RECONNECT: Duration = Duration::from_secs(3);

const INSTRUCTIONS: &str = "Messages from the Agent Room app arrive as <channel source=\"agent-room\">. \
The user typed them in the Agent Room window instead of this terminal: treat each one exactly like a \
prompt the user typed here. Answer in the session as usual; the app shows your answer.";

type Out = Arc<Mutex<io::Stdout>>;

pub fn serve() -> Result<(), String> {
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let ready = Arc::new(AtomicBool::new(false));
    if let Some((claude_pid, args)) = crate::find_claude(std::os::unix::process::parent_id())
        && loaded_as_channel(&args)
    {
        let info = ChannelInfo { claude_pid, session_id: std::env::var("CLAUDE_CODE_SESSION_ID").ok() };
        let (out, ready) = (out.clone(), ready.clone());
        thread::spawn(move || relay_from_app(&info, &out, &ready));
    }
    // Claude Code closes stdin when the session ends; then this process ends too.
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("stdin: {e}"))?;
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        if request.get("method").and_then(Value::as_str) == Some("notifications/initialized") {
            ready.store(true, Ordering::SeqCst);
        }
        if let Some(response) = answer(&request) {
            write_line(&out, &response)?;
        }
    }
    Ok(())
}

/// Started with `--dangerously-load-development-channels plugin:agent-room@…` or
/// `--channels plugin:agent-room@…`.
fn loaded_as_channel(args: &[String]) -> bool {
    let flag = args.iter().any(|arg| {
        arg.starts_with("--dangerously-load-development-channels") || arg.starts_with("--channels")
    });
    flag && args.iter().any(|arg| arg.contains("plugin:agent-room@"))
}

/// The JSON-RPC answer to one request; notifications get none.
fn answer(request: &Value) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "experimental": { "claude/channel": {} } },
            "serverInfo": { "name": "agent-room", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn notification(text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/claude/channel",
        "params": { "content": text, "meta": {} },
    })
}

fn write_line(out: &Out, message: &Value) -> Result<(), String> {
    let mut out = out.lock().map_err(|_| "stdout lock poisoned")?;
    writeln!(out, "{message}").and_then(|()| out.flush()).map_err(|e| format!("stdout: {e}"))
}

/// Keeps a connection to the app (reconnecting while it is closed) and forwards what
/// it sends.
fn relay_from_app(info: &ChannelInfo, out: &Out, ready: &AtomicBool) {
    let Ok(socket) = crate::state_dir().map(|dir| dir.join(SOCKET_NAME)) else { return };
    let Ok(mut hello) = serde_json::to_string(&ChannelHello { channel: info.clone() }) else { return };
    hello.push('\n');
    loop {
        if let Ok(mut stream) = UnixStream::connect(&socket)
            && stream.write_all(hello.as_bytes()).is_ok()
        {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let Ok(message) = serde_json::from_str::<ChannelMessage>(&line) else { continue };
                // Claude Code drops notifications sent before the MCP handshake ends.
                while !ready.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
                if write_line(out, &notification(&message.text)).is_err() {
                    return;
                }
            }
        }
        thread::sleep(RECONNECT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_the_channel_and_answers_requests() {
        let init = answer(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }))
            .unwrap();
        assert_eq!(init["result"]["capabilities"]["experimental"]["claude/channel"], json!({}));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(answer(&json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" })).unwrap()["result"], json!({}));
        assert!(answer(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })).unwrap()["error"].is_object());
        assert!(answer(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        assert_eq!(notification("hi")["params"]["content"], "hi");
    }

    #[test]
    fn only_listens_when_started_as_a_channel() {
        let args = |line: &str| line.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert!(loaded_as_channel(&args("claude --dangerously-load-development-channels plugin:agent-room@agent-room")));
        assert!(loaded_as_channel(&args("claude --channels plugin:agent-room@agent-room")));
        assert!(!loaded_as_channel(&args("claude")));
        assert!(!loaded_as_channel(&args("claude --dangerously-load-development-channels server:webhook")));
    }
}
