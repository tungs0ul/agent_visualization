//! The view out of the panoramic window (PLAN.md M29, the user's starship reference):
//! a big cloudy blue planet with a glowing atmosphere, a ringed planet, nebula
//! clouds, asteroids and a few distant ships. Everything is built once at start,
//! from textures made here with a little noise (no image files), and never redrawn
//! on its own: it costs nothing while the view dozes.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::world3d::Kit;

/// Camera and window, to place things where the camera sees them through the glass.
const EYE: Vec3 = crate::view3d::FULL_EYE;
const GLASS_Z: f32 = -6.95;

/// A point on the window (glass x -7.5..7.5, y 0.5..3.95) carried on along the
/// camera's line of sight to depth `z`, and how much bigger things must be there to
/// look the same size on the glass.
pub fn through_window(glass_x: f32, glass_y: f32, z: f32) -> (Vec3, f32) {
    let reach = (EYE.z - z) / (EYE.z - GLASS_Z);
    (Vec3::new(glass_x * reach, EYE.y + (glass_y - EYE.y) * reach, z), reach)
}

/// Turned to face the camera (for the flat glows and clouds).
fn facing_camera(at: Vec3) -> Quat {
    Transform::from_translation(at).looking_at(EYE, Vec3::Y).rotation
}

pub fn build(kit: &mut Kit, images: &mut Assets<Image>) {
    let rim = images.add(rim_texture(128));
    // The big blue planet, low on the right, its lit side towards the upper left.
    let (at, reach) = through_window(5.0, 3.2, -48.0);
    let radius = 3.0 * reach;
    planet(kit, images, at, radius, PlanetLook::Ocean, &rim, LinearRgba::rgb(0.25, 0.75, 2.4));
    // A ringed gas giant, small and far, up on the left.
    let (at, reach) = through_window(-4.8, 5.8, -85.0);
    let radius = 0.65 * reach;
    planet(kit, images, at, radius, PlanetLook::Gas, &rim, LinearRgba::rgb(0.9, 0.7, 0.5));
    let ring = kit.materials.add(StandardMaterial {
        base_color: Color::srgba(0.85, 0.75, 0.6, 0.55),
        emissive: LinearRgba::rgb(0.25, 0.2, 0.15),
        alpha_mode: AlphaMode::Blend,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let tilt = Quat::from_rotation_x(-1.2) * Quat::from_rotation_z(0.35);
    kit.mesh(Annulus::new(radius * 1.45, radius * 2.2).mesh().resolution(64).build(), ring, Transform::from_translation(at).with_rotation(tilt));

    // Nebula clouds far behind: soft additive billboards in purple, blue and cyan.
    let cloud = images.add(cloud_texture(256, 7));
    let cloud_b = images.add(cloud_texture(256, 23));
    for (glass_x, glass_y, size, color, texture) in [
        (-3.0, 5.4, 6.5, LinearRgba::rgb(1.4, 0.4, 2.2), &cloud),
        (-6.0, 2.8, 5.0, LinearRgba::rgb(0.4, 0.8, 2.2), &cloud_b),
        (1.5, 6.2, 5.5, LinearRgba::rgb(0.3, 1.1, 2.0), &cloud_b),
        (4.5, 5.8, 4.0, LinearRgba::rgb(1.1, 0.3, 1.8), &cloud),
    ] {
        let (at, reach) = through_window(glass_x, glass_y, -120.0);
        billboard(kit, texture, at, size * reach, color);
    }

    // Asteroids drifting between the planets: a few dozen low-poly rocks sharing one
    // mesh and one material.
    let rock = kit.meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    let stone = kit.materials.add(StandardMaterial { base_color: Color::srgb(0.22, 0.2, 0.22), perceptual_roughness: 0.95, ..default() });
    let mut seed = 0x9E37_79B9_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    for _ in 0..36 {
        let (glass_x, glass_y) = (-7.0 + next() * 6.5, 1.0 + next() * 5.5);
        let (at, reach) = through_window(glass_x, glass_y, -24.0 - next() * 20.0);
        let size = (0.04 + next() * 0.12) * reach;
        let scale = Vec3::new(size * (0.7 + next() * 0.6), size * (0.6 + next() * 0.5), size * (0.7 + next() * 0.6));
        let spin = Quat::from_euler(EulerRot::XYZ, next() * 6.0, next() * 6.0, next() * 6.0);
        kit.commands.spawn((Mesh3d(rock.clone()), MeshMaterial3d(stone.clone()), Transform::from_translation(at).with_rotation(spin).with_scale(scale)));
    }

    // A few distant ships, each a hull with a bright engine.
    let hull = kit.meshes.add(Cuboid::new(1.0, 0.25, 0.35));
    let metal = kit.materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.65, 0.75), metallic: 0.8, perceptual_roughness: 0.35, ..default() });
    let engine = kit.glow(LinearRgba::rgb(1.5, 4.0, 8.0));
    let flame = kit.meshes.add(Sphere::new(0.14));
    for (glass_x, glass_y, z, heading) in [(-1.6, 4.4, -35.0, 0.3), (2.6, 6.4, -55.0, -0.5), (6.6, 1.9, -30.0, 2.6)] {
        let (at, reach) = through_window(glass_x, glass_y, z);
        let scale = Vec3::splat(0.28 * reach);
        let ship = kit
            .commands
            .spawn((Mesh3d(hull.clone()), MeshMaterial3d(metal.clone()), Transform::from_translation(at).with_rotation(Quat::from_rotation_y(heading)).with_scale(scale)))
            .id();
        kit.commands.spawn((Mesh3d(flame.clone()), MeshMaterial3d(engine.clone()), Transform::from_xyz(-0.55, 0.0, 0.0), ChildOf(ship)));
    }
}

#[derive(Clone, Copy)]
enum PlanetLook {
    Ocean,
    Gas,
}

/// A planet: a textured sphere lit by the room's key light (so it has a day and a night
/// side), plus a camera-facing glow ring around its edge for the atmosphere.
fn planet(kit: &mut Kit, images: &mut Assets<Image>, at: Vec3, radius: f32, look: PlanetLook, rim: &Handle<Image>, air: LinearRgba) {
    let surface = images.add(planet_texture(512, 256, look));
    let material = kit.materials.add(StandardMaterial {
        base_color_texture: Some(surface),
        perceptual_roughness: 0.8,
        emissive: air * 0.02,
        ..default()
    });
    kit.mesh(Sphere::new(radius).mesh().uv(64, 32), material, Transform::from_translation(at).with_rotation(Quat::from_rotation_x(-1.25) * Quat::from_rotation_z(0.25)));
    // (Tilted so its pole, where the texture pinches, faces away from the camera above.)
    // The glow sits just in front of the planet, a little larger, so its bright edge
    // rings the silhouette and fades outward.
    let toward = (EYE - at).normalize() * radius * 1.05;
    billboard(kit, rim, at + toward, radius * 2.5, air);
}

/// A flat additive picture facing the camera.
fn billboard(kit: &mut Kit, texture: &Handle<Image>, at: Vec3, size: f32, color: LinearRgba) {
    let material = kit.materials.add(StandardMaterial {
        // Unlit: the base colour is what shows (emissive is ignored).
        base_color: Color::LinearRgba(color),
        base_color_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    kit.mesh(Rectangle::new(size, size), material, Transform::from_translation(at).with_rotation(facing_camera(at)));
}

// ---------------------------------------------------------------- textures

fn image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xFFFF) as f32 / 65_535.0
}

/// Smooth value noise, tiling every `period` cells along x.
fn noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |dx: i32, dy: i32| hash((x0 + dx).rem_euclid(period.max(1)), y0 + dy, seed);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Several octaves of noise, 0..1.
fn fbm(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for octave in 0..5 {
        sum += amp * noise(x * freq, y * freq, period * freq as i32, seed + octave);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// An equirectangular planet surface: oceans, land and white clouds, or gas bands.
fn planet_texture(width: u32, height: u32, look: PlanetLook) -> Image {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f32 / width as f32 * 8.0, y as f32 / height as f32 * 4.0);
            let rgb = match look {
                PlanetLook::Ocean => {
                    let land = fbm(u, v, 8, 1);
                    let cloud = fbm(u * 1.7, v * 2.2, 14, 9);
                    let base = if land > 0.56 { [0.16, 0.32, 0.22] } else { [0.03, 0.12, 0.42] };
                    let shade = 0.75 + 0.5 * (land - 0.5).abs();
                    let white = ((cloud - 0.5) * 3.0).clamp(0.0, 1.0);
                    [0, 1, 2].map(|i| base[i] * shade * (1.0 - white) + 0.92 * white)
                }
                PlanetLook::Gas => {
                    let band = (v * 6.0 + fbm(u, v * 0.5, 8, 5) * 2.5).sin() * 0.5 + 0.5;
                    [0.75 + band * 0.2, 0.6 + band * 0.15, 0.45 + band * 0.1]
                }
            };
            pixels.extend(rgb.map(|c| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8));
            pixels.push(255);
        }
    }
    image(width, height, pixels)
}

/// A soft cloud: fbm noise fading to nothing at the edges (alpha and colour alike,
/// for additive blending).
fn cloud_texture(size: u32, seed: u32) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32 * 2.0 - 1.0, y as f32 / size as f32 * 2.0 - 1.0);
            let edge = (1.0 - (u * u + v * v).sqrt()).clamp(0.0, 1.0);
            let n = fbm(x as f32 / size as f32 * 6.0, y as f32 / size as f32 * 6.0, 6, seed);
            let value = ((n - 0.35) * 1.8).clamp(0.0, 1.0) * edge * edge;
            let byte = (value * 255.0) as u8;
            pixels.extend([byte, byte, byte, byte]);
        }
    }
    image(size, size, pixels)
}

/// A glow ring: bright at 0.4 of the radius (the planet's edge on a quad 2.5x its
/// radius), dark inside, fading outward.
fn rim_texture(size: u32) -> Image {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32 * 2.0 - 1.0, y as f32 / size as f32 * 2.0 - 1.0);
            let r = (u * u + v * v).sqrt();
            let edge = 0.4;
            let value = if r < edge { ((r - edge + 0.06) / 0.06).clamp(0.0, 1.0).powi(2) } else { (1.0 - (r - edge) / (1.0 - edge)).clamp(0.0, 1.0).powi(3) };
            let byte = (value * 255.0) as u8;
            pixels.extend([byte, byte, byte, byte]);
        }
    }
    image(size, size, pixels)
}

/// The surround polished metal reflects (a cubemap, 6 faces of `size`): dark navy, a
/// cyan band at the height of the wall light strips, the ceiling's round fixtures and
/// strips overhead, and the window's blue planet glow ahead.
pub fn environment(size: u32) -> Image {
    use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension};
    let mut pixels = Vec::with_capacity((size * size * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let (u, v) = ((x as f32 + 0.5) / size as f32 * 2.0 - 1.0, (y as f32 + 0.5) / size as f32 * 2.0 - 1.0);
                // The usual cubemap face order: +x, -x, +y, -y, +z, -z.
                let dir = match face {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let mut rgb = Vec3::new(0.012, 0.02, 0.045);
                // Wall light strips all round.
                let band = (1.0 - ((dir.y - 0.12).abs() / 0.05)).clamp(0.0, 1.0);
                rgb += Vec3::new(0.1, 0.55, 1.0) * band * 2.5;
                // The ceiling's round fixtures and light strips (`hull`), seen from the floor.
                for fixture in [Vec3::new(0.0, 0.9, -0.45), Vec3::new(-0.55, 0.8, 0.1), Vec3::new(0.55, 0.8, 0.1), Vec3::new(-0.5, 0.82, 0.4), Vec3::new(0.5, 0.82, 0.4)] {
                    let near = dir.dot(fixture.normalize());
                    rgb += Vec3::new(0.7, 0.91, 1.0) * ((near - 0.975) / 0.025).clamp(0.0, 1.0) * 9.0;
                }
                if dir.y > 0.5 && (dir.z * 6.0).fract().abs() < 0.04 {
                    rgb += Vec3::new(0.7, 0.91, 1.0) * 2.0;
                }
                // The window ahead (-z), lit by the blue planet.
                if dir.z < -0.6 && dir.y > -0.05 && dir.y < 0.45 {
                    rgb += Vec3::new(0.05, 0.18, 0.45) * ((-dir.z - 0.6) / 0.4);
                }
                for channel in [rgb.x, rgb.y, rgb.z, 1.0] {
                    pixels.extend_from_slice(&f16_bits(channel).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 6 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..Default::default() });
    image
}

/// A small non-negative float as half-precision bits (no infinities or NaNs here).
fn f16_bits(value: f32) -> u16 {
    let bits = value.max(0.0).to_bits();
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    if exponent <= 0 {
        return 0;
    }
    if exponent >= 31 {
        return 0x7BFF;
    }
    ((exponent as u16) << 10) | ((bits >> 13) & 0x3FF) as u16
}
