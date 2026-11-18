//! What the agents did, for two consoles (PLAN.md M20): the processing lab lists each
//! session's recent commands with their outcome and output, the wall screen shows
//! today's numbers. Both come from hook events, kept in memory.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use bevy::prelude::Resource;
use room_proto::event::{AgentRef, EventKind, RoomEvent};

/// Commands kept per session.
const KEPT: usize = 30;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Running,
    Succeeded,
    Failed,
}

pub struct Command {
    tool_use_id: String,
    pub text: String,
    pub started: Instant,
    pub took: Option<Duration>,
    pub outcome: Outcome,
    /// The end of its stdout and stderr, or the error.
    pub output: String,
}

/// Today's numbers, reset at local midnight.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Day {
    /// Days since 1970 in local time.
    pub date: u64,
    pub prompts: u32,
    pub turns: u32,
    /// Tool calls by tool name.
    pub tools: BTreeMap<String, u32>,
    /// Tool calls per local hour.
    pub hours: [u32; 24],
}

#[derive(Resource)]
pub struct Activity {
    commands: HashMap<String, VecDeque<Command>>,
    pub day: Day,
    /// Local time minus UTC, read once at start.
    utc_offset: i64,
}

impl Default for Activity {
    fn default() -> Self {
        Self { commands: HashMap::new(), day: Day::default(), utc_offset: utc_offset() }
    }
}

impl Activity {
    /// The session's commands, newest first.
    pub fn commands(&self, session: &str) -> impl Iterator<Item = &Command> {
        self.commands.get(session).into_iter().flat_map(|commands| commands.iter().rev())
    }

    pub fn on_event(&mut self, event: &RoomEvent) {
        self.on_event_at(event, unix_now());
    }

    fn on_event_at(&mut self, event: &RoomEvent, now: i64) {
        let local = now + self.utc_offset;
        let date = local.div_euclid(86_400) as u64;
        if self.day.date != date {
            self.day = Day { date, ..Day::default() };
        }
        match &event.kind {
            EventKind::UserPrompt { .. } => self.day.prompts += 1,
            EventKind::TurnEnded { .. } if event.agent == AgentRef::Main => self.day.turns += 1,
            EventKind::ToolStarted { tool_use_id, tool } => {
                *self.day.tools.entry(tool.name.clone()).or_default() += 1;
                self.day.hours[(local.rem_euclid(86_400) / 3_600) as usize] += 1;
                if tool.name == "Bash"
                    && let Some(text) = tool.input["command"].as_str()
                {
                    let commands = self.commands.entry(event.session_id.clone()).or_default();
                    commands.push_back(Command {
                        tool_use_id: tool_use_id.clone(),
                        text: text.to_owned(),
                        started: Instant::now(),
                        took: None,
                        outcome: Outcome::Running,
                        output: String::new(),
                    });
                    while commands.len() > KEPT {
                        commands.pop_front();
                    }
                }
            }
            EventKind::ToolFinished { tool_use_id, output, .. } => {
                self.finish(&event.session_id, tool_use_id, Outcome::Succeeded, output);
            }
            EventKind::ToolFailed { tool_use_id, error, .. } => {
                self.finish(&event.session_id, tool_use_id, Outcome::Failed, error);
            }
            _ => {}
        }
    }

    fn finish(&mut self, session: &str, tool_use_id: &str, outcome: Outcome, output: &str) {
        let Some(command) =
            self.commands.get_mut(session).and_then(|commands| commands.iter_mut().rev().find(|c| c.tool_use_id == tool_use_id))
        else {
            return;
        };
        command.outcome = outcome;
        command.took = Some(command.started.elapsed());
        command.output = output.to_owned();
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

/// Seconds east of UTC, from `date +%z` ("+0100"); 0 if that fails.
fn utc_offset() -> i64 {
    let Ok(out) = std::process::Command::new("date").arg("+%z").output() else { return 0 };
    parse_offset(String::from_utf8_lossy(&out.stdout).trim()).unwrap_or(0)
}

fn parse_offset(text: &str) -> Option<i64> {
    let sign = match text.get(..1)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let hours: i64 = text.get(1..3)?.parse().ok()?;
    let minutes: i64 = text.get(3..5)?.parse().ok()?;
    Some(sign * (hours * 3_600 + minutes * 60))
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

    #[test]
    fn commands_end_with_their_outcome_and_output() {
        let mut activity = Activity { utc_offset: 0, ..Activity::default() };
        activity.on_event_at(&bash("1", "cargo test"), 0);
        activity.on_event_at(&bash("2", "ls nope"), 0);
        let done = json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "1",
            "tool_response": { "stdout": "ok" } });
        activity.on_event_at(&event(done), 0);
        let failed = json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": "2", "error": "no such file" });
        activity.on_event_at(&event(failed), 0);
        let shown: Vec<_> = activity.commands("s").map(|c| (c.text.as_str(), c.outcome, c.output.as_str())).collect();
        assert_eq!(shown, [("ls nope", Outcome::Failed, "no such file"), ("cargo test", Outcome::Succeeded, "ok")]);
        assert!(activity.commands("s").all(|c| c.took.is_some()));
    }

    #[test]
    fn the_day_counts_tools_by_hour_and_starts_over_at_midnight() {
        let mut activity = Activity { utc_offset: 3_600, ..Activity::default() };
        // 22:30 UTC is 23:30 local.
        let late = 22 * 3_600 + 1_800;
        activity.on_event_at(&bash("1", "ls"), late);
        activity.on_event_at(&event(json!({ "hook_event_name": "Stop" })), late);
        assert_eq!((activity.day.hours[23], activity.day.turns, activity.day.tools["Bash"]), (1, 1, 1));
        // An hour later it is the next local day.
        activity.on_event_at(&bash("2", "ls"), late + 3_600);
        assert_eq!((activity.day.hours[0], activity.day.hours[23], activity.day.turns), (1, 0, 0));
        assert_eq!(parse_offset("-0530"), Some(-(5 * 3_600 + 30 * 60)));
    }
}
