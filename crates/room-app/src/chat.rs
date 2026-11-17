//! Sessions started from the app (PLAN.md M8a): Claude Code runs headless as a child
//! process (`claude -p` with stream-json in and out) in a folder you choose, and you
//! chat with it in a panel instead of a terminal. Its hook events reach the room through
//! the installed plugin like any other session.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread;

use bevy::prelude::*;
use crossbeam_channel::{Receiver, unbounded};
use serde_json::{Value, json};

use crate::server::Waker;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    You,
    Claude,
    Note,
}

pub struct ChatSession {
    pub cwd: String,
    pub transcript: Vec<(Speaker, String)>,
    /// A turn is running: Claude hasn't sent its `result` yet.
    pub busy: bool,
    pub session_id: Option<String>,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl ChatSession {
    /// Starts `claude -p` in `cwd`. `AGENT_ROOM_PLUGIN_DIR` adds `--plugin-dir` for
    /// development; normally the installed plugin is used.
    fn start(cwd: &str, waker: Waker) -> Result<Self, String> {
        let mut command = Command::new("claude");
        command
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(plugin) = std::env::var_os("AGENT_ROOM_PLUGIN_DIR") {
            command.arg("--plugin-dir").arg(plugin);
        }
        // Extra arguments, e.g. `AGENT_ROOM_CLAUDE_ARGS="--model sonnet"`.
        if let Ok(extra) = std::env::var("AGENT_ROOM_CLAUDE_ARGS") {
            command.args(extra.split_whitespace());
        }
        let mut child = command.spawn().map_err(|e| format!("starting claude: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (sender, lines) = unbounded();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
                if let Some(wake) = waker.get() {
                    wake();
                }
            }
        });
        Ok(Self { cwd: cwd.to_owned(), transcript: Vec::new(), busy: false, session_id: None, child, stdin, lines })
    }

    pub fn send(&mut self, text: &str) {
        let line = json!({ "type": "user", "message": { "role": "user", "content": text } }).to_string();
        if writeln!(self.stdin, "{line}").and_then(|()| self.stdin.flush()).is_err() {
            self.transcript.push((Speaker::Note, "Claude has stopped. Start a new session.".to_owned()));
            return;
        }
        self.transcript.push((Speaker::You, text.to_owned()));
        self.busy = true;
    }

    /// Applies what Claude printed since the last frame.
    fn read(&mut self) {
        for line in self.lines.try_iter().collect::<Vec<_>>() {
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            match event.get("type").and_then(Value::as_str) {
                Some("assistant") => {
                    let texts = event["message"]["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|block| block["type"] == "text")
                        .map(|block| block["text"].as_str().unwrap_or_default().to_owned());
                    for text in texts.filter(|t| !t.trim().is_empty()) {
                        self.transcript.push((Speaker::Claude, text));
                    }
                }
                Some("result") => {
                    self.busy = false;
                    self.session_id = event["session_id"].as_str().map(str::to_owned);
                }
                _ => {}
            }
        }
    }
}

impl Drop for ChatSession {
    /// Never leave a Claude process behind (AGENTS.md).
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The app's own sessions; one at a time for now.
#[derive(Resource, Default)]
pub struct Chats {
    pub session: Option<ChatSession>,
    /// The chat window is open.
    pub open: bool,
    pub folder: String,
    pub input: String,
    pub error: Option<String>,
}

impl Chats {
    pub fn start(&mut self, waker: Waker) {
        self.session = None;
        match ChatSession::start(self.folder.trim(), waker) {
            Ok(session) => {
                self.session = Some(session);
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
    }
}

pub fn read_chats(mut chats: ResMut<Chats>) {
    if let Some(session) = chats.session.as_mut() {
        session.read();
    }
}

/// The folder field starts at the home directory.
pub fn init_folder(mut chats: ResMut<Chats>) {
    chats.folder = std::env::var("HOME").unwrap_or_default();
}

/// Dev/test only: `AGENT_ROOM_DEV_CHAT="<folder>|<message>"` opens the chat, starts a
/// session in the folder and sends the message, so the flow can be tested without a click.
pub fn dev_chat(mut chats: ResMut<Chats>, server: Res<crate::server::Server>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    let Ok(spec) = std::env::var("AGENT_ROOM_DEV_CHAT") else { return };
    let Some((folder, message)) = spec.split_once('|') else { return };
    chats.open = true;
    chats.folder = folder.to_owned();
    chats.start(server.waker.clone());
    if let Some(session) = chats.session.as_mut() {
        session.send(message);
    }
}
