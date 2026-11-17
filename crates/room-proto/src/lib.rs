//! Shared types for Agent Room.
//!
//! - [`HookEnvelope`] / [`PermissionReply`]: what `room-hook` and the app send over the
//!   Unix socket (one JSON line each way, see PLAN.md 3.5).
//! - [`event`]: raw Claude Code hook JSON parsed into typed [`event::RoomEvent`]s.
//! - [`describe`]: short human text for a tool call, used for speech bubbles.

pub mod describe;
pub mod event;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const PROTOCOL_VERSION: u32 = 1;
pub const SOCKET_NAME: &str = "room.sock";

/// `~/.agent-room`, or `$AGENT_ROOM_DIR` when set (tests, multiple dev instances).
pub fn state_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("AGENT_ROOM_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".agent-room"))
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
        let needs_reply = hook.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest");
        Self { v: PROTOCOL_VERSION, needs_reply, hook }
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
}

impl PermissionReply {
    /// The JSON Claude Code expects on stdout from a `PermissionRequest` hook.
    pub fn hook_output(&self) -> Value {
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
        let reply = PermissionReply { decision: Decision::Allow, message: Some("ignored".into()) };
        assert_eq!(
            reply.hook_output(),
            json!({ "hookSpecificOutput": { "hookEventName": "PermissionRequest", "decision": { "behavior": "allow" } } })
        );
    }

    #[test]
    fn deny_output_carries_message() {
        let reply = PermissionReply { decision: Decision::Deny, message: Some("Not now".into()) };
        assert_eq!(reply.hook_output()["hookSpecificOutput"]["decision"], json!({ "behavior": "deny", "message": "Not now" }));
    }

    #[test]
    fn reply_round_trips_through_json() {
        let line = r#"{"decision":"deny","message":"no"}"#;
        let reply: PermissionReply = serde_json::from_str(line).unwrap();
        assert_eq!(reply, PermissionReply { decision: Decision::Deny, message: Some("no".into()) });
        let reply: PermissionReply = serde_json::from_str(r#"{"decision":"allow"}"#).unwrap();
        assert_eq!(serde_json::to_string(&reply).unwrap(), r#"{"decision":"allow"}"#);
    }
}
