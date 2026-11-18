//! What the app knows so far: the event log and the permission requests waiting for an
//! answer. M3+ adds the room scene on top of the same events.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use room_proto::describe::truncate;
use room_proto::event::{AgentRef, EventKind, RoomEvent, ToolCall};

use crate::server::ReplyHandle;

/// Oldest lines are dropped beyond this.
pub const MAX_LINES: usize = 500;
/// A permission card ignores clicks this long after it appears, so a click meant for
/// something else can't approve it (PLAN.md 7).
pub const INPUT_GUARD: Duration = Duration::from_millis(300);

pub struct LogLine {
    pub session: String,
    pub agent: String,
    pub text: String,
    /// When it happened (the activity flow shows how long ago).
    pub at: Instant,
}

pub struct PendingPermission {
    pub session: String,
    /// Who asks, to show the card next to that character.
    pub who: crate::model::Who,
    pub agent: String,
    pub tool: ToolCall,
    pub cwd: Option<String>,
    pub shown_at: Instant,
    pub expires_at: Instant,
    pub reply: ReplyHandle,
    /// Optional reason typed into the card; sent to Claude on Deny.
    pub reason: String,
}

impl PendingPermission {
    pub fn accepts_input(&self, now: Instant) -> bool {
        now >= self.shown_at + INPUT_GUARD
    }
}

/// The card the Y and N keys answer: the oldest one that accepts input yet.
pub fn keyboard_target(pending: &[PendingPermission], now: Instant) -> Option<usize> {
    pending.first().filter(|card| card.accepts_input(now)).map(|_| 0)
}

#[derive(Resource, Default)]
pub struct RoomLog {
    pub lines: VecDeque<LogLine>,
    pub pending: Vec<PendingPermission>,
    /// Claude's multiple-choice questions waiting for an answer (centre pop-up).
    pub questions: Vec<PendingQuestion>,
    /// Claude's last answer in the shown session. Shown until closed or the next prompt
    /// in that same session; kept so the "Last answer" button can open it again.
    pub final_answer: Option<FinalAnswer>,
}

pub struct FinalAnswer {
    pub session: String,
    pub text: String,
    /// The pop-up is showing it.
    pub open: bool,
}

/// While on, permission requests are allowed without asking, except `always_asks`.
/// The user's explicit choice; off at every start (PLAN.md M9).
#[derive(Resource, Default)]
pub struct AutoAccept(pub bool);

/// Commands that still ask even with auto-accept on: hard to undo or outward-facing.
const ALWAYS_ASK: [&str; 8] =
    ["rm -rf", "rm -fr", "git push", "git reset --hard", "sudo ", "mkfs", "dd if=", "git clean -f"];
/// Of those, the ones that ask even inside a temporary folder (PLAN.md M13).
const ASK_EVEN_IN_TMP: [&str; 2] = ["git push", "sudo "];
/// Shell syntax that makes a command too hard to judge by its words: chaining, pipes,
/// redirects, substitutions, home and parent paths, escapes.
const UNCLEAR_SHELL: [&str; 13] = [";", "&", "|", "<", ">", "`", "$", "~", "..", "\\", "(", "{", "\n"];

/// Auto-accept rules (PLAN.md M13, the user's words: "read, edit is allow, doing
/// anything in tmp is allow, git push need to ask"). Read and Edit tools never match.
pub fn always_asks(tool: &ToolCall, cwd: Option<&str>) -> bool {
    always_asks_with(tool, cwd, &tmp_roots())
}

fn always_asks_with(tool: &ToolCall, cwd: Option<&str>, tmp: &[String]) -> bool {
    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else { return false };
    if !ALWAYS_ASK.iter().any(|pattern| command.contains(pattern)) {
        return false;
    }
    ASK_EVEN_IN_TMP.iter().any(|pattern| command.contains(pattern)) || !only_touches_tmp(command, cwd, tmp)
}

/// The temporary folders: `/tmp`, `$TMPDIR`, and their `/private` real paths on macOS.
fn tmp_roots() -> Vec<String> {
    let mut roots = vec!["/tmp".to_owned(), "/private/tmp".to_owned()];
    if let Ok(dir) = std::env::var("TMPDIR") {
        let dir = dir.trim_end_matches('/').to_owned();
        if dir.starts_with('/') && dir.len() > 1 {
            roots.push(format!("/private{dir}"));
            roots.push(dir);
        }
    }
    roots
}

fn inside(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|root| path.strip_prefix(root.as_str()).is_some_and(|rest| rest.len() > 1 && rest.starts_with('/')))
}

/// True when the command is one simple command whose every argument is a path strictly
/// inside a temporary folder (or a relative one while the working folder is there).
/// Anything unclear counts as outside.
fn only_touches_tmp(command: &str, cwd: Option<&str>, tmp: &[String]) -> bool {
    let mut command = command.to_owned();
    // `tmp_roots` puts `$TMPDIR` itself fourth.
    for (name, roots) in [("${TMPDIR}", tmp.get(3)), ("$TMPDIR", tmp.get(3))] {
        if let Some(root) = roots {
            command = command.replace(name, root);
        }
    }
    if UNCLEAR_SHELL.iter().any(|unclear| command.contains(unclear)) {
        return false;
    }
    let cwd_in_tmp = cwd.is_some_and(|cwd| inside(cwd, tmp) || tmp.iter().any(|root| root == cwd.trim_end_matches('/')));
    let mut words = command.split_whitespace().map(|word| word.trim_matches(|c| c == '"' || c == '\''));
    words.next();
    words.all(|word| {
        let value = word.split_once('=').map_or(word, |(_, value)| value);
        if word.starts_with('-') && !word.contains('=') {
            true
        } else if value.starts_with('/') {
            inside(value, tmp)
        } else {
            cwd_in_tmp && !value.contains('\'') && !value.contains('"')
        }
    })
}

pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

pub struct Question {
    pub text: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi: bool,
    pub chosen: Vec<bool>,
    /// A typed answer instead of the options.
    pub other: String,
}

impl Question {
    /// The answer Claude Code expects: chosen labels joined with commas, or the typed text.
    pub fn answer(&self) -> Option<String> {
        let other = self.other.trim();
        if !other.is_empty() {
            return Some(other.to_owned());
        }
        let labels: Vec<&str> = self
            .options
            .iter()
            .zip(&self.chosen)
            .filter(|(_, chosen)| **chosen)
            .map(|(option, _)| option.label.as_str())
            .collect();
        (!labels.is_empty()).then(|| labels.join(", "))
    }
}

pub struct PendingQuestion {
    pub session: String,
    pub agent: String,
    pub questions: Vec<Question>,
    pub shown_at: Instant,
    pub expires_at: Instant,
    pub reply: ReplyHandle,
}

impl PendingQuestion {
    pub fn accepts_input(&self, now: Instant) -> bool {
        now >= self.shown_at + INPUT_GUARD
    }

    /// Question text ⇒ answer, once every question has one.
    pub fn answers(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.questions.iter().map(|q| Some((q.text.clone(), serde_json::Value::String(q.answer()?)))).collect()
    }
}

/// The questions of an `AskUserQuestion` call.
pub fn parse_questions(input: &serde_json::Value) -> Vec<Question> {
    let text = |v: &serde_json::Value, key: &str| v.get(key).and_then(|s| s.as_str()).unwrap_or_default().to_owned();
    let list = |v: &serde_json::Value, key: &str| v.get(key).and_then(|a| a.as_array()).cloned().unwrap_or_default();
    list(input, "questions")
        .iter()
        .map(|q| {
            let options: Vec<QuestionOption> = list(q, "options")
                .iter()
                .map(|o| QuestionOption { label: text(o, "label"), description: text(o, "description") })
                .collect();
            Question {
                text: text(q, "question"),
                header: text(q, "header"),
                multi: q.get("multiSelect").and_then(|m| m.as_bool()).unwrap_or(false),
                chosen: vec![false; options.len()],
                options,
                other: String::new(),
            }
        })
        .collect()
}

impl RoomLog {
    pub fn push(&mut self, event: &RoomEvent) {
        if let Some(text) = summary(&event.kind) {
            self.note(&event.session_id, agent_label(&event.agent), text);
        }
    }

    pub fn note(&mut self, session: &str, agent: String, text: String) {
        // Mirrors the on-screen log; `RUST_LOG=agent_room=debug` shows it in the terminal.
        debug!("{agent}: {text}");
        self.lines.push_back(LogLine { session: session.to_owned(), agent, text, at: Instant::now() });
        while self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
    }
}

pub fn agent_label(agent: &AgentRef) -> String {
    match agent {
        AgentRef::Main => "Claude".to_owned(),
        AgentRef::Sub { id, agent_type } => {
            let kind = if agent_type.is_empty() { "helper" } else { agent_type };
            let short: String = id.chars().take(4).collect();
            format!("{kind} {short}")
        }
    }
}

/// One log line per event, or `None` for events that only matter to the scene.
pub fn summary(kind: &EventKind) -> Option<String> {
    let text = match kind {
        EventKind::SessionStarted { model } => match model {
            Some(model) => format!("Session started ({model})"),
            None => "Session started".to_owned(),
        },
        EventKind::SessionEnded { .. } => "Session ended".to_owned(),
        EventKind::UserPrompt { text } => format!("You: {}", truncate(text.trim(), 80)),
        EventKind::ReportReceived { from_agent } => {
            format!("Got the report from {}", agent_label(&AgentRef::Sub { id: from_agent.clone(), agent_type: String::new() }))
        }
        EventKind::ToolStarted { tool, .. } => tool.describe().1,
        EventKind::ToolFinished { launched: Some(agent), .. } => format!("Started helper: {}", agent.description),
        EventKind::ToolFinished { .. } => return None,
        EventKind::ToolFailed { tool_name, error, interrupted, .. } => {
            if *interrupted {
                format!("{tool_name} interrupted")
            } else {
                format!("{tool_name} failed: {}", truncate(error, 60))
            }
        }
        EventKind::Handback { message, .. } => format!("Report: {}", truncate(message, 80)),
        EventKind::PermissionAsked { tool } => format!("Needs permission: {}", tool.describe().1),
        EventKind::PermissionAutoDenied { tool_name, .. } => format!("Auto mode denied {tool_name}"),
        EventKind::SubagentStarted => "Joined".to_owned(),
        EventKind::SubagentStopped { last_message: Some(text) } => format!("Done: {}", truncate(text, 80)),
        EventKind::SubagentStopped { last_message: None } => "Done".to_owned(),
        EventKind::Notification { message, .. } => message.clone(),
        EventKind::TurnEnded { last_message } => match last_message {
            Some(text) => format!("Done: {}", truncate(text, 80)),
            None => "Done".to_owned(),
        },
        EventKind::TurnFailed { error } => format!("Stopped: {}", truncate(error, 80)),
        EventKind::Compacting => "Compacting the conversation".to_owned(),
        EventKind::Unknown { event } => event.clone(),
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use room_proto::event::LaunchedAgent;
    use serde_json::json;

    #[test]
    fn auto_accept_still_asks_for_dangerous_commands() {
        let tmp = ["/tmp".to_owned(), "/private/tmp".to_owned(), "/private/var/T".to_owned(), "/var/T".to_owned()];
        let bash = |command: &str| ToolCall { name: "Bash".into(), input: json!({ "command": command }) };
        let asks = |command: &str, cwd: &str| always_asks_with(&bash(command), Some(cwd), &tmp);
        let repo = "/Users/me/repo";
        assert!(asks("rm -rf target", repo));
        assert!(asks("git push origin main", repo));
        assert!(!asks("cargo test", repo));
        assert!(!always_asks_with(&ToolCall { name: "Edit".into(), input: json!({ "file_path": "a.rs" }) }, Some(repo), &tmp));
        // Anything inside a temporary folder is allowed (PLAN.md M13)...
        assert!(!asks("rm -rf /tmp/ar-test", repo));
        assert!(!asks("rm -rf \"$TMPDIR/ar-test\" /private/tmp/x", repo));
        assert!(!asks("rm -rf build", "/tmp/ar-test"));
        assert!(!asks("git reset --hard", "/var/T/clone"));
        // ...but git push still asks, and anything that could reach outside does.
        assert!(asks("git push", "/tmp/ar-test"));
        assert!(asks("sudo rm -rf /tmp/x", repo));
        assert!(asks("rm -rf /tmp", repo));
        assert!(asks("rm -rf /tmp/../Users/me", repo));
        assert!(asks("rm -rf /tmp/x ~", repo));
        assert!(asks("cd /tmp && rm -rf repo", repo));
        assert!(asks("rm -rf /tmp/x; rm -rf build", repo));
        assert!(asks("rm -rf $HOME/x", repo));
        assert!(asks("rm -rf /tmpfoo", repo));
        assert!(asks("rm -rf build", repo));
        assert!(asks("dd if=/tmp/a of=/dev/disk2", repo));
    }

    #[test]
    fn questions_parse_and_answer() {
        let input = json!({ "questions": [
            { "question": "Which?", "header": "Pick", "options": [{ "label": "A" }, { "label": "B" }], "multiSelect": true }
        ] });
        let mut questions = parse_questions(&input);
        assert_eq!(questions[0].options.len(), 2);
        assert_eq!(questions[0].answer(), None);
        questions[0].chosen = vec![true, true];
        assert_eq!(questions[0].answer().as_deref(), Some("A, B"));
        questions[0].other = "neither".into();
        assert_eq!(questions[0].answer().as_deref(), Some("neither"));
    }

    #[test]
    fn agent_labels() {
        assert_eq!(agent_label(&AgentRef::Main), "Claude");
        let sub = AgentRef::Sub { id: "a7f0fb44".into(), agent_type: "Explore".into() };
        assert_eq!(agent_label(&sub), "Explore a7f0");
        assert_eq!(agent_label(&AgentRef::Sub { id: "ab".into(), agent_type: String::new() }), "helper ab");
    }

    #[test]
    fn summaries() {
        let tool = ToolCall { name: "Bash".into(), input: json!({ "command": "ls", "description": "List files" }) };
        assert_eq!(summary(&EventKind::ToolStarted { tool_use_id: "t".into(), tool }).unwrap(), "List files");
        let finished = EventKind::ToolFinished { tool_use_id: "t".into(), tool_name: "Bash".into(), launched: None };
        assert_eq!(summary(&finished), None);
        let launched = LaunchedAgent { agent_id: "a1".into(), description: "Count files".into(), background: true };
        let launch = EventKind::ToolFinished { tool_use_id: "t".into(), tool_name: "Agent".into(), launched: Some(launched) };
        assert_eq!(summary(&launch).unwrap(), "Started helper: Count files");
    }

    #[test]
    fn log_is_capped() {
        let mut log = RoomLog::default();
        let event = RoomEvent {
            session_id: "s1".into(),
            cwd: Some("/tmp".into()),
            transcript_path: None,
            agent: AgentRef::Main,
            kind: EventKind::UserPrompt { text: "hi".into() },
            headless: false,
            claude_pid: None,
        };
        for _ in 0..MAX_LINES + 10 {
            log.push(&event);
        }
        assert_eq!(log.lines.len(), MAX_LINES);
    }
}
