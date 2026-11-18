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
use crate::world3d::{INBOX, Location};

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

#[derive(Component)]
pub(crate) struct Avatar {
    /// The agent it is, in which session.
    pub(crate) who: Who,
    session: String,
    /// What it does now (from the director).
    pose: Pose,
    /// A sub-agent that has stopped: it hands its report over, then leaves.
    done: bool,
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
    /// The location it is at or walking to.
    at: Location,
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
) {
    commands.insert_resource(Clips::load(&assets, &mut graphs));

    // Night in space: a dark backdrop, bloom so emissive parts glow (PLAN.md M17).
    commands.spawn((
        RoomCamera3d,
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.06)),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Msaa::Sample4,
        Transform::from_translation(FULL_EYE).looking_at(FULL_LOOK, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 2500.0, color: Color::srgb(0.75, 0.75, 1.0), shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
        CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 30.0, ..default() }.build(),
    ));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.55, 0.5, 1.0), brightness: 350.0, ..default() });
    let mut kit = crate::world3d::Kit { commands: &mut commands, assets: &assets, meshes: &mut meshes, materials: &mut materials };
    crate::world3d::build(&mut kit);
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

/// Plays the director: every robot of the shown session walks to its place and plays
/// the clip for its pose; new agents get a robot, finished ones leave once their
/// report is handed over, and robots of other sessions go.
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
    // Before any session, Claude waits at its desk.
    let cast = model.shown().map_or_else(|| director::cast(&Default::default()), director::cast);
    let roles: HashMap<Who, Role> = cast.into_iter().map(|role| (role.who.clone(), role)).collect();
    let session_id = model.shown().map_or("", |session| session.id.as_str());
    // Who stays, with what to do now.
    let mut staying: Vec<(Entity, Location)> = Vec::new();
    for (entity, mut avatar, _) in &mut avatars {
        let role = roles.get(&avatar.who).filter(|_| avatar.session == session_id);
        let Some(role) = role.filter(|_| !(avatar.done && avatar.reported)) else {
            commands.entity(entity).despawn();
            continue;
        };
        avatar.pose = role.pose;
        avatar.done = role.done;
        let place = director::location_for(role.pose, avatar.who == Who::Main, avatar.at);
        staying.push((entity, place));
    }
    // Several at one place stand side by side.
    staying.sort_by_key(|(entity, location)| (*location as u8, entity.to_bits()));
    let mut slots: HashMap<Entity, (Location, usize)> = HashMap::new();
    let mut count: HashMap<Location, usize> = HashMap::new();
    for (entity, location) in staying {
        let slot = count.entry(location).or_default();
        slots.insert(entity, (location, *slot));
        *slot += 1;
    }
    for (avatar_entity, mut avatar, mut transform) in &mut avatars {
        let Some(&(location, slot)) = slots.get(&avatar_entity) else { continue };
        avatar.at = location;
        // Claude has its own spot at the main desk; sub-agents there stand in front of
        // the platform, facing it.
        let (target, face) = match location {
            Location::MainDesk if avatar.who == Who::Main => location.work_spot(0),
            Location::MainDesk => (visitor_spot(slot), PI),
            _ => location.work_spot(slot),
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
            let step = (WALK_SPEED * time.delta_secs()).min(flat.length());
            let next = here + heading * step;
            // Up or down a platform edge gradually.
            let y = here.y + (target.y - here.y) * (step / flat.length()).min(1.0);
            transform.translation = Vec3::new(next.x, y, next.z);
        } else {
            transform.translation = target;
        }
        let facing = if walking { Quat::from_rotation_y(transform_heading(here, transform.translation, flat)) } else { Quat::from_rotation_y(face) };
        transform.rotation = transform.rotation.slerp(facing, (time.delta_secs() * 8.0).min(1.0));
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
    // Newcomers: Claude starts at its desk; a new sub-agent appears beside it to get
    // its task. Agents that finished before the room saw them get no robot.
    for role in roles.values() {
        if role.done || avatars.iter().any(|(_, avatar, _)| avatar.who == role.who && avatar.session == session_id) {
            continue;
        }
        let is_main = role.who == Who::Main;
        let (scale, glow) = if is_main { (1.6, MAIN_GLOW) } else { (1.3, role.accent.to_linear() * 3.0) };
        let start = if is_main { Location::MainDesk.work_spot(0).0 } else { visitor_spot(0) };
        let avatar = commands
            .spawn((
                Avatar {
                    who: role.who.clone(),
                    session: session_id.to_owned(),
                    pose: role.pose,
                    done: false,
                    handing: 0.0,
                    rig: Rig::Robot,
                    glow,
                    accent: role.accent,
                    player: None,
                    playing: None,
                    reported: false,
                    at: Location::MainDesk,
                },
                Transform::from_translation(start).with_scale(Vec3::splat(scale)),
                Visibility::default(),
            ))
            .id();
        let model = scene_child(&mut commands, &assets, ROBOT, avatar);
        commands.entity(model).observe(prepare_avatar);
    }
}

/// A robot's walking pace (metres per second).
const WALK_SPEED: f32 = 1.8;

/// Where a sub-agent stands at the main desk: in front of the platform, facing Claude.
fn visitor_spot(slot: usize) -> Vec3 {
    Location::MainDesk.center() + Vec3::new(-1.0 + 2.0 * (slot % 2) as f32, 0.0, 2.4 + 0.6 * (slot / 2) as f32)
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
    let desk = Location::MainDesk.work_spot(0).0;
    let main_at_desk = avatars.iter().any(|(avatar, at)| avatar.who == Who::Main && at.translation.distance(desk) < 0.15);
    let mut left = documents.iter().count();
    for (mut avatar, at) in &mut avatars {
        if avatar.who == Who::Main {
            continue;
        }
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
                Transform::from_translation(INBOX + Vec3::Y * 0.04 * left as f32),
            ));
            left += 1;
        }
    }
    // Claude is back: it takes the documents.
    if main_at_desk {
        for (entity, _) in &documents {
            commands.entity(entity).despawn();
        }
        return;
    }
    // Waiting documents bob gently so they catch the eye.
    let t = time.elapsed_secs();
    for (i, (_, mut at)) in documents.iter_mut().enumerate() {
        at.translation.y = INBOX.y + 0.04 * i as f32 + 0.03 * (t * 2.0 + i as f32).sin();
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
    model: Res<Model>,
    camera: Query<(&Camera, &GlobalTransform), With<RoomCamera3d>>,
    avatars: Query<(&Avatar, &GlobalTransform)>,
    mut selected: ResMut<crate::hud::Selected>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Ok((camera, camera_at)) = camera.single() else { return Ok(()) };
    let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
    // A sign over each location, in its colour.
    for location in crate::world3d::Location::ALL.into_iter().filter(|_| !mode.mini) {
        let Ok(screen) = camera.world_to_viewport(camera_at, location.center() + Vec3::Y * 2.6) else { continue };
        let screen = screen + offset;
        let glow = location.glow();
        let peak = glow.red.max(glow.green).max(glow.blue).max(1e-3);
        let color = egui::Color32::from_rgb(
            (120.0 + 135.0 * glow.red / peak) as u8,
            (120.0 + 135.0 * glow.green / peak) as u8,
            (120.0 + 135.0 * glow.blue / peak) as u8,
        );
        egui::Area::new(egui::Id::new(("place3d", location.title())))
            .fixed_pos(egui::pos2(screen.x, screen.y))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .interactable(false)
            .order(egui::Order::Background)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(12, 14, 32, 210))
                    .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(location.title()).strong().color(color));
                        ui.label(egui::RichText::new(location.subtitle()).small().color(egui::Color32::from_gray(190)));
                    });
            });
    }
    let Some(session) = model.shown() else { return Ok(()) };
    for (avatar, at) in &avatars {
        let (name, doing) = match &avatar.who {
            Who::Main => ("Claude", &session.main_doing),
            Who::Sub(id) => match session.agent(id) {
                Some(info) => (info.agent_type.as_str(), &info.doing),
                None => continue,
            },
        };
        let lift = if mode.mini { 1.35 } else { 1.05 };
        let head = at.translation() + Vec3::Y * lift * at.scale().y;
        let Ok(screen) = camera.world_to_viewport(camera_at, head) else { continue };
        let screen = screen + offset;
        egui::Area::new(egui::Id::new(("label3d", &avatar.who)))
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
                    ui.label(egui::RichText::new(name).small().strong().color(color));
                    if let Some(doing) = doing {
                        ui.label(egui::RichText::new(doing).small());
                    }
                });
                // Clicking a robot's label opens its details (`hud`).
                let click = shown.response.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                if click.clicked() {
                    selected.0 = Some(avatar.who.clone());
                }
            });
    }
    Ok(())
}

/// The full view shows the whole workspace; the mini widget follows Claude's robot
/// up close, since the whole station would be too small to read there.
pub fn frame(
    mut view: ResMut<View3d>,
    mode: Res<crate::mini::ViewMode>,
    time: Res<Time>,
    avatars: Query<(&Avatar, &Transform), Without<RoomCamera3d>>,
    mut camera: Query<&mut Transform, With<RoomCamera3d>>,
) {
    let Ok(mut camera) = camera.single_mut() else { return };
    let (eye, look) = match avatars.iter().find(|(avatar, _)| avatar.who == Who::Main) {
        Some((_, at)) if mode.mini => (at.translation + Vec3::new(0.0, 3.0, 4.2), at.translation + Vec3::Y * 0.8),
        _ => (FULL_EYE, FULL_LOOK),
    };
    let wanted = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    let blend = (time.delta_secs() * 4.0).min(1.0);
    if camera.translation.distance(wanted.translation) > 0.001 {
        view.moving = true;
        camera.translation = camera.translation.lerp(wanted.translation, blend);
        camera.rotation = camera.rotation.slerp(wanted.rotation, blend);
    }
}

/// Where the camera looks at the whole workspace from.
const FULL_EYE: Vec3 = Vec3::new(0.0, 11.0, 13.5);
const FULL_LOOK: Vec3 = Vec3::new(0.0, 0.0, -0.4);

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
#[allow(clippy::type_complexity)]
pub fn activity_holograms(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    avatars: Query<(Entity, &Avatar, &Transform), Without<ActivityHolo>>,
    mut holos: Query<(Entity, &mut ActivityHolo, &mut Transform)>,
) {
    let mut shown: HashMap<Entity, Pose> = HashMap::new();
    for (entity, mut holo, mut at) in &mut holos {
        let robot = avatars.get(holo.of).ok();
        let pose = robot.map(|(_, avatar, _)| avatar.pose);
        let walking = robot.is_some_and(|(_, avatar, _)| avatar.playing == Some(Act::Walk));
        if pose != Some(holo.pose) || walking {
            commands.entity(entity).despawn();
            continue;
        }
        let Some((_, _, robot_at)) = robot else { continue };
        shown.insert(holo.of, holo.pose);
        holo.age += time.delta_secs();
        let grow = (holo.age / 0.35).min(1.0);
        let bob = 0.04 * (time.elapsed_secs() * 1.6 + (holo.of.to_bits() % 97) as f32).sin();
        let side = robot_at.rotation * Vec3::new(0.62, 0.0, 0.15) * robot_at.scale.x;
        at.translation = robot_at.translation + side + Vec3::Y * (0.95 * robot_at.scale.y + bob);
        at.rotation = Quat::from_rotation_y(-0.35);
        at.scale = Vec3::splat(grow * 0.9);
    }
    for (robot, avatar, _) in &avatars {
        if shown.contains_key(&robot) || avatar.playing == Some(Act::Walk) {
            continue;
        }
        let Some(file) = holo_for(avatar.pose) else { continue };
        commands.spawn((
            ActivityHolo { of: robot, pose: avatar.pose, age: 0.0 },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(file))),
            Transform::from_scale(Vec3::ZERO),
        ));
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
    let centre = Location::MainDesk.center();
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
        assert!(goal.xz().distance(Location::MainDesk.center().xz()) > 2.4);
        // Clear of the platform: straight to the target.
        let target = Vec3::new(6.0, 0.0, 4.0);
        assert_eq!(detour(Vec3::new(-6.0, 0.0, 4.0), target), target);
        // Going onto the platform (Claude's own desk): straight in.
        let desk = Location::MainDesk.work_spot(0).0;
        assert_eq!(detour(Vec3::new(-6.0, 0.0, 0.0), desk), desk);
    }
}
