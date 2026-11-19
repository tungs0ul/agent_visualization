//! The ship around the bridge (PLAN.md M29, the user's reference): the walls carried
//! up to a ceiling at 7.5 m, the back opened into a tall framed observation window,
//! and the ceiling lit by large recessed round fixtures and LED strips. The fixtures
//! are real spot lights shining down, so the floor and desks are lit in bright pools
//! with deep shadow between, and the glossy floor reflects them. Warm amber lamps
//! glow under each desk. Built once; nothing here moves.

use bevy::prelude::*;

use crate::world3d::{Kit, Spot};

/// Where the ceiling's underside is, and where the original walls stop.
pub const CEILING: f32 = 7.5;
const WALL_TOP: f32 = 4.5;
/// The shell's plan (`environment.py`): back wall at z -6.95 between x -7.5..7.5, rounded
/// corners of radius 2.6, side walls at x ±10.1 up to the open front at z 5.
const BACK_Z: f32 = -6.95;
const BACK_HALF: f32 = 7.5;
const SIDE_X: f32 = 10.1;
const CORNER_R: f32 = 2.6;
const FRONT_Z: f32 = 5.0;
/// Width of one wall bay (PROMPT_03 D).
const BAY: f32 = 2.4;

/// The brief's glow colours (linear): cyan #38C8FF for strips and rings, azure
/// #2E8BFF for the ceiling strips and fixture discs. Never white: the tonemapper burns
/// a bright saturated colour to a white core by itself and keeps the halo coloured.
const CYAN: LinearRgba = LinearRgba::rgb(0.04, 0.58, 1.0);
const AZURE: LinearRgba = LinearRgba::rgb(0.027, 0.258, 1.0);
/// The fixtures' light: a cool blue-white (#CFE6FF), a light colour, not a glow.
const FIXTURE_LIGHT: Color = Color::srgb(0.81, 0.9, 1.0);

pub fn build(kit: &mut Kit) {
    // The brief's materials (section 3.5): wall panels #122241, frames #1A2640.
    let hull = kit.materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x12, 0x22, 0x41),
        metallic: 0.5,
        perceptual_roughness: 0.45,
        ..default()
    });
    // Frames, mullions and beams in silver #9AA6B8 (PROMPT_03 A): neutral metal that
    // reflects the cyan strips rather than being blue itself.
    let trim = kit.materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x9A, 0xA6, 0xB8),
        metallic: 0.9,
        perceptual_roughness: 0.3,
        ..default()
    });
    let groove = kit.materials.add(StandardMaterial { base_color: Color::srgb_u8(0x05, 0x0A, 0x16), perceptual_roughness: 0.7, ..default() });
    let bolt = kit.materials.add(StandardMaterial { base_color: Color::srgb_u8(0x2A, 0x31, 0x40), metallic: 0.9, perceptual_roughness: 0.35, ..default() });
    let amber = kit.glow(LinearRgba::rgb(1.0, 0.33, 0.045) * 6.0);
    let green = kit.glow(LinearRgba::rgb(0.03, 0.75, 0.2) * 5.0);
    // A few short violet #8A5CFF strips in the side wall bands (crop 13).
    let violet = kit.glow(LinearRgba::rgb(0.254, 0.107, 1.0) * 5.0);
    let strip = kit.glow(CYAN * 2.4);
    let white = kit.glow(AZURE * 2.6);
    let mid = (WALL_TOP + CEILING) / 2.0;
    let tall = CEILING - WALL_TOP;

    // The back: a tall open frame onto space, mullions with a light strip each.
    kit.mesh(Cuboid::new(BACK_HALF * 2.0 + 0.6, 0.35, 0.45), trim.clone(), Transform::from_xyz(0.0, CEILING - 0.18, BACK_Z));
    kit.mesh(Cuboid::new(BACK_HALF * 2.0 + 0.6, 0.18, 0.45), trim.clone(), Transform::from_xyz(0.0, WALL_TOP + 0.09, BACK_Z));
    for x in [-7.5, -5.0, -2.5, 0.0, 2.5, 5.0, 7.5] {
        kit.mesh(Cuboid::new(0.24, tall, 0.34), hull.clone(), Transform::from_xyz(x, mid, BACK_Z));
        kit.mesh(Cuboid::new(0.05, tall - 0.3, 0.02), strip.clone(), Transform::from_xyz(x, mid, BACK_Z + 0.18));
    }
    // The rounded corners: three flat panels each, a light strip along them.
    for side in [-1.0f32, 1.0] {
        let centre = Vec2::new(side * BACK_HALF, BACK_Z + CORNER_R);
        for step in 0..3 {
            let a0 = (step as f32) / 3.0 * std::f32::consts::FRAC_PI_2;
            let a1 = (step as f32 + 1.0) / 3.0 * std::f32::consts::FRAC_PI_2;
            let at = |angle: f32| centre + Vec2::new(side * angle.sin(), -angle.cos()) * CORNER_R;
            let (p, q) = (at(a0), at(a1));
            let middle = (p + q) / 2.0;
            let length = p.distance(q);
            let turn = (q.y - p.y).atan2(q.x - p.x);
            let rotation = Quat::from_rotation_y(-turn);
            kit.mesh(Cuboid::new(length + 0.05, tall, 0.3), hull.clone(), Transform::from_xyz(middle.x, mid, middle.y).with_rotation(rotation));
            let inward = Vec2::new(-(q.y - p.y), q.x - p.x).normalize() * side * -0.16;
            for y in [WALL_TOP + 0.5, CEILING - 0.5] {
                kit.mesh(Cuboid::new(length, 0.04, 0.02), strip.clone(), Transform::from_xyz(middle.x + inward.x, y, middle.y + inward.y).with_rotation(rotation));
            }
        }
        // The side walls, two light strips along each.
        let length = FRONT_Z - (BACK_Z + CORNER_R);
        let z = (FRONT_Z + BACK_Z + CORNER_R) / 2.0;
        kit.mesh(Cuboid::new(0.3, tall, length), hull.clone(), Transform::from_xyz(side * SIDE_X, mid, z));
        for y in [WALL_TOP + 0.5, CEILING - 0.5] {
            kit.mesh(Cuboid::new(0.02, 0.04, length - 0.2), strip.clone(), Transform::from_xyz(side * (SIDE_X - 0.17), y, z));
        }
        // 2.4 m bays (PROMPT_03 D): a silver pilaster with a vertical light strip and
        // bolts between bays, two panel grooves across each, and a status light
        // cluster on every other bay, so no stretch of wall is bare.
        let face = side * (SIDE_X - 0.16);
        let first = BACK_Z + CORNER_R;
        let bays = ((FRONT_Z - first) / BAY).round().max(1.0) as usize;
        let bay = (FRONT_Z - first) / bays as f32;
        for k in 0..=bays {
            let z = first + k as f32 * bay;
            kit.mesh(Cuboid::new(0.14, tall, 0.2), trim.clone(), Transform::from_xyz(face, mid, z));
            kit.mesh(Cuboid::new(0.02, tall - 0.4, 0.035), strip.clone(), Transform::from_xyz(face - side * 0.075, mid, z));
            for y in [WALL_TOP + 0.25, CEILING - 0.25] {
                kit.mesh(Cuboid::new(0.03, 0.05, 0.05), bolt.clone(), Transform::from_xyz(face - side * 0.075, y, z - 0.06));
                kit.mesh(Cuboid::new(0.03, 0.05, 0.05), bolt.clone(), Transform::from_xyz(face - side * 0.075, y, z + 0.06));
            }
            if k == bays {
                continue;
            }
            let centre = z + bay / 2.0;
            for y in [WALL_TOP + 1.35, WALL_TOP + 2.15] {
                kit.mesh(Cuboid::new(0.015, 0.025, bay - 0.4), groove.clone(), Transform::from_xyz(face + side * 0.005, y, centre));
            }
            if k % 2 == 0 {
                for (i, light) in [amber.clone(), strip.clone(), green.clone()].into_iter().enumerate() {
                    kit.mesh(Cuboid::new(0.02, 0.05, 0.08), light, Transform::from_xyz(face - side * 0.005, WALL_TOP + 1.7, centre - 0.15 + i as f32 * 0.15));
                }
            }
        }
        for (k, y) in [WALL_TOP + 0.8, WALL_TOP + 1.0].into_iter().enumerate() {
            let along = z - 1.5 + k as f32 * 2.2;
            kit.mesh(Cuboid::new(0.02, 0.05, 1.2), violet.clone(), Transform::from_xyz(side * (SIDE_X - 0.17), y, along));
        }
    }

    // The ceiling: a dark slab, structural beams with icy light strips beneath them.
    let width = SIDE_X * 2.0 + 0.6;
    let depth = FRONT_Z - BACK_Z + 0.5;
    let middle_z = (FRONT_Z + BACK_Z) / 2.0;
    // Ceiling #0B1428 (brief 3.5), darker than the walls.
    let ceiling = kit.materials.add(StandardMaterial {
        base_color: Color::srgb_u8(0x0B, 0x14, 0x28),
        metallic: 0.5,
        perceptual_roughness: 0.5,
        ..default()
    });
    ceiling_part(kit, Cuboid::new(width, 0.3, depth), ceiling, Transform::from_xyz(0.0, CEILING + 0.15, middle_z));
    for z in [-1.0, 2.6] {
        ceiling_part(kit, Cuboid::new(width - 0.6, 0.4, 0.55), trim.clone(), Transform::from_xyz(0.0, CEILING - 0.2, z));
        ceiling_part(kit, Cuboid::new(width - 0.8, 0.03, 0.1), white.clone(), Transform::from_xyz(0.0, CEILING - 0.42, z));
    }
    for x in [-8.4, 8.4] {
        ceiling_part(kit, Cuboid::new(0.55, 0.4, depth - 0.6), trim.clone(), Transform::from_xyz(x, CEILING - 0.2, middle_z));
        ceiling_part(kit, Cuboid::new(0.1, 0.03, depth - 0.8), strip.clone(), Transform::from_xyz(x, CEILING - 0.42, middle_z));
    }

    // Recessed round fixtures, each a real spot light: the big one over the command
    // core, the others over the crew desks.
    let command = Spot::Command.center();
    let mut fixtures = vec![(Vec2::new(command.x, command.z), 1.7, 4_000_000.0)];
    fixtures.extend([(-4.0, -0.9), (4.0, -0.9), (-4.6, 3.2), (4.6, 3.2)].map(|(x, z)| (Vec2::new(x, z), 1.1, 2_600_000.0)));
    let disc = kit.glow(AZURE * 3.2);
    let ring = kit.glow(CYAN * 2.8);
    for (at, radius, lumens) in fixtures {
        let centre = Vec3::new(at.x, CEILING, at.y);
        let face_down = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        ceiling_part(kit, Circle::new(radius), disc.clone(), Transform::from_translation(centre - Vec3::Y * 0.02).with_rotation(face_down));
        ceiling_part(kit, Torus::new(radius, radius + 0.2), trim.clone(), Transform::from_translation(centre - Vec3::Y * 0.06));
        ceiling_part(kit, Torus::new(radius + 0.3, radius + 0.38), ring.clone(), Transform::from_translation(centre - Vec3::Y * 0.04));
        kit.commands.spawn((
            SpotLight {
                color: FIXTURE_LIGHT,
                intensity: lumens,
                range: 16.0,
                // A small source: a big one paints a round disc on the glossy floor.
                radius: 0.1,
                inner_angle: 0.35,
                outer_angle: 0.85,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_translation(centre - Vec3::Y * 0.1).looking_at(Vec3::new(at.x, 0.0, at.y), Vec3::Z),
            crate::look::BaseIntensity(lumens),
            all_layers(),
        ));
    }

}

/// The render layer the ceiling lives on. The main camera leaves it out while it looks
/// down from above the ceiling (a cutaway, as in the user's reference), and draws it
/// once it's inside the room; lights and the reflection probe always see it, so its
/// shadows and reflections stay.
pub const CEILING_LAYER: usize = 1;
/// Layers for everything that must see the ceiling too: lights and probe cameras.
pub fn all_layers() -> bevy::camera::visibility::RenderLayers {
    bevy::camera::visibility::RenderLayers::from_layers(&[0, CEILING_LAYER])
}

fn ceiling_part(kit: &mut Kit, mesh: impl Into<Mesh>, material: Handle<StandardMaterial>, at: Transform) {
    let part = kit.mesh(mesh, material, at);
    kit.commands.entity(part).insert(bevy::camera::visibility::RenderLayers::layer(CEILING_LAYER));
}

/// Shows the ceiling only while the camera is below it.
pub fn cutaway(mut camera: Query<(&Transform, &mut bevy::camera::visibility::RenderLayers), With<crate::view3d::RoomCamera3d>>) {
    let Ok((at, mut layers)) = camera.single_mut() else { return };
    let wanted = if at.translation.y > CEILING - 0.2 { bevy::camera::visibility::RenderLayers::layer(0) } else { all_layers() };
    if *layers != wanted {
        *layers = wanted;
    }
}
