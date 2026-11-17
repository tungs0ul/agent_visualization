//! The room (PLAN.md 4.1) and who is in it: the main Claude character at the center
//! desk, one desk per sub-agent around it. Hook events set each character's pose and
//! speech bubble; sub-agents walk in through the door, report back with a paper plane
//! and walk out again (animation in `anim.rs`).

use std::collections::HashMap;

use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

use crate::anim::{
    self, Accent, Animating, Arrive, Bubble, BubbleRef, Chip, Done, Part, Phase, Place, Rest, RigOf, Station, Walk, say,
};
use crate::camera::CameraControl;
use crate::mini::Furniture;
use crate::model::{Change, Model, Pose, Who};

// Draw order is by z: room -100..-60, characters 0..9 (their desk 5..8 in front of the
// body), bubbles +30, planes 60. Never change an entity's z after spawning it: Bevy 0.20
// keeps the 2D sort key from when the entity was first drawn (see `anim::Walk::at`).

/// The design-space size of the office. A wide window shows all of it; a small one
/// follows the agents (`follow_action`).
pub const ROOM_SIZE: Vec2 = Vec2::new(1400.0, 900.0);
const FLOOR_SIZE: Vec2 = Vec2::new(1360.0, 860.0);
const DOOR: Vec2 = Vec2::new(0.0, -430.0);
/// Characters reach their seat through this point behind the desk.
const BEHIND_DESK: Vec2 = Vec2::new(0.0, 85.0);
/// Stepping out sideways from behind a desk clears it at this distance.
const BESIDE_DESK: f32 = 88.0;

const MAIN_SEAT: Vec2 = Vec2::new(0.0, 60.0);
const MAIN_SCALE: f32 = 1.4;
/// Sub-agent seats around Claude, filled in this order: near the center first.
const SEATS: [Vec2; 8] = [
    Vec2::new(-250.0, 200.0),
    Vec2::new(250.0, 200.0),
    Vec2::new(-265.0, 55.0),
    Vec2::new(265.0, 55.0),
    Vec2::new(-250.0, -95.0),
    Vec2::new(250.0, -95.0),
    Vec2::new(-115.0, -190.0),
    Vec2::new(115.0, -190.0),
];
/// An agent stays this long in a place before it walks to the next one, so bursts of
/// quick tool calls don't make it run back and forth.
const MIN_DWELL: f32 = 2.0;
/// Below this logical width the camera follows the agents instead of showing the
/// whole office.
const FOLLOW_BELOW_WIDTH: f32 = 640.0;
/// The smallest part of the office the follow camera zooms in to.
const MIN_VIEW: Vec2 = Vec2::new(560.0, 380.0);

/// A zone of the office: where agents go for a kind of work (PLAN.md 4.4b).
struct Zone {
    place: Place,
    center: Vec2,
    size: Vec2,
    tint: Color,
}

const ZONES: [Zone; 6] = [
    Zone { place: Place::Library, center: Vec2::new(-500.0, 235.0), size: Vec2::new(300.0, 190.0), tint: rgb(0xEC, 0xEF, 0xE0) },
    Zone { place: Place::Servers, center: Vec2::new(500.0, 235.0), size: Vec2::new(300.0, 190.0), tint: rgb(0xE5, 0xE9, 0xF0) },
    Zone { place: Place::WebBay, center: Vec2::new(-500.0, -115.0), size: Vec2::new(300.0, 230.0), tint: rgb(0xE3, 0xEE, 0xF3) },
    Zone { place: Place::Approval, center: Vec2::new(500.0, -115.0), size: Vec2::new(300.0, 230.0), tint: rgb(0xF5, 0xE6, 0xE1) },
    // The lounge by the door: decoration and a place to pass on the way out.
    Zone { place: Place::Door, center: Vec2::new(0.0, -340.0), size: Vec2::new(460.0, 140.0), tint: rgb(0xF2, 0xE8, 0xDB) },
    // Work in another repository than the session's (its sign lists them).
    Zone { place: Place::Away, center: Vec2::new(-470.0, -345.0), size: Vec2::new(370.0, 150.0), tint: rgb(0xEC, 0xE6, 0xF4) },
];
/// Standing spots in a zone, relative to its center (props fill the top half).
const SPOTS: [Vec2; 4] =
    [Vec2::new(-75.0, -35.0), Vec2::new(75.0, -35.0), Vec2::new(-25.0, -75.0), Vec2::new(25.0, -75.0)];

pub(crate) const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::srgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

pub const BACKDROP: Color = rgb(0xE9, 0xE1, 0xD4);
pub(crate) const FLOOR: Color = rgb(0xF7, 0xF1, 0xE8);
pub(crate) const FLOOR_EDGE: Color = rgb(0xE2, 0xD5, 0xC3);
pub(crate) const WALL: Color = rgb(0xEA, 0xDE, 0xCD);
pub(crate) const WINDOW_GLASS: Color = rgb(0xCF, 0xE6, 0xF2);
pub(crate) const WINDOW_FRAME: Color = rgb(0xFF, 0xFF, 0xFF);
pub(crate) const RUG: Color = rgb(0xF0, 0xE4, 0xD4);
pub(crate) const RUG_EDGE: Color = rgb(0xE3, 0xD3, 0xBE);
pub(crate) const DOOR_COLOR: Color = rgb(0xD2, 0xC1, 0xA9);
pub(crate) const DESK: Color = rgb(0xD9, 0xB4, 0x8F);
pub(crate) const DESK_EDGE: Color = rgb(0xC2, 0x99, 0x70);
pub(crate) const LAPTOP: Color = rgb(0x3A, 0x3F, 0x4B);
pub(crate) const EYES: Color = rgb(0x2E, 0x2A, 0x27);
pub(crate) const LEAF: Color = rgb(0x7F, 0xB0, 0x7A);
pub(crate) const LEAF_DARK: Color = rgb(0x5E, 0x93, 0x5C);
pub(crate) const POT: Color = rgb(0xC9, 0x8B, 0x6B);
pub(crate) const ALERT: Color = rgb(0xD9, 0x4D, 0x47);
pub(crate) const ZONE_LABEL: Color = rgb(0x6E, 0x61, 0x52);
pub(crate) const SHELF: Color = rgb(0xB0, 0x80, 0x58);
const BOOKS: [Color; 5] = [rgb(0xD9, 0x6C, 0x4F), rgb(0x5B, 0x8D, 0xEF), rgb(0xE5, 0xC0, 0x4B), rgb(0x4C, 0xAF, 0x7A), rgb(0x9B, 0x6B, 0xDF)];
pub(crate) const RACK: Color = rgb(0x3B, 0x42, 0x50);
pub(crate) const LED_GREEN: Color = rgb(0x6B, 0xD3, 0x8A);
pub(crate) const LED_BLUE: Color = rgb(0x6F, 0xB7, 0xFF);
pub(crate) const GLOBE_SEA: Color = rgb(0x7D, 0xB7, 0xE8);
pub(crate) const SOFA: Color = rgb(0xC9, 0x8B, 0x6B);
pub(crate) const SOFA_CUSHION: Color = rgb(0xDB, 0xA0, 0x80);
pub const TEXT: Color = rgb(0x2B, 0x26, 0x21);
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

    pub fn character(&self, who: &Who) -> Option<Entity> {
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

    commands.spawn((Furniture, rounded(FLOOR_SIZE, 28.0, FLOOR, Some((FLOOR_EDGE, 4.0))), Transform::from_xyz(0.0, 0.0, -100.0)));
    // Back wall with two windows.
    let wall_height = 90.0;
    let wall_y = FLOOR_SIZE.y / 2.0 - wall_height / 2.0;
    commands.spawn((
        Furniture,
        rounded(Vec2::new(FLOOR_SIZE.x, wall_height), 28.0, WALL, None),
        Transform::from_xyz(0.0, wall_y, -90.0),
    ));
    for x in [-150.0, 150.0] {
        commands.spawn((
            Furniture,
            rounded(Vec2::new(150.0, 56.0), 8.0, WINDOW_GLASS, Some((WINDOW_FRAME, 6.0))),
            Transform::from_xyz(x, wall_y + 4.0, -80.0),
        ));
    }
    commands.spawn((Furniture, rounded(Vec2::new(130.0, 12.0), 6.0, DOOR_COLOR, None), Transform::from_translation(DOOR.extend(-80.0))));
    commands.spawn((Furniture, ellipse(Vec2::new(210.0, 140.0), RUG, Some((RUG_EDGE, 3.0))), MAIN_SEAT.extend(-70.0).into_transform()));
    for zone in &ZONES {
        spawn_zone(&mut commands, zone, &font);
    }

    spawn_desk(&mut commands, MAIN_SEAT, MAIN_SCALE, MAIN_ACCENT, "Claude", &font);
    let main = spawn_character(&mut commands, MAIN_SEAT, MAIN_SCALE, MAIN_ACCENT, "main", &font);
    roster.main = Some(main);
}

/// A zone's rug, name and props. Props sit in the top half; agents stand in front.
fn spawn_zone(commands: &mut Commands, zone: &Zone, font: &UiFont) {
    let root = commands.spawn((Furniture, zone.center.extend(-95.0).into_transform(), Visibility::default())).id();
    let mut put = |shape: Shape, at: Vec2, z: f32| {
        commands.spawn((shape, Transform::from_translation(at.extend(z)), ChildOf(root)));
    };
    put(rounded(zone.size, 22.0, zone.tint, None), Vec2::ZERO, 0.0);
    let top = zone.size.y / 2.0;
    match zone.place {
        Place::Library => {
            for x in [-95.0, 0.0, 95.0] {
                put(rounded(Vec2::new(78.0, 74.0), 5.0, SHELF, None), Vec2::new(x, top - 62.0), 50.0);
                for (row, y) in [18.0, -2.0, -22.0].into_iter().enumerate() {
                    for (i, dx) in [-24.0, -12.0, 0.0, 12.0, 24.0].into_iter().enumerate() {
                        let book = BOOKS[(i + row * 2 + (x as i32).unsigned_abs() as usize) % BOOKS.len()];
                        put(rounded(Vec2::new(8.0, 15.0), 1.5, book, None), Vec2::new(x + dx, top - 62.0 + y), 51.0);
                    }
                }
            }
        }
        Place::Servers => {
            for x in [-95.0, 0.0, 95.0] {
                put(rounded(Vec2::new(62.0, 88.0), 6.0, RACK, None), Vec2::new(x, top - 66.0), 50.0);
                for (row, y) in [26.0, 8.0, -10.0, -28.0].into_iter().enumerate() {
                    let led = if (row + (x as i32).unsigned_abs() as usize / 95).is_multiple_of(2) { LED_GREEN } else { LED_BLUE };
                    put(circle(3.5, led), Vec2::new(x + 16.0, top - 66.0 + y), 51.0);
                    put(rounded(Vec2::new(26.0, 4.0), 2.0, rgb(0x55, 0x5E, 0x6E), None), Vec2::new(x - 8.0, top - 66.0 + y), 51.0);
                }
            }
        }
        Place::WebBay => {
            put(rounded(Vec2::new(190.0, 64.0), 8.0, WINDOW_GLASS, Some((WINDOW_FRAME, 6.0))), Vec2::new(-30.0, top - 52.0), 50.0);
            put(rounded(Vec2::new(6.0, 40.0), 3.0, DESK_EDGE, None), Vec2::new(105.0, top - 100.0), 50.0);
            put(circle(28.0, GLOBE_SEA), Vec2::new(105.0, top - 70.0), 51.0);
            put(ellipse(Vec2::new(12.0, 16.0), LEAF, None), Vec2::new(97.0, top - 64.0), 52.0);
            put(ellipse(Vec2::new(9.0, 7.0), LEAF, None), Vec2::new(116.0, top - 80.0), 52.0);
        }
        Place::Approval => {
            put(rounded(Vec2::new(140.0, 46.0), 10.0, DESK, Some((DESK_EDGE, 3.0))), Vec2::new(0.0, top - 62.0), 50.0);
            put(circle(14.0, ALERT.with_alpha(0.25)), Vec2::new(45.0, top - 52.0), 51.0);
            put(circle(8.0, ALERT), Vec2::new(45.0, top - 52.0), 52.0);
            put(rounded(Vec2::new(36.0, 24.0), 3.0, Color::WHITE, Some((DESK_EDGE, 1.5))), Vec2::new(-30.0, top - 60.0), 51.0);
        }
        Place::Away => {
            for x in [-90.0, 90.0] {
                put(rounded(Vec2::new(110.0, 38.0), 9.0, DESK, Some((DESK_EDGE, 3.0))), Vec2::new(x, top - 58.0), 50.0);
                put(rounded(Vec2::new(36.0, 20.0), 3.0, LAPTOP, None), Vec2::new(x, top - 52.0), 51.0);
            }
        }
        _ => {
            // Lounge: sofa, coffee machine, plants.
            put(rounded(Vec2::new(170.0, 44.0), 14.0, SOFA, None), Vec2::new(-80.0, 6.0), 50.0);
            for dx in [-40.0, 40.0] {
                put(rounded(Vec2::new(70.0, 24.0), 8.0, SOFA_CUSHION, None), Vec2::new(-80.0 + dx, 2.0), 51.0);
            }
            put(rounded(Vec2::new(42.0, 54.0), 6.0, RACK, None), Vec2::new(120.0, 10.0), 50.0);
            put(circle(6.0, Color::WHITE), Vec2::new(120.0, 2.0), 51.0);
        }
    }
    let mut label = commands.spawn((
        Text2d(zone.place.name().to_owned()),
        TextFont::from_font_size(13.0).with_font(font.0.clone()),
        TextColor(ZONE_LABEL),
        Transform::from_xyz(-zone.size.x / 2.0 + 44.0, top - 14.0, 60.0),
        ChildOf(root),
    ));
    if zone.place == Place::Away {
        // At the bottom edge, where speech bubbles of the agents inside don't cover it.
        label.insert((AwaySign, bevy::sprite::Anchor::CENTER_LEFT, Transform::from_xyz(-zone.size.x / 2.0 + 14.0, -top + 14.0, 60.0)));
    }
    if zone.place == Place::Door {
        spawn_plant(commands, zone.center + Vec2::new(-200.0, 10.0));
        spawn_plant(commands, zone.center + Vec2::new(200.0, 10.0));
    }
}

/// Where an agent should be for what it is doing; `None` while it reports (that walk
/// is driven by the model's `Deliver`).
fn place_for(pose: Pose) -> Option<Place> {
    match pose {
        Pose::Reading => Some(Place::Library),
        Pose::Terminal => Some(Place::Servers),
        Pose::Web => Some(Place::WebBay),
        Pose::Waiting => Some(Place::Approval),
        Pose::Reporting => None,
        Pose::Idle | Pose::Thinking | Pose::Typing | Pose::Delegating => Some(Place::Desk),
    }
}

fn zone_spot(place: Place, slot: usize) -> Vec2 {
    let zone = ZONES.iter().find(|zone| zone.place == place).map_or(Vec2::ZERO, |zone| zone.center);
    zone + SPOTS[slot % SPOTS.len()]
}

/// A sub-agent's own seat.
#[derive(Component)]
pub struct Home {
    seat: Vec2,
    slot: usize,
    id: String,
}

/// The "Other repos" room's sign, listing the repositories in use there.
#[derive(Component)]
pub struct AwaySign;

pub fn update_away_sign(model: Res<Model>, mut signs: Query<&mut Text2d, With<AwaySign>>) {
    if !model.is_changed() {
        return;
    }
    let Some(session) = model.shown() else { return };
    let home = session.home_repo.as_ref().map(|repo| &repo.root);
    let mut away: Vec<String> = session
        .agents
        .iter()
        .filter(|(_, info)| info.status != crate::model::Status::Done)
        .filter_map(|(_, info)| info.repo.as_ref())
        .chain(session.main_repo.as_ref())
        .filter(|repo| Some(&repo.root) != home)
        .map(|repo| repo.label())
        .collect();
    away.dedup();
    let text = if away.is_empty() { "Other repos".to_owned() } else { format!("Other repos: {}", away.join(", ")) };
    for mut sign in &mut signs {
        if sign.0 != text {
            sign.0.clone_from(&text);
        }
    }
}

/// Whether a sub-agent's latest work is in another repository than the session's.
fn works_elsewhere(model: &Model, id: &str) -> bool {
    model.shown().is_some_and(|session| {
        let repo = session.agent(id).and_then(|info| info.repo.as_ref());
        repo.is_some_and(|repo| Some(&repo.root) != session.home_repo.as_ref().map(|home| &home.root))
    })
}

type RoutableAgent<'a> = (Entity, &'a Transform, &'a Pose, &'a Home, &'a Station);

/// Sends each sub-agent to the place that matches what it is doing, once it has spent
/// `MIN_DWELL` where it is. Agents mid-walk finish first, then go to the latest place.
pub fn route_agents(
    mut commands: Commands,
    time: Res<Time>,
    agents: Query<RoutableAgent, (Without<Walk>, Without<Done>)>,
    model: Res<Model>,
) {
    let now = time.elapsed_secs();
    for (entity, transform, pose, home, station) in &agents {
        let Some(mut want) = place_for(*pose) else { continue };
        // Work in another repository happens in its own room, except asking permission.
        if want != Place::Approval && works_elsewhere(&model, &home.id) {
            want = Place::Away;
        }
        if station.place == want || station.place == Place::Claude || now - station.since < MIN_DWELL {
            continue;
        }
        let here = transform.translation.truncate();
        let walk = match want {
            Place::Desk => Walk::new(here, home.seat + BEHIND_DESK, home.seat, Arrive::Sit),
            place => {
                let via = if station.place == Place::Desk { step_out(home.seat) } else { here };
                Walk::new(here, via, zone_spot(place, home.slot), Arrive::Stand(place))
            }
        };
        commands.entity(entity).insert(walk);
    }
}

/// Beside the desk, toward the middle of the office: where an agent steps out to.
fn step_out(seat: Vec2) -> Vec2 {
    let side = if seat.x < 0.0 { -1.0 } else { 1.0 };
    seat + Vec2::new(-side * BESIDE_DESK, 0.0)
}

/// Wide window: show the whole office. Small window: zoom to where the agents are,
/// gliding there smoothly.
pub fn follow_action(
    time: Res<Time>,
    camera: Single<(&Camera, &mut Transform, &mut Projection), With<WorldCamera>>,
    agents: Query<&Transform, (With<Agent>, Without<WorldCamera>)>,
    control: Res<CameraControl>,
    mut animating: ResMut<Animating>,
    mut zoom: ResMut<Zoom>,
) {
    let (camera, mut transform, mut projection) = camera.into_inner();
    let Some(viewport) = camera.logical_viewport_size() else { return };
    let Projection::Orthographic(ortho) = &mut *projection else { return };
    let ScalingMode::AutoMin { min_width, min_height } = &mut ortho.scaling_mode else { return };

    let (target_center, target_size) = if let Some((center, size)) = control.manual {
        // The user zoomed or panned: follow the hand at once (easing made dragging feel
        // laggy) and hold that view until a double-click resets it. `camera_input`
        // already keeps it inside the room.
        transform.translation.x = center.x;
        transform.translation.y = center.y;
        (*min_width, *min_height) = (size.x, size.y);
        (center, size)
    } else if viewport.x >= FOLLOW_BELOW_WIDTH {
        (Vec2::ZERO, ROOM_SIZE)
    } else {
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for agent in &agents {
            min = min.min(agent.translation.truncate());
            max = max.max(agent.translation.truncate());
        }
        // Room for names below and speech bubbles above.
        min -= Vec2::new(160.0, 110.0);
        max += Vec2::new(160.0, 190.0);
        let size = (max - min).max(MIN_VIEW).min(ROOM_SIZE);
        let half_room = (ROOM_SIZE - size) / 2.0;
        (((min + max) / 2.0).clamp(-half_room, half_room), size)
    };

    let blend = 1.0 - (-4.0 * time.delta_secs()).exp();
    let center = transform.translation.truncate().lerp(target_center, blend);
    let size = Vec2::new(*min_width, *min_height).lerp(target_size, blend);
    transform.translation.x = center.x;
    transform.translation.y = center.y;
    (*min_width, *min_height) = (size.x, size.y);
    // `AutoMin` fits the whole size into the viewport, so the tighter axis decides.
    let units_per_pixel = (size / viewport).max_element() * ortho.scale;
    zoom.set_if_neq(Zoom { units_per_pixel });
    if center.distance(target_center) > 0.5 || size.distance(target_size) > 0.5 {
        animating.0 = true;
    }
}

/// Room units per logical screen pixel in the frame being drawn (1 = normal size,
/// 2 = everything looks half as big).
#[derive(Resource, PartialEq)]
pub struct Zoom {
    pub units_per_pixel: f32,
}

impl Default for Zoom {
    fn default() -> Self {
        Self { units_per_pixel: 1.0 }
    }
}

trait IntoTransform {
    fn into_transform(self) -> Transform;
}

impl IntoTransform for Vec3 {
    fn into_transform(self) -> Transform {
        Transform::from_translation(self)
    }
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
    // From its desk it steps out sideways first; from anywhere else it walks straight.
    let via = if here.distance(seat) < 20.0 { step_out(seat) } else { here };
    // Beside the front of Claude's desk, low enough that the speech bubbles of
    // Claude and the visitor don't collide; visitors from one side stand in a column.
    let row = (slot / 2) as f32;
    let spot = MAIN_SEAT + Vec2::new(side * 150.0, -40.0 - row * 40.0);
    let claudes_hand = MAIN_SEAT + Vec2::new(side * 38.0, 0.0);
    Walk::new(here, via, spot, Arrive::HandOver { to: claudes_hand })
}

/// Gives a new sub-agent a desk and walks it in from the door.
fn seat_agent(commands: &mut Commands, roster: &mut Roster, id: &str, agent_type: &str, font: &UiFont) -> Option<Entity> {
    let slot = roster.free_slot()?;
    let seat = SEATS[slot];
    let accent = accent_for(agent_type);
    let character = spawn_character(commands, DOOR, 1.0, accent, id, font);
    let desk = spawn_desk(commands, seat, 1.0, accent, agent_type, font);
    commands.entity(character).insert((Walk::new(DOOR, seat + BEHIND_DESK, seat, Arrive::Sit), Home { seat, slot, id: id.to_owned() }));
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
    // An open book in both hands while reading (above the desk and laptop).
    part(anim::book(accent), Some(Part::Book), Transform::from_xyz(0.0, 4.0 * s, 8.6).with_scale(Vec3::splat(s)), root);
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
    // "→ Library" under the feet while walking (see `anim::update_chips`).
    commands.spawn((
        Chip,
        RigOf(root),
        Text2d::default(),
        TextFont::from_font_size(12.0).with_font(font.0.clone()),
        TextColor(accent),
        Visibility::Hidden,
        Transform::from_xyz(0.0, -28.0 * s, 9.5),
        ChildOf(root),
    ));
    let bubble = anim::spawn_bubble(commands, root, 92.0 * s, font);
    commands.entity(root).insert(BubbleRef(bubble));
    root
}

fn spawn_plant(commands: &mut Commands, at: Vec2) {
    let plant = commands.spawn((Furniture, Transform::from_translation(at.extend(-60.0)), Visibility::default())).id();
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
