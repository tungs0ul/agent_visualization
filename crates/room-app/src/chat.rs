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

use crate::model::SessionState;
use crate::server::{Channel, Waker};

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
    /// The usage limits Claude Code last reported (PLAN.md M10), for `Usage`.
    pub limits: Option<room_proto::limits::Limits>,
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl ChatSession {
    /// Starts `claude -p` in `cwd`; with `resume`, continues that session (same id, its
    /// history included). `AGENT_ROOM_PLUGIN_DIR` adds `--plugin-dir` for development;
    /// normally the installed plugin is used.
    fn start(cwd: &str, resume: Option<&str>, waker: Waker) -> Result<Self, String> {
        let mut command = Command::new("claude");
        command
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            .current_dir(cwd)
            // Tells room-hook this `-p` session is the user's, not another tool's helper.
            .env("AGENT_ROOM_CHAT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(id) = resume {
            command.args(["--resume", id]);
        }
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
        let mut transcript = Vec::new();
        if resume.is_some() {
            transcript.push((Speaker::Note, "Resumed: Claude remembers this session's conversation.".to_owned()));
        }
        Ok(Self {
            cwd: cwd.to_owned(),
            transcript,
            busy: false,
            session_id: resume.map(str::to_owned),
            limits: None,
            child,
            stdin,
            lines,
        })
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
                Some("rate_limit_event") => self.limits = room_proto::limits::Limits::from_stream(&event).or(self.limits),
                Some("result") => {
                    self.busy = false;
                    let id = event["session_id"].as_str().map(str::to_owned);
                    if id.is_some() && id != self.session_id
                        && let Some(id) = &id
                    {
                        remember_chat_session(id);
                    }
                    self.session_id = id;
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
    pub folder: String,
    pub input: String,
    pub error: Option<String>,
    /// Earlier sessions in `past_folder`, newest first (read when the window opens or
    /// the folder changes).
    pub past: Vec<PastSession>,
    past_folder: String,
    /// Channels of sessions open in a terminal (PLAN.md M8b).
    channels: Vec<Channel>,
    /// What the user sent to terminal sessions, by session id, oldest first.
    pub sent: Vec<(String, String)>,
}

/// A session Claude Code saved on disk, which `claude --resume <id>` continues.
pub struct PastSession {
    pub id: String,
    /// The first thing the user typed, to recognise it.
    pub title: String,
    pub modified: std::time::SystemTime,
}

impl Chats {
    /// Points a new chat at the folder of the session the room shows.
    pub fn open_for(&mut self, cwd: Option<&str>) {
        if self.session.is_none()
            && let Some(cwd) = cwd
        {
            self.folder = cwd.to_owned();
        }
    }

    /// Re-reads the folder's saved sessions when the folder changed.
    pub fn refresh_past(&mut self) {
        let folder = self.folder.trim();
        if folder != self.past_folder {
            self.past = past_sessions(folder, PAST_SESSIONS);
            self.past_folder = folder.to_owned();
        }
    }

    pub fn start(&mut self, waker: Waker) {
        self.launch(None, waker);
    }

    pub fn resume(&mut self, id: &str, waker: Waker) {
        self.launch(Some(id), waker);
    }

    /// The session has a channel the app can type into: it was started with
    /// `--dangerously-load-development-channels plugin:agent-room@agent-room`.
    pub fn has_channel(&self, session: &SessionState) -> bool {
        self.channel_index(session).is_some()
    }

    fn channel_index(&self, session: &SessionState) -> Option<usize> {
        // `/clear` and `/resume` start a new session in the same process: match the
        // process first, the session id the channel started in second.
        self.channels
            .iter()
            .position(|c| session.claude_pid == Some(c.info.claude_pid))
            .or_else(|| self.channels.iter().position(|c| c.info.session_id.as_deref() == Some(session.id.as_str())))
    }

    /// Sends what the user typed to a terminal session, as if typed there.
    pub fn send_to_terminal(&mut self, session: &SessionState, text: &str) {
        let Some(index) = self.channel_index(session) else { return };
        if self.channels[index].send(text) {
            self.sent.push((session.id.clone(), text.to_owned()));
            self.error = None;
        } else {
            self.channels.remove(index);
            self.error = Some("That session's channel has closed.".to_owned());
        }
    }

    fn launch(&mut self, resume: Option<&str>, waker: Waker) {
        self.session = None;
        match ChatSession::start(self.folder.trim(), resume, waker) {
            Ok(session) => {
                self.session = Some(session);
                self.error = None;
            }
            Err(err) => self.error = Some(err),
        }
    }
}

/// Ids of sessions started from the app, so they can be resumed later although
/// Claude Code saves them as headless (`sdk-cli`) sessions, like other tools' helpers.
fn chat_sessions_file() -> Option<std::path::PathBuf> {
    room_proto::state_dir().map(|dir| dir.join("chat-sessions.txt"))
}

fn remember_chat_session(id: &str) {
    let Some(path) = chat_sessions_file() else { return };
    let known = std::fs::read_to_string(&path).unwrap_or_default();
    if known.lines().any(|line| line == id) {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{id}");
    }
}

/// How many saved sessions the chat window offers to resume.
const PAST_SESSIONS: usize = 8;

/// Claude Code keeps each folder's transcripts in `~/.claude/projects/<folder with
/// every non-alphanumeric character replaced by '-'>/<session id>.jsonl`.
fn past_sessions(folder: &str, limit: usize) -> Vec<PastSession> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let slug: String = folder.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let dir = std::path::Path::new(&home).join(".claude").join("projects").join(slug);
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let chats = chat_sessions_file().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    files
        .into_iter()
        .filter_map(|(modified, path)| {
            let id = path.file_stem()?.to_str()?.to_owned();
            let (title, headless) = first_prompt(&path)?;
            // Other tools' `claude -p` helpers (summaries etc.) aren't worth resuming.
            if headless && !chats.lines().any(|line| line == id) {
                return None;
            }
            Some(PastSession { id, title, modified })
        })
        .take(limit)
        .collect()
}

/// The first prompt the user typed in a transcript, and whether the session ran
/// headless (`claude -p`, entrypoint `sdk-cli`); `None` if there is no prompt (e.g. a
/// session that was opened and closed without a message).
fn first_prompt(path: &std::path::Path) -> Option<(String, bool)> {
    let file = std::fs::File::open(path).ok()?;
    BufReader::new(file).lines().map_while(Result::ok).take(400).find_map(|line| {
        let entry: Value = serde_json::from_str(&line).ok()?;
        if entry["type"] != "user" || entry["isMeta"] == true {
            return None;
        }
        let headless = entry["entrypoint"] == "sdk-cli";
        let content = &entry["message"]["content"];
        let text = match content {
            Value::String(text) => text.clone(),
            Value::Array(blocks) => blocks.iter().find(|b| b["type"] == "text")?["text"].as_str()?.to_owned(),
            _ => return None,
        };
        let text = text.trim();
        // Slash commands, hook output and other wrappers start with a tag.
        (!text.is_empty() && !text.starts_with('<')).then(|| (room_proto::describe::truncate(text, 70), headless))
    })
}

pub fn read_chats(
    mut chats: ResMut<Chats>,
    server: Res<crate::server::Server>,
    mut usage: ResMut<crate::usage::Usage>,
    open: Res<crate::console::OpenConsole>,
) {
    if let Some(session) = chats.session.as_mut() {
        session.read();
        if let Some(limits) = session.limits.take() {
            usage.report_limits(limits);
        }
    }
    let new: Vec<Channel> = server.channels.try_iter().collect();
    for channel in new {
        // A reconnect after the app restarted, or a process reusing a pid: keep the newest.
        chats.channels.retain(|old| old.info.claude_pid != channel.info.claude_pid);
        chats.channels.push(channel);
    }
    // Only while the window is open, so the closed app costs nothing per frame.
    if open.is(crate::console::Console::Chat) {
        chats.channels.retain(|channel| !channel.is_closed());
    }
}

/// The folder field starts at the home directory.
pub fn init_folder(mut chats: ResMut<Chats>) {
    chats.folder = std::env::var("HOME").unwrap_or_default();
}

/// Dev/test only: `AGENT_ROOM_DEV_CHAT="<folder>|<message>"` opens the chat, starts a
/// session in the folder and sends the message, so the flow can be tested without a click.
/// With an empty message it only opens the window (to check the session chooser).
pub fn dev_chat(
    mut chats: ResMut<Chats>,
    server: Res<crate::server::Server>,
    mut open: ResMut<crate::console::OpenConsole>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    *done = true;
    let Ok(spec) = std::env::var("AGENT_ROOM_DEV_CHAT") else { return };
    let Some((folder, message)) = spec.split_once('|') else { return };
    open.0 = Some(crate::console::Console::Chat);
    if message.is_empty() {
        chats.open_for(Some(folder));
        return;
    }
    chats.folder = folder.to_owned();
    chats.start(server.waker.clone());
    if let Some(session) = chats.session.as_mut() {
        session.send(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_prompt_skips_wrappers_and_flags_headless() {
        let dir = std::env::temp_dir().join(format!("room-chat-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let lines = [
            json!({ "type": "user", "isMeta": true, "message": { "content": "meta" } }),
            json!({ "type": "user", "message": { "content": "<command-name>/clear</command-name>" } }),
            json!({ "type": "assistant", "message": { "content": [] } }),
            json!({ "type": "user", "entrypoint": "sdk-cli", "message": { "content": [{ "type": "text", "text": " fix it " }] } }),
        ];
        std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        assert_eq!(first_prompt(&path), Some(("fix it".to_owned(), true)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
