//! Parses every recorded hook payload in `tests/fixtures/` (redacted from real Claude Code
//! sessions, see PLAN.md 3.4) and checks the events the app relies on.

use std::path::Path;

use room_proto::describe::Activity;
use room_proto::event::{AgentRef, EventKind, RoomEvent};
use serde_json::Value;

fn load(name: &str) -> RoomEvent {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let hook: Value = serde_json::from_str(&text).unwrap();
    RoomEvent::parse(&hook).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn is_sub(agent: &AgentRef) -> bool {
    matches!(agent, AgentRef::Sub { .. })
}

#[test]
fn every_fixture_parses_to_a_known_event() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned();
        let event = load(&name);
        assert!(!matches!(event.kind, EventKind::Unknown { .. }), "{name} parsed as Unknown");
        count += 1;
    }
    assert!(count >= 20, "expected at least 20 fixtures, found {count}");
}

#[test]
fn session_start_and_end() {
    let start = load("session_start");
    assert_eq!(start.agent, AgentRef::Main);
    assert!(matches!(start.kind, EventKind::SessionStarted { model: Some(_) }));
    assert!(matches!(load("session_end").kind, EventKind::SessionEnded { reason: Some(_) }));
}

#[test]
fn user_prompt_vs_subagent_report() {
    assert_eq!(load("user_prompt").kind, EventKind::UserPrompt { text: "go".into() });
    match load("user_prompt_handback").kind {
        EventKind::ReportReceived { from_agent } => assert!(!from_agent.is_empty()),
        other => panic!("handback prompt parsed as {other:?}"),
    }
}

#[test]
fn main_and_sub_agent_tool_calls() {
    let main = load("pre_tool_bash_main");
    assert_eq!(main.agent, AgentRef::Main);
    let EventKind::ToolStarted { tool_use_id, tool } = main.kind else { panic!("not ToolStarted") };
    assert!(tool_use_id.starts_with("toolu_"));
    assert_eq!(tool.describe().0, Activity::Terminal);

    let sub = load("pre_tool_sub");
    assert!(matches!(&sub.agent, AgentRef::Sub { agent_type, .. } if agent_type == "Explore"));
    assert!(matches!(load("post_tool_sub").kind, EventKind::ToolFinished { launched: None, .. }));
}

#[test]
fn background_agent_launch_links_the_agent_id() {
    let EventKind::ToolStarted { tool, .. } = load("pre_tool_agent").kind else { panic!("not ToolStarted") };
    assert_eq!(tool.describe().0, Activity::Delegating);

    let EventKind::ToolFinished { tool_name, launched: Some(launched), .. } = load("post_tool_agent_async").kind else {
        panic!("no launched agent");
    };
    assert_eq!(tool_name, "Agent");
    assert!(launched.background);

    // The launched id is the same id the sub-agent's own events carry.
    let start = load("subagent_start");
    assert!(matches!(start.kind, EventKind::SubagentStarted));
    assert_eq!(start.agent, AgentRef::Sub { id: launched.agent_id, agent_type: "Explore".into() });

    let EventKind::ToolFinished { launched: Some(sync), .. } = load("post_tool_agent_sync").kind else {
        panic!("no launched agent");
    };
    assert!(!sync.background);
}

#[test]
fn handback_and_subagent_stop() {
    let handback = load("pre_tool_handback");
    assert!(is_sub(&handback.agent));
    let EventKind::Handback { message, .. } = handback.kind else { panic!("not Handback") };
    assert!(!message.is_empty());

    assert!(matches!(load("subagent_stop_handback").kind, EventKind::SubagentStopped { last_message: None }));
    assert!(matches!(load("subagent_stop_message").kind, EventKind::SubagentStopped { last_message: Some(_) }));
    assert!(matches!(load("subagent_stop_untyped").agent, AgentRef::Sub { ref agent_type, .. } if agent_type.is_empty()));
}

#[test]
fn permissions_and_failures() {
    let main = load("permission_request_main");
    assert_eq!(main.agent, AgentRef::Main);
    assert!(matches!(main.kind, EventKind::PermissionAsked { ref tool } if tool.name == "Bash"));
    assert!(is_sub(&load("permission_request_sub").agent));

    let EventKind::ToolFailed { error, interrupted, .. } = load("post_tool_failure").kind else { panic!("not ToolFailed") };
    assert!(error.starts_with("Exit code 1"));
    assert!(!interrupted);
}

#[test]
fn turn_end() {
    assert!(matches!(load("stop").kind, EventKind::TurnEnded { last_message: Some(_) }));
}
