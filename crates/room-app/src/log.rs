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
        };
        for _ in 0..MAX_LINES + 10 {
            log.push(&event);
        }
        assert_eq!(log.lines.len(), MAX_LINES);
    }
}
