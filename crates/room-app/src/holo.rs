//! The big 3D hologram over a crew desk while a sub-agent works there (the user,
//! 2026-11-19: the robot's label hid the desk's screen, so the screen's subject now
//! floats over the desk instead, in 3D). Each desk shows its screen's subject
//! (`screens::SUBJECTS`): the prototype's model exported from Blender
//! (`tools/blender/export_holo_subjects.sh`), drawn with `HoloMaterial`, a small
//! shader for the prototype's fresnel edge glow. A faint beam rises from the screen to
//! it. It grows in when the robot arrives, turns slowly while the robot works, holds
//! still while it waits, and shrinks away when the desk is free: idle desks cost
//! nothing.

use std::collections::HashMap;

use bevy::gltf::GltfMaterialExtras;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};

use crate::view3d::{Avatar, RoomCamera3d, View3d};
use crate::world3d::{DESKS, Spot};

pub struct HoloPlugin;

impl Plugin for HoloPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<HoloMaterial>::default()).init_resource::<Converted>();
    }
}

/// Emissive light that moves toward a rim colour where the surface turns away from
/// the viewer (`assets/shaders/holo.wesl`), added as light or drawn solid.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct HoloMaterial {
    #[uniform(0)]
    look: HoloLook,
    #[texture(1)]
    #[sampler(2)]
    texture: Option<Handle<Image>>,
    alpha_mode: AlphaMode,
}

#[derive(ShaderType, Debug, Clone, Copy)]
struct HoloLook {
    /// rgb: light face-on; a: output alpha (0 added as light, 1 solid).
    base: Vec4,
    /// rgb: light at the silhouette.
    rim: Vec4,
    /// x, y: k and e in `(1 - |n.v|^k)^e`; z: gain.
    shape: Vec4,
}

impl Material for HoloMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/holo.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// The models' light, against the Blender look (AgX, exposure 0) under the app's
/// tone mapping and bloom.
const GAIN: f32 = 0.55;
/// How big the hologram is (the models are 1 m on their largest side), and where its
/// centre floats over the desk centre: above the main screen (top 1.6 m), behind the
/// robot, so neither the robot nor its label covers it.
const SIZE: f32 = 1.7;
const CENTRE: Vec3 = Vec3::new(0.0, 2.55, -0.35);
/// Where the beam starts: the top edge of the desk's main screen (`screens::SCREENS`).
const SCREEN_TOP: f32 = 1.6;
/// Radians per second while the desk works.
const TURN: f32 = 0.35;
/// Seconds to grow in or shrink away.
const GROW: f32 = 0.45;

/// A desk's hologram: the root at its centre, unrotated (the beam stays upright).
#[derive(Component)]
pub(crate) struct DeskHolo {
    desk: u8,
    /// 0..1 grown.
    grown: f32,
    /// Its turn so far.
    turn: f32,
}

/// The subject's model under a `DeskHolo`; `faces` once its scene showed a "spin"
/// node: such subjects (sun, black hole, galaxy) face the camera as on the screen
/// and only that node turns.
#[derive(Component)]
pub(crate) struct HoloModel {
    desk: u8,
    faces: bool,
}

/// The part of a facing subject that turns about its local Y: the sun's ball, the
/// black hole's disc, the galaxy's stars.
#[derive(Component)]
pub(crate) struct HoloSpin {
    desk: u8,
    rest: Quat,
}

/// Each glTF material's hologram material, made once.
#[derive(Resource, Default)]
pub(crate) struct Converted(HashMap<AssetId<StandardMaterial>, Handle<HoloMaterial>>);

/// The model file of subject `index` of `screens::SUBJECTS`.
fn model_file(index: usize) -> String {
    format!("models/holo/{}.glb", crate::screens::SUBJECTS[index].0)
}

/// Shows a desk's hologram while a robot is at it, turns it while the robot works,
/// and removes it when the desk is free.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn desk_holograms(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    mut view: ResMut<View3d>,
    avatars: Query<&Avatar>,
    camera: Query<&GlobalTransform, With<RoomCamera3d>>,
    mut holos: Query<(Entity, &mut DeskHolo, &mut Transform)>,
    mut models: Query<(&HoloModel, &mut Transform), Without<DeskHolo>>,
    mut spins: Query<(&HoloSpin, &mut Transform), (Without<DeskHolo>, Without<HoloModel>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HoloMaterial>>,
    mut beam: Local<Option<(Handle<Mesh>, Handle<HoloMaterial>)>>,
) {
    let work = crate::view3d::desk_work(&avatars);
    let dt = time.delta_secs();
    let mut turns = [None; DESKS.len()];
    let mut shown = [false; DESKS.len()];
    for (entity, mut holo, mut at) in &mut holos {
        let desk = holo.desk as usize % DESKS.len();
        shown[desk] = true;
        let target = if work[desk].is_some() { 1.0 } else { 0.0 };
        if holo.grown != target {
            holo.grown = if target > holo.grown { (holo.grown + dt / GROW).min(1.0) } else { (holo.grown - dt / GROW).max(0.0) };
            view.moving = true;
            if holo.grown == 0.0 {
                commands.entity(entity).despawn();
                continue;
            }
        }
        if work[desk] == Some(true) {
            holo.turn += dt * TURN;
        }
        // ease in and out, so it pops neither in nor out
        let grown = holo.grown * holo.grown * (3.0 - 2.0 * holo.grown);
        at.scale = Vec3::splat(grown.max(0.001));
        turns[desk] = Some((holo.turn, at.translation));
    }
    let eye = camera.single().map(|camera| camera.translation()).ok();
    for (model, mut at) in &mut models {
        let Some((turn, centre)) = turns[model.desk as usize % DESKS.len()] else { continue };
        let rotation = match eye {
            // a facing subject looks at the camera, as on its screen
            Some(eye) if model.faces => Transform::IDENTITY.looking_to(centre - eye, Vec3::Y).rotation,
            _ => Quat::from_rotation_y(turn),
        };
        if at.rotation != rotation {
            at.rotation = rotation;
        }
    }
    for (spin, mut at) in &mut spins {
        let Some((turn, _)) = turns[spin.desk as usize % DESKS.len()] else { continue };
        at.rotation = spin.rest * Quat::from_rotation_y(turn);
    }
    // New holograms for desks that just got their robot.
    let (beam_mesh, beam_glow) = beam
        .get_or_insert_with(|| {
            let look = HoloLook {
                base: Vec4::new(0.08, 0.45, 1.0, 0.0) * 0.5,
                // fades out toward the beam's silhouette, so it reads as a soft volume
                rim: Vec4::ZERO,
                shape: Vec4::new(1.0, 1.5, 1.0, 0.0),
            };
            let texture = images.add(beam_image());
            (meshes.add(beam_mesh()), materials.add(HoloMaterial { look, texture: Some(texture), alpha_mode: AlphaMode::Add }))
        })
        .clone();
    for (desk, state) in work.iter().enumerate() {
        if state.is_none() || shown[desk] {
            continue;
        }
        let root = commands
            .spawn((
                DeskHolo { desk: desk as u8, grown: 0.0, turn: 0.0 },
                Transform::from_translation(Spot::Desk(desk as u8).center() + CENTRE).with_scale(Vec3::splat(0.001)),
                Visibility::default(),
            ))
            .id();
        commands
            .spawn((
                HoloModel { desk: desk as u8, faces: false },
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(model_file(crate::screens::subject(desk as u8))))),
                Transform::from_scale(Vec3::splat(SIZE)),
                ChildOf(root),
            ))
            .observe(prepare_model);
        commands.spawn((Mesh3d(beam_mesh.clone()), MeshMaterial3d(beam_glow.clone()), NotShadowCaster, ChildOf(root)));
        view.moving = true;
    }
}

/// When a subject's model has loaded: its materials become hologram materials, and a
/// "spin" node turns the model into a facing one.
#[allow(clippy::too_many_arguments)]
fn prepare_model(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    mut models: Query<&mut HoloModel>,
    children: Query<&Children>,
    names: Query<(&Name, &Transform)>,
    parts: Query<(&MeshMaterial3d<StandardMaterial>, Option<&GltfMaterialExtras>)>,
    standard: Res<Assets<StandardMaterial>>,
    mut holo: ResMut<Assets<HoloMaterial>>,
    mut converted: ResMut<Converted>,
) {
    let Ok(mut model) = models.get_mut(ready.entity) else { return };
    for child in children.iter_descendants(ready.entity) {
        if let Ok((name, at)) = names.get(child)
            && name.as_str() == "spin"
        {
            model.faces = true;
            commands.entity(child).insert(HoloSpin { desk: model.desk, rest: at.rotation });
        }
        let Ok((material, extras)) = parts.get(child) else { continue };
        let id = material.0.id();
        let handle = match converted.0.get(&id) {
            Some(handle) => handle.clone(),
            None => {
                let Some(source) = standard.get(&material.0) else { continue };
                let handle = holo.add(hologram_material(source, extras.map(|extras| extras.value.as_str())));
                converted.0.insert(id, handle.clone());
                handle
            }
        };
        commands.entity(child).remove::<MeshMaterial3d<StandardMaterial>>().insert((MeshMaterial3d(handle), NotShadowCaster));
    }
}

/// The hologram material for a model's glTF material: its emissive light (colour,
/// strength, texture), and from its extras (`holo_glb.py`) the rim glow `holo_rim`
/// [r, g, b, k, e] and `holo_solid`.
fn hologram_material(source: &StandardMaterial, extras: Option<&str>) -> HoloMaterial {
    let extras: serde_json::Value = extras.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
    let solid = extras["holo_solid"].as_bool().unwrap_or(false);
    let base = source.emissive.to_vec3();
    let (rim, k, e) = match extras["holo_rim"].as_array().map(|values| values.iter().filter_map(|v| v.as_f64()).map(|v| v as f32).collect::<Vec<_>>()) {
        Some(v) if v.len() == 5 => (Vec3::new(v[0], v[1], v[2]), v[3], v[4]),
        _ => (base, 1.0, 1.0),
    };
    HoloMaterial {
        look: HoloLook {
            base: base.extend(if solid { 1.0 } else { 0.0 }),
            rim: rim.extend(0.0),
            shape: Vec4::new(k.max(0.01), e, GAIN, 0.0),
        },
        texture: source.emissive_texture.clone(),
        alpha_mode: if solid { AlphaMode::Opaque } else { AlphaMode::Add },
    }
}

/// An open cone from the top of the desk's screen up to the hologram (root space).
fn beam_mesh() -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    const SIDES: u32 = 32;
    let (bottom, top) = (SCREEN_TOP - CENTRE.y, -0.25);
    let (r_bottom, r_top) = (0.3, 0.75);
    let (mut positions, mut normals, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let slope = (r_top - r_bottom) / (top - bottom);
    for i in 0..=SIDES {
        let u = i as f32 / SIDES as f32;
        let (sin, cos) = (u * std::f32::consts::TAU).sin_cos();
        let normal = Vec3::new(cos, -slope, sin).normalize();
        for (y, r, v) in [(bottom, r_bottom, 1.0), (top, r_top, 0.0)] {
            positions.push([cos * r, y, sin * r]);
            normals.push(normal.to_array());
            uvs.push([u, v]);
        }
    }
    for i in 0..SIDES {
        let a = i * 2;
        indices.extend([a, a + 1, a + 2, a + 1, a + 3, a + 2]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// The beam's light along it: bright at the screen, fading out toward the hologram
/// (uv v: 1 at the screen, 0 at the top).
fn beam_image() -> Image {
    const H: usize = 64;
    let mut bytes = Vec::with_capacity(H * 4);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        let light = v.powf(1.6) * (1.0 - ((v - 0.97) / 0.03).clamp(0.0, 1.0));
        let byte = (light.clamp(0.0, 1.0) * 255.0) as u8;
        bytes.extend([byte, byte, byte, 255]);
    }
    crate::space::image(1, H as u32, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_subject_has_its_model() {
        for index in 0..crate::screens::SUBJECTS.len() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets").join(model_file(index));
            assert!(path.exists(), "{} missing: run tools/blender/export_holo_subjects.sh", path.display());
        }
    }

    #[test]
    fn rim_glow_comes_from_the_extras() {
        let source = StandardMaterial { emissive: LinearRgba::rgb(0.1, 0.2, 0.3), ..default() };
        let rim = hologram_material(&source, Some(r#"{"holo_rim": [1, 2, 3, 0.5, 2.0]}"#));
        assert_eq!(rim.look.rim.truncate(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!((rim.look.shape.x, rim.look.shape.y), (0.5, 2.0));
        assert_eq!(rim.alpha_mode, AlphaMode::Add);
        let solid = hologram_material(&source, Some(r#"{"holo_solid": true}"#));
        assert_eq!(solid.alpha_mode, AlphaMode::Opaque);
        assert_eq!(solid.look.base.w, 1.0);
        let plain = hologram_material(&source, None);
        assert_eq!(plain.look.rim.truncate(), plain.look.base.truncate());
    }
}
