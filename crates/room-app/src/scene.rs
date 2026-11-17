//! The room (PLAN.md 4.1): floor, wall, the main Claude character at the center desk,
//! and one desk per sub-agent on the seats around it. M3 is static; M4 animates it.

use std::collections::HashMap;
use std::time::Duration;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use room_proto::event::{AgentRef, EventKind, RoomEvent};

/// The design-space size of the room. The camera always shows all of it.
pub const ROOM_SIZE: Vec2 = Vec2::new(900.0, 640.0);
/// A sub-agent's desk stays this long after it stops, before it disappears.
const LEAVE_AFTER: Duration = Duration::from_millis(1500);

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
const DOOR: Color = rgb(0xD2, 0xC1, 0xA9);
const DESK: Color = rgb(0xD9, 0xB4, 0x8F);
const DESK_EDGE: Color = rgb(0xC2, 0x99, 0x70);
const LAPTOP: Color = rgb(0x3A, 0x3F, 0x4B);
const EYES: Color = rgb(0x2E, 0x2A, 0x27);
const LEAF: Color = rgb(0x7F, 0xB0, 0x7A);
const LEAF_DARK: Color = rgb(0x5E, 0x93, 0x5C);
const POT: Color = rgb(0xC9, 0x8B, 0x6B);
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

#[derive(Component)]
pub struct Leaving(Timer);

struct Seat {
    slot: usize,
    entity: Entity,
}

/// Which sub-agent sits where, for the session shown in the room.
#[derive(Resource, Default)]
pub struct Roster {
    session: Option<String>,
    seated: HashMap<String, Seat>,
    /// Desks of agents that just finished, still on screen: entity ⇒ seat index.
    leaving: HashMap<Entity, usize>,
    /// Every sub-agent seen this session, seated or gone: id ⇒ type. Reports arrive
    /// after the agent stopped, and still need its name.
    known: HashMap<String, String>,
}

impl Roster {
    pub fn agent_type(&self, agent_id: &str) -> Option<&str> {
        self.known.get(agent_id).map(String::as_str)
    }

    /// True while something in the room still changes on its own.
    pub fn is_animating(&self) -> bool {
        !self.leaving.is_empty()
    }

    fn free_slot(&self) -> Option<usize> {
        (0..SEATS.len()).find(|slot| {
            !self.seated.values().any(|seat| seat.slot == *slot) && !self.leaving.values().any(|s| s == slot)
        })
    }

    fn clear(&mut self, commands: &mut Commands) {
        self.known.clear();
        for seat in self.seated.drain().map(|(_, seat)| seat) {
            commands.entity(seat.entity).despawn();
        }
        for entity in self.leaving.drain().map(|(entity, _)| entity) {
            commands.entity(entity).despawn();
        }
    }
}

pub fn spawn_room(mut commands: Commands) {
    commands.spawn((
        WorldCamera,
        Camera2d,
        Msaa::Sample4,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin { min_width: ROOM_SIZE.x, min_height: ROOM_SIZE.y },
            ..OrthographicProjection::default_2d()
        }),
    ));

    let floor = ROOM_SIZE - Vec2::splat(40.0);
    commands.spawn((rounded(floor, 28.0, FLOOR, Some((FLOOR_EDGE, 4.0))), Transform::from_xyz(0.0, 0.0, -10.0)));
    // Back wall with two windows.
    let wall_height = 90.0;
    let wall_y = floor.y / 2.0 - wall_height / 2.0;
    commands.spawn((rounded(Vec2::new(floor.x, wall_height), 28.0, WALL, None), Transform::from_xyz(0.0, wall_y, -9.0)));
    for x in [-170.0, 170.0] {
        commands.spawn((
            rounded(Vec2::new(150.0, 56.0), 8.0, WINDOW_GLASS, Some((WINDOW_FRAME, 6.0))),
            Transform::from_xyz(x, wall_y + 4.0, -8.0),
        ));
    }
    // Door gap in the front edge.
    commands.spawn((rounded(Vec2::new(130.0, 12.0), 6.0, DOOR, None), Transform::from_xyz(0.0, -floor.y / 2.0, -8.0)));
    // Rug under the main desk.
    commands.spawn((ellipse(Vec2::new(210.0, 140.0), RUG, Some((RUG_EDGE, 3.0))), Transform::from_xyz(0.0, 0.0, -7.0)));
    spawn_plant(&mut commands, Vec2::new(-floor.x / 2.0 + 50.0, wall_y - 70.0));
    spawn_plant(&mut commands, Vec2::new(floor.x / 2.0 - 50.0, wall_y - 70.0));

    spawn_character(&mut commands, MAIN_SEAT, MAIN_SCALE, MAIN_ACCENT, "Claude");
}

/// Seats sub-agents as they appear and clears desks when they finish.
pub fn apply_events(mut commands: Commands, mut events: MessageReader<HookEvent>, mut roster: ResMut<Roster>) {
    for HookEvent(event) in events.read() {
        if !follow_session(&mut roster, event, &mut commands) {
            continue;
        }
        match (&event.agent, &event.kind) {
            (_, EventKind::SessionEnded { .. }) => roster.clear(&mut commands),
            (AgentRef::Sub { id, .. }, EventKind::SubagentStopped { .. }) => {
                // Unknown ids are Claude Code's internal helpers (PLAN.md 3.4): ignore.
                if let Some(seat) = roster.seated.remove(id) {
                    commands.entity(seat.entity).insert(Leaving(Timer::new(LEAVE_AFTER, TimerMode::Once)));
                    roster.leaving.insert(seat.entity, seat.slot);
                }
            }
            // Any event from an unseen sub-agent seats it, so starting the app in the
            // middle of a session still shows everyone who is working.
            (AgentRef::Sub { id, agent_type }, _) if !agent_type.is_empty() && !roster.seated.contains_key(id) => {
                roster.known.insert(id.clone(), agent_type.clone());
                let Some(slot) = roster.free_slot() else { continue };
                let entity = spawn_character(&mut commands, SEATS[slot], 1.0, accent_for(agent_type), agent_type);
                roster.seated.insert(id.clone(), Seat { slot, entity });
            }
            _ => {}
        }
    }
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

pub fn clear_leavers(
    mut commands: Commands,
    time: Res<Time>,
    mut roster: ResMut<Roster>,
    mut leavers: Query<(Entity, &mut Leaving)>,
) {
    for (entity, mut leaving) in &mut leavers {
        if leaving.0.tick(time.delta()).is_finished() {
            commands.entity(entity).despawn();
            roster.leaving.remove(&entity);
        }
    }
}

/// Stable accent per agent type: the common ones get fixed colors.
pub fn accent_for(agent_type: &str) -> Color {
    let index = match agent_type {
        "Explore" => 0,
        "Plan" => 1,
        "general-purpose" => 2,
        other => other.bytes().fold(0usize, |hash, b| hash.wrapping_mul(31).wrapping_add(b as usize)) % SUB_ACCENTS.len(),
    };
    SUB_ACCENTS[index]
}

/// A seated character behind a desk with a laptop, and a name tag. Returns the parent.
fn spawn_character(commands: &mut Commands, at: Vec2, scale: f32, accent: Color, name: &str) -> Entity {
    let s = scale;
    commands
        .spawn((Transform::from_translation(at.extend(0.0)), Visibility::default()))
        .with_children(|parent| {
            // Shadow, body, head, eyes: drawn back to front.
            parent.spawn((ellipse(Vec2::new(40.0, 10.0) * s, FLOOR_EDGE, None), Transform::from_xyz(0.0, -4.0 * s, 0.1)));
            parent.spawn((rounded(Vec2::new(46.0, 54.0) * s, 20.0 * s, accent, None), Transform::from_xyz(0.0, 8.0 * s, 0.2)));
            parent.spawn((circle(21.0 * s, accent), Transform::from_xyz(0.0, 44.0 * s, 0.3)));
            for x in [-7.5, 7.5] {
                parent.spawn((ellipse(Vec2::new(3.2, 4.4) * s, EYES, None), Transform::from_xyz(x * s, 46.0 * s, 0.4)));
            }
            // Desk in front of the body, laptop lid facing us.
            parent.spawn((
                rounded(Vec2::new(112.0, 44.0) * s, 10.0 * s, DESK, Some((DESK_EDGE, 3.0 * s))),
                Transform::from_xyz(0.0, -20.0 * s, 0.5),
            ));
            parent.spawn((rounded(Vec2::new(42.0, 26.0) * s, 4.0 * s, LAPTOP, None), Transform::from_xyz(0.0, -6.0 * s, 0.6)));
            parent.spawn((circle(3.0 * s, accent), Transform::from_xyz(0.0, -6.0 * s, 0.7)));
            parent.spawn((
                Text2d(name.to_owned()),
                TextFont::from_font_size(13.0 * s.sqrt()),
                TextColor(TEXT),
                Transform::from_xyz(0.0, -54.0 * s, 0.8),
            ));
        })
        .id()
}

fn spawn_plant(commands: &mut Commands, at: Vec2) {
    commands
        .spawn((Transform::from_translation(at.extend(-6.0)), Visibility::default()))
        .with_children(|parent| {
            parent.spawn((rounded(Vec2::new(34.0, 30.0), 6.0, POT, None), Transform::from_xyz(0.0, -18.0, 0.1)));
            for (x, y, r, color) in [(-12.0, 6.0, 16.0, LEAF_DARK), (12.0, 8.0, 15.0, LEAF_DARK), (0.0, 16.0, 18.0, LEAF)] {
                parent.spawn((circle(r, color), Transform::from_xyz(x, y, 0.2)));
            }
        });
}

fn rounded(size: Vec2, radius: f32, fill: Color, stroke: Option<(Color, f32)>) -> Shape {
    let rect = shapes::Rectangle { extents: size, origin: RectangleOrigin::Center, radii: Some(BorderRadii::single(radius)) };
    let builder = ShapeBuilder::with(&rect).fill(fill);
    match stroke {
        Some(stroke) => builder.stroke(stroke).build(),
        None => builder.build(),
    }
}

fn ellipse(radii: Vec2, fill: Color, stroke: Option<(Color, f32)>) -> Shape {
    let shape = shapes::Ellipse { radii, center: Vec2::ZERO };
    let builder = ShapeBuilder::with(&shape).fill(fill);
    match stroke {
        Some(stroke) => builder.stroke(stroke).build(),
        None => builder.build(),
    }
}

fn circle(radius: f32, fill: Color) -> Shape {
    ShapeBuilder::with(&shapes::Circle { radius, center: Vec2::ZERO }).fill(fill).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_are_stable_and_fixed_for_common_types() {
        assert_eq!(accent_for("Explore"), SUB_ACCENTS[0]);
        assert_eq!(accent_for("Plan"), SUB_ACCENTS[1]);
        assert_eq!(accent_for("ecc:code-reviewer"), accent_for("ecc:code-reviewer"));
    }
}
