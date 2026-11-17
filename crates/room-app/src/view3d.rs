//! 3D view (PLAN.md M15), switched with the 3 key or the "3D" button.
//!
//! The 2D room stays the source of truth for who is there and what each one does;
//! in 3D every agent is a robot that walks to the location for its work (PLAN.md M17).
//! The workspace itself is built in `world3d`. The robot comes from
//! `tools/blender/robot.py`; each agent shows its colour on the robot's ear rings and
//! chest light. Other models are CC0 (`assets/models/*/License.txt`).

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

use crate::anim::{Accent, Animating};
use crate::model::{Model, Pose, Who};
use crate::scene::{Agent, Roster, WorldCamera};
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

#[derive(Resource, Default)]
pub struct View3d {
    pub on: bool,
    built: bool,
}

impl View3d {
    /// 3D is the default view (PLAN.md M17); `AGENT_ROOM_VIEW3D=0` starts in 2D.
    pub fn from_env() -> Self {
        Self { on: std::env::var("AGENT_ROOM_VIEW3D").map_or(true, |v| v != "0"), built: false }
    }
}

/// Asks to switch between the 2D and 3D view (the egui button sends it).
#[derive(Message)]
pub struct Toggle3d;

#[derive(Component)]
pub(crate) struct RoomCamera3d;

/// Which rig a character uses (one for now; room for more models later).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Rig {
    Robot,
}

#[derive(Component)]
pub(crate) struct Avatar {
    /// The 2D character it follows.
    of: Entity,
    rig: Rig,
    /// The glow of its `Accent` material (ear rings, chest light).
    glow: LinearRgba,
    accent: Color,
    /// The animation player inside the loaded model.
    player: Option<Entity>,
    playing: Option<Act>,
    /// Its current report has been handed over or left on the desk.
    reported: bool,
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


/// 3 or the button switches the view.
pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut requests: MessageReader<Toggle3d>,
    mut view: ResMut<View3d>,
) {
    let key = keys.just_pressed(KeyCode::Digit3) && !egui_input.wants_any_keyboard_input();
    if key || requests.read().count() % 2 == 1 {
        view.on = !view.on;
    }
}

/// Builds the 3D room the first time the view is switched on.
pub fn build(
    mut commands: Commands,
    mut view: ResMut<View3d>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if !view.on || view.built {
        return;
    }
    view.built = true;
    commands.insert_resource(Clips::load(&assets, &mut graphs));

    // Night in space: a dark backdrop, bloom so emissive parts glow (PLAN.md M17).
    commands.spawn((
        RoomCamera3d,
        Camera3d::default(),
        Camera {
            order: 0,
            is_active: false,
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

/// The 2D camera draws when 3D is off and the other way round; the 3D one uses the
/// same screen area (left of the side panel).
pub fn cameras(
    view: Res<View3d>,
    mut flat: Query<&mut Camera, (With<WorldCamera>, Without<RoomCamera3d>)>,
    mut deep: Query<&mut Camera, (With<RoomCamera3d>, Without<WorldCamera>)>,
    mut animating: ResMut<Animating>,
) {
    let Ok(mut flat) = flat.single_mut() else { return };
    if flat.is_active == view.on {
        flat.is_active = !view.on;
    }
    if let Ok(mut deep) = deep.single_mut() {
        if deep.is_active != view.on {
            deep.is_active = view.on;
        }
        let same = match (&deep.viewport, &flat.viewport) {
            (Some(a), Some(b)) => a.physical_position == b.physical_position && a.physical_size == b.physical_size,
            (None, None) => true,
            _ => false,
        };
        if !same {
            deep.viewport = flat.viewport.clone();
        }
    }
    // The characters are always animated in 3D.
    if view.on {
        animating.0 = true;
    }
}

type Characters<'w, 's> = Query<'w, 's, (Entity, &'static Accent, &'static Pose), (With<Agent>, Without<Avatar>)>;
type Avatars<'w, 's> = Query<'w, 's, (Entity, &'static mut Avatar, &'static mut Transform), Without<Agent>>;

/// Gives every 2D character and desk a 3D stand-in, moves them, drops stale ones.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    view: Res<View3d>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    roster: Res<Roster>,
    characters: Characters,
    mut avatars: Avatars,
    clips: Option<Res<Clips>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    if !view.on {
        return;
    }
    let main = roster.character(&Who::Main);
    // Where everyone wants to be; several at one place stand side by side.
    let mut wanted: Vec<(Entity, Location)> = avatars
        .iter()
        .filter_map(|(entity, avatar, _)| {
            let (_, _, pose) = characters.get(avatar.of).ok()?;
            Some((entity, location_for(*pose)))
        })
        .collect();
    wanted.sort_by_key(|(entity, location)| (*location as u8, entity.to_bits()));
    let mut slots: HashMap<Entity, (Location, usize)> = HashMap::new();
    let mut count: HashMap<Location, usize> = HashMap::new();
    for (entity, location) in wanted {
        let slot = count.entry(location).or_default();
        slots.insert(entity, (location, *slot));
        *slot += 1;
    }
    let mut have: HashMap<Entity, ()> = HashMap::new();
    for (avatar_entity, mut avatar, mut transform) in &mut avatars {
        let Ok((_, _, pose)) = characters.get(avatar.of) else {
            commands.entity(avatar_entity).despawn();
            continue;
        };
        have.insert(avatar.of, ());
        let Some(&(location, slot)) = slots.get(&avatar_entity) else { continue };
        // Claude has its own spot at the main desk; sub-agents there stand in front of
        // the platform, facing it.
        let is_main = Some(avatar.of) == main;
        let (target, face) = match location {
            Location::MainDesk if is_main => location.work_spot(0),
            Location::MainDesk => (visitor_spot(slot), PI),
            _ => location.work_spot(slot),
        };
        // Walk there in a straight line at a robot's pace.
        let here = transform.translation;
        let to = target - here;
        let flat = Vec3::new(to.x, 0.0, to.z);
        let walking = flat.length() > 0.05;
        if walking {
            let step = (WALK_SPEED * time.delta_secs()).min(flat.length());
            let next = here + flat.normalize() * step;
            // Up or down a platform edge gradually.
            let y = here.y + (target.y - here.y) * (step / flat.length()).min(1.0);
            transform.translation = Vec3::new(next.x, y, next.z);
        } else {
            transform.translation = target;
        }
        let facing = if walking { Quat::from_rotation_y(flat.x.atan2(flat.z)) } else { Quat::from_rotation_y(face) };
        transform.rotation = transform.rotation.slerp(facing, (time.delta_secs() * 8.0).min(1.0));
        let act = if walking {
            Act::Walk
        } else {
            match pose {
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
    for (entity, accent, _) in &characters {
        if have.contains_key(&entity) {
            continue;
        }
        let is_main = Some(entity) == main;
        let (scale, glow) = if is_main { (1.6, MAIN_GLOW) } else { (1.3, accent.0.to_linear() * 3.0) };
        // Claude starts at its desk; a new sub-agent appears beside it to get its task.
        let start = if is_main { Location::MainDesk.work_spot(0).0 } else { visitor_spot(0) };
        let avatar = commands
            .spawn((
                Avatar { of: entity, rig: Rig::Robot, glow, accent: accent.0, player: None, playing: None, reported: false },
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

/// Where a robot goes for what it is doing (PLAN.md M17). Sub-agents waiting, getting
/// their task or bringing a report go to the main desk too (to its visitor spots).
fn location_for(pose: Pose) -> Location {
    match pose {
        Pose::Reading => Location::FileReading,
        Pose::Web => Location::Bookshelf,
        Pose::Typing => Location::WritingDesk,
        Pose::Terminal => Location::Lab,
        Pose::Idle | Pose::Thinking | Pose::Delegating | Pose::Waiting | Pose::Reporting => Location::MainDesk,
    }
}

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
    view: Res<View3d>,
    time: Res<Time>,
    roster: Res<Roster>,
    characters: Query<&Pose, With<Agent>>,
    mut avatars: Query<(&mut Avatar, &Transform), Without<Document>>,
    mut documents: Query<(Entity, &mut Transform), With<Document>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !view.on {
        return;
    }
    let main = roster.character(&Who::Main);
    let desk = Location::MainDesk.work_spot(0).0;
    let main_at_desk = avatars.iter().any(|(avatar, at)| Some(avatar.of) == main && at.translation.distance(desk) < 0.15);
    let mut left = documents.iter().count();
    for (mut avatar, at) in &mut avatars {
        if Some(avatar.of) == main {
            continue;
        }
        let reporting = characters.get(avatar.of).is_ok_and(|pose| *pose == Pose::Reporting);
        if !reporting {
            avatar.reported = false;
            continue;
        }
        let arrived = (0..8).any(|slot| at.translation.distance(visitor_spot(slot)) < 0.15);
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

/// Names and what each one is doing above the 3D characters, and the 2D/3D switch.
#[allow(clippy::too_many_arguments)]
pub fn labels(
    mut contexts: EguiContexts,
    view: Res<View3d>,
    mode: Res<crate::mini::ViewMode>,
    mut toggle: MessageWriter<Toggle3d>,
    model: Res<Model>,
    roster: Res<Roster>,
    camera: Query<(&Camera, &GlobalTransform), With<RoomCamera3d>>,
    avatars: Query<(&Avatar, &GlobalTransform)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !mode.mini {
        egui::Area::new("view-switch".into()).anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 8.0)).show(ctx, |ui| {
            let label = if view.on { "2D" } else { "3D" };
            if ui.small_button(label).on_hover_text("Switch between the 2D and 3D room (3)").clicked() {
                toggle.write(Toggle3d);
            }
        });
    }
    if !view.on {
        return Ok(());
    }
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
    let mut lines: HashMap<Entity, (String, Option<String>)> = HashMap::new();
    if let Some(main) = roster.character(&Who::Main) {
        lines.insert(main, ("Claude".to_owned(), session.main_doing.clone()));
    }
    for (id, info) in &session.agents {
        if let Some(entity) = roster.character(&Who::Sub(id.clone())) {
            lines.insert(entity, (info.agent_type.clone(), info.doing.clone()));
        }
    }
    for (avatar, at) in &avatars {
        let Some((name, doing)) = lines.get(&avatar.of) else { continue };
        let lift = if mode.mini { 1.35 } else { 1.05 };
        let head = at.translation() + Vec3::Y * lift * at.scale().y;
        let Ok(screen) = camera.world_to_viewport(camera_at, head) else { continue };
        let screen = screen + offset;
        egui::Area::new(egui::Id::new(("label3d", avatar.of)))
            .fixed_pos(egui::pos2(screen.x, screen.y))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(220.0);
                    let tint = avatar.accent.to_srgba();
                    let color =
                        egui::Color32::from_rgb((tint.red * 255.0) as u8, (tint.green * 255.0) as u8, (tint.blue * 255.0) as u8);
                    ui.label(egui::RichText::new(name).small().strong().color(color));
                    if let Some(doing) = doing {
                        ui.label(egui::RichText::new(doing).small());
                    }
                });
            });
    }
    Ok(())
}

/// The full view shows the whole workspace; the mini widget follows Claude's robot
/// up close, since the whole station would be too small to read there.
pub fn frame(
    view: Res<View3d>,
    mode: Res<crate::mini::ViewMode>,
    time: Res<Time>,
    roster: Res<Roster>,
    avatars: Query<(&Avatar, &Transform), Without<RoomCamera3d>>,
    mut camera: Query<&mut Transform, With<RoomCamera3d>>,
) {
    if !view.on {
        return;
    }
    let Ok(mut camera) = camera.single_mut() else { return };
    let main = roster.character(&Who::Main);
    let (eye, look) = match avatars.iter().find(|(avatar, _)| Some(avatar.of) == main) {
        Some((_, at)) if mode.mini => (at.translation + Vec3::new(0.0, 3.0, 4.2), at.translation + Vec3::Y * 0.8),
        _ => (FULL_EYE, FULL_LOOK),
    };
    let wanted = Transform::from_translation(eye).looking_at(look, Vec3::Y);
    let blend = (time.delta_secs() * 4.0).min(1.0);
    if camera.translation.distance(wanted.translation) > 0.001 {
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
    view: Res<View3d>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    characters: Query<&Pose, With<Agent>>,
    avatars: Query<(Entity, &Avatar, &Transform), Without<ActivityHolo>>,
    mut holos: Query<(Entity, &mut ActivityHolo, &mut Transform)>,
) {
    if !view.on {
        return;
    }
    let mut shown: HashMap<Entity, Pose> = HashMap::new();
    for (entity, mut holo, mut at) in &mut holos {
        let robot = avatars.get(holo.of).ok();
        let pose = robot.and_then(|(_, avatar, _)| characters.get(avatar.of).ok()).copied();
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
        let Ok(pose) = characters.get(avatar.of) else { continue };
        let Some(file) = holo_for(*pose) else { continue };
        commands.spawn((
            ActivityHolo { of: robot, pose: *pose, age: 0.0 },
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
    view: Res<View3d>,
    time: Res<Time>,
    avatars: Query<(&Avatar, &Transform), Without<TrailDot>>,
    mut dots: Query<(Entity, &mut TrailDot, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut since: Local<f32>,
    mut look: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    if !view.on {
        return;
    }
    let dt = time.delta_secs();
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
