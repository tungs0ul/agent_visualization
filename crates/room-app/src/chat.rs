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

use room_proto::event::ToolCall;

use crate::model::SessionState;
use crate::server::{Channel, Waker};

/// One line of a chat.
#[derive(Clone, PartialEq, Debug)]
pub enum Entry {
    You(String),
    /// Sent, but Claude hasn't taken it yet (it is busy): drawn dimmed, as in the
    /// terminal. Becomes `You` when Claude takes it, see [`track_pickup`].
    Queued(String),
    Claude(String),
    /// A tool Claude used: "Read main.rs", and how it went.
    Tool { id: String, label: String, step: Step },
    Note(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Running,
    Done,
    Failed,
}

/// Claude Code's permission modes the chat offers (`set_permission_mode`).
pub const MODES: [(&str, &str); 4] = [
    ("default", "Ask before acting"),
    ("acceptEdits", "Accept edits"),
    ("plan", "Plan only"),
    ("auto", "Auto (a classifier decides)"),
];

/// Models the chat offers (`set_model`); Claude Code resolves the alias.
pub const MODELS: [&str; 3] = ["opus", "sonnet", "haiku"];

/// Entries kept from a resumed session's history.
const HISTORY: usize = 120;

pub struct ChatSession {
    pub cwd: String,
    pub transcript: Vec<Entry>,
    /// Claude's answer as it streams in, until the whole message arrives.
    pub streaming: String,
    /// A turn is running: Claude hasn't sent its `result` yet.
    pub busy: bool,
    pub session_id: Option<String>,
    /// The model Claude Code reports (e.g. `claude-opus-5-5`).
    pub model: String,
    pub mode: &'static str,
    /// The usage limits Claude Code last reported (PLAN.md M10), for `Usage`.
    pub limits: Option<room_proto::limits::Limits>,
    /// The slash commands and skills this session offers (its `init` event), for `/`.
    pub commands: Vec<String>,
    /// Claude took a message during the running turn (see [`track_pickup`]).
    taken_this_turn: bool,
    requests: u32,
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
            // Claude's answer arrives as it is written, not only when it is done.
            .arg("--include-partial-messages")
            // Echoes each message once Claude takes it, so a queued one can show as such.
            .arg("--replay-user-messages")
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
        if let Some(id) = resume {
            transcript = history(&transcript_path(cwd, id));
            transcript.push(Entry::Note("Resumed: Claude remembers this session.".to_owned()));
        }
        let mut session = Self {
            cwd: cwd.to_owned(),
            transcript,
            streaming: String::new(),
            busy: false,
            session_id: resume.map(str::to_owned),
            model: String::new(),
            mode: "default",
            limits: None,
            commands: Vec::new(),
            taken_this_turn: false,
            requests: 0,
            child,
            stdin,
            lines,
        };
        // Ask for the slash commands now: the `init` event that also lists them only
        // comes with the first turn, and `/` should work before that.
        session.control(json!({ "subtype": "initialize" }));
        Ok(session)
    }

    fn write(&mut self, message: &Value) -> bool {
        writeln!(self.stdin, "{message}").and_then(|()| self.stdin.flush()).is_ok()
    }

    pub fn send(&mut self, text: &str) {
        self.send_with(text, &[]);
    }

    /// A message with images (PLAN.md M24): image blocks first, then the text.
    pub fn send_with(&mut self, text: &str, images: &[crate::compose::Attachment]) {
        let content = if images.is_empty() {
            json!(text)
        } else {
            let mut blocks: Vec<Value> = images.iter().map(crate::compose::Attachment::block).collect();
            if !text.is_empty() {
                blocks.push(json!({ "type": "text", "text": text }));
            }
            Value::Array(blocks)
        };
        if !self.write(&json!({ "type": "user", "message": { "role": "user", "content": content } })) {
            self.transcript.push(Entry::Note("Claude has stopped. Start a new session.".to_owned()));
            return;
        }
        let names: String = images.iter().map(|image| format!("[image: {}]\n", image.name)).collect();
        self.transcript.push(Entry::Queued(format!("{names}{text}").trim_end().to_owned()));
        self.busy = true;
    }

    /// Keeps a fresh command list and caches it on disk for chats not yet started.
    fn take_commands(&mut self, commands: Vec<String>) {
        if !commands.is_empty() && commands != self.commands {
            crate::compose::save_commands(&commands);
            self.commands = commands;
        }
    }

    /// A control request (the Agent SDK's protocol over stdin).
    fn control(&mut self, request: Value) {
        self.requests += 1;
        let id = format!("room-{}", self.requests);
        self.write(&json!({ "type": "control_request", "request_id": id, "request": request }));
    }

    /// Stops the running turn; the session stays.
    pub fn interrupt(&mut self) {
        self.control(json!({ "subtype": "interrupt" }));
    }

    pub fn set_model(&mut self, model: &str) {
        self.control(json!({ "subtype": "set_model", "model": model }));
    }

    pub fn set_mode(&mut self, mode: &'static str) {
        self.control(json!({ "subtype": "set_permission_mode", "mode": mode }));
        self.mode = mode;
    }

    /// The folder's name, for its tab.
    /// Its `claude` process, so the room sees the session end when the tab closes.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn name(&self) -> &str {
        self.cwd.trim_end_matches('/').rsplit('/').next().unwrap_or(&self.cwd)
    }

    /// Applies what Claude printed since the last frame.
    fn read(&mut self) {
        for line in self.lines.try_iter().collect::<Vec<_>>() {
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            self.apply(&event);
        }
    }

    fn apply(&mut self, event: &Value) {
        // Sub-agents' messages carry the tool call that started them: the room shows
        // those agents; the chat follows the main conversation.
        if !event["parent_tool_use_id"].is_null() {
            return;
        }
        track_pickup(&mut self.transcript, &mut self.taken_this_turn, event);
        match event["type"].as_str() {
            Some("stream_event") => {
                let inner = &event["event"];
                match inner["type"].as_str() {
                    Some("message_start") => self.streaming.clear(),
                    Some("content_block_delta") if inner["delta"]["type"] == "text_delta" => {
                        self.streaming.push_str(inner["delta"]["text"].as_str().unwrap_or_default());
                    }
                    _ => {}
                }
            }
            Some("assistant") => {
                self.streaming.clear();
                for block in event["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => {
                            let text = block["text"].as_str().unwrap_or_default();
                            if !text.trim().is_empty() {
                                self.transcript.push(Entry::Claude(text.to_owned()));
                            }
                        }
                        Some("tool_use") => self.transcript.push(tool_entry(block)),
                        _ => {}
                    }
                }
            }
            Some("user") => {
                for block in event["message"]["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "tool_result") {
                    let id = block["tool_use_id"].as_str().unwrap_or_default();
                    let failed = block["is_error"] == true;
                    if let Some(Entry::Tool { step, .. }) =
                        self.transcript.iter_mut().rev().find(|entry| matches!(entry, Entry::Tool { id: tool, .. } if tool == id))
                    {
                        *step = if failed { Step::Failed } else { Step::Done };
                    }
                }
            }
            Some("system") if event["subtype"] == "init" => {
                self.model = event["model"].as_str().unwrap_or_default().to_owned();
                let commands: Vec<String> =
                    event["slash_commands"].as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect();
                self.take_commands(commands);
            }
            Some("control_response") => {
                let response = &event["response"]["response"];
                if let Some(model) = response["model"].as_str() {
                    self.model = model.to_owned();
                }
                // The `initialize` answer: `commands` as `{name, description, ...}`.
                let names: Vec<String> = response["commands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|command| command["name"].as_str().map(str::to_owned))
                    .collect();
                self.take_commands(names);
                if event["response"]["subtype"] == "error" {
                    let error = event["response"]["error"].as_str().unwrap_or("refused");
                    self.transcript.push(Entry::Note(format!("Claude Code said no: {error}")));
                }
            }
            Some("rate_limit_event") => self.limits = room_proto::limits::Limits::from_stream(event).or(self.limits),
            Some("result") => {
                self.busy = false;
                self.streaming.clear();
                // Tools still running when a turn ends were stopped.
                for entry in &mut self.transcript {
                    if let Entry::Tool { step: step @ Step::Running, .. } = entry {
                        *step = Step::Failed;
                    }
                }
                if event["is_error"] == true && event["subtype"] != "success" {
                    self.transcript.push(Entry::Note("The turn ended early.".to_owned()));
                }
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

/// "Read main.rs", from a `tool_use` block.
fn tool_entry(block: &Value) -> Entry {
    let call = ToolCall { name: block["name"].as_str().unwrap_or("tool").to_owned(), input: block["input"].clone() };
    Entry::Tool { id: block["id"].as_str().unwrap_or_default().to_owned(), label: call.describe().1, step: Step::Running }
}

/// Where Claude Code keeps a folder's session (see [`past_sessions`]).
fn transcript_path(folder: &str, id: &str) -> std::path::PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let slug: String = folder.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    std::path::Path::new(&home).join(".claude").join("projects").join(slug).join(format!("{id}.jsonl"))
}

/// The conversation so far, from a session's transcript: what the user typed,
/// Claude's text and its tool steps, the last [`HISTORY`] entries.
fn history(path: &std::path::Path) -> Vec<Entry> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        if entry["isSidechain"] == true || entry["isMeta"] == true {
            continue;
        }
        let content = &entry["message"]["content"];
        match (entry["type"].as_str(), content) {
            (Some("user"), Value::String(text)) if !text.trim_start().starts_with('<') => entries.push(Entry::You(text.clone())),
            (Some("user"), Value::Array(blocks)) => {
                for block in blocks {
                    if block["type"] == "text"
                        && let Some(text) = block["text"].as_str().filter(|text| !text.trim_start().starts_with('<'))
                    {
                        entries.push(Entry::You(text.to_owned()));
                    }
                    if block["type"] == "tool_result"
                        && let Some(Entry::Tool { step, .. }) = entries.iter_mut().rev().find(
                            |e| matches!(e, Entry::Tool { id, .. } if Some(id.as_str()) == block["tool_use_id"].as_str()),
                        )
                    {
                        *step = if block["is_error"] == true { Step::Failed } else { Step::Done };
                    }
                }
            }
            (Some("assistant"), Value::Array(blocks)) => {
                for block in blocks {
                    match block["type"].as_str() {
                        Some("text") if block["text"].as_str().is_some_and(|t| !t.trim().is_empty()) => {
                            entries.push(Entry::Claude(block["text"].as_str().unwrap_or_default().to_owned()));
                        }
                        Some("tool_use") => entries.push(tool_entry(block)),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    // A step without a recorded result finished long ago.
    for entry in &mut entries {
        if let Entry::Tool { step: step @ Step::Running, .. } = entry {
            *step = Step::Done;
        }
    }
    let skip = entries.len().saturating_sub(HISTORY);
    entries.split_off(skip)
}

impl Drop for ChatSession {
    /// Never leave a Claude process behind (AGENTS.md).
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The app's own sessions, one tab each.
#[derive(Resource, Default)]
pub struct Chats {
    pub sessions: Vec<ChatSession>,
    /// The tab shown; `sessions.len()` shows the chooser for a new one.
    pub active: usize,
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
    /// The active session's folder's files, for `@` suggestions (PLAN.md M24).
    pub files: crate::compose::FileIndex,
    /// Images dropped on the window, sent with the next message.
    pub attachments: Vec<crate::compose::Attachment>,
    /// Slash commands from the last session that listed them (read once from disk),
    /// for `/` before the open chat's own session has started.
    pub known_commands: Option<Vec<String>>,
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
        if self.session().is_none()
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

    /// The tab shown, unless the chooser for a new one is.
    pub fn session(&self) -> Option<&ChatSession> {
        self.sessions.get(self.active)
    }

    pub fn session_mut(&mut self) -> Option<&mut ChatSession> {
        self.sessions.get_mut(self.active)
    }

    /// Ends the shown tab's session (its process stops).
    pub fn close_active(&mut self) {
        if self.active < self.sessions.len() {
            self.sessions.remove(self.active);
            self.active = self.active.min(self.sessions.len().saturating_sub(1));
        }
    }

    fn launch(&mut self, resume: Option<&str>, waker: Waker) {
        match ChatSession::start(self.folder.trim(), resume, waker) {
            Ok(session) => {
                self.sessions.push(session);
                self.active = self.sessions.len() - 1;
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
    for session in &mut chats.sessions {
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
    if let Some(session) = chats.session_mut() {
        session.send(message);
    }
}

/// Marks the oldest queued message as taken when Claude takes one. Claude Code echoes
/// a message (`isReplay`) when it reads it: at once when idle, or mid-turn after the
/// running tool when busy. Local commands such as `/context` are never echoed, but a
/// turn always takes at least one message, so a turn that ends without an echo took one.
fn track_pickup(transcript: &mut [Entry], taken_this_turn: &mut bool, event: &Value) {
    let taken = match event["type"].as_str() {
        Some("user") if event["isReplay"] == true => {
            *taken_this_turn = true;
            true
        }
        Some("result") => !std::mem::take(taken_this_turn),
        _ => false,
    };
    if taken && let Some(entry) = transcript.iter_mut().find(|entry| matches!(entry, Entry::Queued(_))) {
        let Entry::Queued(text) = entry else { return };
        *entry = Entry::You(std::mem::take(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_stays_queued_until_claude_takes_it() {
        let mut transcript = vec![Entry::Queued("run it".into()), Entry::Queued("also this".into()), Entry::Queued("/context".into())];
        let mut taken = false;
        let mut feed = |transcript: &mut Vec<Entry>, event: Value| track_pickup(transcript, &mut taken, &event);
        let replay = |text: &str| json!({ "type": "user", "isReplay": true, "message": { "content": text } });
        let tool_result = json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "t" }] } });
        // Order measured from `claude -p --replay-user-messages`: the second message,
        // sent during a tool call, is echoed right after the tool's result.
        feed(&mut transcript, replay("run it"));
        assert_eq!(transcript[0], Entry::You("run it".into()));
        assert_eq!(transcript[1], Entry::Queued("also this".into()));
        feed(&mut transcript, tool_result);
        assert_eq!(transcript[1], Entry::Queued("also this".into()), "a tool result takes nothing");
        feed(&mut transcript, replay("also this"));
        feed(&mut transcript, json!({ "type": "result" }));
        assert_eq!(transcript[1], Entry::You("also this".into()));
        assert_eq!(transcript[2], Entry::Queued("/context".into()), "a turn that echoed took nothing more at its end");
        // `/context` runs as its own turn with no echo.
        feed(&mut transcript, json!({ "type": "result" }));
        assert_eq!(transcript[2], Entry::You("/context".into()));
    }

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

    #[test]
    fn history_shows_prompts_answers_and_tool_steps() {
        let dir = std::env::temp_dir().join(format!("room-history-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.jsonl");
        let lines = [
            json!({ "type": "user", "message": { "content": "fix the bug" } }),
            json!({ "type": "assistant", "message": { "content": [
                { "type": "text", "text": "Looking." },
                { "type": "tool_use", "id": "t1", "name": "Read", "input": { "file_path": "/x/main.rs" } } ] } }),
            json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "is_error": true }] } }),
            json!({ "type": "assistant", "isSidechain": true, "message": { "content": [{ "type": "text", "text": "sub-agent" }] } }),
        ];
        std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let entries = history(&path);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], Entry::You("fix the bug".into()));
        assert!(matches!(&entries[2], Entry::Tool { step: Step::Failed, label, .. } if label.contains("main.rs")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
