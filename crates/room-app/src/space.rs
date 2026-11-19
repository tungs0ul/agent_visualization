//! The view out of the panoramic window (PLAN.md M29, the user's starship reference):
//! a big cloudy blue planet with a glowing atmosphere, a small planet, nebula clouds,
//! asteroids and a few distant ships. Everything is built once at start (the planets
//! are sprites rendered in Blender, the clouds noise textures made here) and never
//! redrawn on its own: it costs nothing while the view dozes.

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
    // The big blue planet fills the lower right, only its glowing upper part above the
    // window sill (the target's crop 12), and a small tan-and-blue planet sits in the
    // left window (crop 13). Sprites rendered once in Blender (`tools/blender/planets.py`).
    let (at, reach) = through_window(5.5, -1.5, -48.0);
    planet_sprite(kit, images, include_bytes!("../../../assets/textures/space/ocean.png"), at, 4.5 * reach);
    let (at, reach) = through_window(-4.6, 3.0, -85.0);
    planet_sprite(kit, images, include_bytes!("../../../assets/textures/space/terra.png"), at, 0.75 * reach);

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

/// A planet: a camera-facing quad with a sprite rendered in Blender (premultiplied
/// RGBA, 2.6 radii wide: the body hides the sky, its atmosphere adds light round it).
fn planet_sprite(kit: &mut Kit, images: &mut Assets<Image>, png_bytes: &[u8], at: Vec3, radius: f32) {
    let material = kit.materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::new(1.4, 1.4, 1.4, 1.0)),
        base_color_texture: Some(images.add(decode_png(png_bytes))),
        alpha_mode: AlphaMode::Premultiplied,
        unlit: true,
        ..default()
    });
    let size = radius * 2.6;
    // `facing_camera` turns the quad's back (-Z) to the camera; half a turn about its
    // own up axis shows its front, so it isn't culled and the picture isn't mirrored.
    let rotation = facing_camera(at) * Quat::from_rotation_y(std::f32::consts::PI);
    kit.mesh(Rectangle::new(size, size), material, Transform::from_translation(at).with_rotation(rotation));
}

/// An RGB8 or RGBA8 PNG compiled into the binary, as a mipmapped RGBA image.
pub(crate) fn decode_png(bytes: &[u8]) -> Image {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("compiled-in PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("PNG size")];
    let info = reader.next_frame(&mut buffer).expect("PNG decodes");
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        other => panic!("compiled-in PNGs are RGB or RGBA, not {other:?}"),
    };
    image(info.width, info.height, rgba)
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

/// An RGBA image with a full mip chain and trilinear, 16x anisotropic sampling, so a
/// texture seen small or at a grazing angle stays smooth instead of sparkling (FIX 01).
pub(crate) fn image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    let mut data = pixels.clone();
    let (mut level, mut w, mut h, mut levels) = (pixels.clone(), width, height, 1);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |px: u32, py: u32| level[((py.min(h - 1) * w + px.min(w - 1)) * 4 + c) as usize] as u32;
                    let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    next[((y * nw + x) * 4 + c) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        (level, w, h, levels) = (next, nw, nh, levels + 1);
    }
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..default()
    });
    image
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
