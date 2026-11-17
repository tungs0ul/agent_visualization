//! The room (PLAN.md 4.1) and who is in it: the main Claude character at the center
//! desk, one desk per sub-agent around it. Hook events set each character's pose and
//! speech bubble; sub-agents walk in through the door, report back with a paper plane
//! and walk out again (animation in `anim.rs`).

use std::collections::HashMap;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

use crate::anim::{
    self, Accent, Arrive, Bubble, BubbleRef, Done, Part, Phase, Rest, RigOf, Walk, say,
};
use crate::model::{Change, Pose, Who};

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

/// A model change for the scene to show (see `model::Change`).
#[derive(Message, Clone)]
pub struct SceneChange(pub Change);

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
}

/// Which entities draw whom. Who is in the room is decided by the model.
#[derive(Resource, Default)]
pub struct Roster {
    main: Option<Entity>,
    seated: HashMap<String, Seat>,
    /// Desks of agents on their way out: desk ⇒ seat index, freed once the desk is gone.
    leaving: HashMap<Entity, usize>,
}

impl Roster {
    fn free_slot(&self) -> Option<usize> {
        (0..SEATS.len()).find(|slot| {
            !self.seated.values().any(|seat| seat.slot == *slot) && !self.leaving.values().any(|s| s == slot)
        })
    }

    fn character(&self, who: &Who) -> Option<Entity> {
        match who {
            Who::Main => self.main,
            Who::Sub(id) => self.seated.get(id).map(|seat| seat.character),
        }
    }

    fn clear(&mut self, commands: &mut Commands) {
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

/// Shows model changes: arrivals, poses, speech, hand-overs, departures.
pub fn apply_changes(
    mut commands: Commands,
    mut changes: MessageReader<SceneChange>,
    mut roster: ResMut<Roster>,
    font: Res<UiFont>,
    mut characters: Query<(&Transform, &mut Pose, &BubbleRef), With<Agent>>,
    mut bubbles: Query<&mut Bubble>,
    desks: Query<(), With<Desk>>,
) {
    roster.leaving.retain(|desk, _| desks.contains(*desk));
    // Characters spawned in this batch can't be queried yet; their first pose and
    // words are applied next frame (see `apply_pending`).
    let mut pending: HashMap<Entity, Pending> = HashMap::new();
    for SceneChange(change) in changes.read() {
        match change {
            Change::ShowSession => roster.clear(&mut commands),
            Change::Joined { id, agent_type, greeting } => {
                if roster.seated.contains_key(id) {
                    continue;
                }
                if let Some(character) = seat_agent(&mut commands, &mut roster, id, agent_type, &font) {
                    pending.entry(character).or_default().line = Some(greeting.clone());
                }
            }
            Change::Pose { who, pose } => {
                let Some(entity) = roster.character(who) else { continue };
                match characters.get_mut(entity) {
                    Ok((_, mut current, _)) => {
                        current.set_if_neq(*pose);
                    }
                    Err(_) => pending.entry(entity).or_default().pose = Some(*pose),
                }
            }
            Change::Say { who, line } => {
                let Some(entity) = roster.character(who) else { continue };
                match characters.get(entity) {
                    Ok((_, _, bubble)) => say(&mut bubbles, bubble.0, line),
                    Err(_) => pending.entry(entity).or_default().line = Some(line.clone()),
                }
            }
            Change::Deliver { id } => {
                let Some(seat) = roster.seated.get(id) else { continue };
                if let Ok((transform, _, _)) = characters.get(seat.character) {
                    commands.entity(seat.character).insert(deliver(transform, seat.slot));
                }
            }
            Change::Leave { id } => {
                let Some(seat) = roster.seated.remove(id) else { continue };
                commands.entity(seat.character).insert(Done::new(seat.desk, DOOR));
                roster.leaving.insert(seat.desk, seat.slot);
            }
        }
    }
    for (entity, change) in pending {
        commands.entity(entity).insert(change);
    }
}

/// A pose and words for a character that didn't exist yet when they came in.
#[derive(Component, Default)]
pub struct Pending {
    pose: Option<Pose>,
    line: Option<String>,
}

pub fn apply_pending(
    mut commands: Commands,
    mut waiting: Query<(Entity, &Pending, &mut Pose, &BubbleRef)>,
    mut bubbles: Query<&mut Bubble>,
) {
    for (entity, pending, mut pose, bubble) in &mut waiting {
        if let Some(next) = pending.pose {
            *pose = next;
        }
        if let Some(line) = &pending.line {
            say(&mut bubbles, bubble.0, line);
        }
        commands.entity(entity).remove::<Pending>();
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
    let slot = roster.free_slot()?;
    let seat = SEATS[slot];
    let accent = accent_for(agent_type);
    let character = spawn_character(commands, DOOR, 1.0, accent, id, font);
    let desk = spawn_desk(commands, seat, 1.0, accent, agent_type, font);
    commands.entity(character).insert(Walk::new(DOOR, seat + BEHIND_DESK, seat, Arrive::Sit));
    roster.seated.insert(id.to_owned(), Seat { slot, character, desk });
    Some(character)
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

    #[test]
    fn accents_are_stable_and_fixed_for_common_types() {
        assert_eq!(accent_for("Explore"), SUB_ACCENTS[0]);
        assert_eq!(accent_for("Plan"), SUB_ACCENTS[1]);
        assert_eq!(accent_for("ecc:code-reviewer"), accent_for("ecc:code-reviewer"));
    }
}
