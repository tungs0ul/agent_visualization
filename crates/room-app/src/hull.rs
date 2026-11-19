//! The ship around the bridge (PLAN.md M29, the user's reference): the walls carried
//! up to a ceiling at 7.5 m, the back opened into a tall framed observation window,
//! and the ceiling lit by large recessed round fixtures and LED strips. The fixtures
//! are real spot lights shining down, so the floor and desks are lit in bright pools
//! with deep shadow between, and the glossy floor reflects them. Warm amber lamps
//! glow under each desk. Built once; nothing here moves.

use bevy::prelude::*;

use crate::world3d::{DESKS, Kit, Spot};

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

/// The palette the user gave: icy blue-white, cyan, architectural blue, amber.
const ICY: LinearRgba = LinearRgba::rgb(0.70, 0.91, 1.0);
const CYAN: LinearRgba = LinearRgba::rgb(0.02, 0.62, 1.0);

/// An under-desk amber lamp (hidden on Low graphics).
#[derive(Component)]
pub struct DeskLamp;

pub fn build(kit: &mut Kit) {
    let hull = kit.materials.add(StandardMaterial {
        base_color: Color::srgb(0.075, 0.1, 0.15),
        metallic: 0.85,
        perceptual_roughness: 0.32,
        ..default()
    });
    let trim = kit.materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.6, 0.7),
        metallic: 1.0,
        perceptual_roughness: 0.2,
        ..default()
    });
    let strip = kit.glow(CYAN * 5.0);
    let white = kit.glow(ICY * 10.0);
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
    }

    // The ceiling: a dark slab, structural beams with icy light strips beneath them.
    let width = SIDE_X * 2.0 + 0.6;
    let depth = FRONT_Z - BACK_Z + 0.5;
    let middle_z = (FRONT_Z + BACK_Z) / 2.0;
    kit.mesh(Cuboid::new(width, 0.3, depth), hull.clone(), Transform::from_xyz(0.0, CEILING + 0.15, middle_z));
    for z in [-1.0, 2.6] {
        kit.mesh(Cuboid::new(width - 0.6, 0.4, 0.55), trim.clone(), Transform::from_xyz(0.0, CEILING - 0.2, z));
        kit.mesh(Cuboid::new(width - 0.8, 0.03, 0.1), white.clone(), Transform::from_xyz(0.0, CEILING - 0.42, z));
    }
    for x in [-8.4, 8.4] {
        kit.mesh(Cuboid::new(0.55, 0.4, depth - 0.6), trim.clone(), Transform::from_xyz(x, CEILING - 0.2, middle_z));
        kit.mesh(Cuboid::new(0.1, 0.03, depth - 0.8), strip.clone(), Transform::from_xyz(x, CEILING - 0.42, middle_z));
    }

    // Recessed round fixtures, each a real spot light: the big one over the command
    // core, the others over the crew desks.
    let command = Spot::Command.center();
    let mut fixtures = vec![(Vec2::new(command.x, command.z), 1.7, 1_800_000.0)];
    fixtures.extend([(-4.0, -0.9), (4.0, -0.9), (-4.6, 3.2), (4.6, 3.2)].map(|(x, z)| (Vec2::new(x, z), 1.1, 1_100_000.0)));
    let disc = kit.glow(ICY * 14.0);
    let ring = kit.glow(CYAN * 7.0);
    for (at, radius, lumens) in fixtures {
        let centre = Vec3::new(at.x, CEILING, at.y);
        let face_down = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        kit.mesh(Circle::new(radius), disc.clone(), Transform::from_translation(centre - Vec3::Y * 0.02).with_rotation(face_down));
        kit.mesh(Torus::new(radius, radius + 0.2), trim.clone(), Transform::from_translation(centre - Vec3::Y * 0.06));
        kit.mesh(Torus::new(radius + 0.3, radius + 0.38), ring.clone(), Transform::from_translation(centre - Vec3::Y * 0.04));
        kit.commands.spawn((
            SpotLight {
                color: Color::LinearRgba(ICY),
                intensity: lumens,
                range: 16.0,
                radius: radius * 0.6,
                inner_angle: 0.35,
                outer_angle: 0.85,
                ..default()
            },
            Transform::from_translation(centre - Vec3::Y * 0.1).looking_at(Vec3::new(at.x, 0.0, at.y), Vec3::Z),
        ));
    }

    // Warm amber under every desk, low and soft.
    for index in 0..DESKS.len() {
        let desk = Spot::Desk(index as u8).center();
        kit.commands.spawn((
            PointLight { color: Color::srgb(1.0, 0.7, 0.36), intensity: 14_000.0, range: 2.0, radius: 0.6, ..default() },
            Transform::from_translation(desk + Vec3::new(0.0, 0.65, -0.05)),
            DeskLamp,
        ));
    }
}
