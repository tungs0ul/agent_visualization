//! Short, human text for what an agent is doing, for speech bubbles and the event log.

use serde_json::Value;

use crate::event::ToolCall;

/// What the character animates while a tool runs (PLAN.md 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Activity {
    Reading,
    Writing,
    Terminal,
    Web,
    Delegating,
    Reporting,
    Thinking,
}

/// Bubble text is cut to this many characters.
pub const MAX_LABEL_CHARS: usize = 60;

impl ToolCall {
    pub fn describe(&self) -> (Activity, String) {
        let input = &self.input;
        let field = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();
        let (activity, label) = match self.name.as_str() {
            "Read" => (Activity::Reading, format!("Reading {}", file_name(field("file_path")))),
            "Grep" => (Activity::Reading, format!("Searching for {}", field("pattern"))),
            "Glob" => (Activity::Reading, format!("Looking for {}", field("pattern"))),
            "LSP" => (Activity::Reading, "Checking the code".to_owned()),
            "Edit" => (Activity::Writing, format!("Editing {}", file_name(field("file_path")))),
            "Write" => (Activity::Writing, format!("Writing {}", file_name(field("file_path")))),
            "NotebookEdit" => (Activity::Writing, format!("Editing {}", file_name(field("notebook_path")))),
            "Bash" => {
                let description = field("description");
                let label = if description.is_empty() { field("command") } else { description };
                (Activity::Terminal, label.to_owned())
            }
            "WebFetch" => (Activity::Web, format!("Reading {}", host(field("url")))),
            "WebSearch" => (Activity::Web, format!("Searching the web: {}", field("query"))),
            "Agent" | "Task" => {
                let helper = match field("subagent_type") {
                    "" => "a helper",
                    other => other,
                };
                (Activity::Delegating, format!("Asking {helper}: {}", field("description")))
            }
            "SubagentHandback" => (Activity::Reporting, "Reporting back".to_owned()),
            "AskUserQuestion" => (Activity::Thinking, "Asking you a question".to_owned()),
            "ExitPlanMode" => (Activity::Thinking, "Plan ready for your review".to_owned()),
            name if name.starts_with("mcp__") => (Activity::Terminal, mcp_label(name)),
            name => (Activity::Thinking, name.to_owned()),
        };
        (activity, truncate(label.trim(), MAX_LABEL_CHARS))
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest)
}

/// `mcp__claude_ai_Gmail__search_threads` ⇒ `Gmail: search threads`.
fn mcp_label(name: &str) -> String {
    let mut parts = name.trim_start_matches("mcp__").splitn(2, "__");
    let server = parts.next().unwrap_or_default();
    let server = server.rsplit('_').next().unwrap_or(server);
    match parts.next() {
        Some(tool) => format!("{server}: {}", tool.replace('_', " ")),
        None => server.to_owned(),
    }
}

/// Cuts at a character boundary and adds an ellipsis.
pub fn truncate(text: &str, max_chars: usize) -> String {
    let first_line = text.lines().next().unwrap_or_default();
    if first_line.chars().count() <= max_chars && first_line.len() == text.len() {
        return text.to_owned();
    }
    let cut: String = first_line.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn describe(name: &str, input: Value) -> (Activity, String) {
        ToolCall { name: name.into(), input }.describe()
    }

    #[test]
    fn file_tools_show_the_file_name() {
        assert_eq!(describe("Read", json!({ "file_path": "/a/b/PLAN.md" })), (Activity::Reading, "Reading PLAN.md".into()));
        assert_eq!(describe("Edit", json!({ "file_path": "src/main.rs" })), (Activity::Writing, "Editing main.rs".into()));
    }

    #[test]
    fn bash_prefers_description_over_command() {
        assert_eq!(describe("Bash", json!({ "command": "ls", "description": "List files" })).1, "List files");
        assert_eq!(describe("Bash", json!({ "command": "ls -la" })).1, "ls -la");
    }

    #[test]
    fn agent_names_the_helper() {
        let input = json!({ "subagent_type": "Explore", "description": "Count Rust files" });
        assert_eq!(describe("Agent", input), (Activity::Delegating, "Asking Explore: Count Rust files".into()));
        assert_eq!(describe("Agent", json!({ "description": "x" })).1, "Asking a helper: x");
    }

    #[test]
    fn web_and_mcp_labels() {
        assert_eq!(describe("WebFetch", json!({ "url": "https://docs.rs/bevy/latest" })).1, "Reading docs.rs");
        assert_eq!(describe("mcp__claude_ai_Gmail__search_threads", json!({})).1, "Gmail: search threads");
        assert_eq!(describe("mcp__blender__execute_blender_code", json!({})).1, "blender: execute blender code");
    }

    #[test]
    fn unknown_tool_falls_back_to_its_name() {
        assert_eq!(describe("TodoWrite", Value::Null), (Activity::Thinking, "TodoWrite".into()));
    }

    #[test]
    fn truncate_is_char_safe_and_single_line() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("Đây là một câu tiếng Việt dài", 10), "Đây là mộ…");
        assert_eq!(truncate("line one\nline two", 60), "line one…");
    }
}
