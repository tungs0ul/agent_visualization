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
    /// At the command core robots face the camera. At a crew desk the robot stands
    /// by the desk's end, half turned to its screens, so it no longer hides the desk
    /// and you see its face and its screens together (the user, 2026-11-19).
    pub fn work_spot(self, slot: usize) -> (Vec3, f32) {
        match self {
            Spot::Command => {
                let side = [0.0, -0.75, 0.75, -1.5, 1.5][slot % 5];
                (self.center() + Vec3::new(side, 0.35, 0.0), 0.0)
            }
            Spot::Desk(_) => {
                let side = if slot % 2 == 0 { 1.0 } else { -1.0 };
                let along = 0.45 + 0.35 * (slot / 2) as f32;
                (self.center() + Vec3::new(side * 1.3, 0.043, along), -side * 0.6)
            }
        }
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
        Daylight::Night => (Color::srgb(0.01, 0.02, 0.06), Color::srgb(0.48, 0.64, 1.0), 190.0, 1.0, None),
        Daylight::Day => {
            (Color::srgb(0.05, 0.09, 0.2), Color::srgb(0.7, 0.8, 1.0), 300.0, 0.3, Some((Vec3::new(-8.0, -4.0, -26.0), LinearRgba::rgb(12.0, 10.0, 7.0))))
        }
        Daylight::Dawn | Daylight::Dusk => {
            (Color::srgb(0.06, 0.05, 0.12), Color::srgb(0.8, 0.72, 0.85), 240.0, 0.6, Some((Vec3::new(-10.0, -8.0, -26.0), LinearRgba::rgb(12.0, 4.0, 1.0))))
        }
    };
    // Sky colour, ambient light and exposure come from the look settings (`look`).
    let _ = (clear, light, brightness, &mut camera, &mut ambient);
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
    pub fn glow(&mut self, color: LinearRgba) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial { base_color: Color::BLACK, emissive: color, ..default() })
    }

    pub fn mesh(&mut self, mesh: impl Into<Mesh>, material: Handle<StandardMaterial>, at: Transform) -> Entity {
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
    command_core(kit, command);
    // Cinematic light (PLAN.md M29): a dim blue room lit in pools. Cyan from the
    // command core and the teleporter, warm orange accents by the back desks. No
    // shadows, a short range each: cheap.
    for (at, color, lumens, range) in [
        (command + Vec3::new(0.0, 3.8, -0.4), Color::srgb(0.35, 0.8, 1.0), 350_000.0, 9.0),
        (teleporter_spot() + Vec3::Y * 1.2, Color::srgb(0.3, 0.75, 1.0), 300_000.0, 6.0),
    ] {
        kit.commands.spawn((PointLight { color, intensity: lumens, range, ..default() }, Transform::from_translation(at), crate::look::BaseIntensity(lumens)));
    }
    // Practical light (brief 5.3): a spot straight down on the core's dais.
    let core_spot = 1_800_000.0;
    kit.commands.spawn((
        SpotLight { color: Color::srgb_u8(0x9C, 0xD8, 0xFF), intensity: core_spot, range: 12.0, radius: 0.1, inner_angle: 0.25, outer_angle: 0.55, shadow_maps_enabled: true, ..default() },
        Transform::from_translation(command + Vec3::Y * 7.2).looking_at(command, Vec3::Z),
        crate::look::BaseIntensity(core_spot),
    ));
    // No screen-wash point lights at the desks: on the glossy pads they showed as
    // round blobs, not reflections (the user, 2026-11-19). The screens' glow and the
    // interior reflection probe light the desks instead.
    for index in 0..DESKS.len() {
        let at = Spot::Desk(index as u8).center();
        kit.model("models/stations/desk_console.glb", at);
        // The desk's own light over the floor ring, coloured by its robot's state
        // (`view3d::desk_lights`).
        let light = kit.materials.add(StandardMaterial { base_color: Color::BLACK, emissive: DeskState::Free.glow(), ..default() });
        // A thin light tube on the floor just outside the desk's own disc (`stations.py`
        // `floor_disc`: r 1.45, 0.3 m forward): dark while the desk is free. A tube,
        // not a wide halo plane: bloom makes the halo (PROMPT_02 step 1).
        let ring = kit.meshes.add(Torus::new(1.565, 1.59));
        kit.commands.spawn((
            Mesh3d(ring),
            MeshMaterial3d(light),
            Transform::from_translation(at + Vec3::new(0.0, 0.015, 0.3)),
            DeskLight { desk: index as u8, shown: DeskState::Free },
        ));
        let dir = (at - command).normalize();
        kit.path(command + dir * 2.6, at - dir * 1.4, desk_glow(index));
    }
    kit.model("models/stations/teleporter.glb", teleporter_spot());
    let dir = (teleporter_spot() - command).normalize();
    kit.path(command + dir * 2.6, teleporter_spot() - dir * 1.6, LinearRgba::rgb(0.4, 1.6, 3.2));
    sky(kit);
}

/// What a crew desk shows (PLAN.md M28): free, or its robot's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeskState {
    Free,
    Working,
    Waiting,
    Done,
    Failed,
}

impl DeskState {
    /// The halo's light: none when free, the status colours otherwise (`palette`:
    /// cyan, amber, green, red).
    pub fn glow(self) -> LinearRgba {
        match self {
            DeskState::Free => LinearRgba::BLACK,
            DeskState::Working => LinearRgba::rgb(0.3, 4.6, 9.0),
            DeskState::Waiting => LinearRgba::rgb(9.0, 5.0, 0.9),
            DeskState::Done => LinearRgba::rgb(0.9, 8.0, 3.6),
            DeskState::Failed => LinearRgba::rgb(9.0, 1.4, 1.8),
        }
    }
}

/// A crew desk's light ring and the state it shows now.
#[derive(Component)]
pub struct DeskLight {
    pub desk: u8,
    pub shown: DeskState,
}

/// The command core (PLAN.md M29, the user's reference image): a glass hologram
/// cylinder over the command platform with a glowing ring at its foot and top.
/// Additive, so Claude stays visible inside; static, so it costs no frames.
fn command_core(kit: &mut Kit, command: Vec3) {
    const RADIUS: f32 = 1.3;
    const HEIGHT: f32 = 2.4;
    let base = command.y + 0.38;
    let glass = kit.materials.add(StandardMaterial {
        // Unlit: the base colour is what shows, added onto what is behind.
        base_color: Color::LinearRgba(LinearRgba::rgb(0.02, 0.1, 0.24)),
        alpha_mode: AlphaMode::Add,
        double_sided: true,
        cull_mode: None,
        unlit: true,
        ..default()
    });
    let at = Transform::from_translation(command + Vec3::Y * (base + HEIGHT / 2.0));
    kit.mesh(Cylinder::new(RADIUS, HEIGHT).mesh().resolution(48).without_caps().build(), glass, at);
    let ring = kit.glow(LinearRgba::rgb(0.6, 3.0, 5.0));
    for height in [base + 0.02, base + HEIGHT] {
        let at = Transform::from_translation(command + Vec3::Y * height);
        kit.mesh(Torus::new(RADIUS - 0.03, RADIUS + 0.03), ring.clone(), at);
    }
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

/// Stars and a planet outside the panoramic window.
fn sky(kit: &mut Kit) {
    // Still stars (the user: "we dont need to move the star, i just want it to look
    // real"): many tiny points, most faint, a few bright and bluish or warm, spread
    // deep behind the window where the camera sees them through the glass.
    let star = kit.meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap());
    let tiers = [
        (kit.glow(LinearRgba::rgb(0.9, 0.95, 1.1)), 0.012, 0.70),
        (kit.glow(LinearRgba::rgb(2.2, 2.4, 3.0)), 0.018, 0.25),
        (kit.glow(LinearRgba::rgb(6.0, 6.5, 9.0)), 0.026, 0.04),
        (kit.glow(LinearRgba::rgb(7.0, 5.5, 3.5)), 0.026, 0.01),
    ];
    let bright = tiers[1].0.clone();
    let mut seed = 0x2545_F491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 10_000) as f32 / 10_000.0
    };
    for _ in 0..900 {
        let pick = next();
        let mut sum = 0.0;
        let (material, size, _) = tiers
            .iter()
            .find(|(_, _, share)| {
                sum += share;
                pick <= sum
            })
            .unwrap_or(&tiers[0]);
        let (at, reach) = crate::space::through_window(-9.0 + next() * 18.0, 0.2 + next() * 7.4, -40.0 - next() * 140.0);
        kit.commands.spawn((
            Mesh3d(star.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(size * reach * (0.7 + next() * 0.6))),
        ));
    }
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
