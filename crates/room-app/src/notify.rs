//! Notifications (V2.md phase 4): a small toast for what matters at a glance (an
//! agent joined, finished, failed, needs you, Claude is done) and, unless the user
//! turned sound off, a soft macOS system sound. Sounds play through `afplay`, one
//! short process per sound, so nothing runs between them (an always-open audio
//! stream kept the system's audio service busy).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use room_proto::event::{AgentRef, EventKind, RoomEvent};

use crate::log::agent_label;

/// What happened, as a notification.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Joined,
    Finished,
    Failed,
    Waiting,
    Done,
    /// A commit or a test run succeeded (`wins`).
    Win,
}

impl Kind {
    /// The macOS system sound for it: quiet, distinct.
    fn sound(self) -> &'static str {
        match self {
            Kind::Joined => "/System/Library/Sounds/Pop.aiff",
            Kind::Finished => "/System/Library/Sounds/Glass.aiff",
            Kind::Failed => "/System/Library/Sounds/Basso.aiff",
            Kind::Waiting => "/System/Library/Sounds/Ping.aiff",
            Kind::Done => "/System/Library/Sounds/Purr.aiff",
            Kind::Win => "/System/Library/Sounds/Hero.aiff",
        }
    }
}

/// The notification an event deserves, if any. `project` names the session when
/// more than one is live.
pub fn notice(event: &RoomEvent, project: Option<&str>) -> Option<(Kind, String)> {
    let who = agent_label(&event.agent);
    let sub = matches!(event.agent, AgentRef::Sub { .. });
    let (kind, text) = match &event.kind {
        EventKind::SubagentStarted if sub => (Kind::Joined, format!("{who} joined")),
        EventKind::SubagentStopped { .. } | EventKind::Handback { .. } if sub => {
            (Kind::Finished, format!("{who} handed over its report"))
        }
        EventKind::ToolFailed { tool_name, interrupted: false, .. } => (Kind::Failed, format!("{who}: {tool_name} failed")),
        EventKind::TurnFailed { .. } => (Kind::Failed, format!("{who}: the turn failed")),
        EventKind::PermissionAsked { tool } => (Kind::Waiting, format!("{who} needs your OK: {}", tool.name)),
        EventKind::ToolStarted { tool, .. } if tool.name == room_proto::QUESTION_TOOL => {
            (Kind::Waiting, format!("{who} has a question"))
        }
        EventKind::TurnEnded { .. } if !sub => (Kind::Done, format!("{who} is done")),
        _ => return None,
    };
    Some((kind, match project {
        Some(project) => format!("{project} · {text}"),
        None => text,
    }))
}

/// How long a toast stays.
pub const TOAST_SECS: Duration = Duration::from_secs(5);
/// At most this many at once (the oldest go first).
const MAX_TOASTS: usize = 4;

#[derive(bevy::prelude::Resource, Default)]
pub struct Toasts {
    pub shown: VecDeque<(Kind, String, Instant)>,
}

impl Toasts {
    /// Shows a toast and, if `sound` is on, plays its sound.
    pub fn push(&mut self, kind: Kind, text: String, sound: bool) {
        // The same thing twice in a row (two failures in a burst) shows once.
        if self.shown.back().is_some_and(|(last, last_text, _)| *last == kind && *last_text == text) {
            return;
        }
        self.shown.push_back((kind, text, Instant::now()));
        while self.shown.len() > MAX_TOASTS {
            self.shown.pop_front();
        }
        if sound {
            play(kind.sound());
        }
    }

    /// Drops toasts that are over; true while any is left to show.
    pub fn tidy(&mut self) -> bool {
        self.shown.retain(|(_, _, at)| at.elapsed() < TOAST_SECS);
        !self.shown.is_empty()
    }
}

/// Plays a sound file quietly in the background; the process ends with the sound.
fn play(file: &'static str) {
    std::thread::spawn(move || {
        let _ = std::process::Command::new("afplay").args([file, "-v", "0.35"]).status();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(fields: serde_json::Value) -> RoomEvent {
        let mut hook = json!({ "session_id": "s" });
        hook.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        RoomEvent::parse(&hook).unwrap()
    }

    #[test]
    fn only_what_matters_at_a_glance_notifies() {
        let joined = event(json!({ "hook_event_name": "SubagentStart", "agent_id": "a1b2c3", "agent_type": "Explore" }));
        assert_eq!(notice(&joined, None), Some((Kind::Joined, "Explore a1b2 joined".to_owned())));
        let failed = event(json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": "t", "error": "x" }));
        assert_eq!(notice(&failed, Some("api")), Some((Kind::Failed, "api · Claude: Bash failed".to_owned())));
        let done = event(json!({ "hook_event_name": "Stop" }));
        assert_eq!(notice(&done, None).map(|(kind, _)| kind), Some(Kind::Done));
        // Ordinary work is not news.
        let read = event(json!({ "hook_event_name": "PreToolUse", "tool_name": "Read", "tool_input": {}, "tool_use_id": "t" }));
        assert_eq!(notice(&read, None), None);
    }

    #[test]
    fn a_burst_of_the_same_toast_shows_once() {
        let mut toasts = Toasts::default();
        toasts.push(Kind::Failed, "Claude: Bash failed".into(), false);
        toasts.push(Kind::Failed, "Claude: Bash failed".into(), false);
        toasts.push(Kind::Done, "Claude is done".into(), false);
        assert_eq!(toasts.shown.len(), 2);
    }
}
