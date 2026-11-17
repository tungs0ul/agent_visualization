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

pub fn always_asks(tool: &ToolCall) -> bool {
    let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) else { return false };
    ALWAYS_ASK.iter().any(|pattern| command.contains(pattern))
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
        self.lines.push_back(LogLine { session: session.to_owned(), agent, text });
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
        let bash = |command: &str| ToolCall { name: "Bash".into(), input: json!({ "command": command }) };
        assert!(always_asks(&bash("rm -rf target")));
        assert!(always_asks(&bash("git push origin main")));
        assert!(!always_asks(&bash("cargo test")));
        assert!(!always_asks(&ToolCall { name: "Edit".into(), input: json!({ "file_path": "a.rs" }) }));
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
        };
        for _ in 0..MAX_LINES + 10 {
            log.push(&event);
        }
        assert_eq!(log.lines.len(), MAX_LINES);
    }
}
