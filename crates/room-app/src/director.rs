//! The Director (V2.md phase 1): decides what every robot in the 3D HQ does, from the
//! model alone. `view3d` only plays it: it walks robots to the place chosen here and
//! picks the clip for the pose. Everything here is a pure function over
//! `SessionState`, so the rules are unit tested without rendering.

use bevy::prelude::*;

use crate::model::{Model, Pose, SessionState, Status, Who};
use crate::world3d::Location;

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
    let role = |who, pose, accent, done| Role { session: session.id.clone(), seat, who, pose, accent, done };
    let main = role(Who::Main, session.main_pose, MAIN_ACCENT, false);
    let subs = session.agents.iter().map(|(id, info)| Role {
        session: session.id.clone(),
        seat,
        who: Who::Sub(id.clone()),
        pose: info.pose,
        accent: accent_for(&info.agent_type),
        done: info.status == Status::Done,
    });
    std::iter::once(main).chain(subs).collect()
}

/// Where a robot goes for what it is doing (PLAN.md M17), given where it is now.
/// Claude thinks or waits where it stands (a new message no longer sends it back to
/// the centre; it says "On it!" on the spot) and goes home only when the turn ends.
/// Sub-agents waiting, getting their task or bringing a report go to the main desk
/// (to its visitor spots).
pub fn location_for(pose: Pose, is_main: bool, now: Location) -> Location {
    match pose {
        Pose::Reading => Location::Bookshelf,
        Pose::Web => Location::Research,
        Pose::Typing => Location::WritingDesk,
        Pose::Terminal => Location::Lab,
        Pose::Thinking | Pose::Waiting if is_main => now,
        Pose::Idle | Pose::Thinking | Pose::Delegating | Pose::Waiting | Pose::Reporting => Location::MainDesk,
    }
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

    fn places(model: &Model, now: Location) -> Vec<(Who, Location, bool)> {
        cast(model.shown().unwrap(), 0)
            .into_iter()
            .map(|role| {
                let is_main = role.who == Who::Main;
                (role.who, location_for(role.pose, is_main, now), role.done)
            })
            .collect()
    }

    #[test]
    fn a_recorded_turn_moves_the_robots_where_the_work_happens() {
        let mut model = Model::default();
        model.apply(&event(None, json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" })));
        // A new prompt: Claude stays wherever it stands.
        assert_eq!(places(&model, Location::Lab), vec![(Who::Main, Location::Lab, false)]);

        model.apply(&tool(None, "Read", json!({ "file_path": "/x/a.rs" })));
        assert_eq!(places(&model, Location::MainDesk)[0].1, Location::Bookshelf);

        // A sub-agent joins and works at the lab; Claude goes on editing.
        model.apply(&tool(Some("a1"), "Bash", json!({ "command": "ls" })));
        model.apply(&tool(None, "Edit", json!({ "file_path": "/x/a.rs", "old_string": "a", "new_string": "b" })));
        let now = places(&model, Location::Bookshelf);
        assert_eq!(now[0].1, Location::WritingDesk);
        assert_eq!(now[1], (Who::Sub("a1".into()), Location::Lab, false));

        // It stops: done, bringing its report to the main desk.
        model.apply(&event(Some("a1"), json!({ "hook_event_name": "SubagentStop", "last_assistant_message": "found it" })));
        assert_eq!(places(&model, Location::WritingDesk)[1], (Who::Sub("a1".into()), Location::MainDesk, true));

        // The turn ends: Claude walks home.
        model.apply(&event(None, json!({ "hook_event_name": "Stop" })));
        assert_eq!(places(&model, Location::WritingDesk)[0].1, Location::MainDesk);
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
    fn claude_takes_a_new_message_where_it_stands() {
        assert_eq!(location_for(Pose::Thinking, true, Location::Lab), Location::Lab);
        assert_eq!(location_for(Pose::Waiting, true, Location::Lab), Location::Lab);
        assert_eq!(location_for(Pose::Idle, true, Location::Lab), Location::MainDesk);
        // A sub-agent waiting still comes to the main desk.
        assert_eq!(location_for(Pose::Waiting, false, Location::Lab), Location::MainDesk);
    }

    #[test]
    fn accents_are_stable_per_type() {
        assert_eq!(accent_for("Explore"), SUB_ACCENTS[0]);
        assert_eq!(accent_for("Plan"), SUB_ACCENTS[1]);
        assert_eq!(accent_for("ecc:code-reviewer"), accent_for("ecc:code-reviewer"));
    }
}
