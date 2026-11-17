//! The 3D workspace (PLAN.md M17): one shared, neon-lit space HQ with five locations
//! the agents walk between, so movement shows what they are doing.
//!
//! Built from simple meshes with emissive materials (the glow comes from bloom on the
//! 3D camera), plus CC0 models: Kenney Space Station Kit walls and consoles (darkened
//! to fit the night look) and KayKit Furniture shelves, books and plants.

use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};

/// Where things happen (metres; x right, z towards the camera).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Location {
    /// Centre platform: the main agent thinks, plans and coordinates.
    MainDesk,
    /// Research: web pages and references.
    Bookshelf,
    /// Run scripts and programs.
    Lab,
    /// Create or edit files.
    WritingDesk,
    /// Read files (Read, Grep, Glob).
    FileReading,
}

impl Location {
    pub const ALL: [Location; 5] = [Self::MainDesk, Self::Bookshelf, Self::FileReading, Self::Lab, Self::WritingDesk];

    pub fn center(self) -> Vec3 {
        match self {
            Self::MainDesk => Vec3::new(0.0, 0.0, 0.0),
            Self::Bookshelf => Vec3::new(-4.2, 0.0, -3.4),
            Self::Lab => Vec3::new(4.4, 0.0, -3.4),
            Self::WritingDesk => Vec3::new(5.8, 0.0, 1.2),
            Self::FileReading => Vec3::new(-5.8, 0.0, 1.2),
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::MainDesk => "Main desk",
            Self::Bookshelf => "Bookshelf",
            Self::Lab => "Processing lab",
            Self::WritingDesk => "Writing desk",
            Self::FileReading => "File reading",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::MainDesk => "Plan & coordinate",
            Self::Bookshelf => "Research & reference",
            Self::Lab => "Run scripts",
            Self::WritingDesk => "Write files",
            Self::FileReading => "Read files",
        }
    }

    /// The location's signature colour (rings, paths, lights).
    pub fn glow(self) -> LinearRgba {
        match self {
            Self::MainDesk => LinearRgba::rgb(0.2, 0.8, 3.0),
            Self::Bookshelf => LinearRgba::rgb(2.4, 1.2, 0.2),
            Self::Lab => LinearRgba::rgb(0.3, 2.2, 2.6),
            Self::WritingDesk => LinearRgba::rgb(1.6, 0.5, 3.0),
            Self::FileReading => LinearRgba::rgb(0.4, 2.6, 1.0),
        }
    }

    /// Where a robot stands to work here, and the way it faces (radians about y).
    pub fn work_spot(self, slot: usize) -> (Vec3, f32) {
        let side = [0.0, -0.9, 0.9, -1.8, 1.8][slot % 5];
        let c = self.center();
        match self {
            Self::MainDesk => (c + Vec3::new(side, 0.25, 0.55), 0.0),
            // Facing the camera, so you see what it does (reading a tablet).
            Self::Bookshelf => (c + Vec3::new(side, 0.0, 0.9), 0.0),
            Self::Lab => (c + Vec3::new(side, 0.0, 1.0), 0.0),
            Self::WritingDesk => (c + Vec3::new(-0.9, 0.0, side * 0.6), FRAC_PI_2),
            Self::FileReading => (c + Vec3::new(side * 0.7, 0.0, 0.9), 0.0),
        }
    }
}

/// Station kit pieces are 1 unit per tile; a tile here is this many metres.
const TILE: f32 = 1.5;
const FLOOR: Vec2 = Vec2::new(18.0, 11.0);
const BACK: f32 = -5.5;

/// Models that should be darkened to the night look once loaded.
#[derive(Component, Clone, Copy)]
struct Darken(f32);

/// Light that breathes slowly (rings, orbs, holo screens).
#[derive(Component)]
pub struct Pulse {
    base: LinearRgba,
    speed: f32,
    phase: f32,
}

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

    fn solid(&mut self, color: Color, metallic: f32, roughness: f32) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial { base_color: color, metallic, perceptual_roughness: roughness, ..default() })
    }

    fn glass(&mut self, tint: LinearRgba) -> Handle<StandardMaterial> {
        self.materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::new(0.5, 0.8, 1.0, 0.18)),
            emissive: tint * 0.25,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.05,
            reflectance: 0.8,
            ..default()
        })
    }

    fn mesh(&mut self, mesh: impl Into<Mesh>, material: Handle<StandardMaterial>, at: Transform) -> Entity {
        let mesh = self.meshes.add(mesh.into());
        self.commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), at)).id()
    }

    fn model(&mut self, file: &'static str, at: Vec3, turn: f32, scale: f32, darken: f32) -> Entity {
        let parent = self
            .commands
            .spawn((
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn)).with_scale(Vec3::splat(scale)),
                Visibility::default(),
            ))
            .id();
        self.commands
            .spawn((
                WorldAssetRoot(self.assets.load(GltfAssetLabel::Scene(0).from_asset(file))),
                Transform::default(),
                ChildOf(parent),
                Darken(darken),
            ))
            .observe(darken_model);
        parent
    }

    /// A glowing ring lying flat (on the floor or a platform).
    fn ring(&mut self, center: Vec3, radius: f32, width: f32, color: LinearRgba, pulse: f32) {
        let material = self.glow(color);
        let ring = self.mesh(
            Torus::new(radius - width, radius),
            material,
            Transform::from_translation(center).with_scale(Vec3::new(1.0, 0.25, 1.0)),
        );
        if pulse > 0.0 {
            self.commands.entity(ring).insert(Pulse { base: color, speed: pulse, phase: center.x + center.z });
        }
    }

    /// A glowing line on the floor from `a` to `b`, with chevrons pointing to `b`.
    fn path(&mut self, a: Vec3, b: Vec3, color: LinearRgba) {
        let flat = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
        let length = flat.length();
        let turn = flat.x.atan2(flat.z);
        let line = self.glow(color * 0.6);
        let at = Transform::from_translation((a + b) / 2.0 + Vec3::Y * 0.012).with_rotation(Quat::from_rotation_y(turn));
        self.mesh(Cuboid::new(0.06, 0.01, length), line, at);
        let chevron = self.glow(color);
        let steps = (length / 1.1) as i32;
        for i in 1..steps {
            let p = a + flat * (i as f32 / steps as f32) + Vec3::Y * 0.015;
            for side in [-1.0, 1.0] {
                let arm = Transform::from_translation(p + Quat::from_rotation_y(turn) * Vec3::new(side * 0.08, 0.0, -0.06))
                    .with_rotation(Quat::from_rotation_y(turn + side * 0.75));
                self.mesh(Cuboid::new(0.04, 0.01, 0.22), chevron.clone(), arm);
            }
        }
    }

    /// A floating holo screen.
    fn holo(&mut self, at: Vec3, turn: f32, size: Vec2, color: LinearRgba) {
        let material = self.materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::new(0.2, 0.6, 1.0, 0.35)),
            emissive: color * 0.8,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        let screen = self.mesh(
            Rectangle::new(size.x, size.y),
            material,
            Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn) * Quat::from_rotation_x(-0.15)),
        );
        self.commands.entity(screen).insert(Pulse { base: color * 0.8, speed: 1.3, phase: at.x });
    }
}

/// Builds the whole workspace once.
pub fn build(kit: &mut Kit) {
    floor(kit);
    walls(kit);
    sky(kit);
    main_desk(kit);
    bookshelf(kit);
    lab(kit);
    writing_desk(kit);
    file_reading(kit);
    // Glowing paths from the main desk to every location.
    for location in Location::ALL.into_iter().skip(1) {
        let from = Location::MainDesk.center();
        let to = location.center();
        let dir = (to - from).normalize();
        kit.path(from + dir * 2.1, to - dir * 1.4, location.glow());
    }
    for (x, z, turn) in [(-8.2, -4.6, 0.3), (8.2, -4.6, 1.2), (-8.3, 4.6, 2.0), (8.3, 4.6, 0.8), (-2.6, -4.7, 0.0)] {
        kit.model("models/furniture/cactus_medium_A.gltf", Vec3::new(x, 0.0, z), turn, 1.4, 0.8);
    }
    for location in Location::ALL {
        let light = PointLight {
            color: Color::LinearRgba(location.glow()),
            intensity: 90_000.0,
            range: 6.0,
            shadow_maps_enabled: false,
            ..default()
        };
        kit.commands.spawn((light, Transform::from_translation(location.center() + Vec3::Y * 2.2)));
    }
}

fn floor(kit: &mut Kit) {
    let deck = kit.solid(Color::srgb(0.10, 0.10, 0.17), 0.7, 0.32);
    kit.mesh(Cuboid::new(FLOOR.x, 0.2, FLOOR.y), deck, Transform::from_xyz(0.0, -0.1, 0.0));
    // Panel seams: a faint glowing grid.
    let seam = kit.glow(LinearRgba::rgb(0.10, 0.08, 0.35));
    let cols = (FLOOR.x / TILE) as i32;
    let rows = (FLOOR.y / TILE) as i32;
    for i in 0..=cols {
        let x = -FLOOR.x / 2.0 + i as f32 * TILE;
        kit.mesh(Cuboid::new(0.025, 0.005, FLOOR.y), seam.clone(), Transform::from_xyz(x, 0.003, 0.0));
    }
    for j in 0..=rows {
        let z = -FLOOR.y / 2.0 + j as f32 * TILE;
        kit.mesh(Cuboid::new(FLOOR.x, 0.005, 0.025), seam.clone(), Transform::from_xyz(0.0, 0.003, z));
    }
    // A glowing edge around the deck.
    let edge = kit.glow(LinearRgba::rgb(0.3, 0.6, 2.5));
    kit.mesh(Cuboid::new(FLOOR.x, 0.05, 0.05), edge.clone(), Transform::from_xyz(0.0, 0.0, FLOOR.y / 2.0));
    for side in [-1.0, 1.0] {
        kit.mesh(Cuboid::new(0.05, 0.05, FLOOR.y), edge.clone(), Transform::from_xyz(side * FLOOR.x / 2.0, 0.0, 0.0));
    }
}

fn walls(kit: &mut Kit) {
    // The back wall: station kit pieces, darkened, with windows onto space.
    let cols = (FLOOR.x / TILE) as i32;
    for i in 0..cols {
        let x = -FLOOR.x / 2.0 + TILE / 2.0 + i as f32 * TILE;
        let file = match i % 4 {
            1 => "models/station/wall-window.glb",
            2 => "models/station/wall-window-banner.glb",
            _ => "models/station/wall-pillar.glb",
        };
        kit.model(file, Vec3::new(x, 0.0, BACK), 0.0, TILE * 1.4, 0.45);
    }
    let strip = kit.glow(LinearRgba::rgb(0.6, 0.3, 3.0));
    kit.mesh(Cuboid::new(FLOOR.x, 0.06, 0.06), strip.clone(), Transform::from_xyz(0.0, 2.05, BACK + 0.3));
    kit.mesh(Cuboid::new(FLOOR.x, 0.04, 0.04), strip, Transform::from_xyz(0.0, 0.25, BACK + 0.3));
}

/// Stars and a planet behind the windows.
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
    for _ in 0..220 {
        let at = Vec3::new((next() - 0.5) * 60.0, 1.0 + next() * 22.0, -14.0 - next() * 10.0);
        let size = 0.6 + next() * 1.6;
        kit.commands.spawn((
            Mesh3d(star.clone()),
            MeshMaterial3d(bright.clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(size)),
        ));
    }
    let planet = kit.materials.add(StandardMaterial {
        base_color: Color::srgb(0.25, 0.35, 0.9),
        emissive: LinearRgba::rgb(0.15, 0.25, 0.9),
        perceptual_roughness: 0.9,
        ..default()
    });
    kit.mesh(Sphere::new(4.5), planet, Transform::from_xyz(11.0, 8.0, -22.0));
}

fn main_desk(kit: &mut Kit) {
    let c = Location::MainDesk.center();
    let metal = kit.solid(Color::srgb(0.22, 0.23, 0.32), 0.8, 0.3);
    let dark = kit.solid(Color::srgb(0.07, 0.07, 0.12), 0.6, 0.4);
    kit.mesh(Cylinder::new(2.0, 0.14), dark, Transform::from_translation(c + Vec3::Y * 0.07));
    kit.mesh(Cylinder::new(1.55, 0.25), metal, Transform::from_translation(c + Vec3::Y * 0.125));
    kit.ring(c + Vec3::Y * 0.145, 2.0, 0.05, Location::MainDesk.glow(), 0.8);
    kit.ring(c + Vec3::Y * 0.255, 1.55, 0.04, LinearRgba::rgb(3.0, 1.4, 0.2), 0.0);
    kit.ring(c + Vec3::Y * 0.26, 1.0, 0.025, Location::MainDesk.glow(), 1.6);
    kit.model("models/station/computer-wide.glb", c + Vec3::new(0.0, 0.25, -0.35), 0.0, 1.5, 0.6);
    for (x, turn) in [(-1.0, 0.5), (1.0, -0.5)] {
        kit.holo(c + Vec3::new(x, 1.25, 0.1), turn, Vec2::new(0.55, 0.38), LinearRgba::rgb(0.3, 1.2, 3.0));
    }
}

fn bookshelf(kit: &mut Kit) {
    let c = Location::Bookshelf.center();
    // Two tall shelves full of books, with glowing edges.
    let frame = kit.solid(Color::srgb(0.12, 0.12, 0.2), 0.7, 0.35);
    let edge = kit.glow(Location::Bookshelf.glow() * 0.8);
    let covers: Vec<Handle<StandardMaterial>> = [
        Color::srgb(0.85, 0.35, 0.25),
        Color::srgb(0.25, 0.55, 0.9),
        Color::srgb(0.95, 0.75, 0.3),
        Color::srgb(0.4, 0.8, 0.55),
        Color::srgb(0.7, 0.45, 0.9),
        Color::srgb(0.9, 0.9, 0.95),
    ]
    .into_iter()
    .map(|color| kit.solid(color, 0.0, 0.6))
    .collect();
    let mut seed = 7u32;
    for shelf_x in [-0.95, 0.95] {
        let at = c + Vec3::new(shelf_x, 0.0, -0.75);
        let (w, h, d) = (1.7, 2.3, 0.45);
        kit.mesh(Cuboid::new(w, 0.06, d), frame.clone(), Transform::from_translation(at + Vec3::Y * h));
        kit.mesh(Cuboid::new(w, h, 0.04), frame.clone(), Transform::from_translation(at + Vec3::new(0.0, h / 2.0, -d / 2.0)));
        for side in [-1.0, 1.0] {
            kit.mesh(Cuboid::new(0.06, h, d), frame.clone(), Transform::from_translation(at + Vec3::new(side * w / 2.0, h / 2.0, 0.0)));
            kit.mesh(Cuboid::new(0.03, h, 0.03), edge.clone(), Transform::from_translation(at + Vec3::new(side * w / 2.0, h / 2.0, d / 2.0)));
        }
        for row in 0..4 {
            let y = 0.12 + row as f32 * 0.55;
            kit.mesh(Cuboid::new(w, 0.04, d), frame.clone(), Transform::from_translation(at + Vec3::Y * y));
            kit.mesh(Cuboid::new(w, 0.02, 0.02), edge.clone(), Transform::from_translation(at + Vec3::new(0.0, y + 0.02, d / 2.0)));
            let mut x = -w / 2.0 + 0.1;
            while x < w / 2.0 - 0.12 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let thick = 0.05 + (seed >> 16) as f32 % 4.0 * 0.012;
                let tall = 0.3 + (seed >> 20) as f32 % 5.0 * 0.03;
                let cover = covers[(seed >> 8) as usize % covers.len()].clone();
                kit.mesh(Cuboid::new(thick, tall, 0.3), cover, Transform::from_translation(at + Vec3::new(x + thick / 2.0, y + 0.02 + tall / 2.0, 0.02)));
                x += thick + 0.008;
            }
        }
    }
    kit.ring(c + Vec3::new(0.0, 0.01, 0.9), 0.9, 0.04, Location::Bookshelf.glow(), 0.7);
}

fn lab(kit: &mut Kit) {
    let c = Location::Lab.center();
    let base = kit.solid(Color::srgb(0.18, 0.19, 0.27), 0.8, 0.35);
    for dx in [-1.3, 0.0, 1.3] {
        let at = c + Vec3::new(dx, 0.0, -0.5);
        kit.mesh(Cylinder::new(0.42, 0.2), base.clone(), Transform::from_translation(at + Vec3::Y * 0.1));
        let glass = kit.glass(Location::Lab.glow());
        kit.mesh(Cylinder::new(0.36, 1.3), glass, Transform::from_translation(at + Vec3::Y * 0.85));
        let core = kit.glow(Location::Lab.glow() * 1.5);
        let orb = kit.mesh(Sphere::new(0.14), core, Transform::from_translation(at + Vec3::Y * 0.85));
        kit.commands.entity(orb).insert(Pulse { base: Location::Lab.glow() * 1.5, speed: 2.0, phase: dx });
        kit.ring(at + Vec3::Y * 0.21, 0.42, 0.03, Location::Lab.glow(), 0.0);
        kit.mesh(Cylinder::new(0.4, 0.06), base.clone(), Transform::from_translation(at + Vec3::Y * 1.53));
    }
    kit.model("models/station/computer-system.glb", c + Vec3::new(2.4, 0.0, -0.2), -0.6, 1.4, 0.6);
    kit.ring(c + Vec3::new(0.0, 0.01, 1.0), 0.9, 0.04, Location::Lab.glow(), 0.9);
}

fn writing_desk(kit: &mut Kit) {
    let c = Location::WritingDesk.center();
    kit.model("models/station/table-large.glb", c + Vec3::new(0.2, 0.0, 0.0), FRAC_PI_2, 1.4, 0.6);
    kit.model("models/station/computer-screen.glb", c + Vec3::new(0.35, 0.56, -0.3), -FRAC_PI_2, 1.1, 0.6);
    kit.model("models/station/computer-screen.glb", c + Vec3::new(0.35, 0.56, 0.35), -FRAC_PI_2, 1.1, 0.6);
    kit.holo(c + Vec3::new(0.1, 1.5, 0.0), -FRAC_PI_2, Vec2::new(0.9, 0.5), Location::WritingDesk.glow());
    kit.ring(c + Vec3::new(-0.9, 0.01, 0.0), 0.9, 0.04, Location::WritingDesk.glow(), 0.8);
}


/// Night look for loaded models: darker, slightly blue.
fn darken_model(
    ready: On<WorldInstanceReady>,
    darken: Query<&Darken>,
    children: Query<&Children>,
    meshes: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let Ok(&Darken(factor)) = darken.get(ready.entity) else { return };
    for child in children.iter_descendants(ready.entity) {
        let Ok(material) = meshes.get(child) else { continue };
        let Some(original) = materials.get(&material.0).cloned() else { continue };
        let base = original.base_color.to_linear();
        let dimmed = StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::new(
                base.red * factor * 0.85,
                base.green * factor * 0.85,
                base.blue * factor,
                base.alpha,
            )),
            ..original
        };
        commands.entity(child).insert(MeshMaterial3d(materials.add(dimmed)));
    }
}

/// Rings, orbs and holo screens breathe slowly (only while the 3D view is on).
pub fn pulse(
    view: Res<crate::view3d::View3d>,
    time: Res<Time>,
    glows: Query<(&Pulse, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !view.on {
        return;
    }
    let t = time.elapsed_secs();
    for (pulse, material) in &glows {
        if let Some(mut material) = materials.get_mut(&material.0) {
            let k = 0.75 + 0.25 * (t * pulse.speed + pulse.phase).sin();
            material.emissive = pulse.base * k;
        }
    }
}

/// A reading workstation: a desk with document holo screens (until the Blender
/// station models replace these placeholders).
fn file_reading(kit: &mut Kit) {
    let c = Location::FileReading.center();
    kit.model("models/station/table-large.glb", c + Vec3::new(0.0, 0.0, -0.2), 0.0, 1.4, 0.6);
    for (x, turn) in [(-0.45, 0.25), (0.45, -0.25)] {
        kit.holo(c + Vec3::new(x, 1.35, -0.1), turn, Vec2::new(0.5, 0.65), Location::FileReading.glow());
    }
    kit.ring(c + Vec3::new(0.0, 0.01, 0.9), 0.9, 0.04, Location::FileReading.glow(), 0.8);
}
