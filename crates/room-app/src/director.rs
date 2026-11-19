//! The Director (V2.md phase 1): decides what every robot in the 3D HQ does, from the
//! model alone. `view3d` only plays it: it walks robots to the place chosen here and
//! picks the clip for the pose. Everything here is a pure function over
//! `SessionState`, so the rules are unit tested without rendering.

use bevy::prelude::*;

use crate::model::{Model, Pose, SessionState, Status, Who};
use crate::world3d::Spot;

/// Claude's own colour in the panels.
pub const MAIN_ACCENT: Color = rgb(0xE0, 0x7A, 0x4F);
const SUB_ACCENTS: [Color; 8] = [
    rgb(0x5B, 0x8D, 0xEF),
    rgb(0x9B, 0x6B, 0xDF),
    rgb(0x4C, 0xAF, 0x7A),
    rgb(0xF2, 0xA5, 0x41),
    rgb(0x3F, 0xB8, 0xAF),
    rgb(0xE4, 0x6C, 0x9A),
    rgb(0x7A, 0x8B, 0xA6),
    rgb(0xC9, 0xA2, 0x27),
];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

/// Stable accent per agent type: the common ones get fixed colours.
pub fn accent_for(agent_type: &str) -> Color {
    let index = match agent_type {
        "Explore" => 0,
        "Plan" => 1,
        "general-purpose" => 2,
        other => other.bytes().fold(0usize, |hash, b| hash.wrapping_mul(31).wrapping_add(b as usize)) % SUB_ACCENTS.len(),
    };
    SUB_ACCENTS[index]
}

/// One robot the HQ shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Role {
    /// Its session (empty for the Claude waiting before any session).
    pub session: String,
    /// Its session's seat at the main desk (0 is the centre).
    pub seat: usize,
    pub who: Who,
    pub pose: Pose,
    /// Its colour (sub-agents by type).
    pub accent: Color,
    /// A sub-agent that has stopped: it brings its report over, then leaves.
    pub done: bool,
    /// Its last tool call failed: a warning until something succeeds.
    pub failed: bool,
}

/// Everyone in the HQ: every live session's Claude and sub-agents (V2.md phase
/// 3b), or one Claude waiting at its desk before any session.
pub fn cast_all(model: &Model) -> Vec<Role> {
    let live = model.live();
    if live.is_empty() {
        return cast(&SessionState::default(), 0);
    }
    live.into_iter().enumerate().flat_map(|(seat, session)| cast(session, seat)).collect()
}

/// Everyone in one session: Claude first, then the sub-agents in the order they
/// joined (finished ones included, so a robot can finish its hand-over).
pub fn cast(session: &SessionState, seat: usize) -> Vec<Role> {
    let main = Role {
        session: session.id.clone(),
        seat,
        who: Who::Main,
        pose: session.main_pose,
        accent: MAIN_ACCENT,
        done: false,
        failed: session.main_failed,
    };
    let subs = session.agents.iter().map(|(id, info)| Role {
        session: session.id.clone(),
        seat,
        who: Who::Sub(id.clone()),
        pose: info.pose,
        accent: accent_for(&info.agent_type),
        done: info.status == Status::Done,
        failed: info.failed,
    });
    std::iter::once(main).chain(subs).collect()
}

/// Where a robot goes for what it is doing (PLAN.md M26, M27), given where it is now
/// and its desk. Robots work at their desk and walk only for what deserves a look,
/// because motion at the edge of vision draws the eye: a steady stream of tool calls
/// would keep the room moving and tell the user nothing.
///
/// - Claude works at the command station. Its activity hologram shows which kind of
///   tool it uses.
/// - A sub-agent takes a crew desk (`free`, see [`free_desk`]) when it first works,
///   and keeps it for every tool until it leaves.
/// - Walks that mean something: a sub-agent comes to the command station for its
///   task, to hand over its report, and when it needs the user (it waves there).
///   Arriving and leaving through the teleporter is `view3d`'s.
pub fn location_for(pose: Pose, is_main: bool, now: Spot, desk: &mut Option<u8>, free: impl FnOnce() -> u8) -> Spot {
    if is_main {
        return Spot::Command;
    }
    match pose {
        Pose::Delegating | Pose::Reporting | Pose::Waiting => Spot::Command,
        Pose::Thinking | Pose::Idle => now,
        Pose::Reading | Pose::Web | Pose::Typing | Pose::Terminal => Spot::Desk(*desk.get_or_insert_with(free)),
    }
}

/// The desk a sub-agent starting work takes: the first free one, or when all `desks`
/// are taken, the one fewest robots share (they then stand side by side).
pub fn free_desk(taken: &[u8], desks: u8) -> u8 {
    (0..desks).min_by_key(|desk| (taken.iter().filter(|taken| *taken == desk).count(), *desk)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use room_proto::event::RoomEvent;
    use serde_json::{Value, json};

    fn event(sub: Option<&str>, fields: Value) -> RoomEvent {
        let mut hook = json!({ "session_id": "s", "cwd": "/x" });
        hook.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
        if let Some(id) = sub {
            hook["agent_id"] = json!(id);
            hook["agent_type"] = json!("Explore");
        }
        RoomEvent::parse(&hook).unwrap()
    }

    fn tool(sub: Option<&str>, name: &str, input: Value) -> RoomEvent {
        event(sub, json!({ "hook_event_name": "PreToolUse", "tool_name": name, "tool_input": input, "tool_use_id": "t" }))
    }

    fn places(model: &Model, now: Spot) -> Vec<(Who, Spot, bool)> {
        cast(model.shown().unwrap(), 0)
            .into_iter()
            .map(|role| {
                let is_main = role.who == Who::Main;
                (role.who, location_for(role.pose, is_main, now, &mut None, || 3), role.done)
            })
            .collect()
    }

    #[test]
    fn claude_works_at_command_and_sub_agents_at_a_crew_desk() {
        let mut model = Model::default();
        model.apply(&event(None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" })));
        assert_eq!(places(&model, Spot::Command), vec![(Who::Main, Spot::Command, false)]);

        // Claude reads and edits without leaving its seat.
        model.apply(&tool(None, "Read", json!({ "file_path": "/x/a.rs" })));
        assert_eq!(places(&model, Spot::Command)[0].1, Spot::Command);

        // A sub-agent goes to a crew desk (the free one the view hands it).
        model.apply(&tool(Some("a1"), "Bash", json!({ "command": "ls" })));
        model.apply(&tool(None, "Edit", json!({ "file_path": "/x/a.rs", "old_string": "a", "new_string": "b" })));
        let now = places(&model, Spot::Command);
        assert_eq!(now[0].1, Spot::Command);
        assert_eq!(now[1], (Who::Sub("a1".into()), Spot::Desk(3), false));

        // It stops: done, bringing its report to the command station.
        model.apply(&event(Some("a1"), json!({ "hook_event_name": "SubagentStop", "last_assistant_message": "found it" })));
        assert_eq!(places(&model, Spot::Desk(3))[1], (Who::Sub("a1".into()), Spot::Command, true));
    }

    #[test]
    fn a_sub_agent_keeps_its_desk_whatever_tool_it_uses() {
        let mut desk = None;
        let mut place = |pose, now| location_for(pose, false, now, &mut desk, || 5);
        assert_eq!(place(Pose::Delegating, Spot::Command), Spot::Command, "comes for its task");
        assert_eq!(place(Pose::Thinking, Spot::Command), Spot::Command);
        assert_eq!(place(Pose::Reading, Spot::Command), Spot::Desk(5), "first tool: takes a desk");
        for pose in [Pose::Terminal, Pose::Typing, Pose::Web, Pose::Thinking] {
            assert_eq!(place(pose, Spot::Desk(5)), Spot::Desk(5), "{pose:?}");
        }
        // What deserves a look: needing the user, the report.
        assert_eq!(place(Pose::Waiting, Spot::Desk(5)), Spot::Command);
        assert_eq!(place(Pose::Reporting, Spot::Desk(5)), Spot::Command);
    }

    #[test]
    fn sub_agents_take_the_first_free_desk_then_share() {
        assert_eq!(free_desk(&[], 8), 0);
        assert_eq!(free_desk(&[0, 1, 3], 8), 2);
        // All taken: the desk with the fewest robots, the first of those.
        assert_eq!(free_desk(&[0, 1, 2], 3), 0);
        assert_eq!(free_desk(&[0, 0, 1, 2], 3), 1);
    }

    #[test]
    fn every_live_session_has_its_claude_in_the_hq() {
        let mut model = Model::default();
        // Before any session: one Claude waits at the desk.
        let waiting = cast_all(&model);
        assert_eq!((waiting.len(), waiting[0].who.clone()), (1, Who::Main));

        let prompt = |id: &str| {
            let mut hook = json!({ "session_id": id, "hook_event_name": "UserPromptSubmit", "prompt": "go" });
            hook["cwd"] = json!(format!("/{id}"));
            RoomEvent::parse(&hook).unwrap()
        };
        model.apply(&prompt("a"));
        model.apply(&prompt("b"));
        model.apply(&prompt("a"));
        let seats: Vec<(String, usize)> = cast_all(&model).into_iter().map(|role| (role.session, role.seat)).collect();
        // Seats follow when the session first appeared, not the latest activity.
        assert_eq!(seats, vec![("a".to_owned(), 0), ("b".to_owned(), 1)]);

        // An ended session's robots leave the HQ.
        model.apply(&RoomEvent::parse(&json!({ "session_id": "a", "hook_event_name": "SessionEnd", "reason": "exit" })).unwrap());
        let left: Vec<String> = cast_all(&model).into_iter().map(|role| role.session).collect();
        assert_eq!(left, vec!["b".to_owned()]);
    }

    #[test]
    fn a_failed_tool_warns_until_something_succeeds() {
        let mut model = Model::default();
        let failed = |model: &Model| cast(model.shown().unwrap(), 0)[0].failed;
        model.apply(&event(None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" })));
        model.apply(&event(
            None,
            json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash", "tool_use_id": "t", "error": "exit 1", "is_interrupt": false }),
        ));
        assert!(failed(&model));
        // Thinking it over doesn't clear it; the next tool call that works does.
        model.apply(&tool(None, "Bash", json!({ "command": "ls" })));
        assert!(failed(&model));
        model.apply(&event(None, json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "t2" })));
        assert!(!failed(&model));
    }

    #[test]
    fn claude_never_leaves_its_seat() {
        for pose in [Pose::Idle, Pose::Thinking, Pose::Reading, Pose::Terminal, Pose::Waiting, Pose::Delegating] {
            assert_eq!(location_for(pose, true, Spot::Desk(2), &mut None, || 0), Spot::Command, "{pose:?}");
        }
    }

    #[test]
    fn accents_are_stable_per_type() {
        assert_eq!(accent_for("Explore"), SUB_ACCENTS[0]);
        assert_eq!(accent_for("Plan"), SUB_ACCENTS[1]);
        assert_eq!(accent_for("ecc:code-reviewer"), accent_for("ecc:code-reviewer"));
    }
}
