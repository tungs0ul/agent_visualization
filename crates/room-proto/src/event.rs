//! Raw Claude Code hook JSON parsed into typed events.
//!
//! Field names and quirks are documented in PLAN.md 3.4. Parsing is lenient: only
//! `session_id` and `hook_event_name` are required, anything else missing falls back to
//! a default, and unknown events become [`EventKind::Unknown`].

use serde_json::Value;

/// Who an event belongs to. Events from inside a sub-agent carry `agent_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentRef {
    Main,
    Sub { id: String, agent_type: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub input: Value,
}

/// A sub-agent launched by the main agent's `Agent` tool (from its `tool_response`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedAgent {
    pub agent_id: String,
    pub description: String,
    /// Background agents report back later through `SubagentHandback`.
    pub background: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    SessionStarted { model: Option<String> },
    SessionEnded { reason: Option<String> },
    /// A prompt typed by the user. Starts a turn.
    UserPrompt { text: String },
    /// A background sub-agent's report delivered to the main agent. Claude Code sends it
    /// as a `UserPromptSubmit` starting with `<agent-message from="<id>">`.
    ReportReceived { from_agent: String },
    ToolStarted { tool_use_id: String, tool: ToolCall },
    ToolFinished { tool_use_id: String, tool_name: String, launched: Option<LaunchedAgent> },
    ToolFailed { tool_use_id: String, tool_name: String, error: String, interrupted: bool },
    /// A sub-agent hands its final report to the main agent (`SubagentHandback` tool).
    Handback { tool_use_id: String, message: String },
    PermissionAsked { tool: ToolCall },
    /// Auto mode refused a tool call. Manual denials fire no event at all.
    PermissionAutoDenied { tool_use_id: String, tool_name: String, reason: String },
    SubagentStarted,
    /// `last_message` is absent when the agent reported through `SubagentHandback`.
    SubagentStopped { last_message: Option<String> },
    Notification { kind: Option<String>, message: String },
    /// The main agent finished its turn.
    TurnEnded { last_message: Option<String> },
    TurnFailed { error: String },
    Unknown { event: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoomEvent {
    pub session_id: String,
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    pub agent: AgentRef,
    pub kind: EventKind,
    /// Started with `claude -p` by another tool ([`crate::HEADLESS_FIELD`]).
    pub headless: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    MissingField(&'static str),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingField(name) => write!(f, "hook JSON has no `{name}`"),
        }
    }
}

impl std::error::Error for ParseError {}

const HANDBACK_TOOL: &str = "SubagentHandback";
const REPORT_PREFIX: &str = "<agent-message from=\"";

impl RoomEvent {
    pub fn parse(hook: &Value) -> Result<Self, ParseError> {
        let session_id = req_str(hook, "session_id")?;
        let event = req_str(hook, "hook_event_name")?;
        let agent = match opt_str(hook, "agent_id") {
            Some(id) => AgentRef::Sub { id, agent_type: opt_str(hook, "agent_type").unwrap_or_default() },
            None => AgentRef::Main,
        };
        let kind = match event.as_str() {
            "SessionStart" => EventKind::SessionStarted { model: opt_str(hook, "model") },
            "SessionEnd" => EventKind::SessionEnded { reason: opt_str(hook, "reason") },
            "UserPromptSubmit" => {
                let text = str_or_empty(hook, "prompt");
                match report_sender(&text) {
                    Some(from_agent) => EventKind::ReportReceived { from_agent },
                    None => EventKind::UserPrompt { text },
                }
            }
            "PreToolUse" => {
                let tool = tool_call(hook);
                let tool_use_id = str_or_empty(hook, "tool_use_id");
                if tool.name == HANDBACK_TOOL {
                    let message = tool.input.get("message").and_then(Value::as_str).unwrap_or_default().to_owned();
                    EventKind::Handback { tool_use_id, message }
                } else {
                    EventKind::ToolStarted { tool_use_id, tool }
                }
            }
            "PostToolUse" => EventKind::ToolFinished {
                tool_use_id: str_or_empty(hook, "tool_use_id"),
                tool_name: str_or_empty(hook, "tool_name"),
                launched: hook.get("tool_response").and_then(launched_agent),
            },
            "PostToolUseFailure" => EventKind::ToolFailed {
                tool_use_id: str_or_empty(hook, "tool_use_id"),
                tool_name: str_or_empty(hook, "tool_name"),
                error: str_or_empty(hook, "error"),
                interrupted: hook.get("is_interrupt").and_then(Value::as_bool).unwrap_or(false),
            },
            "PermissionRequest" => EventKind::PermissionAsked { tool: tool_call(hook) },
            "PermissionDenied" => EventKind::PermissionAutoDenied {
                tool_use_id: str_or_empty(hook, "tool_use_id"),
                tool_name: str_or_empty(hook, "tool_name"),
                reason: str_or_empty(hook, "reason"),
            },
            "SubagentStart" => EventKind::SubagentStarted,
            "SubagentStop" => EventKind::SubagentStopped { last_message: opt_str(hook, "last_assistant_message") },
            "Notification" => EventKind::Notification {
                kind: opt_str(hook, "notification_type"),
                message: str_or_empty(hook, "message"),
            },
            "Stop" => EventKind::TurnEnded { last_message: opt_str(hook, "last_assistant_message") },
            "StopFailure" => EventKind::TurnFailed {
                error: opt_str(hook, "last_assistant_message").unwrap_or_else(|| str_or_empty(hook, "error")),
            },
            _ => EventKind::Unknown { event },
        };
        Ok(Self {
            session_id,
            cwd: opt_str(hook, "cwd"),
            transcript_path: opt_str(hook, "transcript_path"),
            agent,
            kind,
            headless: hook.get(crate::HEADLESS_FIELD).and_then(Value::as_bool).unwrap_or(false),
        })
    }
}

/// `<agent-message from="a7f0fb44b80dc8b7f">…` ⇒ `a7f0fb44b80dc8b7f`.
fn report_sender(prompt: &str) -> Option<String> {
    let rest = prompt.trim_start().strip_prefix(REPORT_PREFIX)?;
    let id = &rest[..rest.find('"')?];
    (!id.is_empty()).then(|| id.to_owned())
}

fn launched_agent(response: &Value) -> Option<LaunchedAgent> {
    let agent_id = response.get("agentId")?.as_str()?.to_owned();
    Some(LaunchedAgent {
        agent_id,
        description: response.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
        background: response.get("isAsync").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn tool_call(hook: &Value) -> ToolCall {
    ToolCall {
        name: str_or_empty(hook, "tool_name"),
        input: hook.get("tool_input").cloned().unwrap_or(Value::Null),
    }
}

fn req_str(hook: &Value, key: &'static str) -> Result<String, ParseError> {
    opt_str(hook, key).ok_or(ParseError::MissingField(key))
}

fn opt_str(hook: &Value, key: &str) -> Option<String> {
    hook.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn str_or_empty(hook: &Value, key: &str) -> String {
    opt_str(hook, key).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn requires_session_and_event_name() {
        assert_eq!(RoomEvent::parse(&json!({ "hook_event_name": "Stop" })), Err(ParseError::MissingField("session_id")));
        assert_eq!(RoomEvent::parse(&json!({ "session_id": "s" })), Err(ParseError::MissingField("hook_event_name")));
    }

    #[test]
    fn unknown_event_is_kept() {
        let event = RoomEvent::parse(&json!({ "session_id": "s", "hook_event_name": "PreCompact" })).unwrap();
        assert_eq!(event.kind, EventKind::Unknown { event: "PreCompact".into() });
        assert_eq!(event.agent, AgentRef::Main);
    }

    #[test]
    fn report_sender_needs_the_exact_prefix() {
        assert_eq!(report_sender("<agent-message from=\"abc\">\nhi"), Some("abc".into()));
        assert_eq!(report_sender("please read <agent-message from=\"abc\">"), None);
        assert_eq!(report_sender("<agent-message from=\"\">"), None);
        assert_eq!(report_sender("<agent-message from=\"abc"), None);
    }
}
