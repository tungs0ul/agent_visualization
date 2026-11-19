//! The 3D workspace (PLAN.md M17, M27): a starship bridge. Claude works at the command
//! station by the window, sub-agents at crew desks in front of it, and the stars
//! stream past while anyone works.
//!
//! Three layers, each its own models (Blender scripts in `tools/blender/`):
//! environment (`environment.py`: floor with docking rings, the station shell with
//! its panoramic window), furniture (`stations.py`: the workstations), agents (robots,
//! activity holograms and light trails, in `view3d`). This module builds the first
//! two plus the glowing paths between stations and the sky outside the window.

use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;

/// Where a robot works (PLAN.md M27): Claude's command station at the back by the
/// window, or one of the crew desks in front of it. Metres; x right, z towards the
/// camera. The floor model has a docking ring under each (`environment.py`
/// `STATIONS`, keep in step).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Spot {
    Command,
    Desk(u8),
}

/// The crew desks (x, z): two at the back beside the command station, four in the
/// middle row, two in front either side of the teleporter.
pub const DESKS: [(f32, f32); 8] =
    [(-5.4, -2.4), (5.4, -2.4), (-2.6, 0.6), (2.6, 0.6), (-6.8, 0.6), (6.8, 0.6), (-4.6, 3.6), (4.6, 3.6)];

impl Spot {
    pub fn center(self) -> Vec3 {
        match self {
            Spot::Command => Vec3::new(0.0, 0.0, -3.6),
            Spot::Desk(index) => {
                let (x, z) = DESKS[index as usize % DESKS.len()];
                Vec3::new(x, 0.0, z)
            }
        }
    }

    /// The top of the platform or floor disc built into the station's model
    /// (`tools/blender/stations.py`: the main desk's three tiers rise to 0.35 m, a
    /// writing desk's `floor_disc` to 0.05 m), plus a little margin.
    pub fn platform_top(self) -> f32 {
        match self {
            Spot::Command => 0.38,
            Spot::Desk(_) => 0.08,
        }
    }

    /// Where a robot stands to work here and the way it faces (radians about y).
    /// Robots face the camera with the holograms floating behind them, so you see
    /// both the robot and what it works on (as in the user's reference image).
    pub fn work_spot(self, slot: usize) -> (Vec3, f32) {
        let side = [0.0, -0.75, 0.75, -1.5, 1.5][slot % 5];
        let (front, height) = match self {
            Spot::Command => (0.0, 0.35),
            Spot::Desk(_) => (0.85, 0.043),
        };
        (self.center() + Vec3::new(side, height, front), 0.0)
    }
}

/// Where the command station's inbox tray is (sub-agents leave documents there).
pub fn inbox() -> Vec3 {
    Spot::Command.center() + Vec3::new(0.6, 1.2, -0.86)
}

/// The teleporter (`tools/blender/teleporter.py`): every agent arrives and leaves
/// here (V2.md phase 3). Front centre, between the front desks.
pub fn teleporter_spot() -> Vec3 {
    Vec3::new(0.0, 0.0, 4.1)
}
/// Height of its deck, where a robot stands to beam in or out.
pub const TELEPORTER_DECK: f32 = 0.22;

/// The sky's parts that change with the time of day.
#[derive(Resource)]
pub struct Sky {
    stars: Handle<StandardMaterial>,
    sun: Entity,
    sun_glow: Handle<StandardMaterial>,
}

/// The time of day outside the window, from the local clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Daylight {
    Night,
    Dawn,
    Day,
    Dusk,
}

impl Daylight {
    pub fn at(hour: u32) -> Self {
        match hour {
            6..8 => Daylight::Dawn,
            8..18 => Daylight::Day,
            18..20 => Daylight::Dusk,
            _ => Daylight::Night,
        }
    }
}

/// The local hour, from `date` (std has no time zones; this runs every ten
/// minutes at most).
fn local_hour() -> Option<u32> {
    // `AGENT_ROOM_HOUR=12` shows that hour's light (screenshot tests).
    if let Some(hour) = std::env::var("AGENT_ROOM_HOUR").ok().and_then(|hour| hour.parse().ok()) {
        return Some(hour);
    }
    let out = std::process::Command::new("date").arg("+%H").output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Day and night outside the window, following the local time: a sun by day, low
/// and orange at dawn and dusk; dimmer stars, a lighter sky and warmer light. Checked
/// every ten minutes while the app draws; the scene changes only with the phase.
#[allow(clippy::too_many_arguments)]
pub fn daylight(
    time: Res<Time>,
    sky: Option<Res<Sky>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera: Query<&mut Camera, With<crate::view3d::RoomCamera3d>>,
    mut parts: Query<(&mut Transform, &mut Visibility)>,
    mut checked: Local<Option<f32>>,
    mut shown: Local<Option<Daylight>>,
) {
    let Some(sky) = sky else { return };
    let now = time.elapsed_secs();
    if checked.is_some_and(|at| now - at < 600.0) {
        return;
    }
    *checked = Some(now);
    let Some(phase) = local_hour().map(Daylight::at) else { return };
    if *shown == Some(phase) {
        return;
    }
    *shown = Some(phase);
    let (clear, light, brightness, stars, sun) = match phase {
        Daylight::Night => (Color::srgb(0.02, 0.02, 0.06), Color::srgb(0.55, 0.5, 1.0), 350.0, 1.0, None),
        Daylight::Day => {
            (Color::srgb(0.05, 0.09, 0.2), Color::srgb(0.8, 0.85, 1.0), 600.0, 0.3, Some((Vec3::new(-8.0, -4.0, -26.0), LinearRgba::rgb(12.0, 10.0, 7.0))))
        }
        Daylight::Dawn | Daylight::Dusk => {
            (Color::srgb(0.1, 0.05, 0.1), Color::srgb(1.0, 0.7, 0.6), 450.0, 0.6, Some((Vec3::new(-10.0, -8.0, -26.0), LinearRgba::rgb(12.0, 4.0, 1.0))))
        }
    };
    if let Ok(mut camera) = camera.single_mut() {
        camera.clear_color = ClearColorConfig::Custom(clear);
    }
    ambient.color = light;
    ambient.brightness = brightness;
    if let Some(mut material) = materials.get_mut(&sky.stars) {
        material.emissive = LinearRgba::rgb(3.0, 3.2, 4.0) * stars;
    }
    if let Ok((mut at, mut visibility)) = parts.get_mut(sky.sun) {
        *visibility = if sun.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if let Some((spot, glow)) = sun {
            at.translation = spot;
            if let Some(mut material) = materials.get_mut(&sky.sun_glow) {
                material.emissive = glow;
            }
        }
    }
}

/// The root of the command station's model. A click opens its console only when it
/// lands on the model itself, not on the docking ring in the floor around it (M19).
#[derive(Component, Clone, Copy)]
pub struct StationBody;

/// What building the world needs.
pub struct Kit<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub assets: &'a AssetServer,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
}

impl Kit<'_, '_, '_> {
    fn glow(&mut self, color: LinearRgba) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial { base_color: Color::BLACK, emissive: color, ..default() })
    }

    fn mesh(&mut self, mesh: impl Into<Mesh>, material: Handle<StandardMaterial>, at: Transform) -> Entity {
        let mesh = self.meshes.add(mesh.into());
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), at)).id()
    }

    fn model(&mut self, file: &'static str, at: Vec3) -> Entity {
        self.commands
            .spawn((WorldAssetRoot(self.assets.load(GltfAssetLabel::Scene(0).from_asset(file))), Transform::from_translation(at)))
            .id()
    }

    /// A glowing route on the floor from `a` to `b`, with chevrons pointing to `b`.
    fn path(&mut self, a: Vec3, b: Vec3, color: LinearRgba) {
        let flat = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
        let length = flat.length();
        let turn = flat.x.atan2(flat.z);
        let line = self.glow(color * 0.5);
        let at = Transform::from_translation((a + b) / 2.0 + Vec3::Y * 0.012).with_rotation(Quat::from_rotation_y(turn));
        self.mesh(Cuboid::new(0.05, 0.008, length), line, at);
        let chevron = self.glow(color);
        let steps = (length / 1.2) as i32;
        for i in 1..steps {
            let p = a + flat * (i as f32 / steps as f32) + Vec3::Y * 0.014;
            for side in [-1.0, 1.0] {
                let arm = Transform::from_translation(p + Quat::from_rotation_y(turn) * Vec3::new(side * 0.08, 0.0, -0.06))
                    .with_rotation(Quat::from_rotation_y(turn + side * 0.75));
                self.mesh(Cuboid::new(0.04, 0.008, 0.22), chevron.clone(), arm);
            }
        }
    }
}

/// Builds the bridge once: the shell with its panoramic window, the main desk as the
/// command station, holo-screen writing desks as crew desks, the teleporter, and
/// glowing routes from the command station to every desk.
pub fn build(kit: &mut Kit) {
    kit.model("models/environment/floor.glb", Vec3::ZERO);
    kit.model("models/environment/shell.glb", Vec3::ZERO);
    let command = Spot::Command.center();
    let station = kit.model("models/stations/main_desk.glb", command);
    kit.commands.entity(station).insert(StationBody);
    for index in 0..DESKS.len() {
        let at = Spot::Desk(index as u8).center();
        kit.model("models/stations/writing_desk.glb", at);
        let dir = (at - command).normalize();
        kit.path(command + dir * 2.6, at - dir * 1.4, desk_glow(index));
    }
    kit.model("models/stations/teleporter.glb", teleporter_spot());
    let dir = (teleporter_spot() - command).normalize();
    kit.path(command + dir * 2.6, teleporter_spot() - dir * 1.6, LinearRgba::rgb(0.4, 1.6, 3.2));
    sky(kit);
}

/// Each desk's route colour, so the floor isn't one flat blue.
fn desk_glow(index: usize) -> LinearRgba {
    [
        LinearRgba::rgb(0.4, 2.6, 1.0),
        LinearRgba::rgb(0.3, 2.2, 2.6),
        LinearRgba::rgb(2.4, 1.2, 0.2),
        LinearRgba::rgb(1.6, 0.5, 3.0),
    ][index % 4]
}

/// A star outside: it streams past while the ship flies (`cruise`). `along` goes from
/// 1 (far away) to 0 (just outside the window); `across` and `up` place it in the
/// window (see [`star_spot`]).
#[derive(Component)]
pub struct Star {
    along: f32,
    across: f32,
    up: f32,
}

/// How fast the stars stream past (metres per second), ramping up and down.
#[derive(Resource, Default)]
pub struct Cruise {
    speed: f32,
}

/// Top speed while agents work, and how quickly the ship gets there or stops.
const CRUISE_SPEED: f32 = 9.0;
const CRUISE_RAMP: f32 = 6.0;
/// Stars fly from `FAR` to `NEAR` (z), then start over far away. 400 tiny spheres
/// share one mesh and material, and move only while the ship flies.
const STARS: usize = 400;
const FAR: f32 = -70.0;
const NEAR: f32 = -15.0;

/// The ship flies while any session works and stops when all are idle (PLAN.md M27):
/// stars stream towards the window, stretched with speed. Still stars cost nothing,
/// so the view can doze as before.
pub fn cruise(
    time: Res<Time>,
    model: Res<crate::model::Model>,
    mut cruise: ResMut<Cruise>,
    mut view: ResMut<crate::view3d::View3d>,
    mut stars: Query<(&mut Star, &mut Transform)>,
) {
    let working = model.live().iter().any(|session| session.is_busy());
    let goal = if working { CRUISE_SPEED } else { 0.0 };
    let dt = time.delta_secs().min(0.1);
    let before = cruise.speed;
    cruise.speed += (goal - cruise.speed).clamp(-CRUISE_RAMP * dt, CRUISE_RAMP * dt);
    if cruise.speed == 0.0 && before == 0.0 {
        return;
    }
    if cruise.speed != goal {
        view.moving = true;
    }
    // Short streaks along the way each star moves: long ones read as rain.
    let stretch = 1.0 + cruise.speed * 0.5;
    for (mut star, mut at) in &mut stars {
        star.along -= cruise.speed * dt / (NEAR - FAR);
        // Flown past: start over far away.
        if star.along < 0.0 {
            star.along += 1.0;
        }
        let next = star_spot(star.along, star.across, star.up);
        let heading = next - at.translation;
        at.translation = next;
        if heading.length_squared() > 1e-8 && heading.length() < 5.0 {
            at.look_to(heading, Vec3::Y);
        }
        at.scale.z = at.scale.x * stretch;
    }
}

/// A star's place: `along` 1 is far away, 0 just outside the window; `across` and `up`
/// (0..1) are where on the panoramic window (glass y 0.5..3.95 at z -6.95, x
/// -7.5..7.5) the camera sees it. Far stars gather near the window's centre and
/// spread out as they come closer, so flying forward looks like a warp.
fn star_spot(along: f32, across: f32, up: f32) -> Vec3 {
    const EYE: Vec3 = Vec3::new(0.0, 11.0, 13.5);
    const GLASS_Z: f32 = -6.95;
    let z = NEAR + (FAR - NEAR) * along;
    let reach = (EYE.z - z) / (EYE.z - GLASS_Z);
    let spread = 0.35 + 1.4 * (1.0 - along);
    let glass_y = 2.2 + (up - 0.5) * 4.0 * spread;
    let glass_x = (across - 0.5) * 16.0 * spread;
    Vec3::new(glass_x * reach, EYE.y + (glass_y - EYE.y) * reach, z)
}

/// Stars and a planet outside the panoramic window.
fn sky(kit: &mut Kit) {
    let star = kit.meshes.add(Sphere::new(0.04));
    let bright = kit.glow(LinearRgba::rgb(3.0, 3.2, 4.0));
    let mut seed = 0x2545_F491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    for _ in 0..STARS {
        let (along, across, up) = (next(), next(), next());
        let at = star_spot(along, across, up);
        let size = 0.6 + next() * 1.8;
        kit.commands.spawn((
            Star { along, across, up },
            Mesh3d(star.clone()),
            MeshMaterial3d(bright.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(size)),
        ));
    }
    let planet = kit.materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.35, 0.9),
        emissive: LinearRgba::rgb(0.2, 0.3, 1.0),
        perceptual_roughness: 0.9,
        ..default()
    });
    kit.mesh(Sphere::new(5.0), planet, Transform::from_xyz(12.0, 6.5, -26.0));
    // The sun, shown by day (`daylight`); placed where the high camera sees it
    // through the window.
    let sun_glow = kit.glow(LinearRgba::rgb(12.0, 10.0, 7.0));
    let sun = kit.mesh(Sphere::new(1.6), sun_glow.clone(), Transform::from_xyz(-8.0, -4.0, -26.0));
    kit.commands.entity(sun).insert(Visibility::Hidden);
    kit.commands.insert_resource(Sky { stars: bright, sun, sun_glow });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hour_picks_the_light() {
        assert_eq!(Daylight::at(3), Daylight::Night);
        assert_eq!(Daylight::at(7), Daylight::Dawn);
        assert_eq!(Daylight::at(12), Daylight::Day);
        assert_eq!(Daylight::at(19), Daylight::Dusk);
        assert_eq!(Daylight::at(23), Daylight::Night);
    }
}
