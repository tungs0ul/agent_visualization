//! The mission report (PLAN.md M28): a Markdown summary of the shown session, written
//! to `~/.agent-room/reports/` and opened. Each agent's report is quoted as its own
//! words; beside it stands what the hooks saw (tool calls, failures, files), so a
//! reader can tell a claim from an observation.

use std::fmt::Write as _;

use crate::model::{SessionState, Status};
use crate::usage::Tokens;
use crate::work::{Outcome, Work, unmentioned_failures};

/// What goes into a report besides the session itself.
pub struct Extras<'a> {
    pub prompts: &'a [String],
    pub answer: Option<&'a str>,
    pub tokens: Option<Tokens>,
    /// How long the session has run, in seconds, if known.
    pub runtime: Option<u64>,
}

pub fn render(session: &SessionState, extras: &Extras) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Mission report: {}\n", session.name());
    if let Some(cwd) = &session.cwd {
        let _ = writeln!(out, "- Folder: `{cwd}`");
    }
    if let Some(model) = &session.model {
        let _ = writeln!(out, "- Model: {model}");
    }
    if let Some(secs) = extras.runtime {
        let _ = writeln!(out, "- Runtime: {}m {}s", secs / 60, secs % 60);
    }
    if let Some(tokens) = extras.tokens {
        let _ = writeln!(out, "- Tokens: {} in, {} out", tokens.input, tokens.output);
    }
    let calls = session.main_work.calls.len() + session.agents.iter().map(|(_, info)| info.work.calls.len()).sum::<usize>();
    let failed = session.main_work.failures() + session.agents.iter().map(|(_, info)| info.work.failures()).sum::<usize>();
    let _ = writeln!(out, "- Tool calls seen: {calls}, failed: {failed}");
    let _ = writeln!(out, "- Sub-agents: {}\n", session.agents.len());

    if !extras.prompts.is_empty() {
        let _ = writeln!(out, "## Asked\n");
        for prompt in extras.prompts {
            let _ = writeln!(out, "> {}\n", prompt.replace('\n', "\n> "));
        }
    }
    let _ = writeln!(out, "## Claude\n");
    work_section(&mut out, &session.main_work);
    if let Some(answer) = extras.answer {
        let _ = writeln!(out, "**Final answer (its own words):**\n");
        let _ = writeln!(out, "> {}\n", answer.replace('\n', "\n> "));
    }
    for (id, info) in &session.agents {
        let short: String = id.chars().take(4).collect();
        let status = match info.status {
            Status::Working => "working",
            Status::Waiting => "waiting for you",
            Status::Done => "done",
        };
        let _ = writeln!(out, "## {} {short} ({status})\n", info.agent_type);
        if let Some(task) = &info.task {
            let _ = writeln!(out, "**Task:** {task}\n");
        }
        work_section(&mut out, &info.work);
        if let Some(report) = &info.report {
            let _ = writeln!(out, "**Report (its own words):**\n");
            let _ = writeln!(out, "> {}\n", report.replace('\n', "\n> "));
            if let Some(count) = unmentioned_failures(report, &info.work) {
                let _ = writeln!(out, "**Check:** {count} failed call(s) that the report doesn't mention.\n");
            }
        }
    }
    out
}

/// What the hooks saw one agent do: counts, the files it wrote, its failures.
fn work_section(out: &mut String, work: &Work) {
    let _ = writeln!(out, "Seen by Agent Room: {} tool call(s), {} failed.\n", work.calls.len(), work.failures());
    if !work.files.is_empty() {
        let _ = writeln!(out, "Files written:\n");
        for file in &work.files {
            let _ = writeln!(out, "- `{file}`");
        }
        out.push('\n');
    }
    let failures: Vec<_> = work
        .calls
        .iter()
        .filter_map(|call| match &call.outcome {
            Outcome::Failed { error, .. } => Some((call, error)),
            _ => None,
        })
        .collect();
    if !failures.is_empty() {
        let _ = writeln!(out, "Failed calls:\n");
        for (call, error) in failures {
            let first = error.lines().next().unwrap_or_default();
            let _ = writeln!(out, "- {}: {}", call.label, room_proto::describe::truncate(first, 160));
        }
        out.push('\n');
    }
}

/// Writes the report and returns its path.
pub fn save(session: &SessionState, text: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = room_proto::state_dir().ok_or_else(|| std::io::Error::other("no home folder"))?.join("reports");
    std::fs::create_dir_all(&dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let name: String = session.name().chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let path = dir.join(format!("{name}-{stamp}.md"));
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use room_proto::event::RoomEvent;
    use serde_json::{Value, json};

    fn event(sub: Option<&str>, fields: Value) -> RoomEvent {
        let mut hook = json!({ "session_id": "s", "cwd": "/src/app" });
        hook.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        if let Some(id) = sub {
            hook["agent_id"] = json!(id);
            hook["agent_type"] = json!("Explore");
        }
        RoomEvent::parse(&hook).unwrap()
    }

    #[test]
    fn a_report_quotes_claims_and_lists_what_was_seen() {
        let mut model = Model::default();
        model.apply(&event(None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "fix it" })));
        model.apply(&event(
            Some("a1"),
            json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": "cargo test" }, "tool_use_id": "t1" }),
        ));
        model.apply(&event(
            Some("a1"),
            json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": "t1", "error": "2 tests failed" }),
        ));
        model.apply(&event(Some("a1"), json!({ "hook_event_name": "SubagentStop", "last_assistant_message": "All good." })));
        let prompts = ["fix it".to_owned()];
        let text = render(model.shown().unwrap(), &Extras { prompts: &prompts, answer: None, tokens: None, runtime: Some(65) });
        assert!(text.starts_with("# Mission report: app"));
        assert!(text.contains("- Runtime: 1m 5s"));
        assert!(text.contains("> fix it"));
        assert!(text.contains("**Report (its own words):**\n\n> All good."));
        assert!(text.contains("**Check:** 1 failed call(s)"));
        assert!(text.contains("- Bash: cargo test: 2 tests failed") || text.contains("2 tests failed"));
    }
}
