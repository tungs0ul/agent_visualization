//! Small celebrations from real events: a burst of light when a commit or a test
//! run succeeds, and a trophy for every finished turn (click one to read that
//! turn's answer again). Nothing is invented: both come from hook events.

use std::collections::HashMap;
use std::time::Instant;

use bevy::prelude::Resource;
use room_proto::event::{AgentRef, EventKind, RoomEvent};

use crate::model::Who;

/// What succeeded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Win {
    Committed,
    TestsPassed,
}

impl Win {
    pub fn text(self) -> &'static str {
        match self {
            Win::Committed => "committed",
            Win::TestsPassed => "tests passed",
        }
    }
}

/// A finished turn on the shelf.
pub struct Trophy {
    pub session: String,
    pub project: String,
    pub answer: String,
    pub at: Instant,
}

/// The most trophies the shelf holds (the oldest go).
pub const SHELF: usize = 12;

#[derive(Resource, Default)]
pub struct Wins {
    /// Bash commands waiting for their result, by tool call id.
    commands: HashMap<String, String>,
    /// Bursts for the HQ to play: (session, who, what).
    pub bursts: Vec<(String, Who, Win)>,
    pub trophies: Vec<Trophy>,
}

impl Wins {
    /// Follows an event; returns the win it completes, if any.
    pub fn on_event(&mut self, event: &RoomEvent, project: &str) -> Option<Win> {
        match &event.kind {
            EventKind::ToolStarted { tool_use_id, tool } if tool.name == "Bash" => {
                if let Some(command) = tool.input["command"].as_str() {
                    self.commands.insert(tool_use_id.clone(), command.to_owned());
                }
                None
            }
            EventKind::ToolFailed { tool_use_id, .. } => {
                self.commands.remove(tool_use_id);
                None
            }
            EventKind::ToolFinished { tool_use_id, .. } => {
                let win = classify(&self.commands.remove(tool_use_id)?)?;
                let who = match &event.agent {
                    AgentRef::Main => Who::Main,
                    AgentRef::Sub { id, .. } => Who::Sub(id.clone()),
                };
                self.bursts.push((event.session_id.clone(), who, win));
                Some(win)
            }
            EventKind::TurnEnded { last_message: Some(answer) } if event.agent == AgentRef::Main => {
                self.trophies.push(Trophy {
                    session: event.session_id.clone(),
                    project: project.to_owned(),
                    answer: answer.clone(),
                    at: Instant::now(),
                });
                if self.trophies.len() > SHELF {
                    self.trophies.remove(0);
                }
                None
            }
            _ => None,
        }
    }
}

/// A successful command worth celebrating: a commit or a test run.
fn classify(command: &str) -> Option<Win> {
    const TESTS: [&str; 10] =
        ["cargo test", "npm test", "pnpm test", "yarn test", "bun test", "pytest", "go test", "vitest", "jest", "cargo nextest"];
    if command.contains("git commit") {
        Some(Win::Committed)
    } else if TESTS.iter().any(|test| command.contains(test)) {
        Some(Win::TestsPassed)
    } else {
        None
    }
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

    fn bash(id: &str, command: &str) -> RoomEvent {
        event(json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": id, "tool_input": { "command": command } }))
    }

    fn finished(id: &str) -> RoomEvent {
        event(json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": id }))
    }

    #[test]
    fn only_a_successful_commit_or_test_run_is_a_win() {
        let mut wins = Wins::default();
        wins.on_event(&bash("1", "git add . && git commit -m 'x'"), "p");
        assert_eq!(wins.on_event(&finished("1"), "p"), Some(Win::Committed));
        wins.on_event(&bash("2", "cargo test -q"), "p");
        assert_eq!(wins.on_event(&finished("2"), "p"), Some(Win::TestsPassed));
        // A failed run is no win, and ordinary commands aren't either.
        wins.on_event(&bash("3", "cargo test"), "p");
        wins.on_event(&event(json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": "3", "error": "x" })), "p");
        assert_eq!(wins.on_event(&finished("3"), "p"), None);
        wins.on_event(&bash("4", "ls"), "p");
        assert_eq!(wins.on_event(&finished("4"), "p"), None);
        assert_eq!(wins.bursts.len(), 2);
    }

    #[test]
    fn every_finished_turn_is_a_trophy() {
        let mut wins = Wins::default();
        for i in 0..SHELF + 2 {
            wins.on_event(&event(json!({ "hook_event_name": "Stop", "last_assistant_message": format!("answer {i}") })), "p");
        }
        assert_eq!(wins.trophies.len(), SHELF);
        assert_eq!(wins.trophies.last().unwrap().answer, format!("answer {}", SHELF + 1));
    }
}
