//! The 3D workspace (PLAN.md M17): one shared space-station HQ with five holographic
//! workstations the robots walk between, so movement shows what they are doing.
//!
//! Three layers, each its own models (Blender scripts in `tools/blender/`):
//! environment (`environment.py`: floor with docking rings, the station shell with
//! its panoramic window), furniture (`stations.py`: the workstations), agents (robots,
//! activity holograms and light trails, in `view3d`). This module builds the first
//! two plus the glowing paths between stations and the sky outside the window.

use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;

/// Where things happen (metres; x right, z towards the camera). The floor model has
/// a docking ring under each of these spots.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Location {
    /// Centre platform: the main agent thinks, plans and coordinates.
    MainDesk,
    /// Read files (Read, Grep, Glob).
    Bookshelf,
    /// Search and fetch the web: a knowledge-graph console (`tools/blender/research.py`).
    Research,
    /// Run scripts and programs.
    Lab,
    /// Create or edit files.
    WritingDesk,
}

impl Location {
    pub const ALL: [Location; 5] = [Self::MainDesk, Self::Bookshelf, Self::Research, Self::Lab, Self::WritingDesk];

    pub fn center(self) -> Vec3 {
        match self {
            Self::MainDesk => Vec3::new(0.0, 0.0, 0.0),
            Self::Bookshelf => Vec3::new(-4.2, 0.0, -3.4),
            Self::Research => Vec3::new(-5.8, 0.0, 1.2),
            Self::Lab => Vec3::new(4.4, 0.0, -3.4),
            Self::WritingDesk => Vec3::new(5.8, 0.0, 1.2),
        }
    }

    /// The top of the platform or floor ring built into the station's model
    /// (`tools/blender/stations.py`: the main desk's three tiers rise to 0.35 m, the
    /// other stations' `floor_disc`s to 0.05 m), plus a little margin.
    pub fn platform_top(self) -> f32 {
        match self {
            Self::MainDesk => 0.38,
            _ => 0.08,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::MainDesk => "Main desk",
            Self::Bookshelf => "Bookshelf",
            Self::Research => "Research",
            Self::Lab => "Processing lab",
            Self::WritingDesk => "Writing desk",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::MainDesk => "Plan & coordinate",
            Self::Bookshelf => "Read files",
            Self::Research => "Search the web",
            Self::Lab => "Run scripts",
            Self::WritingDesk => "Write files",
        }
    }

    /// The location's signature colour (paths, lights, sign).
    pub fn glow(self) -> LinearRgba {
        match self {
            Self::MainDesk => LinearRgba::rgb(0.2, 0.8, 3.0),
            Self::Bookshelf => LinearRgba::rgb(0.4, 2.6, 1.0),
            Self::Research => LinearRgba::rgb(2.4, 1.2, 0.2),
            Self::Lab => LinearRgba::rgb(0.3, 2.2, 2.6),
            Self::WritingDesk => LinearRgba::rgb(1.6, 0.5, 3.0),
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::MainDesk => "models/stations/main_desk.glb",
            Self::Bookshelf => "models/stations/bookshelf.glb",
            Self::Research => "models/stations/research.glb",
            Self::Lab => "models/stations/lab.glb",
            Self::WritingDesk => "models/stations/writing_desk.glb",
        }
    }

    /// Where a robot stands to work here and the way it faces (radians about y).
    /// Robots face the camera with the holograms floating behind them, so you see
    /// both the robot and what it works on (as in the user's reference image).
    pub fn work_spot(self, slot: usize) -> (Vec3, f32) {
        let side = [0.0, -0.75, 0.75, -1.5, 1.5][slot % 5];
        let (front, height) = match self {
            Self::MainDesk => (0.0, 0.35),
            Self::Bookshelf => (1.3, 0.033),
            Self::Research => (0.85, 0.043),
            Self::Lab => (1.0, 0.053),
            Self::WritingDesk => (0.85, 0.043),
        };
        (self.center() + Vec3::new(side, height, front), 0.0)
    }
}

/// Where the main desk's inbox tray is (sub-agents leave documents there).
pub const INBOX: Vec3 = Vec3::new(0.6, 1.2, -0.86);

/// The teleporter (`tools/blender/teleporter.py`): every agent arrives and leaves
/// here (V2.md phase 3). Front left, between the reading desk and the main platform.
pub const TELEPORTER: Vec3 = Vec3::new(-3.4, 0.0, 4.0);
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

/// The root of a workstation's model. A click opens its console only when it lands on
/// the model itself, not on the docking ring in the floor around it (PLAN.md M19).
#[derive(Component, Clone, Copy)]
pub struct StationBody(pub Location);

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

/// Builds the environment and the workstations once.
pub fn build(kit: &mut Kit) {
    kit.model("models/environment/floor.glb", Vec3::ZERO);
    kit.model("models/environment/shell.glb", Vec3::ZERO);
    for location in Location::ALL {
        let station = kit.model(location.model(), location.center());
        kit.commands.entity(station).insert(StationBody(location));
    }
    kit.model("models/stations/teleporter.glb", TELEPORTER);
    // Routes from the main desk to every workstation.
    for location in Location::ALL.into_iter().skip(1) {
        let from = Location::MainDesk.center();
        let to = location.center();
        let dir = (to - from).normalize();
        kit.path(from + dir * 2.6, to - dir * 1.4, location.glow());
    }
    // And to the teleporter.
    let dir = (TELEPORTER - Location::MainDesk.center()).normalize();
    kit.path(dir * 2.6, TELEPORTER - dir * 1.6, LinearRgba::rgb(0.4, 1.6, 3.2));
    sky(kit);
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
    for _ in 0..260 {
        let at = Vec3::new((next() - 0.5) * 70.0, 0.5 + next() * 24.0, -16.0 - next() * 12.0);
        let size = 0.6 + next() * 1.8;
        kit.commands.spawn((
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
