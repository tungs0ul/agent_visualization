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
    /// Research: web pages and references.
    Bookshelf,
    /// Read files (Read, Grep, Glob).
    FileReading,
    /// Run scripts and programs.
    Lab,
    /// Create or edit files.
    WritingDesk,
}

impl Location {
    pub const ALL: [Location; 5] = [Self::MainDesk, Self::Bookshelf, Self::FileReading, Self::Lab, Self::WritingDesk];

    pub fn center(self) -> Vec3 {
        match self {
            Self::MainDesk => Vec3::new(0.0, 0.0, 0.0),
            Self::Bookshelf => Vec3::new(-4.2, 0.0, -3.4),
            Self::FileReading => Vec3::new(-5.8, 0.0, 1.2),
            Self::Lab => Vec3::new(4.4, 0.0, -3.4),
            Self::WritingDesk => Vec3::new(5.8, 0.0, 1.2),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::MainDesk => "Main desk",
            Self::Bookshelf => "Bookshelf",
            Self::FileReading => "File reading",
            Self::Lab => "Processing lab",
            Self::WritingDesk => "Writing desk",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::MainDesk => "Plan & coordinate",
            Self::Bookshelf => "Research & reference",
            Self::FileReading => "Read files",
            Self::Lab => "Run scripts",
            Self::WritingDesk => "Write files",
        }
    }

    /// The location's signature colour (paths, lights, sign).
    pub fn glow(self) -> LinearRgba {
        match self {
            Self::MainDesk => LinearRgba::rgb(0.2, 0.8, 3.0),
            Self::Bookshelf => LinearRgba::rgb(2.4, 1.2, 0.2),
            Self::FileReading => LinearRgba::rgb(0.4, 2.6, 1.0),
            Self::Lab => LinearRgba::rgb(0.3, 2.2, 2.6),
            Self::WritingDesk => LinearRgba::rgb(1.6, 0.5, 3.0),
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::MainDesk => "models/stations/main_desk.glb",
            Self::Bookshelf => "models/stations/bookshelf.glb",
            Self::FileReading => "models/stations/file_reading.glb",
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
            Self::FileReading => (0.85, 0.043),
            Self::Lab => (1.0, 0.053),
            Self::WritingDesk => (0.85, 0.043),
        };
        (self.center() + Vec3::new(side, height, front), 0.0)
    }
}

/// Where the main desk's inbox tray is (sub-agents leave documents there).
pub const INBOX: Vec3 = Vec3::new(0.6, 1.2, -0.86);

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
        kit.model(location.model(), location.center());
    }
    // Routes from the main desk to every workstation.
    for location in Location::ALL.into_iter().skip(1) {
        let from = Location::MainDesk.center();
        let to = location.center();
        let dir = (to - from).normalize();
        kit.path(from + dir * 2.6, to - dir * 1.4, location.glow());
    }
    // Each workstation casts its colour on the polished floor.
    for location in Location::ALL {
        let light = PointLight {
            color: Color::LinearRgba(location.glow()),
            intensity: 70_000.0,
            range: 6.0,
            shadow_maps_enabled: false,
            ..default()
        };
        kit.commands.spawn((light, Transform::from_translation(location.center() + Vec3::new(0.0, 2.4, 0.8))));
    }
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
}
