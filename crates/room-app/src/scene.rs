//! The room (PLAN.md 4.1) and who is in it: the main Claude character at the center
//! desk, one desk per sub-agent around it. Hook events set each character's pose and
//! speech bubble; sub-agents walk in through the door, report back with a paper plane
//! and walk out again (animation in `anim.rs`).

use std::collections::HashMap;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use room_proto::describe::Activity;
use room_proto::event::{AgentRef, EventKind, RoomEvent};

use crate::anim::{
    self, Accent, Arrive, Bubble, BubbleRef, Done, Part, Phase, Pose, Rest, RigOf, Walk, say,
};

// Draw order is by z: room -100..-60, characters 0..9 (their desk 5..8 in front of the
// body), bubbles +30, planes 60. Never change an entity's z after spawning it: Bevy 0.20
// keeps the 2D sort key from when the entity was first drawn (see `anim::Walk::at`).

/// The design-space size of the room. The camera always shows all of it.
pub const ROOM_SIZE: Vec2 = Vec2::new(900.0, 640.0);
const FLOOR_SIZE: Vec2 = Vec2::new(860.0, 600.0);
const DOOR: Vec2 = Vec2::new(0.0, -300.0);
/// Characters reach their seat through this point behind the desk.
const BEHIND_DESK: Vec2 = Vec2::new(0.0, 85.0);
/// Stepping out sideways from behind a desk clears it at this distance.
const BESIDE_DESK: f32 = 88.0;

const MAIN_SEAT: Vec2 = Vec2::new(0.0, 10.0);
const MAIN_SCALE: f32 = 1.4;
/// Sub-agent seats, filled in this order: near the center first, left and right.
const SEATS: [Vec2; 8] = [
    Vec2::new(-300.0, 130.0),
    Vec2::new(300.0, 130.0),
    Vec2::new(-330.0, -20.0),
    Vec2::new(330.0, -20.0),
    Vec2::new(-290.0, -170.0),
    Vec2::new(290.0, -170.0),
    Vec2::new(-150.0, -215.0),
    Vec2::new(150.0, -215.0),
];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

pub const BACKDROP: Color = rgb(0xE9, 0xE1, 0xD4);
const FLOOR: Color = rgb(0xF7, 0xF1, 0xE8);
const FLOOR_EDGE: Color = rgb(0xE2, 0xD5, 0xC3);
const WALL: Color = rgb(0xEA, 0xDE, 0xCD);
const WINDOW_GLASS: Color = rgb(0xCF, 0xE6, 0xF2);
const WINDOW_FRAME: Color = rgb(0xFF, 0xFF, 0xFF);
const RUG: Color = rgb(0xF0, 0xE4, 0xD4);
const RUG_EDGE: Color = rgb(0xE3, 0xD3, 0xBE);
const DOOR_COLOR: Color = rgb(0xD2, 0xC1, 0xA9);
const DESK: Color = rgb(0xD9, 0xB4, 0x8F);
const DESK_EDGE: Color = rgb(0xC2, 0x99, 0x70);
const LAPTOP: Color = rgb(0x3A, 0x3F, 0x4B);
const EYES: Color = rgb(0x2E, 0x2A, 0x27);
const LEAF: Color = rgb(0x7F, 0xB0, 0x7A);
const LEAF_DARK: Color = rgb(0x5E, 0x93, 0x5C);
const POT: Color = rgb(0xC9, 0x8B, 0x6B);
const ALERT: Color = rgb(0xD9, 0x4D, 0x47);
pub const TEXT: Color = rgb(0x4A, 0x42, 0x3A);
const MAIN_ACCENT: Color = rgb(0xE0, 0x7A, 0x4F);
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

/// A parsed hook event, for every system that reacts to Claude Code.
#[derive(Message, Clone)]
pub struct HookEvent(pub RoomEvent);

/// The camera that draws the room (egui has its own).
#[derive(Component)]
pub struct WorldCamera;

/// The bundled UI font (Nunito, `assets/fonts`), embedded so `cargo install` works.
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

#[derive(Component)]
pub struct Desk;

/// A character in the room (main Claude or a sub-agent).
#[derive(Component)]
pub struct Agent;

struct Seat {
    slot: usize,
    character: Entity,
    desk: Entity,
    /// Already threw its report (through `SubagentHandback`).
    reported: bool,
}

/// Who sits where, for the session shown in the room.
#[derive(Resource, Default)]
pub struct Roster {
    session: Option<String>,
    main: Option<Entity>,
    seated: HashMap<String, Seat>,
    /// Desks of agents on their way out: desk ⇒ seat index, freed once the desk is gone.
    leaving: HashMap<Entity, usize>,
    /// Every sub-agent seen this session, seated or gone: id ⇒ type. Reports arrive
    /// after the agent stopped, and still need its name.
    known: HashMap<String, String>,
    /// What the main agent asked each sub-agent to do: id ⇒ description.
    briefs: HashMap<String, String>,
}

impl Roster {
    pub fn agent_type(&self, agent_id: &str) -> Option<&str> {
        self.known.get(agent_id).map(String::as_str)
    }

    fn free_slot(&self) -> Option<usize> {
        (0..SEATS.len()).find(|slot| {
            !self.seated.values().any(|seat| seat.slot == *slot) && !self.leaving.values().any(|s| s == slot)
        })
    }

    fn clear(&mut self, commands: &mut Commands) {
        self.known.clear();
        self.briefs.clear();
        for seat in self.seated.drain().map(|(_, seat)| seat) {
            commands.entity(seat.character).despawn();
            commands.entity(seat.desk).despawn();
        }
        // Their characters are mid-walk and despawn the desk on arrival; just forget them.
        self.leaving.clear();
    }
}

pub fn load_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = Font::from_bytes(include_bytes!("../../../assets/fonts/Nunito.ttf").to_vec());
    commands.insert_resource(UiFont(fonts.add(font)));
}

pub fn spawn_room(mut commands: Commands, font: Res<UiFont>, mut roster: ResMut<Roster>) {
    commands.spawn((
        WorldCamera,
        Camera2d,
        Msaa::Sample4,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin { min_width: ROOM_SIZE.x, min_height: ROOM_SIZE.y },
            ..OrthographicProjection::default_2d()
        }),
    ));

    commands.spawn((rounded(FLOOR_SIZE, 28.0, FLOOR, Some((FLOOR_EDGE, 4.0))), Transform::from_xyz(0.0, 0.0, -100.0)));
    // Back wall with two windows.
    let wall_height = 90.0;
    let wall_y = FLOOR_SIZE.y / 2.0 - wall_height / 2.0;
    commands.spawn((
        rounded(Vec2::new(FLOOR_SIZE.x, wall_height), 28.0, WALL, None),
        Transform::from_xyz(0.0, wall_y, -90.0),
    ));
    for x in [-170.0, 170.0] {
        commands.spawn((
            rounded(Vec2::new(150.0, 56.0), 8.0, WINDOW_GLASS, Some((WINDOW_FRAME, 6.0))),
            Transform::from_xyz(x, wall_y + 4.0, -80.0),
        ));
    }
    commands.spawn((rounded(Vec2::new(130.0, 12.0), 6.0, DOOR_COLOR, None), Transform::from_translation(DOOR.extend(-80.0))));
    commands.spawn((ellipse(Vec2::new(210.0, 140.0), RUG, Some((RUG_EDGE, 3.0))), Transform::from_xyz(0.0, 0.0, -70.0)));
    spawn_plant(&mut commands, Vec2::new(-FLOOR_SIZE.x / 2.0 + 50.0, wall_y - 70.0));
    spawn_plant(&mut commands, Vec2::new(FLOOR_SIZE.x / 2.0 - 50.0, wall_y - 70.0));

    spawn_desk(&mut commands, MAIN_SEAT, MAIN_SCALE, MAIN_ACCENT, "Claude", &font);
    let main = spawn_character(&mut commands, MAIN_SEAT, MAIN_SCALE, MAIN_ACCENT, "main", &font);
    roster.main = Some(main);
}

/// Turns hook events into poses, speech, arrivals and departures.
#[allow(clippy::too_many_arguments)]
pub fn apply_events(
    mut commands: Commands,
    mut events: MessageReader<HookEvent>,
    mut roster: ResMut<Roster>,
    font: Res<UiFont>,
    mut characters: Query<(&Agent, &Transform, &mut Pose, &BubbleRef)>,
    mut bubbles: Query<&mut Bubble>,
    desks: Query<(), With<Desk>>,
) {
    roster.leaving.retain(|desk, _| desks.contains(*desk));
    for HookEvent(event) in events.read() {
        if !follow_session(&mut roster, event, &mut commands) {
            continue;
        }
        let Some(main) = roster.main else { continue };
        let entity = match &event.agent {
            AgentRef::Main => {
                if let EventKind::ToolFinished { launched: Some(launched), .. } = &event.kind {
                    roster.briefs.insert(launched.agent_id.clone(), launched.description.clone());
                }
                if matches!(event.kind, EventKind::SessionEnded { .. }) {
                    roster.clear(&mut commands);
                }
                main
            }
            AgentRef::Sub { id, agent_type } => {
                if let EventKind::SubagentStopped { last_message } = &event.kind {
                    // Unknown ids are Claude Code's internal helpers (PLAN.md 3.4): ignore.
                    let Some(seat) = roster.seated.remove(id) else { continue };
                    if let Ok((_, transform, mut pose, bubble)) = characters.get_mut(seat.character) {
                        // No hand-back: it still brings its last words over to Claude.
                        if !seat.reported {
                            *pose = Pose::Reporting;
                            commands.entity(seat.character).insert(deliver(transform, seat.slot));
                            if let Some(text) = last_message {
                                say(&mut bubbles, bubble.0, text);
                            }
                        }
                    }
                    commands.entity(seat.character).insert(Done::new(seat.desk, DOOR));
                    roster.leaving.insert(seat.desk, seat.slot);
                    continue;
                }
                if agent_type.is_empty() {
                    continue;
                }
                match roster.seated.get(id) {
                    Some(seat) => seat.character,
                    None => match seat_agent(&mut commands, &mut roster, id, agent_type, &font) {
                        Some(character) => {
                            let brief = roster.briefs.get(id).cloned().unwrap_or_else(|| "On my way!".to_owned());
                            // The bubble entity exists only after commands apply; greet next event.
                            commands.entity(character).insert(PendingGreeting(brief));
                            continue;
                        }
                        None => continue,
                    },
                }
            }
        };

        let Ok((_, transform, mut pose, bubble)) = characters.get_mut(entity) else { continue };
        let (next_pose, line) = react(&event.kind, *pose, &roster);
        if let Some(next_pose) = next_pose {
            pose.set_if_neq(next_pose);
        }
        if let Some(line) = line {
            say(&mut bubbles, bubble.0, &line);
        }
        if let (AgentRef::Sub { id, .. }, EventKind::Handback { .. }) = (&event.agent, &event.kind)
            && let Some(seat) = roster.seated.get_mut(id)
        {
            seat.reported = true;
            commands.entity(entity).insert(deliver(transform, seat.slot));
        }
    }
}

/// A newly seated sub-agent says what it was asked to do once its bubble exists.
#[derive(Component)]
pub struct PendingGreeting(String);

pub fn greet(
    mut commands: Commands,
    greeters: Query<(Entity, &PendingGreeting, &BubbleRef)>,
    mut bubbles: Query<&mut Bubble>,
) {
    for (entity, greeting, bubble) in &greeters {
        say(&mut bubbles, bubble.0, &greeting.0);
        commands.entity(entity).remove::<PendingGreeting>();
    }
}

/// Pose and speech for an event, given the character's current pose.
fn react(kind: &EventKind, current: Pose, roster: &Roster) -> (Option<Pose>, Option<String>) {
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
        EventKind::PermissionAutoDenied { tool_name, .. } => (Some(Pose::Thinking), Some(format!("Not allowed: {tool_name}"))),
        EventKind::Handback { .. } => (Some(Pose::Reporting), Some("Here's my report!".to_owned())),
        EventKind::ReportReceived { from_agent } => {
            let from = roster.agent_type(from_agent).unwrap_or("my helper");
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
        Activity::Writing | Activity::Terminal => Pose::Typing,
        Activity::Web => Pose::Web,
        Activity::Delegating => Pose::Delegating,
        Activity::Reporting => Pose::Reporting,
        Activity::Thinking => Pose::Thinking,
    }
}

/// The walk that takes a report to Claude: step out beside the desk (toward the
/// middle), then over to a spot next to Claude's desk on the agent's side.
fn deliver(transform: &Transform, slot: usize) -> Walk {
    let here = transform.translation.truncate();
    let seat = SEATS[slot];
    let side = if seat.x < 0.0 { -1.0 } else { 1.0 };
    let beside_desk = seat + Vec2::new(-side * BESIDE_DESK, 0.0);
    // Beside the front of Claude's desk, low enough that the speech bubbles of
    // Claude and the visitor don't collide; visitors from one side stand in a column.
    let row = (slot / 2) as f32;
    let spot = MAIN_SEAT + Vec2::new(side * 150.0, -40.0 - row * 40.0);
    let claudes_hand = MAIN_SEAT + Vec2::new(side * 38.0, 0.0);
    Walk::new(here, beside_desk, spot, Arrive::HandOver { to: claudes_hand })
}

/// Gives a new sub-agent a desk and walks it in from the door.
fn seat_agent(commands: &mut Commands, roster: &mut Roster, id: &str, agent_type: &str, font: &UiFont) -> Option<Entity> {
    roster.known.insert(id.to_owned(), agent_type.to_owned());
    let slot = roster.free_slot()?;
    let seat = SEATS[slot];
    let accent = accent_for(agent_type);
    let character = spawn_character(commands, DOOR, 1.0, accent, id, font);
    let desk = spawn_desk(commands, seat, 1.0, accent, agent_type, font);
    commands.entity(character).insert(Walk::new(DOOR, seat + BEHIND_DESK, seat, Arrive::Sit));
    roster.seated.insert(id.to_owned(), Seat { slot, character, desk, reported: false });
    Some(character)
}

/// The room shows one session: the first one seen, until the user starts or types into
/// another. Returns whether this event belongs to the shown session.
fn follow_session(roster: &mut Roster, event: &RoomEvent, commands: &mut Commands) -> bool {
    match &roster.session {
        Some(current) if *current == event.session_id => true,
        Some(_) if !matches!(event.kind, EventKind::SessionStarted { .. } | EventKind::UserPrompt { .. }) => false,
        _ => {
            roster.clear(commands);
            roster.session = Some(event.session_id.clone());
            true
        }
    }
}

/// Stable accent per agent type: the common ones get fixed colors.
pub fn accent_for(agent_type: &str) -> Color {
    let index = match agent_type {
        "Explore" => 0,
        "Plan" => 1,
        "general-purpose" => 2,
        other => {
            other.bytes().fold(0usize, |hash, b| hash.wrapping_mul(31).wrapping_add(b as usize)) % SUB_ACCENTS.len()
        }
    };
    SUB_ACCENTS[index]
}

/// The static half of a seat: desk, laptop lid facing us, and a name tag.
fn spawn_desk(commands: &mut Commands, at: Vec2, s: f32, accent: Color, name: &str, font: &UiFont) -> Entity {
    let desk = commands.spawn((Desk, Transform::from_translation(at.extend(0.0)), Visibility::default())).id();
    commands.spawn((
        rounded(Vec2::new(112.0, 44.0) * s, 10.0 * s, DESK, Some((DESK_EDGE, 3.0 * s))),
        Transform::from_xyz(0.0, -20.0 * s, 5.0),
        ChildOf(desk),
    ));
    commands.spawn((rounded(Vec2::new(42.0, 26.0) * s, 4.0 * s, LAPTOP, None), Transform::from_xyz(0.0, -6.0 * s, 6.0), ChildOf(desk)));
    commands.spawn((circle(3.0 * s, accent), Transform::from_xyz(0.0, -6.0 * s, 7.0), ChildOf(desk)));
    commands.spawn((
        Text2d(name.to_owned()),
        TextFont::from_font_size(13.0 * s.sqrt()).with_font(font.0.clone()),
        TextColor(TEXT),
        Transform::from_xyz(0.0, -54.0 * s, 8.0),
        ChildOf(desk),
    ));
    desk
}

/// The moving half: a rigged character (see `anim::Part`) with a speech bubble.
fn spawn_character(commands: &mut Commands, at: Vec2, s: f32, accent: Color, key: &str, font: &UiFont) -> Entity {
    let root = commands
        .spawn((
            Agent,
            Accent(accent),
            Pose::Idle,
            Phase(anim::phase_for(key)),
            Transform::from_translation(at.extend(0.0)),
            Visibility::default(),
        ))
        .id();
    let mut part = |shape: Shape, part: Option<Part>, transform: Transform, parent: Entity| {
        let mut entity = commands.spawn((shape, transform, ChildOf(parent)));
        if let Some(part) = part {
            entity.insert((part, Rest(transform), RigOf(root)));
        }
        entity.id()
    };
    part(ellipse(Vec2::new(40.0, 10.0) * s, FLOOR_EDGE, None), None, Transform::from_xyz(0.0, -4.0 * s, 1.0), root);
    part(rounded(Vec2::new(46.0, 54.0) * s, 20.0 * s, accent, None), Some(Part::Body), Transform::from_xyz(0.0, 8.0 * s, 2.0), root);
    let head = part(circle(21.0 * s, accent), Some(Part::Head), Transform::from_xyz(0.0, 44.0 * s, 3.0), root);
    for x in [-7.5, 7.5] {
        part(ellipse(Vec2::new(3.2, 4.4) * s, EYES, None), Some(Part::Eye), Transform::from_xyz(x * s, 2.0 * s, 1.0), head);
    }
    // Arms rest on the desk, in front of it (desk parts sit at z 5–8).
    let arm = rounded(Vec2::new(10.0, 18.0) * s, 5.0 * s, accent, None);
    part(arm.clone(), Some(Part::ArmLeft), Transform::from_xyz(-25.0 * s, -2.0 * s, 6.5), root);
    part(arm, Some(Part::ArmRight), Transform::from_xyz(25.0 * s, -2.0 * s, 6.5), root);
    // The report sheet in the right hand, shown only while walking it over to Claude.
    part(anim::paper(accent), Some(Part::Paper), Transform::from_xyz(27.0 * s, 12.0 * s, 8.5), root);
    // "Needs your OK" badge, shown while waiting for a permission answer.
    let badge = part(circle(10.0 * s, ALERT), Some(Part::Badge), Transform::from_xyz(22.0 * s, 70.0 * s, 9.0), root);
    commands.spawn((
        Text2d("!".to_owned()),
        TextFont::from_font_size(15.0 * s).with_font(font.0.clone()),
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, 0.0, 1.0),
        ChildOf(badge),
    ));
    let bubble = anim::spawn_bubble(commands, root, 92.0 * s, font);
    commands.entity(root).insert(BubbleRef(bubble));
    root
}

fn spawn_plant(commands: &mut Commands, at: Vec2) {
    let plant = commands.spawn((Transform::from_translation(at.extend(-60.0)), Visibility::default())).id();
    commands.spawn((rounded(Vec2::new(34.0, 30.0), 6.0, POT, None), Transform::from_xyz(0.0, -18.0, 1.0), ChildOf(plant)));
    for (x, y, r, color) in [(-12.0, 6.0, 16.0, LEAF_DARK), (12.0, 8.0, 15.0, LEAF_DARK), (0.0, 16.0, 18.0, LEAF)] {
        commands.spawn((circle(r, color), Transform::from_xyz(x, y, 2.0), ChildOf(plant)));
    }
}

pub fn rounded(size: Vec2, radius: f32, fill: Color, stroke: Option<(Color, f32)>) -> Shape {
    let rect = shapes::Rectangle { extents: size, origin: RectangleOrigin::Center, radii: Some(BorderRadii::single(radius)) };
    let builder = ShapeBuilder::with(&rect).fill(fill);
    match stroke {
        Some(stroke) => builder.stroke(stroke).build(),
        None => builder.build(),
    }
}

pub fn ellipse(radii: Vec2, fill: Color, stroke: Option<(Color, f32)>) -> Shape {
    let shape = shapes::Ellipse { radii, center: Vec2::ZERO };
    let builder = ShapeBuilder::with(&shape).fill(fill);
    match stroke {
        Some(stroke) => builder.stroke(stroke).build(),
        None => builder.build(),
    }
}

pub fn circle(radius: f32, fill: Color) -> Shape {
    ShapeBuilder::with(&shapes::Circle { radius, center: Vec2::ZERO }).fill(fill).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use room_proto::event::ToolCall;
    use serde_json::json;

    #[test]
    fn accents_are_stable_and_fixed_for_common_types() {
        assert_eq!(accent_for("Explore"), SUB_ACCENTS[0]);
        assert_eq!(accent_for("Plan"), SUB_ACCENTS[1]);
        assert_eq!(accent_for("ecc:code-reviewer"), accent_for("ecc:code-reviewer"));
    }

    #[test]
    fn tools_set_pose_and_speech() {
        let roster = Roster::default();
        let read = EventKind::ToolStarted {
            tool_use_id: "t".into(),
            tool: ToolCall { name: "Read".into(), input: json!({ "file_path": "/x/PLAN.md" }) },
        };
        assert_eq!(react(&read, Pose::Idle, &roster), (Some(Pose::Reading), Some("Reading PLAN.md".into())));

        let ask = EventKind::PermissionAsked { tool: ToolCall { name: "Bash".into(), input: json!({ "command": "ls" }) } };
        assert_eq!(react(&ask, Pose::Typing, &roster).0, Some(Pose::Waiting));

        let done = EventKind::ToolFinished { tool_use_id: "t".into(), tool_name: "Bash".into(), launched: None };
        assert_eq!(react(&done, Pose::Waiting, &roster).0, Some(Pose::Thinking));
        assert_eq!(react(&done, Pose::Typing, &roster).0, None);
    }
}
