//! What is happening, independent of how it is drawn: sessions, the agents in them and
//! what each one is doing. Every hook event goes through [`Model::apply`], which returns
//! the [`Change`]s the room scene animates. The scene, the log and the side panels only
//! read the model; all the tricky rules (sub-agent attribution, which session is shown,
//! Claude Code's internal helpers, reports) live here and are unit tested.

use std::collections::HashMap;

use bevy::prelude::*;
use room_proto::describe::Activity;
use room_proto::event::{AgentRef, EventKind, RoomEvent};

/// What a character is doing; drives its animation.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    #[default]
    Idle,
    Thinking,
    Reading,
    /// Editing files.
    Typing,
    /// Running commands or MCP tools.
    Terminal,
    Web,
    Delegating,
    Reporting,
    Waiting,
}

impl Pose {
    /// One word for lists: what the character is doing.
    pub fn word(self) -> &'static str {
        match self {
            Pose::Idle => "idle",
            Pose::Thinking => "thinking",
            Pose::Reading => "reading",
            Pose::Typing => "writing",
            Pose::Terminal => "running commands",
            Pose::Web => "on the web",
            Pose::Delegating => "delegating",
            Pose::Reporting => "reporting",
            Pose::Waiting => "needs your OK",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Working,
    /// Waiting for a permission answer.
    Waiting,
    /// Stopped; its report (if any) is in `AgentInfo::report`.
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Who {
    Main,
    Sub(String),
}

/// One thing the scene should show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// The room now shows another session: clear every sub-agent.
    ShowSession,
    /// A sub-agent arrives; `greeting` is the task it was given.
    Joined { id: String, agent_type: String, greeting: String },
    Pose { who: Who, pose: Pose },
    Say { who: Who, line: String },
    /// Walk the report over to Claude.
    Deliver { id: String },
    /// Finished: leave once standing still.
    Leave { id: String },
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub agent_type: String,
    pub status: Status,
    pub pose: Pose,
    /// What the main agent asked it to do.
    pub task: Option<String>,
    /// The latest action, as shown in its speech bubble.
    pub doing: Option<String>,
    pub report: Option<String>,
    reported: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub id: String,
    pub cwd: Option<String>,
    pub main_pose: Pose,
    /// What the main agent is doing, as shown in its speech bubble.
    pub main_doing: Option<String>,
    /// Sub-agents in the order they joined, finished ones included.
    pub agents: Vec<(String, AgentInfo)>,
    pub ended: bool,
    /// Tasks handed out with the `Agent` tool, by sub-agent id, until the agent shows up.
    briefs: HashMap<String, String>,
    /// Order of last activity across sessions (higher is more recent).
    last_seen: u64,
}

impl SessionState {
    /// Something is going on: Claude or a sub-agent isn't idle or done.
    pub fn is_busy(&self) -> bool {
        self.main_pose != Pose::Idle || self.agents.iter().any(|(_, info)| info.status != Status::Done)
    }

    /// The project folder name, or the session id when there is no cwd.
    pub fn name(&self) -> String {
        self.cwd
            .as_deref()
            .and_then(|cwd| cwd.rsplit('/').find(|part| !part.is_empty()))
            .map_or_else(|| self.id.chars().take(8).collect(), str::to_owned)
    }

    pub fn agent(&self, id: &str) -> Option<&AgentInfo> {
        self.agents.iter().find(|(agent_id, _)| agent_id == id).map(|(_, info)| info)
    }

    fn agent_mut(&mut self, id: &str) -> Option<&mut AgentInfo> {
        self.agents.iter_mut().find(|(agent_id, _)| agent_id == id).map(|(_, info)| info)
    }
}

/// The shown session counts as quiet once this many events in a row came from other
/// sessions; then the room switches to the session that is working.
const QUIET_EVENTS: u64 = 5;

#[derive(Resource, Default)]
pub struct Model {
    sessions: HashMap<String, SessionState>,
    /// The session the room shows.
    shown: Option<String>,
    events_seen: u64,
}

impl Model {
    /// All sessions, most recently active first.
    pub fn sessions(&self) -> Vec<&SessionState> {
        let mut sessions: Vec<_> = self.sessions.values().collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.last_seen));
        sessions
    }

    /// Shows another session (picked by the user). Returns the changes that redraw the
    /// room for it; nothing if it is unknown or already shown.
    pub fn show(&mut self, session_id: &str) -> Vec<Change> {
        if self.shown.as_deref() == Some(session_id) || !self.sessions.contains_key(session_id) {
            return Vec::new();
        }
        self.shown = Some(session_id.to_owned());
        let mut changes = vec![Change::ShowSession];
        changes.extend(self.replay_shown());
        changes
    }

    pub fn shown(&self) -> Option<&SessionState> {
        self.shown.as_ref().and_then(|id| self.sessions.get(id))
    }

    /// The type of a sub-agent seen in that session (still working or finished).
    pub fn agent_type(&self, session: &str, agent_id: &str) -> Option<&str> {
        let info = self.sessions.get(session)?.agent(agent_id)?;
        Some(info.agent_type.as_str())
    }

    /// Claude Code's own helpers (e.g. a session recap) stop without ever starting and
    /// have no type (PLAN.md 3.4). They don't belong in the room or the log.
    pub fn is_internal_helper(&self, event: &RoomEvent) -> bool {
        matches!(
            (&event.agent, &event.kind),
            (AgentRef::Sub { id, agent_type }, EventKind::SubagentStopped { .. })
                if agent_type.is_empty() && self.agent_type(&event.session_id, id).is_none()
        )
    }

    pub fn apply(&mut self, event: &RoomEvent) -> Vec<Change> {
        let mut changes = Vec::new();
        let session = self.sessions.entry(event.session_id.clone()).or_insert_with(|| SessionState {
            id: event.session_id.clone(),
            ..default()
        });
        if event.cwd.is_some() {
            session.cwd = event.cwd.clone();
        }
        self.events_seen += 1;
        session.last_seen = self.events_seen;
        session.ended = matches!(event.kind, EventKind::SessionEnded { .. });
        update_session(session, event, &mut changes);

        // The room follows the active session: it switches when the user starts or types
        // into another one, or when the shown one has gone quiet (or ended) while another
        // works. Otherwise a stray event from another session could hold the room.
        let user_activity = matches!(event.kind, EventKind::SessionStarted { .. } | EventKind::UserPrompt { .. });
        let shown_quiet = self.shown.as_ref().and_then(|id| self.sessions.get(id)).is_none_or(|shown| {
            shown.ended || self.events_seen.saturating_sub(shown.last_seen) >= QUIET_EVENTS
        });
        match &self.shown {
            Some(shown) if *shown == event.session_id => changes,
            Some(_) if !user_activity && !shown_quiet => Vec::new(),
            _ => {
                self.shown = Some(event.session_id.clone());
                // Redraw the newly shown session from scratch.
                let mut all = vec![Change::ShowSession];
                all.extend(self.replay_shown());
                all.extend(changes.into_iter().filter(|c| matches!(c, Change::Say { .. })));
                all
            }
        }
    }

    /// Changes that bring an empty room up to the shown session's current state.
    fn replay_shown(&self) -> Vec<Change> {
        let Some(session) = self.shown() else { return Vec::new() };
        let mut changes = vec![Change::Pose { who: Who::Main, pose: session.main_pose }];
        for (id, info) in &session.agents {
            if info.status == Status::Done {
                continue;
            }
            let greeting = info.doing.clone().or_else(|| info.task.clone()).unwrap_or_default();
            changes.push(Change::Joined { id: id.clone(), agent_type: info.agent_type.clone(), greeting });
            changes.push(Change::Pose { who: Who::Sub(id.clone()), pose: info.pose });
        }
        changes
    }
}

fn update_session(session: &mut SessionState, event: &RoomEvent, changes: &mut Vec<Change>) {
    match &event.agent {
        AgentRef::Main => {
            match &event.kind {
                EventKind::ToolFinished { launched: Some(launched), .. } => {
                    session.briefs.insert(launched.agent_id.clone(), launched.description.clone());
                }
                EventKind::SessionEnded { .. } => {
                    for (id, info) in &mut session.agents {
                        if info.status != Status::Done {
                            info.status = Status::Done;
                            changes.push(Change::Leave { id: id.clone() });
                        }
                    }
                }
                _ => {}
            }
            let (pose, line) = react(&event.kind, session.main_pose, session);
            if let Some(pose) = pose {
                session.main_pose = pose;
                changes.push(Change::Pose { who: Who::Main, pose });
            }
            if let Some(line) = line {
                session.main_doing = Some(line.clone());
                changes.push(Change::Say { who: Who::Main, line });
            }
        }
        AgentRef::Sub { id, agent_type } => update_sub(session, id, agent_type, &event.kind, changes),
    }
}

fn update_sub(session: &mut SessionState, id: &str, agent_type: &str, kind: &EventKind, changes: &mut Vec<Change>) {
    let who = Who::Sub(id.to_owned());
    if let EventKind::SubagentStopped { last_message } = kind {
        // Unknown ids are Claude Code's internal helpers: ignore.
        let Some(info) = session.agent_mut(id) else { return };
        if info.status == Status::Done {
            return;
        }
        info.status = Status::Done;
        // No hand-back: it still brings its last words over to Claude.
        if !info.reported {
            info.reported = true;
            info.report = last_message.clone();
            info.pose = Pose::Reporting;
            changes.push(Change::Pose { who: who.clone(), pose: Pose::Reporting });
            if let Some(text) = last_message {
                changes.push(Change::Say { who: who.clone(), line: text.clone() });
            }
            changes.push(Change::Deliver { id: id.to_owned() });
        }
        changes.push(Change::Leave { id: id.to_owned() });
        return;
    }
    if agent_type.is_empty() {
        return;
    }
    // Any event from an unseen sub-agent seats it, so starting the app in the middle of
    // a session still shows everyone who is working.
    if session.agent(id).is_none() {
        let task = session.briefs.remove(id);
        let greeting = task.clone().unwrap_or_else(|| "On my way!".to_owned());
        session.agents.push((
            id.to_owned(),
            AgentInfo {
                agent_type: agent_type.to_owned(),
                status: Status::Working,
                pose: Pose::Thinking,
                task,
                doing: None,
                report: None,
                reported: false,
            },
        ));
        changes.push(Change::Joined { id: id.to_owned(), agent_type: agent_type.to_owned(), greeting });
    }

    let current = session.agent(id).map_or(Pose::Idle, |info| info.pose);
    let (pose, line) = react(kind, current, session);
    let Some(info) = session.agent_mut(id) else { return };
    if info.status == Status::Done {
        return;
    }
    if let Some(pose) = pose {
        info.pose = pose;
        info.status = if pose == Pose::Waiting { Status::Waiting } else { Status::Working };
        changes.push(Change::Pose { who: who.clone(), pose });
    }
    if let Some(line) = line {
        info.doing = Some(line.clone());
        changes.push(Change::Say { who, line });
    }
    if let EventKind::Handback { message, .. } = kind {
        info.reported = true;
        info.report = Some(message.clone());
        changes.push(Change::Deliver { id: id.to_owned() });
    }
}

/// Pose and speech for an event, given the character's current pose.
fn react(kind: &EventKind, current: Pose, session: &SessionState) -> (Option<Pose>, Option<String>) {
    match kind {
        EventKind::SessionStarted { .. } => (Some(Pose::Idle), Some("Hello!".to_owned())),
        EventKind::UserPrompt { .. } => (Some(Pose::Thinking), Some("On it!".to_owned())),
        EventKind::ToolStarted { tool, .. } => {
            let (activity, label) = tool.describe();
            (Some(pose_for(activity)), Some(label))
        }
        // An answered permission: back to work.
        EventKind::ToolFinished { .. } if current == Pose::Waiting => (Some(Pose::Thinking), None),
        EventKind::ToolFailed { tool_name, interrupted: false, .. } => {
            (Some(Pose::Thinking), Some(format!("Hmm, {tool_name} failed")))
        }
        EventKind::ToolFailed { .. } => (Some(Pose::Idle), Some("Stopped".to_owned())),
        EventKind::PermissionAsked { tool } => (Some(Pose::Waiting), Some(format!("May I? {}", tool.describe().1))),
        EventKind::PermissionAutoDenied { tool_name, .. } => {
            (Some(Pose::Thinking), Some(format!("Not allowed: {tool_name}")))
        }
        EventKind::Handback { .. } => (Some(Pose::Reporting), Some("Here's my report!".to_owned())),
        EventKind::ReportReceived { from_agent } => {
            let from = session.agent(from_agent).map_or("my helper", |info| info.agent_type.as_str());
            (Some(Pose::Thinking), Some(format!("Got it from {from}")))
        }
        EventKind::Notification { message, .. } => (None, Some(message.clone())),
        EventKind::TurnEnded { .. } => (Some(Pose::Idle), Some("All done".to_owned())),
        EventKind::TurnFailed { .. } => (Some(Pose::Idle), Some("Something went wrong".to_owned())),
        _ => (None, None),
    }
}

fn pose_for(activity: Activity) -> Pose {
    match activity {
        Activity::Reading => Pose::Reading,
        Activity::Writing => Pose::Typing,
        Activity::Terminal => Pose::Terminal,
        Activity::Web => Pose::Web,
        Activity::Delegating => Pose::Delegating,
        Activity::Reporting => Pose::Reporting,
        Activity::Thinking => Pose::Thinking,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn event(session: &str, sub: Option<(&str, &str)>, fields: Value) -> RoomEvent {
        let mut hook = json!({ "session_id": session });
        hook.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        if let Some((id, agent_type)) = sub {
            hook["agent_id"] = json!(id);
            hook["agent_type"] = json!(agent_type);
        }
        RoomEvent::parse(&hook).unwrap()
    }

    fn sub_tool(session: &str, id: &str) -> RoomEvent {
        event(
            session,
            Some((id, "Explore")),
            json!({ "hook_event_name": "PreToolUse", "tool_name": "Read", "tool_input": { "file_path": "/x/a.rs" }, "tool_use_id": "t" }),
        )
    }

    #[test]
    fn first_session_is_shown_and_others_need_user_activity() {
        let mut model = Model::default();
        let changes = model.apply(&event("s1", None, json!({ "hook_event_name": "SessionStart" })));
        assert_eq!(changes.first(), Some(&Change::ShowSession));

        // Background activity in another session doesn't take over the room.
        assert!(model.apply(&sub_tool("s2", "a1")).is_empty());
        assert_eq!(model.shown().unwrap().id, "s1");

        // The user typing there does, and the room catches up with its agents.
        let changes = model.apply(&event("s2", None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" })));
        assert_eq!(changes[0], Change::ShowSession);
        assert!(changes.iter().any(|c| matches!(c, Change::Joined { id, .. } if id == "a1")));
        assert_eq!(model.shown().unwrap().id, "s2");
    }

    #[test]
    fn room_switches_to_the_working_session_when_the_shown_one_is_quiet() {
        let mut model = Model::default();
        model.apply(&event("other", None, json!({ "hook_event_name": "SessionStart" })));
        for i in 0..QUIET_EVENTS {
            let changes = model.apply(&sub_tool("busy", &format!("a{i}")));
            if i + 1 < QUIET_EVENTS {
                assert!(changes.is_empty());
            }
        }
        assert_eq!(model.shown().unwrap().id, "busy");
    }

    #[test]
    fn sub_agent_greets_with_its_task() {
        let mut model = Model::default();
        model.apply(&event(
            "s",
            None,
            json!({ "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "t",
                     "tool_response": { "agentId": "a1", "description": "Count Rust files", "isAsync": true } }),
        ));
        let changes = model.apply(&event("s", Some(("a1", "Explore")), json!({ "hook_event_name": "SubagentStart" })));
        assert_eq!(
            changes[0],
            Change::Joined { id: "a1".into(), agent_type: "Explore".into(), greeting: "Count Rust files".into() }
        );
        assert_eq!(model.shown().unwrap().agent("a1").unwrap().task.as_deref(), Some("Count Rust files"));
    }

    #[test]
    fn handback_delivers_once_then_leaves() {
        let mut model = Model::default();
        model.apply(&sub_tool("s", "a1"));
        let changes = model.apply(&event(
            "s",
            Some(("a1", "Explore")),
            json!({ "hook_event_name": "PreToolUse", "tool_name": "SubagentHandback", "tool_input": { "message": "Found 3" }, "tool_use_id": "h" }),
        ));
        assert!(changes.contains(&Change::Deliver { id: "a1".into() }));
        let changes = model.apply(&event("s", Some(("a1", "Explore")), json!({ "hook_event_name": "SubagentStop" })));
        assert_eq!(changes, vec![Change::Leave { id: "a1".into() }]);
        let info = model.shown().unwrap().agent("a1").unwrap();
        assert_eq!((info.status, info.report.as_deref()), (Status::Done, Some("Found 3")));
    }

    #[test]
    fn stop_without_handback_still_delivers_last_message() {
        let mut model = Model::default();
        model.apply(&sub_tool("s", "a1"));
        let changes = model.apply(&event(
            "s",
            Some(("a1", "Explore")),
            json!({ "hook_event_name": "SubagentStop", "last_assistant_message": "All good" }),
        ));
        assert!(changes.contains(&Change::Say { who: Who::Sub("a1".into()), line: "All good".into() }));
        let deliver = changes.iter().position(|c| *c == Change::Deliver { id: "a1".into() }).unwrap();
        let leave = changes.iter().position(|c| *c == Change::Leave { id: "a1".into() }).unwrap();
        assert!(deliver < leave);
    }

    #[test]
    fn internal_helpers_are_ignored() {
        let mut model = Model::default();
        model.apply(&event("s", None, json!({ "hook_event_name": "SessionStart" })));
        let helper = event(
            "s",
            Some(("zz9", "")),
            json!({ "hook_event_name": "SubagentStop", "last_assistant_message": "recap" }),
        );
        assert!(model.is_internal_helper(&helper));
        assert!(model.apply(&helper).is_empty());
    }

    #[test]
    fn permission_wait_ends_when_the_tool_runs() {
        let mut model = Model::default();
        let ask = event(
            "s",
            None,
            json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": { "command": "ls" } }),
        );
        assert!(model.apply(&ask).contains(&Change::Pose { who: Who::Main, pose: Pose::Waiting }));
        let done = event("s", None, json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "t" }));
        assert_eq!(model.apply(&done), vec![Change::Pose { who: Who::Main, pose: Pose::Thinking }]);
    }

    #[test]
    fn sessions_are_listed_by_recent_activity_and_can_be_picked() {
        let mut model = Model::default();
        model.apply(&event("s1", None, json!({ "hook_event_name": "SessionStart", "cwd": "/Users/me/smart_haus" })));
        model.apply(&sub_tool("s2", "a1"));
        let order: Vec<_> = model.sessions().iter().map(|s| s.id.clone()).collect();
        assert_eq!(order, ["s2", "s1"]);
        assert_eq!(model.sessions()[1].name(), "smart_haus");
        assert!(model.sessions()[0].is_busy());

        assert!(model.show("s1").is_empty(), "already shown");
        let changes = model.show("s2");
        assert_eq!(changes[0], Change::ShowSession);
        assert!(changes.iter().any(|c| matches!(c, Change::Joined { id, .. } if id == "a1")));
        assert!(model.show("nope").is_empty());
    }

    #[test]
    fn main_keeps_thinking_while_its_helpers_work() {
        let mut model = Model::default();
        model.apply(&event("s", None, json!({ "hook_event_name": "SessionStart" })));
        model.apply(&event("s", None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" })));
        model.apply(&event(
            "s",
            None,
            json!({ "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "t",
                     "tool_response": { "agentId": "a1", "description": "Find it", "isAsync": true } }),
        ));
        model.apply(&event("s", Some(("a1", "Explore")), json!({ "hook_event_name": "SubagentStart" })));
        model.apply(&sub_tool("s", "a1"));
        assert_eq!(model.shown().unwrap().main_pose, Pose::Thinking);
    }

    #[test]
    fn report_label_survives_the_agent_leaving() {
        let mut model = Model::default();
        model.apply(&sub_tool("s", "a1"));
        model.apply(&event("s", Some(("a1", "Explore")), json!({ "hook_event_name": "SubagentStop" })));
        let report = event(
            "s",
            None,
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": "<agent-message from=\"a1\">\n[Subagent hand-back]" }),
        );
        assert!(model.apply(&report).contains(&Change::Say { who: Who::Main, line: "Got it from Explore".into() }));
        assert_eq!(model.agent_type("s", "a1"), Some("Explore"));
    }
}
