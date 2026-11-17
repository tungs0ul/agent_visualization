//! Shared types for Agent Room.
//!
//! - [`HookEnvelope`] / [`PermissionReply`]: what `room-hook` and the app send over the
//!   Unix socket (one JSON line each way, see PLAN.md 3.5).
//! - [`event`]: raw Claude Code hook JSON parsed into typed [`event::RoomEvent`]s.
//! - [`describe`]: short human text for a tool call, used for speech bubbles.

pub mod describe;
pub mod event;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u32 = 1;
pub const SOCKET_NAME: &str = "room.sock";
pub const LOCK_NAME: &str = "app.lock";
/// Claude's multiple-choice question tool.
pub const QUESTION_TOOL: &str = "AskUserQuestion";

const DEFAULT_PERMISSION_TIMEOUT: Duration = Duration::from_secs(60);

/// How long `room-hook` waits for an answer before Claude Code falls back to its
/// terminal prompt: `AGENT_ROOM_PERMISSION_TIMEOUT` seconds, default 60. The app uses the
/// same value for its countdown. Must stay well below Claude Code's hook timeout (600 s).
pub fn permission_timeout() -> Duration {
    std::env::var("AGENT_ROOM_PERMISSION_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&secs| secs > 0)
        .map_or(DEFAULT_PERMISSION_TIMEOUT, Duration::from_secs)
}

/// `~/.agent-room`, or `$AGENT_ROOM_DIR` when set (tests, multiple dev instances).
pub fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("AGENT_ROOM_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".agent-room"))
}

/// Added by `room-hook` to `SessionStart` / `UserPromptSubmit` payloads of sessions
/// started with `claude -p` by other tools (not by the app).
pub const HEADLESS_FIELD: &str = "agent_room_headless";

/// Added by `room-hook` to the same payloads: the pid of the `claude` process that runs
/// the session, so the app can find the session's channel (PLAN.md M8b).
pub const CLAUDE_PID_FIELD: &str = "agent_room_claude_pid";

/// First line `room-hook --channel` sends on the socket (instead of a `HookEnvelope`).
/// The connection then stays open and the app writes [`ChannelMessage`] lines to it,
/// one per message the user sends to that terminal session (PLAN.md M8b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelHello {
    pub channel: ChannelInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// The `claude` process that started the channel server.
    pub claude_pid: u32,
    /// The session it started in; `/clear` and `/resume` change the session but not the pid.
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub text: String,
}

/// One hook event, sent by `room-hook` to the app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookEnvelope {
    pub v: u32,
    /// True only for `PermissionRequest`: the hook waits for a [`PermissionReply`].
    pub needs_reply: bool,
    /// The raw hook JSON exactly as Claude Code passed it. Parsed by the app, so a
    /// Claude Code schema change only needs an app update.
    pub hook: Value,
}

impl HookEnvelope {
    pub fn new(hook: Value) -> Self {
        let needs_reply = needs_reply(&hook);
        Self { v: PROTOCOL_VERSION, needs_reply, hook }
    }
}

/// The hook waits for the user's answer to permission requests and to Claude's
/// multiple-choice questions (`AskUserQuestion`, answered from its `PreToolUse` hook).
fn needs_reply(hook: &Value) -> bool {
    let field = |key: &str| hook.get(key).and_then(Value::as_str);
    match field("hook_event_name") {
        Some("PermissionRequest") => true,
        Some("PreToolUse") => field("tool_name") == Some(QUESTION_TOOL),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

/// The user's answer to a permission request, sent by the app back to `room-hook`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionReply {
    pub decision: Decision,
    /// Shown to Claude on deny. Ignored on allow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// For `AskUserQuestion`: question text ⇒ chosen label(s), comma-separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<serde_json::Map<String, Value>>,
}

impl PermissionReply {
    /// The JSON Claude Code expects on stdout from the hook that asked: answers for an
    /// `AskUserQuestion` `PreToolUse` hook, a decision for a `PermissionRequest` hook.
    pub fn hook_output(&self, hook: &Value) -> Value {
        if hook.get("hook_event_name").and_then(Value::as_str) == Some("PreToolUse") {
            return self.question_output(hook);
        }
        self.permission_output()
    }

    /// Echo the questions back with the answers added (`updatedInput` replaces the
    /// whole input), so the question is answered without the terminal prompt.
    fn question_output(&self, hook: &Value) -> Value {
        let mut input = hook.get("tool_input").cloned().unwrap_or_else(|| json!({}));
        if let (Some(fields), Some(answers)) = (input.as_object_mut(), &self.answers) {
            fields.insert("answers".into(), Value::Object(answers.clone()));
        }
        match self.decision {
            Decision::Allow => json!({
                "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": input }
            }),
            Decision::Deny => json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": self.message.clone().unwrap_or_default(),
                }
            }),
        }
    }

    fn permission_output(&self) -> Value {
        let decision = match (self.decision, &self.message) {
            (Decision::Allow, _) => json!({ "behavior": "allow" }),
            (Decision::Deny, Some(message)) => json!({ "behavior": "deny", "message": message }),
            (Decision::Deny, None) => json!({ "behavior": "deny" }),
        };
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": decision,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_needs_reply_only_for_permission_requests() {
        assert!(HookEnvelope::new(json!({ "hook_event_name": "PermissionRequest" })).needs_reply);
        assert!(!HookEnvelope::new(json!({ "hook_event_name": "PreToolUse" })).needs_reply);
        assert!(!HookEnvelope::new(json!({})).needs_reply);
    }

    #[test]
    fn allow_output_has_no_message() {
        let reply = PermissionReply { decision: Decision::Allow, message: Some("ignored".into()), answers: None };
        assert_eq!(
            reply.hook_output(&json!({ "hook_event_name": "PermissionRequest" })),
            json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } })
        );
    }

    #[test]
    fn deny_output_carries_message() {
        let reply = PermissionReply { decision: Decision::Deny, message: Some("Not now".into()), answers: None };
        assert_eq!(reply.hook_output(&json!({ "hook_event_name": "PermissionRequest" }))["hookSpecificOutput"]["decision"], json!({ "behavior": "deny", "message": "Not now" }));
    }

    #[test]
    fn questions_wait_for_a_reply_and_get_their_answers() {
        let hook = json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": { "questions": [{ "question": "Which?", "header": "H", "options": [{ "label": "A" }] }] }
        });
        assert!(HookEnvelope::new(hook.clone()).needs_reply);
        assert!(!HookEnvelope::new(json!({ "hook_event_name": "PreToolUse", "tool_name": "Read" })).needs_reply);

        let mut answers = serde_json::Map::new();
        answers.insert("Which?".into(), json!("A"));
        let reply = PermissionReply { decision: Decision::Allow, message: None, answers: Some(answers) };
        let out = reply.hook_output(&hook)["hookSpecificOutput"].clone();
        assert_eq!(out["permissionDecision"], "allow");
        assert_eq!(out["updatedInput"]["answers"], json!({ "Which?": "A" }));
        assert_eq!(out["updatedInput"]["questions"][0]["question"], "Which?");
    }

    #[test]
    fn reply_round_trips_through_json() {
        let line = r#"{"decision":"deny","message":"no"}"#;
        let reply: PermissionReply = serde_json::from_str(line).unwrap();
        assert_eq!(reply, PermissionReply { decision: Decision::Deny, message: Some("no".into()), answers: None });
        let reply: PermissionReply = serde_json::from_str(r#"{"decision":"allow"}"#).unwrap();
        assert_eq!(serde_json::to_string(&reply).unwrap(), r#"{"decision":"allow"}"#);
    }
}
