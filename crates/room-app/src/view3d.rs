//! 3D view (PLAN.md M15), switched with the 3 key or the "3D" button.
//!
//! The 2D room stays the source of truth: every character and desk there gets a 3D
//! stand-in that copies its position each frame and plays an animation for its pose.
//! The room is a little space-station base built from the Kenney Space Station Kit
//! (`base`). Claude is the KayKit character in its orange; each sub-agent is a random
//! KayKit adventurer. All models are CC0 (`assets/models/*/License.txt`).

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};
use std::time::Duration;

use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use bevy_egui::{EguiContexts, egui};

use crate::anim::{Accent, Animating};
use crate::model::{Model, Pose, Who};
use crate::scene::{Agent, Desk, Roster, WorldCamera};

/// 2D room units per metre.
const UNITS: f32 = 100.0;
/// Station kit pieces are 1 unit to a tile; a tile here is this many metres.
const TILE: f32 = 1.5;
/// Height of the station floor's top; everything stands on it at y = 0.
const FLOOR_TOP: f32 = 0.3 * TILE;

const CLAUDE: &str = "models/kaykit/character.glb";
const ADVENTURERS: [&str; 6] = [
    "models/adventurers/Knight.glb",
    "models/adventurers/Barbarian.glb",
    "models/adventurers/Mage.glb",
    "models/adventurers/Ranger.glb",
    "models/adventurers/Rogue.glb",
    "models/adventurers/Rogue_Hooded.glb",
];
const RIG_GENERAL: &str = "models/adventurers/Rig_Medium_General.glb";
const RIG_MOVES: &str = "models/adventurers/Rig_Medium_MovementBasic.glb";

/// What a character is doing, as an animation.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Act {
    Idle,
    Work,
    Walk,
    Wave,
}

#[derive(Resource, Default)]
pub struct View3d {
    pub on: bool,
    built: bool,
}

impl View3d {
    /// `AGENT_ROOM_VIEW3D=1` starts in 3D (screenshot tests).
    pub fn from_env() -> Self {
        Self { on: std::env::var_os("AGENT_ROOM_VIEW3D").is_some(), built: false }
    }
}

/// Asks to switch between the 2D and 3D view (the egui button sends it).
#[derive(Message)]
pub struct Toggle3d;

#[derive(Component)]
pub(crate) struct RoomCamera3d;

/// Which rig a character uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Rig {
    /// The KayKit animated character (animations in its own file).
    Kaykit,
    /// A KayKit adventurer: `Rig_Medium`, animations in the shared rig files.
    Adventurer,
}

#[derive(Component)]
pub(crate) struct Avatar {
    /// The 2D character it follows.
    of: Entity,
    rig: Rig,
    tint: Option<Color>,
    accent: Color,
    /// The animation player inside the loaded model.
    player: Option<Entity>,
    playing: Option<Act>,
    last: Vec3,
    moving: f32,
}

/// The 3D desk standing in for a 2D `Desk`.
#[derive(Component)]
pub(crate) struct DeskModel {
    of: Entity,
}

/// One animation graph per rig, and its node for each act.
#[derive(Resource)]
pub(crate) struct Clips {
    graphs: HashMap<Rig, (Handle<AnimationGraph>, HashMap<Act, AnimationNodeIndex>)>,
}

impl Clips {
    fn load(assets: &AssetServer, graphs: &mut Assets<AnimationGraph>) -> Self {
        let mut all = HashMap::new();
        // Clip indices: the order of `animations` in each file.
        let kaykit = [(Act::Idle, CLAUDE, 15), (Act::Work, CLAUDE, 16), (Act::Walk, CLAUDE, 28), (Act::Wave, CLAUDE, 29)];
        let adventurer = [
            (Act::Idle, RIG_GENERAL, 6),     // Idle_A
            (Act::Work, RIG_GENERAL, 14),    // Use_Item
            (Act::Walk, RIG_MOVES, 8),       // Walking_A
            (Act::Wave, RIG_GENERAL, 8),     // Interact
        ];
        for (rig, list) in [(Rig::Kaykit, kaykit), (Rig::Adventurer, adventurer)] {
            let (graph, nodes) = AnimationGraph::from_clips(
                list.iter().map(|(_, file, index)| assets.load(GltfAssetLabel::Animation(*index).from_asset(*file))),
            );
            let acts = list.iter().map(|(act, _, _)| *act).zip(nodes).collect();
            all.insert(rig, (graphs.add(graph), acts));
        }
        Self { graphs: all }
    }
}

fn to_3d(at: Vec2) -> Vec3 {
    Vec3::new(at.x / UNITS, 0.0, -at.y / UNITS)
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

    commands.spawn((
        RoomCamera3d,
        Camera3d::default(),
        Camera {
            order: 0,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.16, 0.17, 0.21)),
            ..default()
        },
        Msaa::Sample4,
        Transform::from_xyz(0.0, 11.5, 13.0).looking_at(Vec3::new(0.0, 0.0, 0.6), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 7000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.6, -0.95, 0.0)),
        CascadeShadowConfigBuilder { num_cascades: 1, maximum_distance: 30.0, ..default() }.build(),
    ));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.8, 0.84, 1.0), brightness: 700.0, ..default() });
    // The ground around the base.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(60.0, 60.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.20, 0.21, 0.26),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -FLOOR_TOP, 0.0),
    ));
    base(&mut commands, &assets);
}

/// The station: raised floor, walls with windows at the back and sides, pipes and
/// scaffolding, props per zone, rails, stairs at the door, rocks outside (after the
/// kit's own sample scene).
fn base(commands: &mut Commands, assets: &AssetServer) {
    let mut put = |file: &'static str, x: f32, z: f32, y: f32, turn: f32| {
        spawn_model(commands, assets, file, Vec3::new(x, y, z), turn, Vec3::splat(TILE));
    };
    // Floor: 10 × 6 tiles; panels with some detail tiles, plain at the edges.
    let (cols, rows) = (10, 6);
    let x0 = -(cols as f32 - 1.0) * TILE / 2.0;
    let z0 = -(rows as f32 - 1.0) * TILE / 2.0;
    for col in 0..cols {
        for row in 0..rows {
            let edge = col == 0 || row == 0 || col == cols - 1 || row == rows - 1;
            let file = if edge {
                "models/station/floor.glb"
            } else if (col * 7 + row * 3) % 5 == 0 {
                "models/station/floor-detail.glb"
            } else {
                "models/station/floor-panel.glb"
            };
            put(file, x0 + col as f32 * TILE, z0 + row as f32 * TILE, -FLOOR_TOP, 0.0);
        }
    }
    let back = z0 - TILE / 2.0;
    let (left, right) = (x0 - TILE / 2.0, x0 + (cols as f32 - 0.5) * TILE);
    // Back wall: windows, banners, a wide door in the middle.
    for col in 0..cols {
        let file = match col {
            4 | 5 => "models/station/wall-door-wide-banner.glb",
            1 | 8 => "models/station/wall-window-banner.glb",
            2 | 7 => "models/station/wall-window.glb",
            _ => "models/station/wall.glb",
        };
        put(file, x0 + col as f32 * TILE, back, 0.0, 0.0);
    }
    // Side walls along the back half.
    for row in 0..3 {
        let z = z0 + row as f32 * TILE;
        let file = if row == 1 { "models/station/wall-window.glb" } else { "models/station/wall.glb" };
        put(file, left, z, 0.0, FRAC_PI_2);
        put(file, right, z, 0.0, -FRAC_PI_2);
    }
    put("models/station/wall-corner-round-banner.glb", left, back, 0.0, 0.0);
    put("models/station/wall-corner-round-banner.glb", right, back, 0.0, -FRAC_PI_2);
    // Pipes up the back corners.
    for (x, z) in [(left + 0.5, back + 0.6), (right - 0.5, back + 0.6)] {
        put("models/station/pipe-end-colored.glb", x, z, 0.0, 0.0);
        put("models/station/pipe-ring-colored.glb", x, z, 0.75, 0.0);
        put("models/station/pipe.glb", x, z, 1.5, 0.0);
    }

    // Zone props (`scene::ZONES`): the library's tall containers, server racks under
    // scaffolding, screens in the web bay, the approval console, a lounge by the door.
    let zone = |x: f32, y: f32| to_3d(Vec2::new(x, y));
    for dx in [-90.0, 0.0, 90.0] {
        let at = zone(-500.0 + dx, 290.0);
        put("models/station/container-tall.glb", at.x, at.z, 0.0, 0.0);
    }
    for dx in [-70.0, 70.0] {
        let at = zone(500.0 + dx, 290.0);
        put("models/station/computer-system.glb", at.x, at.z, 0.0, 0.0);
    }
    let rack = zone(640.0, 160.0);
    put("models/station/structure.glb", rack.x + TILE / 2.0, rack.z - TILE / 2.0, 0.0, 0.0);
    put("models/station/container-wide.glb", rack.x, rack.z, 0.0, 0.0);
    let bay = zone(-500.0, -60.0);
    put("models/station/display-wall-wide.glb", bay.x - 0.5, bay.z, 0.0, 0.0);
    put("models/station/table-display-planet.glb", bay.x + 0.9, bay.z + 0.3, 0.3 * TILE, 0.0);
    let approval = zone(500.0, -60.0);
    put("models/station/table-display.glb", approval.x, approval.z, 0.3 * TILE, 0.0);
    let lounge = zone(-140.0, -330.0);
    put("models/station/chair-cushion-headrest.glb", lounge.x, lounge.z, 0.0, PI);
    put("models/station/chair-cushion-headrest.glb", lounge.x + 0.6, lounge.z, 0.0, PI);
    put("models/station/table.glb", lounge.x + 0.3, lounge.z - 0.7, 0.0, 0.0);
    let away = zone(-470.0, -330.0);
    put("models/station/table-large.glb", away.x, away.z, 0.0, 0.0);
    put("models/station/container.glb", away.x + 1.6, away.z, 0.0, 0.0);

    // Front edge: rails, with a gap and stairs at the door.
    let front = z0 + (rows as f32 - 0.5) * TILE;
    for col in 0..cols {
        if col == 4 || col == 5 {
            continue;
        }
        put("models/station/rail.glb", x0 + col as f32 * TILE, front - 0.1, 0.0, 0.0);
    }
    for dx in [-TILE / 2.0, TILE / 2.0] {
        put("models/station/stairs.glb", dx, front + TILE / 2.0, -FLOOR_TOP, PI);
    }
    // Outside: rocks and a skip, like a base on another planet.
    put("models/station/rocks.glb", right + 2.6, front + 0.4, -FLOOR_TOP, 0.0);
    put("models/station/skip-rocks.glb", right + 1.6, front - 2.5, -FLOOR_TOP, 0.4);
    put("models/station/rocks.glb", left - 0.6, front - 1.0, -FLOOR_TOP, 2.2);
    put("models/station/container-flat-open.glb", left - 1.2, front + 0.6, -FLOOR_TOP, 0.3);
}

fn spawn_model(commands: &mut Commands, assets: &AssetServer, file: &'static str, at: Vec3, turn: f32, scale: Vec3) -> Entity {
    let parent = commands
        .spawn((Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn)).with_scale(scale), Visibility::default()))
        .id();
    scene_child(commands, assets, file, parent);
    parent
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

type Characters<'w, 's> = Query<'w, 's, (Entity, &'static Transform, &'static Accent, &'static Pose), (With<Agent>, Without<Avatar>)>;
type Desks<'w, 's> = Query<'w, 's, (Entity, &'static Transform), (With<Desk>, Without<DeskModel>)>;
type Avatars<'w, 's> = Query<'w, 's, (Entity, &'static mut Avatar, &'static mut Transform), (Without<Agent>, Without<Desk>)>;
type DeskModels<'w, 's> =
    Query<'w, 's, (Entity, &'static DeskModel, &'static mut Transform), (Without<Agent>, Without<Desk>, Without<Avatar>)>;

/// Gives every 2D character and desk a 3D stand-in, moves them, drops stale ones.
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    view: Res<View3d>,
    time: Res<Time>,
    assets: Res<AssetServer>,
    roster: Res<Roster>,
    characters: Characters,
    desks: Desks,
    mut avatars: Avatars,
    mut desk_models: DeskModels,
    clips: Option<Res<Clips>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    if !view.on {
        return;
    }
    let main = roster.character(&Who::Main);
    let mut have: HashMap<Entity, ()> = HashMap::new();
    for (avatar_entity, mut avatar, mut transform) in &mut avatars {
        let Ok((_, at, _, pose)) = characters.get(avatar.of) else {
            commands.entity(avatar_entity).despawn();
            continue;
        };
        have.insert(avatar.of, ());
        let target = to_3d(at.translation.truncate());
        let step = target - avatar.last;
        avatar.last = target;
        let speed = step.length() / time.delta_secs().max(1e-3);
        avatar.moving = if speed > 0.05 { 0.25 } else { (avatar.moving - time.delta_secs()).max(0.0) };
        let walking = avatar.moving > 0.0;
        transform.translation = target;
        // The models look down +z, towards the camera.
        let facing = if walking && step.length() > 1e-4 { Quat::from_rotation_y(step.x.atan2(step.z)) } else { Quat::IDENTITY };
        transform.rotation = transform.rotation.slerp(facing, (time.delta_secs() * 10.0).min(1.0));
        let act = if walking {
            Act::Walk
        } else {
            match pose {
                Pose::Idle | Pose::Waiting => Act::Idle,
                Pose::Delegating | Pose::Reporting => Act::Wave,
                _ => Act::Work,
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
    for (entity, at, accent, _) in &characters {
        if have.contains_key(&entity) {
            continue;
        }
        let is_main = Some(entity) == main;
        let (rig, file, scale, tint) = if is_main {
            (Rig::Kaykit, CLAUDE, 0.7, Some(accent.0))
        } else {
            (Rig::Adventurer, ADVENTURERS[random_index(ADVENTURERS.len())], 0.55, None)
        };
        let target = to_3d(at.translation.truncate());
        let avatar = commands
            .spawn((
                Avatar { of: entity, rig, tint, accent: accent.0, player: None, playing: None, last: target, moving: 0.0 },
                Transform::from_translation(target).with_scale(Vec3::splat(scale)),
                Visibility::default(),
            ))
            .id();
        let model = scene_child(&mut commands, &assets, file, avatar);
        commands.entity(model).observe(prepare_avatar);
    }
    // Desks: a station console with a chair, screen facing the one sitting behind it.
    let mut have: HashMap<Entity, ()> = HashMap::new();
    for (entity, model, mut transform) in &mut desk_models {
        let Ok((_, at)) = desks.get(model.of) else {
            commands.entity(entity).despawn();
            continue;
        };
        have.insert(model.of, ());
        transform.translation = to_3d(at.translation.truncate()) + Vec3::Z * 0.45;
    }
    for (entity, at) in &desks {
        if !have.contains_key(&entity) {
            let model = spawn_model(
                &mut commands,
                &assets,
                "models/station/computer-wide.glb",
                to_3d(at.translation.truncate()) + Vec3::Z * 0.45,
                PI,
                Vec3::splat(TILE),
            );
            commands.entity(model).insert(DeskModel { of: entity });
        }
    }
}

/// Sub-agents look different from each other: a random model from the pool.
fn random_index(len: usize) -> usize {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    (nanos as usize / 1000) % len
}

/// Once a model is in the world: hook up its animations and tint it.
#[allow(clippy::too_many_arguments)]
fn prepare_avatar(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
    mut avatars: Query<&mut Avatar>,
    mut players: Query<&mut AnimationPlayer>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    clips: Option<Res<Clips>>,
) {
    let root = ready.entity;
    let Ok(parent) = parents.get(root) else { return };
    let Ok(mut avatar) = avatars.get_mut(parent.parent()) else { return };
    let Some((graph, acts)) = clips.as_ref().and_then(|clips| clips.graphs.get(&avatar.rig)) else { return };
    let player = match avatar.rig {
        // The KayKit character carries its animations: glTF loading made its player.
        Rig::Kaykit => children.iter_descendants(root).find(|child| players.contains(*child)),
        // The adventurers have none in their own file, so nothing is wired for
        // animation. Their rig matches the shared rig files from `Rig_Medium` down:
        // add the player there and name every bone the way those clips address it.
        Rig::Adventurer => {
            let rig = children.iter_descendants(root).find(|child| names.get(*child).is_ok_and(|n| n.as_str() == "Rig_Medium"));
            if let Some(rig) = rig {
                commands.entity(rig).insert(AnimationPlayer::default());
                let mut stack = vec![(rig, vec![names.get(rig).cloned().unwrap_or_default()])];
                while let Some((entity, path)) = stack.pop() {
                    commands.entity(entity).insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(rig)));
                    for child in children.get(entity).into_iter().flatten() {
                        if let Ok(name) = names.get(*child) {
                            let mut next = path.clone();
                            next.push(name.clone());
                            stack.push((*child, next));
                        }
                    }
                }
            }
            rig
        }
    };
    if let Some(entity) = player {
        let mut transitions = AnimationTransitions::new();
        if let Ok(mut player) = players.get_mut(entity) {
            transitions.play(&mut player, acts[&Act::Idle], Duration::ZERO).repeat();
            avatar.playing = Some(Act::Idle);
        }
        // A player added just now is not queryable yet: `sync` starts it next frame.
        commands.entity(entity).insert((AnimationGraphHandle(graph.clone()), transitions));
        avatar.player = Some(entity);
    }
    // Claude's body is plain white-ish: tint it orange. The adventurers keep their
    // own textures; their colour shows on the label.
    if let Some(tint) = avatar.tint {
        for child in children.iter_descendants(root) {
            if let Ok(material) = meshes.get(child)
                && let Some(original) = materials.get(&material.0).cloned()
            {
                let base = original.base_color.to_linear();
                let tint = tint.to_linear();
                let tinted = StandardMaterial {
                    base_color: Color::LinearRgba(LinearRgba::new(
                        base.red * tint.red,
                        base.green * tint.green,
                        base.blue * tint.blue,
                        base.alpha,
                    )),
                    ..original
                };
                commands.entity(child).insert(MeshMaterial3d(materials.add(tinted)));
            }
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
    let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
    for (avatar, at) in &avatars {
        let Some((name, doing)) = lines.get(&avatar.of) else { continue };
        let head = at.translation() + Vec3::Y * 1.5;
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
