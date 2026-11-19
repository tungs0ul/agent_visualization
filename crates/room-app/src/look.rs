//! The look settings (PLAN.md M30, the visual overhaul brief section 14.1): exposure,
//! ambient and key light, reflection strength, light and glow scales, colour grading,
//! vignette and sky colour, in `assets/look.json`. A dev build re-reads the file when
//! it changes on disk (checked twice a second), so the look can be tuned without a
//! rebuild; a release build uses the copy built into the binary.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bevy::post_process::effect_stack::Vignette;
use bevy::prelude::*;
use bevy::render::view::ColorGrading;

const BUILT_IN: &str = include_str!("../../../assets/look.json");
const CHECK_EVERY: Duration = Duration::from_millis(500);

#[derive(Resource, Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct Look {
    pub exposure_ev100: f32,
    pub ambient_brightness: f32,
    pub ambient_color: String,
    pub key_lux: f32,
    pub key_color: String,
    /// The soft white light from above.
    pub top_lux: f32,
    pub env_intensity: f32,
    /// Multiplies every ceiling spot light's own intensity.
    pub spot_scale: f32,
    /// Multiplies every point light's own intensity.
    pub point_scale: f32,
    /// Multiplies the glow of the materials made in code (`palette_glow`).
    pub emissive_scale: f32,
    pub post_saturation: f32,
    pub temperature: f32,
    pub midtones_contrast: f32,
    pub shadows_saturation: f32,
    pub highlights_saturation: f32,
    /// 0 none, 1 black corners.
    pub vignette: f32,
    /// Glow halo strength; 0 turns bloom off.
    pub bloom_intensity: f32,
    /// How wide the halo spreads: 0 tight around the strips, 1 a wide haze.
    pub bloom_spread: f32,
    /// Only light brighter than this (scene units after exposure) blooms, so the
    /// room's surfaces never haze over.
    pub bloom_threshold: f32,
    pub sky_color: String,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            exposure_ev100: 9.7,
            ambient_brightness: 25.0,
            ambient_color: "#1A3A7A".into(),
            key_lux: 1800.0,
            key_color: "#B8D4FF".into(),
            top_lux: 600.0,
            env_intensity: 500.0,
            spot_scale: 1.0,
            point_scale: 1.0,
            emissive_scale: 1.0,
            post_saturation: 1.15,
            temperature: -0.05,
            midtones_contrast: 1.06,
            shadows_saturation: 1.15,
            highlights_saturation: 0.95,
            vignette: 0.3,
            bloom_intensity: 0.12,
            bloom_spread: 0.2,
            bloom_threshold: 1.0,
            sky_color: "#020A1E".into(),
        }
    }
}

impl Look {
    pub fn built_in() -> Self {
        serde_json::from_str(BUILT_IN).unwrap_or_default()
    }

    pub fn color(hex: &str) -> Color {
        Srgba::hex(hex).map(Color::Srgba).unwrap_or(Color::WHITE)
    }
}

/// The window key light.
#[derive(Component)]
pub struct KeyLight;

/// The soft white light from above.
#[derive(Component)]
pub struct TopLight;

/// A light's intensity before the look's scale (so scaling twice doesn't compound).
#[derive(Component, Clone, Copy)]
pub struct BaseIntensity(pub f32);

/// The file a dev build watches: the checkout's `assets/look.json`, or `$AGENT_ROOM_LOOK`.
fn watched() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AGENT_ROOM_LOOK") {
        return Some(PathBuf::from(path));
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/look.json");
    path.exists().then_some(path)
}

/// Re-reads the look file when it changed on disk.
pub fn reload(time: Res<Time<Real>>, mut look: ResMut<Look>, mut last: Local<Option<(f32, Option<SystemTime>)>>) {
    let now = time.elapsed_secs();
    if last.is_some_and(|(at, _)| now - at < CHECK_EVERY.as_secs_f32()) {
        return;
    }
    let Some(path) = watched() else { return };
    let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok();
    let changed = last.is_none_or(|(_, seen)| seen != modified);
    *last = Some((now, modified));
    if !changed {
        return;
    }
    match std::fs::read_to_string(&path).map(|text| serde_json::from_str::<Look>(&text)) {
        Ok(Ok(fresh)) if fresh != *look => {
            info!("look: reloaded {}", path.display());
            *look = fresh;
        }
        Ok(Err(err)) => warn!("look: {}: {err}", path.display()),
        _ => {}
    }
}

/// Puts the look on the camera, the lights and the sky whenever it changes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn apply(
    look: Res<Look>,
    mut commands: Commands,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut camera: Query<(Entity, &mut Camera), With<crate::view3d::RoomCamera3d>>,
    mut sun: Query<&mut DirectionalLight, (With<KeyLight>, Without<TopLight>)>,
    mut top: Query<&mut DirectionalLight, (With<TopLight>, Without<KeyLight>)>,
    mut spots: Query<(&mut SpotLight, &BaseIntensity), Without<PointLight>>,
    mut points: Query<(&mut PointLight, &BaseIntensity), Without<SpotLight>>,
    mut env: Query<&mut bevy::light::GeneratedEnvironmentMapLight>,
    mut done: Local<bool>,
) {
    if *done && !look.is_changed() {
        return;
    }
    let Ok((entity, mut cam)) = camera.single_mut() else { return };
    *done = true;
    ambient.color = Look::color(&look.ambient_color);
    ambient.brightness = look.ambient_brightness;
    cam.clear_color = ClearColorConfig::Custom(Look::color(&look.sky_color));
    let mut grading = ColorGrading::default();
    grading.global.post_saturation = look.post_saturation;
    grading.global.temperature = look.temperature;
    grading.midtones.contrast = look.midtones_contrast;
    grading.shadows.saturation = look.shadows_saturation;
    grading.highlights.saturation = look.highlights_saturation;
    commands.entity(entity).insert((
        bevy::camera::Exposure { ev100: look.exposure_ev100 },
        grading,
        Vignette { intensity: look.vignette, ..default() },
    ));
    if look.bloom_intensity > 0.0 {
        use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
        commands.entity(entity).insert(Bloom {
            intensity: look.bloom_intensity,
            low_frequency_boost: look.bloom_spread,
            high_pass_frequency: 0.4 + 0.6 * look.bloom_spread,
            prefilter: BloomPrefilter { threshold: look.bloom_threshold, threshold_softness: 0.4 },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        });
    } else {
        commands.entity(entity).remove::<bevy::post_process::bloom::Bloom>();
    }
    for mut light in &mut sun {
        light.illuminance = look.key_lux;
        light.color = Look::color(&look.key_color);
    }
    for mut light in &mut top {
        light.illuminance = look.top_lux;
    }
    for (mut spot, base) in &mut spots {
        spot.intensity = base.0 * look.spot_scale;
    }
    for (mut point, base) in &mut points {
        point.intensity = base.0 * look.point_scale;
    }
    for mut map in &mut env {
        map.intensity = look.env_intensity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_look_parses_and_names_real_colours() {
        let look: Look = serde_json::from_str(BUILT_IN).expect("assets/look.json parses");
        for hex in [&look.ambient_color, &look.key_color, &look.sky_color] {
            assert!(Srgba::hex(hex).is_ok(), "{hex}");
        }
        // A missing field takes its default.
        let partial: Look = serde_json::from_str(r#"{"key_lux": 900}"#).unwrap();
        assert_eq!((partial.key_lux, partial.vignette), (900.0, Look::default().vignette));
    }
}
