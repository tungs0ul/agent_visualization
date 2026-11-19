//! What one agent did, as the hooks saw it (PLAN.md M28): every tool call with its
//! input, result and duration, and the files it wrote. The desks, the robot details,
//! the activity console and the mission report read it. It is the record to hold an
//! agent's own report against: what Claude Code observed, not what the agent says.

use std::time::{Duration, Instant};

use room_proto::event::EventKind;

/// Calls kept per agent (the oldest go first).
const KEEP: usize = 200;
/// Characters kept of a tool's output or error.
const OUTPUT: usize = 2000;

#[derive(Debug, Clone, Default)]
pub struct Work {
    pub calls: Vec<Call>,
    /// Files it created or edited, newest first, each once.
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    pub name: String,
    /// One line: "Read main.rs", "Bash: cargo test".
    pub label: String,
    /// The input as Claude Code sent it, pretty JSON (commands as they are).
    pub input: String,
    pub started: Instant,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Done { output: String, took: Duration },
    Failed { error: String, took: Duration },
}

impl Work {
    /// Records a tool event of this agent.
    pub fn on(&mut self, kind: &EventKind) {
        match kind {
            EventKind::ToolStarted { tool_use_id, tool } => {
                if let Some(path) = written_file(&tool.name, &tool.input) {
                    self.files.retain(|known| *known != path);
                    self.files.insert(0, path);
                }
                let input = match tool.input.get("command").and_then(|command| command.as_str()) {
                    Some(command) => command.to_owned(),
                    None => serde_json::to_string_pretty(&tool.input).unwrap_or_default(),
                };
                self.calls.push(Call {
                    id: tool_use_id.clone(),
                    name: tool.name.clone(),
                    label: tool.describe().1,
                    input,
                    started: Instant::now(),
                    outcome: Outcome::Running,
                });
                if self.calls.len() > KEEP {
                    self.calls.remove(0);
                }
            }
            EventKind::ToolFinished { tool_use_id, output, .. } => {
                if let Some(call) = self.running(tool_use_id) {
                    let took = call.started.elapsed();
                    call.outcome = Outcome::Done { output: room_proto::describe::truncate(output, OUTPUT), took };
                }
            }
            EventKind::ToolFailed { tool_use_id, error, .. } => {
                if let Some(call) = self.running(tool_use_id) {
                    let took = call.started.elapsed();
                    call.outcome = Outcome::Failed { error: room_proto::describe::truncate(error, OUTPUT), took };
                }
            }
            _ => {}
        }
    }

    fn running(&mut self, id: &str) -> Option<&mut Call> {
        self.calls.iter_mut().rev().find(|call| call.id == id && call.outcome == Outcome::Running)
    }

    pub fn failures(&self) -> usize {
        self.calls.iter().filter(|call| matches!(call.outcome, Outcome::Failed { .. })).count()
    }

    /// Failed calls that no later successful call of the same tool made good.
    pub fn unrecovered(&self) -> usize {
        self.calls
            .iter()
            .enumerate()
            .filter(|(index, call)| {
                matches!(call.outcome, Outcome::Failed { .. })
                    && !self.calls[index + 1..]
                        .iter()
                        .any(|later| later.name == call.name && matches!(later.outcome, Outcome::Done { .. }))
            })
            .count()
    }

    /// The latest call still running, if any: the tool it uses now.
    pub fn current(&self) -> Option<&Call> {
        self.calls.iter().rev().find(|call| call.outcome == Outcome::Running)
    }
}

/// The file a tool call writes, if it writes one.
fn written_file(tool: &str, input: &serde_json::Value) -> Option<String> {
    let key = match tool {
        "Write" | "Edit" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        _ => return None,
    };
    input.get(key)?.as_str().map(str::to_owned)
}

/// A report that reads as all good while calls failed and nothing made them good: the
/// number of such calls, to show beside the report. A guess from the words only, so
/// it is shown as "check", never as a verdict.
pub fn unmentioned_failures(report: &str, work: &Work) -> Option<usize> {
    let failed = work.unrecovered();
    let lower = report.to_lowercase();
    let mentions = ["fail", "error", "could not", "couldn't", "unable", "not work", "broken", "didn't"];
    (failed > 0 && !mentions.iter().any(|word| lower.contains(word))).then_some(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use room_proto::event::RoomEvent;
    use serde_json::{Value, json};

    fn kind(fields: Value) -> EventKind {
        let mut hook = json!({ "session_id": "s" });
        hook.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        RoomEvent::parse(&hook).unwrap().kind
    }

    fn start(id: &str, name: &str, input: Value) -> EventKind {
        kind(json!({ "hook_event_name": "PreToolUse", "tool_name": name, "tool_input": input, "tool_use_id": id }))
    }

    fn done(id: &str, name: &str) -> EventKind {
        kind(json!({ "hook_event_name": "PostToolUse", "tool_name": name, "tool_use_id": id, "tool_response": "ok" }))
    }

    fn failed(id: &str, name: &str) -> EventKind {
        kind(json!({ "hook_event_name": "PostToolUseFailure", "tool_name": name, "tool_use_id": id, "error": "exit 1" }))
    }

    #[test]
    fn calls_and_files_follow_the_hooks() {
        let mut work = Work::default();
        work.on(&start("1", "Edit", json!({ "file_path": "/x/a.rs", "old_string": "a", "new_string": "b" })));
        work.on(&start("2", "Bash", json!({ "command": "cargo test" })));
        assert_eq!(work.current().map(|call| call.name.as_str()), Some("Bash"));
        work.on(&done("1", "Edit"));
        work.on(&failed("2", "Bash"));
        work.on(&start("3", "Write", json!({ "file_path": "/x/b.rs", "content": "" })));
        work.on(&start("4", "Edit", json!({ "file_path": "/x/a.rs", "old_string": "b", "new_string": "c" })));
        assert_eq!(work.files, vec!["/x/a.rs".to_owned(), "/x/b.rs".to_owned()], "newest first, each once");
        assert_eq!(work.calls[1].input, "cargo test");
        assert!(matches!(work.calls[1].outcome, Outcome::Failed { .. }));
        assert_eq!(work.failures(), 1);
    }

    #[test]
    fn a_report_that_skips_a_failure_gets_a_check() {
        let mut work = Work::default();
        work.on(&start("1", "Bash", json!({ "command": "cargo test" })));
        work.on(&failed("1", "Bash"));
        assert_eq!(unmentioned_failures("All tests pass, done.", &work), Some(1));
        assert_eq!(unmentioned_failures("Two tests fail on CI.", &work), None, "the report says so");
        // A later successful run of the same tool makes it good.
        work.on(&start("2", "Bash", json!({ "command": "cargo test" })));
        work.on(&done("2", "Bash"));
        assert_eq!(unmentioned_failures("All tests pass, done.", &work), None);
    }
}
