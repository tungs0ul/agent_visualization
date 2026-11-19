//! The 3D HQ (PLAN.md M15, M17; V2.md): every agent is a robot that walks to the
//! station for its work. What each robot does comes from `director` (model in,
//! roles and places out); this module plays it: walking, clips, holograms, trails,
//! documents and labels. The workspace itself is built in `world3d`. The robot comes
//! from `tools/blender/robot.py`; each agent shows its colour on the robot's ear rings
//! and chest light.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::Duration;

use bevy::gltf::GltfMaterialName;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use bevy_egui::{EguiContexts, egui};

use crate::director::{self, Role};
use crate::model::{Model, Pose, Who};
use crate::world3d::{Spot, TELEPORTER_DECK, inbox, teleporter_spot};

/// The robot every agent is (`tools/blender/robot.py`): 1 m tall, facing +z.
const ROBOT: &str = "models/robot/robot.glb";
/// Claude's own colour on its ear rings and chest light; sub-agents use their accent.
const MAIN_GLOW: LinearRgba = LinearRgba::rgb(0.2, 1.4, 4.0);

/// What a character is doing, as an animation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Act {
    Idle,
    Think,
    Read,
    Type,
    Walk,
    Wave,
    Handover,
}

/// Something in 3D moves this frame (a robot walks, the camera glides, a trail
/// fades): redraw at full rate. Otherwise only the idle clip plays, slowly, and then
/// the HQ dozes (`main::pace_updates`).
#[derive(Resource, Default)]
pub struct View3d {
    pub moving: bool,
}

#[derive(Component)]
pub(crate) struct RoomCamera3d;

/// Which rig a character uses (one for now; room for more models later).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Rig {
    Robot,
}

/// A robot's life in the HQ: it beams in on the teleporter, works, and when its
/// session ends (or, for a sub-agent, after its hand-over) walks back and beams out.
/// Newest wins: a session that comes back while its robot walks away keeps it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Presence {
    /// Materialising; negative while it waits for the robots beaming in before it.
    Arriving(f32),
    Here,
    /// Walking back to the teleporter, to wait round it.
    Departing,
    /// Stepping onto the deck to beam out.
    Boarding,
    /// Dematerialising.
    Leaving(f32),
}

/// How long beaming in or out takes.
const BEAM_SECS: f32 = 1.4;

#[derive(Component)]
pub(crate) struct Avatar {
    /// The agent it is, in which session.
    pub(crate) who: Who,
    pub(crate) session: String,
    presence: Presence,
    /// Its full size (Claude is bigger).
    size: f32,
    /// What it does now (from the director).
    pose: Pose,
    /// A sub-agent that has stopped: it hands its report over, then leaves.
    done: bool,
    /// Its last tool call failed (red ring and label until something succeeds).
    pub(crate) failed: bool,
    /// Seconds spent handing a report over in person.
    handing: f32,
    rig: Rig,
    /// The glow of its `Accent` material (ear rings, chest light).
    glow: LinearRgba,
    accent: Color,
    /// The animation player inside the loaded model.
    player: Option<Entity>,
    playing: Option<Act>,
    /// Its current report has been handed over or left on the desk.
    reported: bool,
    /// The station a sub-agent works at: where its first tool call took it.
    pub(crate) desk: Option<u8>,
    /// The location it is at or walking to, and since when (seconds of app time).
    at: Spot,
    since: f32,
}


/// One animation graph per rig, and its node for each act.
#[derive(Resource)]
pub(crate) struct Clips {
    graphs: HashMap<Rig, (Handle<AnimationGraph>, HashMap<Act, AnimationNodeIndex>)>,
}

impl Clips {
    fn load(assets: &AssetServer, graphs: &mut Assets<AnimationGraph>) -> Self {
        let mut all = HashMap::new();
        // Clip indices: the order of `animations` in each file (the robot's are
        // alphabetical: Carry, Handover, Idle, Read, Think, Type, Walk, Wave).
        let robot: Vec<(Act, &str, usize)> = vec![
            (Act::Handover, ROBOT, 1),
            (Act::Idle, ROBOT, 2),
            (Act::Read, ROBOT, 3),
            (Act::Think, ROBOT, 4),
            (Act::Type, ROBOT, 5),
            (Act::Walk, ROBOT, 6),
            (Act::Wave, ROBOT, 7),
        ];
        let (graph, nodes) =
            AnimationGraph::from_clips(robot.iter().map(|(_, file, index)| assets.load(GltfAssetLabel::Animation(*index).from_asset(*file))));
        let acts = robot.iter().map(|(act, _, _)| *act).zip(nodes).collect();
        all.insert(Rig::Robot, (graphs.add(graph), acts));
        Self { graphs: all }
    }
}


/// Builds the HQ, its camera and lights at startup.
pub fn build(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(Clips::load(&assets, &mut graphs));

    // Night in space: a dark backdrop, bloom so emissive parts glow (PLAN.md M17).
    commands.spawn((
        RoomCamera3d,
        Camera3d::default(),
        // A wider view, so the lit ceiling shows at the top and the front desks at the
        // bottom (PLAN.md M29).
        Projection::Perspective(PerspectiveProjection { fov: 0.98, ..default() }),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.06)),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        // A little more glow than the default: the light strips and holograms carry
        // the starship look (PLAN.md M29).
        Bloom { intensity: 0.22, ..Bloom::NATURAL },
        Msaa::Sample4,
        Transform::from_translation(FULL_EYE).looking_at(FULL_LOOK, Vec3::Y),
        // What polished metal reflects (PLAN.md M29): a generated bridge-like
        // surround, filtered by Bevy once at start.
        bevy::light::GeneratedEnvironmentMapLight {
            environment_map: images.add(crate::space::environment(64)),
            intensity: 900.0,
            ..default()
        },
    ));
    commands.spawn((
        DirectionalLight { illuminance: 2600.0, color: Color::srgb(0.78, 0.88, 1.0), shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
        CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 30.0, ..default() }.build(),
    ));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.48, 0.64, 1.0), brightness: 190.0, ..default() });
    let mut kit = crate::world3d::Kit { commands: &mut commands, assets: &assets, meshes: &mut meshes, materials: &mut materials };
    crate::world3d::build(&mut kit);
    crate::hull::build(&mut kit);
    crate::space::build(&mut kit, &mut images);
}


/// The model itself, under `parent`. Kenney Space Kit models are exported with their
/// root moved to (2, 0, 1.5); move them back so they stand where they are put.
fn scene_child(commands: &mut Commands, assets: &AssetServer, file: &'static str, parent: Entity) -> Entity {
    let offset = if file.contains("space-kit") { Vec3::new(-2.0, 0.0, -1.5) } else { Vec3::ZERO };
    commands
        .spawn((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(file))),
            Transform::from_translation(offset),
            ChildOf(parent),
        ))
        .id()
}

/// Plays the director: every robot walks to its place and plays the clip for its
/// pose; new agents beam in, and agents whose session ended (or sub-agents done with
/// their hand-over) walk to the teleporter and beam out.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    model: Res<Model>,
    mut avatars: Query<(Entity, &mut Avatar, &mut Transform)>,
    clips: Option<Res<Clips>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    // The systems after this one report movement afresh every frame.
    view.moving = false;
    let dt = time.delta_secs();
    let roles: HashMap<(String, Who), Role> =
        director::cast_all(&model).into_iter().map(|role| ((role.session.clone(), role.who.clone()), role)).collect();
    // The Claude that waited before any session becomes the first session's Claude.
    for (_, mut avatar, _) in &mut avatars {
        if avatar.session.is_empty()
            && avatar.who == Who::Main
            && !roles.contains_key(&(String::new(), Who::Main))
            && let Some(role) = roles.values().find(|role| role.who == Who::Main && role.seat == 0)
        {
            avatar.session = role.session.clone();
        }
    }
    // Who stays and where it goes. Desks already taken, so a sub-agent starting work
    // gets a free one.
    let mut taken: Vec<u8> = avatars.iter().filter_map(|(_, avatar, _)| avatar.desk).collect();
    let mut wanted: Vec<(Entity, Spot)> = Vec::new();
    let mut seats: HashMap<Entity, usize> = HashMap::new();
    for (entity, mut avatar, mut transform) in &mut avatars {
        let role = roles.get(&(avatar.session.clone(), avatar.who.clone()));
        let leave = role.is_none_or(|role| role.done && avatar.reported);
        avatar.presence = match avatar.presence {
            Presence::Arriving(t) if t >= BEAM_SECS => Presence::Here,
            Presence::Arriving(t) => Presence::Arriving(t + dt),
            Presence::Here if leave => Presence::Departing,
            Presence::Departing | Presence::Boarding if !leave => Presence::Here,
            Presence::Leaving(t) if t >= BEAM_SECS => {
                commands.entity(entity).despawn();
                continue;
            }
            Presence::Leaving(t) => Presence::Leaving(t + dt),
            other => other,
        };
        // Beaming: grow in or shrink away on the deck, nothing else.
        if let Presence::Arriving(t) | Presence::Leaving(t) = avatar.presence {
            let grown = (t / BEAM_SECS).clamp(0.0, 1.0);
            let grown = if matches!(avatar.presence, Presence::Leaving(_)) { 1.0 - grown } else { grown };
            transform.scale = Vec3::splat(avatar.size * grown.max(0.001));
            view.moving = true;
            continue;
        }
        transform.scale = Vec3::splat(avatar.size);
        if let Some(role) = role {
            avatar.pose = role.pose;
            avatar.done = role.done;
            avatar.failed = role.failed;
            seats.insert(entity, role.seat);
        }
        if avatar.presence == Presence::Here {
            // Newest wins, but a robot stays a moment where it is before walking on:
            // quick tool calls one after another don't send it running back and forth.
            let (pose, is_main, at) = (avatar.pose, avatar.who == Who::Main, avatar.at);
            let free = || {
                let desk = director::free_desk(&taken, crate::world3d::DESKS.len() as u8);
                taken.push(desk);
                desk
            };
            let place = director::location_for(pose, is_main, at, &mut avatar.desk, free);
            let place = if place != avatar.at && time.elapsed_secs() - avatar.since < DWELL_SECS { avatar.at } else { place };
            wanted.push((entity, place));
        }
    }
    // Several at one place stand side by side.
    wanted.sort_by_key(|(entity, location)| (*location, entity.to_bits()));
    let mut slots: HashMap<Entity, (Spot, usize)> = HashMap::new();
    let mut count: HashMap<Spot, usize> = HashMap::new();
    for (entity, location) in wanted {
        let slot = count.entry(location).or_default();
        slots.insert(entity, (location, *slot));
        *slot += 1;
    }
    let deck = teleporter_spot() + Vec3::Y * TELEPORTER_DECK;
    // One robot on the deck at a time: the others wait round it, and the first to
    // arrive there beams out next.
    let mut deck_taken = avatars.iter().any(|(_, avatar, _)| match avatar.presence {
        Presence::Arriving(t) => t >= 0.0,
        Presence::Leaving(_) | Presence::Boarding => true,
        _ => false,
    });
    let mut departing: Vec<Entity> =
        avatars.iter().filter(|(_, avatar, _)| avatar.presence == Presence::Departing).map(|(entity, _, _)| entity).collect();
    departing.sort_by_key(|entity| entity.to_bits());
    for (avatar_entity, mut avatar, mut transform) in &mut avatars {
        let (target, face) = match avatar.presence {
            Presence::Boarding => (deck, 0.0),
            Presence::Departing => {
                let place = departing.iter().position(|entity| *entity == avatar_entity).unwrap_or(0);
                let waiting = teleporter_spot() + Quat::from_rotation_y(0.9 + 0.7 * place as f32) * Vec3::new(0.0, 0.0, 2.0);
                if !deck_taken && transform.translation.distance(waiting) < 0.1 {
                    deck_taken = true;
                    avatar.presence = Presence::Boarding;
                }
                (waiting, 0.0)
            }
            Presence::Here => {
                let Some(&(location, slot)) = slots.get(&avatar_entity) else { continue };
                if avatar.at != location {
                    avatar.at = location;
                    avatar.since = time.elapsed_secs();
                }
                // Each session's Claude has its own seat at the main desk; sub-agents
                // there stand in front of the platform, facing it.
                match location {
                    Spot::Command if avatar.who == Who::Main => {
                        location.work_spot(seats.get(&avatar_entity).copied().unwrap_or(0))
                    }
                    Spot::Command => (visitor_spot(slot), PI),
                    _ => location.work_spot(slot),
                }
            }
            _ => continue,
        };
        // Walk there at a robot's pace.
        let here = transform.translation;
        let to = target - here;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let walking = flat.length() > 0.05;
        view.moving |= walking;
        if walking {
            // Around the main platform, not across it.
            let goal = detour(here, target) - here;
            let heading = Vec3::new(goal.x, 0.0, goal.z).normalize_or_zero();
            let step = (WALK_SPEED * dt).min(flat.length());
            let next = here + heading * step;
            // Up or down a platform edge gradually.
            let y = here.y + (target.y - here.y) * (step / flat.length()).min(1.0);
            transform.translation = Vec3::new(next.x, y, next.z);
        } else {
            transform.translation = target;
            if avatar.presence == Presence::Boarding && target == deck {
                avatar.presence = Presence::Leaving(0.0);
            }
        }
        let facing = if walking { Quat::from_rotation_y(transform_heading(here, transform.translation, flat)) } else { Quat::from_rotation_y(face) };
        transform.rotation = transform.rotation.slerp(facing, (dt * 8.0).min(1.0));
        let act = if walking {
            Act::Walk
        } else {
            match avatar.pose {
                Pose::Idle => Act::Idle,
                Pose::Waiting => Act::Wave,
                Pose::Thinking => Act::Think,
                Pose::Reading | Pose::Web => Act::Read,
                Pose::Typing | Pose::Terminal => Act::Type,
                Pose::Delegating | Pose::Reporting => Act::Handover,
            }
        };
        if avatar.playing != Some(act)
            && let (Some(player), Some(clips)) = (avatar.player, clips.as_ref())
            && let Ok((mut player, mut transitions)) = players.get_mut(player)
            && let Some(&node) = clips.graphs.get(&avatar.rig).and_then(|(_, acts)| acts.get(&act))
        {
            transitions.play(&mut player, node, Duration::from_millis(200)).repeat();
            avatar.playing = Some(act);
        }
    }
    // Newcomers beam in one after another (never two on the deck at once).
    let mut queue = avatars.iter().filter(|(_, avatar, _)| matches!(avatar.presence, Presence::Arriving(_))).count();
    let mut roles: Vec<&Role> = roles.values().collect();
    roles.sort_by_key(|role| (role.seat, role.who != Who::Main));
    for role in roles {
        let present = avatars.iter().any(|(_, avatar, _)| avatar.who == role.who && avatar.session == role.session);
        if role.done || present {
            continue;
        }
        let is_main = role.who == Who::Main;
        let (size, glow) = if is_main { (1.6, claude_glow(role.seat)) } else { (1.3, role.accent.to_linear() * 3.0) };
        let avatar = commands
            .spawn((
                Avatar {
                    who: role.who.clone(),
                    session: role.session.clone(),
                    presence: Presence::Arriving(-(queue as f32) * BEAM_SECS),
                    size,
                    pose: role.pose,
                    done: false,
                    failed: false,
                    handing: 0.0,
                    rig: Rig::Robot,
                    glow,
                    accent: role.accent,
                    player: None,
                    playing: None,
                    reported: false,
                    desk: None,
                    at: Spot::Command,
                    since: 0.0,
                },
                Transform::from_translation(deck).with_scale(Vec3::splat(0.001)),
                Visibility::default(),
            ))
            .id();
        queue += 1;
        let model = scene_child(&mut commands, &assets, ROBOT, avatar);
        commands.entity(model).observe(prepare_avatar);
    }
}

/// Each session's Claude glows in its own colour (the first one cyan).
fn claude_glow(seat: usize) -> LinearRgba {
    [MAIN_GLOW, LinearRgba::rgb(3.0, 0.5, 2.6), LinearRgba::rgb(0.8, 3.2, 0.9), LinearRgba::rgb(3.4, 1.8, 0.3)][seat % 4]
}

/// The teleporter's moving parts (named nodes in `teleporter.glb`) and its sparks.
#[derive(Default)]
pub struct TeleporterParts {
    rings: Vec<(Entity, f32)>,
    beam: Option<Entity>,
    orbiters: Option<Entity>,
    sparks: Vec<Entity>,
}

/// Sparks spiralling in the beam while a robot beams in or out.
const SPARKS: usize = 36;

/// While a robot beams in or out, the teleporter's rings spin up, its orbiters
/// circle and the beam shows; otherwise it rests (and costs no redraws).
#[allow(clippy::too_many_arguments)]
pub fn teleporter(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    avatars: Query<&Avatar>,
    names: Query<(Entity, &Name)>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut found: Local<TeleporterParts>,
) {
    if found.beam.is_none() {
        for (entity, name) in &names {
            match name.as_str() {
                "Ring0" => found.rings.push((entity, 1.6)),
                "Ring1" => found.rings.push((entity, -1.1)),
                "Ring2" => found.rings.push((entity, 0.7)),
                "Beam" => found.beam = Some(entity),
                "Orbiters" => found.orbiters = Some(entity),
                _ => {}
            }
        }
        // Not loaded yet: try again next frame.
        if found.beam.is_none() {
            found.rings.clear();
            return;
        }
        let mesh = meshes.add(Sphere::new(0.025).mesh().ico(1).unwrap_or_else(|_| Sphere::new(0.025).into()));
        let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(1.5, 5.0, 8.0), ..default() });
        found.sparks = (0..SPARKS)
            .map(|_| commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(glow.clone()), Transform::default(), Visibility::Hidden)).id())
            .collect();
    }
    // How far the current beaming is (0 to 1), arriving or leaving; none: at rest.
    let beaming = avatars
        .iter()
        .filter_map(|avatar| match avatar.presence {
            Presence::Arriving(t) if t >= 0.0 => Some((t / BEAM_SECS, true)),
            Presence::Leaving(t) => Some((t / BEAM_SECS, false)),
            _ => None,
        })
        .next();
    let shown = if beaming.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    for entity in found.beam.iter().chain(&found.sparks) {
        if let Ok((_, mut visibility)) = parts.get_mut(*entity) {
            visibility.set_if_neq(shown);
        }
    }
    let Some((progress, arriving)) = beaming else { return };
    let progress = progress.clamp(0.0, 1.0);
    view.moving = true;
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    // The beam forms, holds and thins away again.
    if let Some(beam) = found.beam
        && let Ok((mut at, _)) = parts.get_mut(beam)
    {
        let width = (progress * std::f32::consts::PI).sin().max(0.05);
        at.scale = Vec3::new(width, 1.0, width);
    }
    // Sparks gather into the robot as it forms, or rise out of it as it goes.
    let gather = if arriving { 1.0 - progress } else { progress };
    for (i, spark) in found.sparks.iter().enumerate() {
        let Ok((mut at, _)) = parts.get_mut(*spark) else { continue };
        let k = i as f32 / SPARKS as f32;
        let climb = (k + t * if arriving { -0.5 } else { 0.5 }).rem_euclid(1.0);
        let angle = k * 23.0 + t * 3.0;
        let radius = 0.15 + 0.75 * gather * (0.6 + 0.4 * (k * 7.0).sin().abs());
        at.translation = teleporter_spot() + Vec3::new(radius * angle.cos(), TELEPORTER_DECK + 0.1 + climb * 2.4, radius * angle.sin());
        at.scale = Vec3::splat(0.6 + 0.8 * (1.0 - climb));
    }
    for &(ring, speed) in &found.rings {
        if let Ok((mut at, _)) = parts.get_mut(ring) {
            at.rotate_y(speed * 3.0 * dt);
        }
    }
    if let Some(orbiters) = found.orbiters
        && let Ok((mut at, _)) = parts.get_mut(orbiters)
    {
        at.rotate_y(2.4 * dt);
    }
}

/// How long a robot stays at a station before it walks on to the next.
const DWELL_SECS: f32 = 2.5;

/// A robot's walking pace (metres per second).
const WALK_SPEED: f32 = 1.8;

/// Where a sub-agent stands at the main desk: in front of the platform, facing Claude.
fn visitor_spot(slot: usize) -> Vec3 {
    Spot::Command.center() + Vec3::new(-1.0 + 2.0 * (slot % 2) as f32, 0.0, 2.4 + 0.6 * (slot / 2) as f32)
}

/// A report a sub-agent left on the main desk while Claude was away.
#[derive(Component)]
pub(crate) struct Document;


/// Sub-agents with a report hand it over in person when Claude is at its desk;
/// otherwise they leave it there as a glowing document, and Claude picks the
/// documents up when it gets back.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn deliver(
    mut commands: Commands,
    time: Res<Time>,
    mut avatars: Query<(&mut Avatar, &Transform), Without<Document>>,
    mut documents: Query<(Entity, &mut Transform), With<Document>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Sessions whose Claude stands at its seat on the main platform.
    let at_desk: Vec<String> = avatars
        .iter()
        .filter(|(avatar, at)| {
            avatar.who == Who::Main && avatar.presence == Presence::Here && at.translation.xz().length() < 2.0 && at.translation.y > 0.3
        })
        .map(|(avatar, _)| avatar.session.clone())
        .collect();
    let mut left = documents.iter().count();
    for (mut avatar, at) in &mut avatars {
        if avatar.who == Who::Main {
            continue;
        }
        let main_at_desk = at_desk.contains(&avatar.session);
        if avatar.pose != Pose::Reporting {
            avatar.reported = false;
            avatar.handing = 0.0;
            continue;
        }
        let arrived = (0..8).any(|slot| at.translation.distance(visitor_spot(slot)) < 0.15);
        // Claude is there: hand it over in person (the handover clip), then it's done.
        if arrived && !avatar.reported && main_at_desk {
            avatar.handing += time.delta_secs();
            avatar.reported = avatar.handing >= HANDOVER_SECS;
        }
        if arrived && !avatar.reported && !main_at_desk {
            avatar.reported = true;
            let glow = materials.add(StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::new(0.6, 0.9, 1.0, 0.8)),
                emissive: LinearRgba::rgb(0.6, 2.5, 4.0),
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
            commands.spawn((
                Document,
                Mesh3d(meshes.add(Cuboid::new(0.24, 0.02, 0.32))),
                MeshMaterial3d(glow),
                Transform::from_translation(inbox() + Vec3::Y * 0.04 * left as f32),
            ));
            left += 1;
        }
    }
    // Claude is back: it takes the documents.
    if !at_desk.is_empty() {
        for (entity, _) in &documents {
            commands.entity(entity).despawn();
        }
        return;
    }
    // Waiting documents bob gently so they catch the eye.
    let t = time.elapsed_secs();
    for (i, (_, mut at)) in documents.iter_mut().enumerate() {
        at.translation.y = inbox().y + 0.04 * i as f32 + 0.03 * (t * 2.0 + i as f32).sin();
        at.rotation = Quat::from_rotation_y(t * 0.6);
    }
}

/// How long an in-person hand-over takes.
const HANDOVER_SECS: f32 = 1.6;

/// Once a robot is in the world: start its animations and give it its colour.
#[allow(clippy::too_many_arguments)]
fn prepare_avatar(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    material_names: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut avatars: Query<&mut Avatar>,
    mut players: Query<&mut AnimationPlayer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    clips: Option<Res<Clips>>,
) {
    let root = ready.entity;
    let Ok(parent) = parents.get(root) else { return };
    let Ok(mut avatar) = avatars.get_mut(parent.parent()) else { return };
    let Some((graph, acts)) = clips.as_ref().and_then(|clips| clips.graphs.get(&avatar.rig)) else { return };
    // The robot carries its animations: glTF loading made its player.
    if let Some(entity) = children.iter_descendants(root).find(|child| players.contains(*child))
        && let Ok(mut player) = players.get_mut(entity)
    {
        let mut transitions = AnimationTransitions::new();
        transitions.play(&mut player, acts[&Act::Idle], Duration::ZERO).repeat();
        commands.entity(entity).insert((AnimationGraphHandle(graph.clone()), transitions));
        avatar.player = Some(entity);
        avatar.playing = Some(Act::Idle);
    }
    // Its ear rings and chest light (`Accent`) in its own colour; the white shell
    // glows a touch so it stays white under the room's coloured lights.
    for child in children.iter_descendants(root) {
        if let Ok((name, material)) = material_names.get(child)
            && name.0 == "Shell"
            && let Some(original) = materials.get(&material.0).cloned()
        {
            let shell = StandardMaterial { emissive: LinearRgba::rgb(0.10, 0.10, 0.12), ..original };
            commands.entity(child).insert(MeshMaterial3d(materials.add(shell)));
        }
        if let Ok((name, material)) = material_names.get(child)
            && name.0 == "Accent"
            && let Some(original) = materials.get(&material.0).cloned()
        {
            let glow = avatar.glow;
            let peak = glow.red.max(glow.green).max(glow.blue).max(1e-3);
            let accent = StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::rgb(glow.red / peak, glow.green / peak, glow.blue / peak)),
                emissive: glow,
                ..original
            };
            commands.entity(child).insert(MeshMaterial3d(materials.add(accent)));
        }
    }
}

/// Station signs, and names with what each one is doing above the robots.
pub fn labels(
    mut contexts: EguiContexts,
    mode: Res<crate::mini::ViewMode>,
    mut model: ResMut<Model>,
    camera: Query<(&Camera, &GlobalTransform), With<RoomCamera3d>>,
    avatars: Query<(&Avatar, &GlobalTransform)>,
    mut selected: ResMut<crate::hud::Selected>,
    mut open: ResMut<crate::console::OpenConsole>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Ok((camera, camera_at)) = camera.single() else { return Ok(()) };
    let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
    // The command panel (PLAN.md M27, M29): the current mission and a button for each
    // console, centred under the top card as in the user's reference image.
    if !mode.mini {
        let color = egui::Color32::from_rgb(130, 200, 255);
        let mission = model.shown().and_then(|session| session.prompts.last()).map(|prompt| room_proto::describe::truncate(prompt.trim(), 110));
        // At the left, level with the bottom of the window, as in the user's reference:
        // centred it hid the view out.
        let rect = camera.logical_viewport_rect().unwrap_or(bevy::math::Rect::new(0.0, 0.0, 800.0, 600.0));
        egui::Area::new("command-sign".into())
            .fixed_pos(egui::pos2(rect.min.x + rect.width() * 0.2, rect.min.y + rect.height() * 0.3))
            .pivot(egui::Align2::CENTER_CENTER)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 14, 32, 210))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.label(egui::RichText::new("✦ Command").strong().color(color));
                        if let Some(mission) = &mission {
                            ui.label(egui::RichText::new(mission).small().color(crate::palette::TEXT));
                        }
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            for console in crate::console::Console::ALL.into_iter().filter(|console| console.on_command()) {
                                let text = egui::RichText::new(console.hint()).small().color(crate::console::HOLO);
                                let key = console.key().map(|key| format!(" ({})", key.name())).unwrap_or_default();
                                if ui.small_button(text).on_hover_text(format!("{}{key}", console.title())).clicked() {
                                    open.toggle(console);
                                }
                            }
                        });
                    });
            });
    }
    // The teleporter and the wall screen have consoles too, with a sign of their own.
    for console in crate::console::Console::ALL.into_iter().filter(|console| !console.on_command() && !mode.mini) {
        let Ok(screen) = camera.world_to_viewport(camera_at, console.anchor() + Vec3::Y * 1.2) else { continue };
        let screen = screen + offset;
        egui::Area::new(egui::Id::new(("console-sign", console.title())))
            .fixed_pos(egui::pos2(screen.x, screen.y))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let sign = egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 14, 32, 190))
                    .stroke(egui::Stroke::new(1.0, crate::console::HOLO.gamma_multiply(0.5)))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| ui.label(egui::RichText::new(console.hint()).small().color(crate::console::HOLO)));
                if ui
                    .interact(sign.response.rect, ui.id().with("open"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    open.toggle(console);
                }
            });
    }
    // Claude's plan, on a glass board over the main desk while it has work left.
    if !mode.mini
        && let Some(session) = model.shown()
        && session.todos.iter().any(|(_, state)| *state != crate::model::TodoState::Done)
        && let Ok(spot) = camera.world_to_viewport(camera_at, Spot::Command.center() + Vec3::new(0.0, 3.3, -1.0))
    {
        plan_board(ctx, spot + offset, &session.todos);
    }
    // With several sessions, each Claude's label names its project.
    let several = model.live().len() > 1;
    let mut picked = None;
    for (avatar, at) in &avatars {
        let Some(session) = model.session(&avatar.session) else { continue };
        let (mut name, doing, repo, task, work) = match &avatar.who {
            Who::Main if several => {
                (format!("Claude · {}", session.name()), &session.main_doing, session.main_repo.as_ref(), None, &session.main_work)
            }
            Who::Main => ("Claude".to_owned(), &session.main_doing, session.main_repo.as_ref(), None, &session.main_work),
            Who::Sub(id) => match session.agent(id) {
                Some(info) => (info.agent_type.clone(), &info.doing, info.repo.as_ref(), info.task.as_deref(), &info.work),
                None => continue,
            },
        };
        // A sub-agent's nameplate (PLAN.md M28): its task, and what the hooks counted.
        let tally = (!work.calls.is_empty()).then(|| {
            let failed = work.failures();
            let mut line = format!("{} tool{}", work.calls.len(), if work.calls.len() == 1 { "" } else { "s" });
            if failed > 0 {
                line.push_str(&format!(" · {failed} failed"));
            }
            if !work.files.is_empty() {
                line.push_str(&format!(" · {} file{}", work.files.len(), if work.files.len() == 1 { "" } else { "s" }));
            }
            if let Some(call) = work.current() {
                line.push_str(&format!(" · now {}", call.name));
            }
            (line, failed > 0)
        });
        // Working in a linked git worktree (PLAN.md M20): say which.
        if let Some(tree) = repo.and_then(|repo| repo.worktree.as_ref().map(|tree| repo.branch.as_ref().unwrap_or(tree))) {
            name.push_str(&format!(" · ⎇ {tree}"));
        }
        // Working in another repository than its session's (PLAN.md M4d): say which.
        if let Some(repo) = repo.filter(|repo| repo.away_from(session.home_repo.as_ref())) {
            name.push(' ');
            name.push_str(&repo.badge());
        }
        let lift = if mode.mini { 1.35 } else { 1.05 };
        let head = at.translation() + Vec3::Y * lift * at.scale().y;
        let Ok(screen) = camera.world_to_viewport(camera_at, head) else { continue };
        let screen = screen + offset;
        egui::Area::new(egui::Id::new(("label3d", &avatar.session, &avatar.who)))
            .fixed_pos(egui::pos2(screen.x, screen.y))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .interactable(true)
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let shown = egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(220.0);
                    let tint = avatar.accent.to_srgba();
                    let color =
                        egui::Color32::from_rgb((tint.red * 255.0) as u8, (tint.green * 255.0) as u8, (tint.blue * 255.0) as u8);
                    ui.label(egui::RichText::new(name.as_str()).small().strong().color(color));
                    if let Some(task) = task {
                        ui.label(egui::RichText::new(room_proto::describe::truncate(task, 60)).small().color(crate::palette::TEXT_DIM));
                    }
                    if let Some(doing) = doing {
                        let text = egui::RichText::new(doing).small();
                        ui.label(if avatar.failed { text.color(crate::palette::RED) } else { text });
                    }
                    if let Some((line, failed)) = &tally
                        && avatar.who != Who::Main
                    {
                        let color = if *failed { crate::palette::AMBER } else { crate::palette::TEXT_DIM };
                        ui.label(egui::RichText::new(line).small().color(color));
                    }
                });
                // Clicking a robot's label opens its details (`hud`).
                let click = shown
                    .response
                    .interact(egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Click: details · double-click: its terminal");
                if click.clicked() {
                    selected.0 = Some(avatar.who.clone());
                    picked = Some(avatar.session.clone());
                }
                // Double-click: straight to the session's terminal.
                if click.double_clicked()
                    && let Some(pid) = session.claude_pid
                {
                    crate::hud::focus_terminal(pid);
                }
            });
    }
    // Its session's panels come up with it.
    if let Some(session) = picked {
        model.show(&session);
    }
    Ok(())
}

/// The full view shows the whole workspace; the mini widget follows Claude's robot
/// up close, since the whole station would be too small to read there.
pub fn frame(
    mut view: ResMut<View3d>,
    mode: Res<crate::mini::ViewMode>,
    model: Res<Model>,
    hand: Res<CameraView>,
    time: Res<Time>,
    avatars: Query<(&Avatar, &Transform), Without<RoomCamera3d>>,
    mut camera: Query<&mut Transform, With<RoomCamera3d>>,
) {
    let Ok(mut camera) = camera.single_mut() else { return };
    let shown = model.shown().map_or("", |session| session.id.as_str());
    let claude = avatars.iter().find(|(avatar, _)| avatar.who == Who::Main && avatar.session == shown);
    let (eye, look) = match claude.or_else(|| avatars.iter().find(|(avatar, _)| avatar.who == Who::Main)) {
        Some((_, at)) if mode.mini => (at.translation + Vec3::new(0.0, 3.0, 4.2), at.translation + Vec3::Y * 0.8),
        _ => {
            // The user's pan and zoom (`camera_input`).
            let look = FULL_LOOK + Vec3::new(hand.pan.x, 0.0, hand.pan.y);
            (look + Quat::from_rotation_y(hand.turn) * Quat::from_rotation_x(hand.tilt) * (FULL_EYE - FULL_LOOK) * hand.zoom, look)
        }
    };
    let wanted = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    // Follow the hand at once (easing made dragging feel laggy); glide otherwise.
    let blend = if hand.is_changed() && !mode.mini { 1.0 } else { (time.delta_secs() * 4.0).min(1.0) };
    if camera.translation.distance(wanted.translation) > 0.001 {
        view.moving = true;
        camera.translation = camera.translation.lerp(wanted.translation, blend);
        camera.rotation = camera.rotation.slerp(wanted.rotation, blend);
    }
}

/// Where the camera looks at the whole workspace from.
/// The camera (PLAN.md M29): lower than before, so the panoramic window shows a big
/// slice of space above the bridge. `space` places things by this eye.
pub(crate) const FULL_EYE: Vec3 = Vec3::new(0.0, 6.0, 12.4);
const FULL_LOOK: Vec3 = Vec3::new(0.0, 1.5, -2.6);

/// The hologram floating beside a robot: a picture of what it is doing right now
/// (PLAN.md M17, "the holograms are the visualisation of its mind").
#[derive(Component)]
pub(crate) struct ActivityHolo {
    /// The robot it belongs to.
    of: Entity,
    pose: Pose,
    /// Seconds since it appeared (it grows in).
    age: f32,
}

/// Which hologram shows what a robot does.
fn holo_for(pose: Pose) -> Option<&'static str> {
    Some(match pose {
        Pose::Reading => "models/stations/holo_document.glb",
        Pose::Typing => "models/stations/holo_editor.glb",
        Pose::Terminal => "models/stations/holo_terminal.glb",
        Pose::Web => "models/stations/holo_cards.glb",
        Pose::Thinking => "models/stations/holo_nodes.glb",
        Pose::Delegating | Pose::Reporting => "models/stations/holo_capsule.glb",
        Pose::Idle | Pose::Waiting => return None,
    })
}

/// Shows, swaps and moves the activity holograms; they appear once a robot has
/// arrived where it works, beside its head, turned a little towards the camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn activity_holograms(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    avatars: Query<(Entity, &Avatar, &Transform), Without<ActivityHolo>>,
    mut holos: Query<(Entity, &mut ActivityHolo, &mut Transform)>,
    mut lines: Query<(&HoloLine, &ChildOf, &mut Transform), (Without<ActivityHolo>, Without<Avatar>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let (line_mesh, line_glow) = look
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(1.0, 0.022, 0.004)),
                materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(1.2, 4.0, 6.0), ..default() }),
            )
        })
        .clone();
    let mut shown: HashMap<Entity, Pose> = HashMap::new();
    for (entity, mut holo, mut at) in &mut holos {
        let robot = avatars.get(holo.of).ok();
        let pose = robot.map(|(_, avatar, _)| avatar.pose);
        let busy_elsewhere = robot.is_some_and(|(_, avatar, _)| avatar.playing == Some(Act::Walk) || avatar.presence != Presence::Here);
        if pose != Some(holo.pose) || busy_elsewhere {
            commands.entity(entity).despawn();
            continue;
        }
        let Some((_, _, robot_at)) = robot else { continue };
        shown.insert(holo.of, holo.pose);
        holo.age += time.delta_secs();
        let grow = (holo.age / 0.35).min(1.0);
        let bob = 0.02 * (time.elapsed_secs() * 1.6 + (holo.of.to_bits() % 97) as f32).sin();
        // A screen floating in front of the robot at chest height, tilted up to the
        // camera, below its face: the robot works at it, you read it.
        let front = robot_at.rotation * Vec3::new(0.0, 0.0, HOLO_FRONT) * robot_at.scale.x;
        at.translation = robot_at.translation + front + Vec3::Y * (HOLO_LIFT * robot_at.scale.y + bob);
        at.rotation = robot_at.rotation * Quat::from_rotation_x(-0.3);
        at.scale = Vec3::splat(grow * HOLO_SIZE);
    }
    for (robot, avatar, _) in &avatars {
        if shown.contains_key(&robot) || avatar.playing == Some(Act::Walk) || avatar.presence != Presence::Here {
            continue;
        }
        let Some(file) = holo_for(avatar.pose) else { continue };
        let holo = commands
            .spawn((
                ActivityHolo { of: robot, pose: avatar.pose, age: 0.0 },
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(file))),
                Transform::from_scale(Vec3::ZERO),
            ))
            .observe(brighten_holo)
            .id();
        // Flat screens get lines of "text" writing themselves (not the real text,
        // which the label above the head shows).
        if matches!(avatar.pose, Pose::Reading | Pose::Typing | Pose::Terminal | Pose::Web) {
            for index in 0..HOLO_LINES {
                commands.spawn((
                    HoloLine { index },
                    Mesh3d(line_mesh.clone()),
                    MeshMaterial3d(line_glow.clone()),
                    Transform::from_scale(Vec3::new(0.0, 1.0, 1.0)),
                    ChildOf(holo),
                ));
            }
        }
    }
    // The lines write left to right one after another, hold, then start over.
    let t = time.elapsed_secs();
    for (line, parent, mut at) in &mut lines {
        let Ok((_, holo, _)) = holos.get(parent.parent()) else { continue };
        let seed = (holo.of.to_bits() % 7) as f32;
        let cycle = (t + seed) % LINE_CYCLE;
        let start = line.index as f32 * 0.55;
        let written = ((cycle - start) / 0.5).clamp(0.0, 1.0);
        let width = 0.32 + 0.22 * ((line.index as f32 * 2.3 + seed).sin() * 0.5 + 0.5);
        let length = width * written;
        at.scale = Vec3::new(length.max(0.0001), 1.0, 1.0);
        at.translation = Vec3::new(-0.3 + length / 2.0, 0.42 - 0.075 * line.index as f32, 0.03);
    }
}

/// A line of glowing "text" on an activity screen.
#[derive(Component)]
pub(crate) struct HoloLine {
    index: usize,
}

const HOLO_LINES: usize = 4;
/// Seconds for all lines to write, hold and clear.
const LINE_CYCLE: f32 = 4.0;

/// Where the activity screen floats, in robot heights (`HOLO_LIFT` up to its bottom
/// edge, `HOLO_FRONT` out in front), and its size.
const HOLO_FRONT: f32 = 0.5;
const HOLO_LIFT: f32 = 0.34;
const HOLO_SIZE: f32 = 0.95;

/// Brighter copies of the holograms' materials, made once per material.
#[derive(Resource, Default)]
pub(crate) struct BrightHolos(HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>);

/// The activity screens glow: their materials, made for the stations' dimmer
/// panels, get twice the light and a little more body.
fn brighten_holo(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bright: ResMut<BrightHolos>,
) {
    for child in children.iter_descendants(ready.entity) {
        let Ok(material) = meshes.get(child) else { continue };
        let id = material.0.id();
        let handle = match bright.0.get(&id) {
            Some(handle) => handle.clone(),
            None => {
                let Some(mut glow) = materials.get(&material.0).cloned() else { continue };
                glow.emissive *= 2.2;
                if glow.alpha_mode == AlphaMode::Blend {
                    glow.base_color = glow.base_color.with_alpha((glow.base_color.alpha() * 1.5).min(0.6));
                }
                let handle = materials.add(glow);
                bright.0.insert(id, handle.clone());
                handle
            }
        };
        commands.entity(child).insert(MeshMaterial3d(handle));
    }
}

/// A spot of light a walking robot leaves on the floor; it shrinks away.
#[derive(Component)]
pub(crate) struct TrailDot {
    age: f32,
}

/// How long a trail dot lasts, and how often a walking robot drops one.
const TRAIL_LIFE: f32 = 1.4;
const TRAIL_EVERY: f32 = 0.09;

/// Walking robots leave a fading cyan light trail on the floor (PLAN.md M17).
#[allow(clippy::too_many_arguments)]
pub fn light_trails(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    avatars: Query<(&Avatar, &Transform), Without<TrailDot>>,
    mut dots: Query<(Entity, &mut TrailDot, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut since: Local<f32>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    let dt = time.delta_secs();
    view.moving |= !dots.is_empty();
    for (entity, mut dot, mut at) in &mut dots {
        dot.age += dt;
        let left = 1.0 - dot.age / TRAIL_LIFE;
        if left <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            at.scale = Vec3::new(left, 1.0, left);
        }
    }
    *since += dt;
    if *since < TRAIL_EVERY {
        return;
    }
    *since = 0.0;
    let (mesh, material) = look
        .get_or_insert_with(|| {
            (
                meshes.add(Cylinder::new(0.09, 0.004)),
                materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(0.4, 2.2, 4.0), ..default() }),
            )
        })
        .clone();
    for (avatar, at) in &avatars {
        if avatar.playing == Some(Act::Walk) {
            commands.spawn((
                TrailDot { age: 0.0 },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(Vec3::new(at.translation.x, at.translation.y + 0.012, at.translation.z)),
            ));
        }
    }
}

/// Where to head for `target` from `here` without crossing the main platform: the
/// target itself, or a point just outside the platform's rim when the straight line
/// would cross it. Recomputed every frame, so the robot curves round the edge.
fn detour(here: Vec3, target: Vec3) -> Vec3 {
    const RIM: f32 = 2.4;
    let centre = Spot::Command.center();
    let (a, b, c) = (here.xz(), target.xz(), centre.xz());
    if a.distance(c) < RIM || b.distance(c) < RIM {
        return target;
    }
    let ab = b - a;
    let t = ((c - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    let closest = a + ab * t;
    if closest.distance(c) >= RIM {
        return target;
    }
    let out = (closest - c).try_normalize().unwrap_or_else(|| ab.perp().normalize_or_zero());
    let point = c + out * (RIM + 0.5);
    Vec3::new(point.x, here.y, point.y)
}

/// Lights each crew desk's ring for the robot working there (PLAN.md M28): off when
/// free, cyan working, amber waiting for the user, green done, red after a failure.
/// The material changes only when a desk's state does.
pub fn desk_lights(
    avatars: Query<&Avatar>,
    mut lights: Query<(&mut crate::world3d::DeskLight, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    use crate::world3d::DeskState;
    for (mut light, material) in &mut lights {
        let state = avatars
            .iter()
            .filter(|avatar| avatar.desk == Some(light.desk) && !matches!(avatar.presence, Presence::Leaving(_)))
            .map(|avatar| match () {
                _ if avatar.failed => DeskState::Failed,
                _ if avatar.done => DeskState::Done,
                _ if avatar.pose == Pose::Waiting => DeskState::Waiting,
                _ => DeskState::Working,
            })
            .max_by_key(|state| *state as u8)
            .unwrap_or(DeskState::Free);
        if state != light.shown {
            light.shown = state;
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.emissive = state.glow();
            }
        }
    }
}

/// Applies the graphics quality (PLAN.md M29) to the camera and lights, at start
/// and whenever the setting changes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn apply_quality(
    mut commands: Commands,
    prefs: Res<crate::ui::PanelPrefs>,
    camera: Query<Entity, With<RoomCamera3d>>,
    mut sun: Query<&mut DirectionalLight>,
    mut lamps: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut desk_lamps: Query<&mut Visibility, With<crate::hull::DeskLamp>>,
    mut done: Local<Option<crate::ui::Quality>>,
) {
    use crate::ui::Quality;
    use bevy::anti_alias::fxaa::Fxaa;
    use bevy::pbr::ScreenSpaceAmbientOcclusion;
    let quality = prefs.quality;
    if *done == Some(quality) {
        return;
    }
    let Ok(camera) = camera.single() else { return };
    *done = Some(quality);
    let mut camera = commands.entity(camera);
    match quality {
        Quality::Low => {
            camera.remove::<(Bloom, ScreenSpaceAmbientOcclusion, Fxaa)>().insert(Msaa::Sample2);
        }
        Quality::Medium => {
            camera.remove::<(ScreenSpaceAmbientOcclusion, Fxaa)>().insert((Msaa::Sample4, Bloom { intensity: 0.22, ..Bloom::NATURAL }));
        }
        Quality::High => {
            // Ambient occlusion needs multisampling off; FXAA smooths the edges instead.
            camera.insert((Msaa::Off, Fxaa::default(), ScreenSpaceAmbientOcclusion::default(), Bloom { intensity: 0.25, ..Bloom::NATURAL }));
        }
    }
    for mut light in &mut sun {
        light.shadow_maps_enabled = quality != Quality::Low;
    }
    // The ceiling fixtures cast shadows on High only; point lamps never do.
    for mut lamp in &mut lamps {
        lamp.shadow_maps_enabled = false;
    }
    for mut spot in &mut spots {
        spot.shadow_maps_enabled = quality == Quality::High;
    }
    // Low drops the amber desk lamps (eight lights fewer).
    for mut visibility in &mut desk_lamps {
        *visibility = if quality == Quality::Low { Visibility::Hidden } else { Visibility::Inherited };
    }
}

/// The way a walking robot faces: where it actually moved this frame.
fn transform_heading(before: Vec3, after: Vec3, fallback: Vec3) -> f32 {
    let moved = Vec3::new(after.x - before.x, 0.0, after.z - before.z);
    let dir = if moved.length() > 1e-5 { moved } else { fallback };
    dir.x.atan2(dir.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots_walk_round_the_main_platform() {
        // Straight across: head for a point outside the rim instead.
        let goal = detour(Vec3::new(-6.0, 0.0, 0.5), Vec3::new(6.0, 0.0, 0.5));
        assert!(goal.xz().distance(Spot::Command.center().xz()) > 2.4);
        // Clear of the platform: straight to the target.
        let target = Vec3::new(6.0, 0.0, 4.0);
        assert_eq!(detour(Vec3::new(-6.0, 0.0, 4.0), target), target);
        // Going onto the platform (Claude's own desk): straight in.
        let desk = Spot::Command.work_spot(0).0;
        assert_eq!(detour(Vec3::new(-6.0, 0.0, 0.0), desk), desk);
    }
}

/// A glowing capsule flying a task to a sub-agent, or its report back (V2.md
/// phase 3).
#[derive(Component)]
pub(crate) struct Capsule {
    from: Vec3,
    /// A robot to fly to (followed while it moves), or a fixed spot (the inbox).
    to: Result<Entity, Vec3>,
    age: f32,
}

/// How long a capsule flies.
const CAPSULE_SECS: f32 = 1.2;

/// A mesh and its material.
type MeshLook = (Handle<Mesh>, Handle<StandardMaterial>);

/// What `links` remembers between frames.
#[derive(Default)]
pub struct LinkState {
    /// Each working sub-agent's line to its Claude.
    lines: HashMap<Entity, Entity>,
    /// Red warning rings under failing robots.
    rings: HashMap<Entity, Entity>,
    /// Orange beacons over robots waiting for the user.
    beacons: HashMap<Entity, Entity>,
    /// Sub-agents that got their task capsule, and robots whose report flew.
    greeted: Vec<Entity>,
    reported: Vec<Entity>,
    look: Option<(MeshLook, MeshLook)>,
}

/// Who works for whom, made visible: a line from each working sub-agent to its
/// session's Claude, a task capsule flying to it when it arrives, its report flying
/// back at hand-over, and a red ring under any robot whose last tool call failed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn links(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    avatars: Query<(Entity, &Avatar, &Transform), (Without<Capsule>, Without<LinkPart>)>,
    mut capsules: Query<(Entity, &mut Capsule, &mut Transform), (Without<Avatar>, Without<LinkPart>)>,
    mut parts: Query<&mut Transform, (With<LinkPart>, Without<Avatar>, Without<Capsule>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<LinkState>,
) {
    let ((line_mesh, line_glow), (ring_mesh, ring_glow)) = state
        .look
        .get_or_insert_with(|| {
            (
                (
                    meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                    materials.add(StandardMaterial {
                        base_color: Color::LinearRgba(LinearRgba::new(0.3, 0.9, 1.0, 0.6)),
                        emissive: LinearRgba::rgb(1.0, 3.6, 6.0),
                        alpha_mode: AlphaMode::Add,
                        ..default()
                    }),
                ),
                (
                    meshes.add(Torus::new(0.42, 0.5)),
                    materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(6.0, 0.5, 0.3), ..default() }),
                ),
            )
        })
        .clone();
    let claude_of = |session: &str| {
        avatars.iter().find(|(_, avatar, _)| avatar.who == Who::Main && avatar.session == session && avatar.presence == Presence::Here)
    };
    let waist = |at: &Transform| at.translation + Vec3::Y * 0.5 * at.scale.y;

    for (robot, avatar, at) in &avatars {
        let is_sub = avatar.who != Who::Main;
        let claude = if is_sub { claude_of(&avatar.session) } else { None };
        // The line, while the sub-agent works for its Claude.
        let working = is_sub && avatar.presence == Presence::Here && !avatar.done;
        match (working, claude, state.lines.get(&robot).copied()) {
            (true, Some((_, _, claude_at)), line) => {
                let line = line.unwrap_or_else(|| {
                    let line = commands
                        .spawn((LinkPart, Mesh3d(line_mesh.clone()), MeshMaterial3d(line_glow.clone()), Transform::default()))
                        .id();
                    state.lines.insert(robot, line);
                    line
                });
                let (a, b) = (waist(at), waist(claude_at));
                if let Ok(mut transform) = parts.get_mut(line) {
                    let length = a.distance(b).max(0.01);
                    *transform = Transform::from_translation((a + b) / 2.0)
                        .looking_at(b, Vec3::Y)
                        .with_scale(Vec3::new(0.04, 0.04, length));
                }
            }
            (_, _, Some(line)) => {
                commands.entity(line).despawn();
                state.lines.remove(&robot);
            }
            _ => {}
        }
        // Its task flies over from Claude once it has arrived.
        if working && !state.greeted.contains(&robot) && let Some((_, _, claude_at)) = claude {
            state.greeted.push(robot);
            spawn_capsule(&mut commands, &assets, waist(claude_at), Ok(robot));
        }
        // Its report flies back: to Claude, or to the inbox when Claude is away.
        if is_sub && avatar.reported && !state.reported.contains(&robot) {
            state.reported.push(robot);
            let to = match claude {
                Some((claude, _, _)) if avatar.handing > 0.0 => Ok(claude),
                _ => Err(inbox()),
            };
            spawn_capsule(&mut commands, &assets, waist(at), to);
        }
        // The beacon: this one waits for you.
        match (avatar.pose == Pose::Waiting && avatar.presence == Presence::Here, state.beacons.get(&robot).copied()) {
            (true, beacon) => {
                let beacon = beacon.unwrap_or_else(|| {
                    let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(6.0, 2.6, 0.2), ..default() });
                    let beacon = commands.spawn((LinkPart, Mesh3d(meshes.add(Sphere::new(0.1))), MeshMaterial3d(glow), Transform::default())).id();
                    state.beacons.insert(robot, beacon);
                    beacon
                });
                if let Ok(mut transform) = parts.get_mut(beacon) {
                    let bob = 0.06 * (time.elapsed_secs() * 3.0).sin();
                    *transform = Transform::from_translation(at.translation + Vec3::Y * (1.25 * at.scale.y + bob));
                }
            }
            (false, Some(beacon)) => {
                commands.entity(beacon).despawn();
                state.beacons.remove(&robot);
            }
            _ => {}
        }
        // The warning ring.
        match (avatar.failed && avatar.presence == Presence::Here, state.rings.get(&robot).copied()) {
            (true, ring) => {
                let ring = ring.unwrap_or_else(|| {
                    let ring = commands
                        .spawn((LinkPart, Mesh3d(ring_mesh.clone()), MeshMaterial3d(ring_glow.clone()), Transform::default()))
                        .id();
                    state.rings.insert(robot, ring);
                    ring
                });
                if let Ok(mut transform) = parts.get_mut(ring) {
                    *transform = Transform::from_translation(at.translation + Vec3::Y * 0.03)
                        .with_scale(Vec3::new(at.scale.x, 0.3, at.scale.x));
                }
            }
            (false, Some(ring)) => {
                commands.entity(ring).despawn();
                state.rings.remove(&robot);
            }
            _ => {}
        }
    }
    // Robots that left: drop what was theirs.
    let gone: Vec<Entity> = state
        .lines
        .keys()
        .chain(state.rings.keys())
        .chain(state.beacons.keys())
        .copied()
        .filter(|robot| avatars.get(*robot).is_err())
        .collect();
    for robot in gone {
        for part in [state.lines.remove(&robot), state.rings.remove(&robot), state.beacons.remove(&robot)].into_iter().flatten() {
            commands.entity(part).despawn();
        }
    }
    state.greeted.retain(|robot| avatars.get(*robot).is_ok());
    state.reported.retain(|robot| avatars.get(*robot).is_ok_and(|(_, avatar, _)| avatar.reported));

    // Capsules fly on a low arc, then vanish into whoever gets them.
    let dt = time.delta_secs();
    for (entity, mut capsule, mut transform) in &mut capsules {
        capsule.age += dt;
        let target = match capsule.to {
            Ok(robot) => avatars.get(robot).map(|(_, _, at)| waist(at)).unwrap_or(capsule.from),
            Err(spot) => spot,
        };
        let k = (capsule.age / CAPSULE_SECS).min(1.0);
        if k >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let lift = (k * std::f32::consts::PI).sin() * 1.2;
        transform.translation = capsule.from.lerp(target, k) + Vec3::Y * lift;
        transform.rotation = Quat::from_rotation_y(capsule.age * 6.0);
        transform.scale = Vec3::splat(1.6 * (k * std::f32::consts::PI).sin().max(0.2));
        view.moving = true;
    }
}

/// A line or ring that `links` owns.
#[derive(Component)]
pub(crate) struct LinkPart;

fn spawn_capsule(commands: &mut Commands, assets: &AssetServer, from: Vec3, to: Result<Entity, Vec3>) {
    commands
        .spawn((
            Capsule { from, to, age: 0.0 },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset("models/stations/holo_capsule.glb"))),
            Transform::from_translation(from).with_scale(Vec3::splat(0.3)),
        ))
        .observe(brighten_holo);
}

/// The user's view of the HQ: panned (metres on the floor), zoomed (1 is the whole
/// HQ, smaller is closer), turned round the point looked at and tilted up or down
/// (radians; positive tilt looks from lower down).
#[derive(Resource)]
pub struct CameraView {
    pan: Vec2,
    zoom: f32,
    turn: f32,
    tilt: f32,
}

impl Default for CameraView {
    fn default() -> Self {
        Self { pan: Vec2::ZERO, zoom: 1.0, turn: 0.0, tilt: 0.0 }
    }
}

/// How far the view tilts: from almost straight down (85 degrees) to a low view
/// (15 degrees above the floor); the default looks down at about 39 degrees.
const TILT_RANGE: (f32, f32) = (-0.8, 0.42);

/// How far the view turns either way: further, the back wall hides the HQ.
const TURN_LIMIT: f32 = 1.05;

/// How far the view may pan (metres from the centre) and zoom.
const PAN_LIMIT: Vec2 = Vec2::new(8.0, 6.0);
const ZOOM_RANGE: (f32, f32) = (0.3, 1.25);

/// Full view only: drag to pan, right-drag or Option-drag to turn (sideways) and
/// tilt (up and down), a two-finger twist to turn, scroll or pinch to zoom, double-click to reset. Only
/// over the HQ itself, never when the pointer is on a panel (its scrolling must not
/// move the room).
/// Which workstation model a click ray hits, triangle-exact, and how high up. Runs
/// only on a click (nothing per frame).
#[derive(bevy::ecs::system::SystemParam)]
pub struct StationPick<'w, 's> {
    ray_cast: bevy::picking::mesh_picking::ray_cast::MeshRayCast<'w, 's>,
    bodies: Query<'w, 's, &'static crate::world3d::StationBody>,
    parents: Query<'w, 's, &'static ChildOf>,
}

impl StationPick<'_, '_> {

    /// The console of the station whose model the ray hits first, unless that hit is on
    /// the station's platform or floor ring.
    fn console(&mut self, ray: Ray3d) -> Option<crate::console::Console> {
        use bevy::picking::mesh_picking::ray_cast::{MeshRayCastSettings, RayCastVisibility};
        let (bodies, parents) = (&self.bodies, &self.parents);
        let on_station = |entity: Entity| {
            std::iter::once(entity).chain(parents.iter_ancestors(entity)).any(|ancestor| bodies.contains(ancestor))
        };
        let settings = MeshRayCastSettings::default()
            .with_filter(&on_station)
            .with_visibility(RayCastVisibility::VisibleInView);
        let hit = self.ray_cast.cast_ray(ray, &settings).first().map(|(_, hit)| hit.point)?;
        crate::console::station_click(hit.y)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn camera_input(
    mode: Res<crate::mini::ViewMode>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<RoomCamera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    (motion, scroll): (Res<bevy::input::mouse::AccumulatedMouseMotion>, Res<bevy::input::mouse::AccumulatedMouseScroll>),
    mut stations: StationPick,
    mut pinch: MessageReader<bevy::input::gestures::PinchGesture>,
    mut twist: MessageReader<bevy::input::gestures::RotationGesture>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut hand: ResMut<CameraView>,
    mut open: ResMut<crate::console::OpenConsole>,
    mut dragging: Local<Option<MouseButton>>,
    mut last_click: Local<f32>,
    mut pressed_at: Local<Option<Vec2>>,
) {
    let pinched: f32 = pinch.read().map(|gesture| gesture.0).sum();
    let twisted: f32 = twist.read().map(|gesture| gesture.0).sum();
    if mode.mini {
        *dragging = None;
        return;
    }
    let over = window.cursor_position().is_some_and(|cursor| {
        camera.single().ok().and_then(|(camera, _)| camera.logical_viewport_rect()).is_some_and(|rect| rect.contains(cursor))
    }) && !egui_input.wants_any_pointer_input();
    let option = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    if buttons.just_pressed(MouseButton::Left) && over {
        let now = time.elapsed_secs();
        if now - *last_click < 0.35 {
            *hand = CameraView::default();
        }
        *last_click = now;
        *dragging = Some(if option { MouseButton::Right } else { MouseButton::Left });
        *pressed_at = window.cursor_position();
    }
    // A click that didn't drag: on a station with a console opens it (PLAN.md M19),
    // anywhere else closes the open one.
    if buttons.just_released(MouseButton::Left)
        && let Some(pressed) = pressed_at.take()
        && let Some(cursor) = window.cursor_position()
        && cursor.distance(pressed) < 5.0
        && let Ok((camera, camera_at)) = camera.single()
    {
        use crate::console::{Console, console_at};
        let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
        // Workstations: only a click on the model itself, not the platform or ring under it.
        let on_station = camera.viewport_to_world(camera_at, cursor - offset).ok().and_then(|ray| stations.console(ray));
        // The teleporter and the wall screen keep the reach around their spot.
        let elsewhere = || {
            let spots = Console::ALL.into_iter().filter(|console| !console.on_command()).filter_map(|console| {
                Some((console, camera.world_to_viewport(camera_at, console.anchor()).ok()? + offset))
            });
            console_at(cursor, spots, 90.0 / hand.zoom)
        };
        open.0 = on_station.or_else(elsewhere);
    }
    if buttons.just_pressed(MouseButton::Right) && over {
        *dragging = Some(MouseButton::Right);
    }
    if !buttons.any_pressed([MouseButton::Left, MouseButton::Right]) {
        *dragging = None;
    }
    if motion.delta != Vec2::ZERO {
        match *dragging {
            // Turn: sideways moves round the HQ, up and down tilts the view.
            Some(MouseButton::Right) => {
                hand.turn = (hand.turn - motion.delta.x * 0.006).clamp(-TURN_LIMIT, TURN_LIMIT);
                hand.tilt = (hand.tilt - motion.delta.y * 0.005).clamp(TILT_RANGE.0, TILT_RANGE.1);
            }
            // Pan: the floor follows the hand, in the direction the camera faces.
            Some(_) => {
                let metres = 0.018 * hand.zoom;
                let moved = Vec2::from_angle(-hand.turn).rotate(motion.delta * metres);
                hand.pan = (hand.pan - moved).clamp(-PAN_LIMIT, PAN_LIMIT);
            }
            None => {}
        }
    }
    if over && twisted != 0.0 {
        hand.turn = (hand.turn + twisted).clamp(-TURN_LIMIT, TURN_LIMIT);
    }
    if over {
        let lines = match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        let step = lines * 0.08 + pinched;
        if step != 0.0 {
            hand.zoom = (hand.zoom * (1.0 - step)).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        }
    }
}

/// Claude's todo list as a holographic board: done items ticked and dimmed, the one
/// in progress bright, the rest waiting.
fn plan_board(ctx: &egui::Context, at: Vec2, todos: &[(String, crate::model::TodoState)]) {
    use crate::model::TodoState;
    const SHOWN: usize = 7;
    egui::Area::new("plan-board".into())
        .fixed_pos(egui::pos2(at.x, at.y))
        .pivot(egui::Align2::CENTER_BOTTOM)
        .interactable(false)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(8, 40, 64, 170))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(90, 210, 255)))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_max_width(260.0);
                    let done = todos.iter().filter(|(_, state)| *state == TodoState::Done).count();
                    ui.label(
                        egui::RichText::new(format!("Plan · {done}/{}", todos.len()))
                            .small()
                            .strong()
                            .color(egui::Color32::from_rgb(90, 210, 255)),
                    );
                    // Around the item in progress, so the board shows where Claude is.
                    let doing = todos.iter().position(|(_, state)| *state == TodoState::Doing).unwrap_or(done);
                    let first = doing.saturating_sub(2).min(todos.len().saturating_sub(SHOWN));
                    for (text, state) in todos.iter().skip(first).take(SHOWN) {
                        let (mark, color) = match state {
                            TodoState::Done => ("✔", egui::Color32::from_rgb(80, 220, 140).gamma_multiply(0.7)),
                            TodoState::Doing => ("➡", egui::Color32::WHITE),
                            TodoState::Pending => ("·", egui::Color32::from_gray(160)),
                        };
                        let short = room_proto::describe::truncate(text, 48);
                        let line = egui::RichText::new(format!("{mark} {short}")).small().color(color);
                        ui.label(if *state == TodoState::Doing { line.strong() } else { line });
                    }
                });
        });
}

/// Where the context tank stands: on the main platform, front right of Claude.
/// Beside Claude on the command station's platform (front right).
fn tank_at() -> Vec3 {
    Spot::Command.center() + Vec3::new(1.45, 0.35, 0.75)
}
const TANK_HEIGHT: f32 = 0.9;

/// The tank's parts and the level it shows now (it eases to the real one).
#[derive(Default)]
pub struct Tank {
    parts: Option<(Entity, Handle<StandardMaterial>)>,
    level: f32,
    /// The level the fill mesh shows (only touched when it changes).
    drawn: Option<f32>,
}

/// A glass tube on the main platform filling with light as Claude's context fills
/// (green, amber past half, red past 80 %); when Claude Code compacts the
/// conversation it drains. Read from the transcript once per turn (`usage`).
#[allow(clippy::too_many_arguments)]
pub fn context_tank(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    model: Res<Model>,
    usage: Res<crate::usage::Usage>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fills: Query<&mut Transform>,
    mut tank: Local<Tank>,
) {
    let (fill, glow) = tank
        .parts
        .get_or_insert_with(|| {
            let glass = materials.add(StandardMaterial {
                base_color: Color::LinearRgba(LinearRgba::new(0.6, 0.85, 1.0, 0.18)),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.1,
                ..default()
            });
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(0.14, TANK_HEIGHT))),
                MeshMaterial3d(glass),
                Transform::from_translation(tank_at() + Vec3::Y * TANK_HEIGHT / 2.0),
            ));
            let base = materials.add(StandardMaterial { base_color: Color::srgb(0.2, 0.22, 0.28), metallic: 0.8, ..default() });
            commands.spawn((Mesh3d(meshes.add(Cylinder::new(0.18, 0.05))), MeshMaterial3d(base), Transform::from_translation(tank_at())));
            let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(0.4, 3.0, 1.2), ..default() });
            let fill = commands
                .spawn((Mesh3d(meshes.add(Cylinder::new(0.11, 1.0))), MeshMaterial3d(glow.clone()), Transform::from_scale(Vec3::ZERO)))
                .id();
            (fill, glow)
        })
        .clone();
    let Some(session) = model.shown() else { return };
    let (size, compacted) = usage.context(&session.id).unwrap_or((0, None));
    let target = (size as f32 / crate::usage::window(session.model.as_deref()) as f32).clamp(0.0, 1.0);
    // Just compacted: drain visibly, then show the new level.
    let draining = compacted.is_some_and(|at| at.elapsed().as_secs_f32() < 1.6);
    let goal = if draining { 0.0 } else { target };
    let speed = if draining { 0.9 } else { 0.5 };
    tank.level += (goal - tank.level).clamp(-speed * time.delta_secs(), speed * time.delta_secs());
    if (tank.level - goal).abs() > 0.001 {
        view.moving = true;
    }
    if tank.drawn.is_some_and(|drawn| (drawn - tank.level).abs() < 1e-4) {
        return;
    }
    tank.drawn = Some(tank.level);
    let height = TANK_HEIGHT * tank.level;
    if let Ok(mut at) = fills.get_mut(fill) {
        *at = Transform::from_translation(tank_at() + Vec3::Y * (0.03 + height / 2.0)).with_scale(Vec3::new(1.0, height.max(0.0001), 1.0));
    }
    let color = match tank.level {
        level if level > 0.8 => LinearRgba::rgb(4.0, 0.5, 0.3),
        level if level > 0.5 => LinearRgba::rgb(3.6, 2.0, 0.2),
        _ => LinearRgba::rgb(0.4, 3.0, 1.2),
    };
    if let Some(mut material) = materials.get_mut(&glow)
        && material.emissive != color
    {
        material.emissive = color;
    }
}

/// A spark of a celebration burst.
#[derive(Component)]
pub(crate) struct Spark {
    velocity: Vec3,
    age: f32,
}

/// How long a burst's sparks fly.
const SPARK_SECS: f32 = 1.6;

/// Where the trophy shelf starts (along the front edge, right of the teleporter) and how far
/// apart trophies stand.
const SHELF_AT: Vec3 = Vec3::new(1.8, 0.0, 5.6);
const TROPHY_STEP: f32 = 0.34;

fn trophy_spot(index: usize) -> Vec3 {
    SHELF_AT + Vec3::new(index as f32 * TROPHY_STEP, 0.42, 0.0)
}

/// What `celebrate` keeps: the shared look and the trophy cubes on the shelf.
#[derive(Default)]
pub struct Celebration {
    look: Option<(Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>)>,
    cubes: Vec<Entity>,
    /// The trophies the cubes show (count and the oldest one's time).
    shown: (usize, Option<std::time::Instant>),
}

/// Plays the wins: a burst of sparks over the robot whose commit or test run
/// succeeded, and a gold cube on the shelf for every finished turn.
#[allow(clippy::too_many_arguments)]
pub fn celebrate(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    time: Res<Time>,
    mut wins: ResMut<crate::wins::Wins>,
    avatars: Query<(&Avatar, &Transform), Without<Spark>>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut state: Local<Celebration>,
) {
    let (cube, spark_glow, gold) = state
        .look
        .get_or_insert_with(|| {
            let shelf = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(0.9, 0.6, 0.12), ..default() });
            let length = TROPHY_STEP * crate::wins::SHELF as f32;
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(length, 0.03, 0.2))),
                MeshMaterial3d(shelf),
                Transform::from_translation(SHELF_AT + Vec3::new(length / 2.0 - TROPHY_STEP / 2.0, 0.3, 0.0)),
            ));
            (
                meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
                materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::rgb(3.0, 2.6, 6.0), ..default() }),
                materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.8, 0.3),
                    emissive: LinearRgba::rgb(2.6, 1.7, 0.3),
                    metallic: 0.9,
                    ..default()
                }),
            )
        })
        .clone();
    // New bursts.
    for (session, who, _) in wins.bursts.drain(..) {
        let Some((_, at)) = avatars.iter().find(|(avatar, _)| avatar.session == session && avatar.who == who) else { continue };
        let from = at.translation + Vec3::Y * 1.1 * at.scale.y;
        for i in 0..28 {
            let angle = i as f32 * 2.399;
            let out = Vec3::new(angle.cos(), 0.0, angle.sin()) * (0.6 + (i % 5) as f32 * 0.25);
            commands.spawn((
                Spark { velocity: out + Vec3::Y * (2.2 + (i % 3) as f32 * 0.6), age: 0.0 },
                Mesh3d(cube.clone()),
                MeshMaterial3d(spark_glow.clone()),
                Transform::from_translation(from).with_scale(Vec3::splat(0.05)),
            ));
        }
    }
    let dt = time.delta_secs();
    for (entity, mut spark, mut at) in &mut sparks {
        spark.age += dt;
        if spark.age >= SPARK_SECS {
            commands.entity(entity).despawn();
            continue;
        }
        spark.velocity.y -= 4.0 * dt;
        at.translation += spark.velocity * dt;
        at.rotate_y(6.0 * dt);
        at.scale = Vec3::splat(0.05 * (1.0 - spark.age / SPARK_SECS));
        view.moving = true;
    }
    // The shelf: rebuilt only when its trophies change.
    let shown = (wins.trophies.len(), wins.trophies.first().map(|trophy| trophy.at));
    if shown != state.shown {
        state.shown = shown;
        for cube in state.cubes.drain(..) {
            commands.entity(cube).despawn();
        }
        state.cubes = (0..wins.trophies.len())
            .map(|index| {
                commands
                    .spawn((
                        Mesh3d(cube.clone()),
                        MeshMaterial3d(gold.clone()),
                        Transform::from_translation(trophy_spot(index))
                            .with_rotation(Quat::from_rotation_y(0.6))
                            .with_scale(Vec3::splat(0.16)),
                    ))
                    .id()
            })
            .collect();
    }
}

/// Each trophy is clickable: it opens that turn's answer again.
pub fn trophy_clicks(
    mut contexts: EguiContexts,
    mode: Res<crate::mini::ViewMode>,
    wins: Res<crate::wins::Wins>,
    mut log: ResMut<crate::log::RoomLog>,
    camera: Query<(&Camera, &GlobalTransform), With<RoomCamera3d>>,
) -> Result {
    if mode.mini || wins.trophies.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let Ok((camera, camera_at)) = camera.single() else { return Ok(()) };
    let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
    for (index, trophy) in wins.trophies.iter().enumerate() {
        let Ok(spot) = camera.world_to_viewport(camera_at, trophy_spot(index)) else { continue };
        let spot = spot + offset;
        egui::Area::new(egui::Id::new(("trophy", index)))
            .fixed_pos(egui::pos2(spot.x, spot.y))
            .pivot(egui::Align2::CENTER_CENTER)
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                let ago = crate::hud::ago(trophy.at);
                let response = response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(format!("{} · {ago} ago: click to read this turn's answer", trophy.project));
                if response.clicked() {
                    log.final_answer =
                        Some(crate::log::FinalAnswer { session: trophy.session.clone(), text: trophy.answer.clone(), open: true });
                }
            });
    }
    Ok(())
}
