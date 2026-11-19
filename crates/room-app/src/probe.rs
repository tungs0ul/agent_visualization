//! The interior reflection probe (FIX 01 step 2, PROMPT_02 step 1): once the room has
//! loaded, six cameras render it from the middle of the bridge into six HDR images,
//! the GPU copies them back, and they become the cubemap the room's metal and glossy
//! floor reflect. Stable (nothing temporal or screen-space) and free after the first
//! second. Robots and anything that moves later aren't in it.

use bevy::asset::RenderAssetUsages;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::GeneratedEnvironmentMapLight;
use bevy::prelude::*;
use bevy::render::gpu_readback::{ReadbackComplete, ReadbackOnce};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension};
use bevy::render::view::DebandDither;

/// Face size in pixels. Must equal the startup cubemap's size (`view3d::build`):
/// Bevy's reflection filter sizes its mip chain from the first map and doesn't resize
/// it when the map is swapped. 256 × 8 bytes = 2048 per row: no row padding to undo.
pub const SIZE: u32 = 256;
/// Where the probe sees the room from: above the floor's middle, below the fixtures.
const AT: Vec3 = Vec3::new(0.0, 1.6, 0.0);
/// Seconds after start before capturing, so every model has loaded.
const WAIT: f32 = 2.5;
/// Frames the cameras render before the copy, so the images hold a finished frame.
const SETTLE_FRAMES: u32 = 3;

/// Cubemap faces in wgpu's order (+x, -x, +y, -y, +z, -z): where each looks, and its up.
const FACES: [(Vec3, Vec3); 6] = [
    (Vec3::X, Vec3::Y),
    (Vec3::NEG_X, Vec3::Y),
    (Vec3::Y, Vec3::NEG_Z),
    (Vec3::NEG_Y, Vec3::Z),
    (Vec3::Z, Vec3::Y),
    (Vec3::NEG_Z, Vec3::Y),
];

#[derive(Default)]
pub enum Stage {
    #[default]
    Waiting,
    Rendering { cameras: Vec<Entity>, images: Vec<Handle<Image>>, frames: u32 },
    Copying { cameras: Vec<Entity> },
    Done,
}

/// The faces the GPU has sent back so far.
#[derive(Resource, Default)]
pub struct Faces(pub [Option<Vec<u8>>; 6]);

#[allow(clippy::too_many_arguments)]
pub fn capture(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut images: ResMut<Assets<Image>>,
    mut faces: ResMut<Faces>,
    look: Res<crate::look::Look>,
    main: Query<Entity, With<crate::view3d::RoomCamera3d>>,
    mut stage: Local<Stage>,
) {
    match &mut *stage {
        Stage::Waiting if time.elapsed_secs() >= WAIT => {
            let sky = crate::look::Look::color(&look.sky_color);
            let mut cameras = Vec::new();
            let mut targets = Vec::new();
            for (order, (forward, up)) in FACES.into_iter().enumerate() {
                let mut image = Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba16Float, None);
                image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
                let handle = images.add(image);
                cameras.push(
                    commands
                        .spawn((
                            Camera3d::default(),
                            Camera { order: -10 - order as isize, clear_color: ClearColorConfig::Custom(sky), ..default() },
                            RenderTarget::Image(handle.clone().into()),
                            Projection::Perspective(PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, ..default() }),
                            // Linear light, no tonemapping, and the main camera's exposure so
                            // lights and glow keep the balance seen on screen. `env_intensity`
                            // (look.json) undoes the exposure: 1.2 × 2^ev100 is "as seen".
                            (Hdr, Tonemapping::None, DebandDither::Disabled, Msaa::Off, Exposure { ev100: look.exposure_ev100 }),
                            Transform::from_translation(AT).looking_to(forward, up),
                        ))
                        .id(),
                );
                targets.push(handle);
            }
            *stage = Stage::Rendering { cameras, images: targets, frames: 0 };
        }
        Stage::Rendering { cameras, images: targets, frames } => {
            *frames += 1;
            if *frames < SETTLE_FRAMES {
                return;
            }
            for (index, handle) in targets.iter().enumerate() {
                commands.spawn(ReadbackOnce::texture(handle.clone())).observe(move |event: On<ReadbackComplete>, mut faces: ResMut<Faces>| {
                    faces.0[index] = Some(event.data.clone());
                });
            }
            *stage = Stage::Copying { cameras: std::mem::take(cameras) };
        }
        Stage::Copying { cameras } => {
            if faces.0.iter().any(Option::is_none) {
                return;
            }
            for camera in cameras.drain(..) {
                commands.entity(camera).despawn();
            }
            let row = (SIZE * 8) as usize;
            let mut data = Vec::with_capacity(row * SIZE as usize * 6);
            for face in faces.0.iter_mut() {
                let face = face.take().unwrap_or_default();
                // A camera's image is the cubemap face seen from outside: mirror each row.
                for line in face.chunks_exact(row) {
                    for pixel in line.chunks_exact(8).rev() {
                        data.extend_from_slice(pixel);
                    }
                }
            }
            if data.len() != row * SIZE as usize * 6 {
                warn!("probe: readback size {} unexpected, keeping the generated reflections", data.len());
                *stage = Stage::Done;
                return;
            }
            let mut cube = Image::new(
                Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 6 },
                TextureDimension::D2,
                data,
                TextureFormat::Rgba16Float,
                RenderAssetUsages::RENDER_WORLD,
            );
            cube.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
            let cube = images.add(cube);
            for camera in &main {
                commands.entity(camera).remove::<GeneratedEnvironmentMapLight>().insert(GeneratedEnvironmentMapLight {
                    environment_map: cube.clone(),
                    intensity: look.env_intensity,
                    ..default()
                });
            }
            info!("probe: interior reflections captured ({SIZE}px faces at {AT})");
            *stage = Stage::Done;
        }
        _ => {}
    }
}
