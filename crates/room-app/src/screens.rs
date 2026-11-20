//! The crew desks' screens (the user: "it is a star ship and you display code?").
//! The main screen shows a starship hologram (rocket, planet, sun, station, ship, black
//! hole, cargo crate, galaxy: one per desk) rendered in Blender and approved by the user; the side panels
//! show gauges, lists and waves painted here once at start. Each screen is dark
//! see-through glass with the art added over it as light, so the art glows and the
//! glass stays dim. They are frameless holograms as in the target
//! (crops 07, 08); the Blender desk (`stations.py`) has only their emitters.

use bevy::prelude::*;

use crate::world3d::{DESKS, Kit, Spot};

/// Where a desk's screens are, relative to the desk centre (glTF space), their size,
/// yaw and tilt in degrees. Keep in step with `stations.py` `build_desk_console`.
const SCREENS: [(Vec3, Vec2, f32, f32); 3] = [
    // the main screen's lower edge sits on the projector bar (top 0.83 m)
    (Vec3::new(0.0, 1.21, -0.335), Vec2::new(1.5, 0.75), 0.0, -6.0),
    // side panels float a few cm over the lenses on the back arms (top 0.94 m)
    (Vec3::new(-0.94, 1.24, -0.3), Vec2::new(0.36, 0.54), 28.0, -4.0),
    (Vec3::new(0.94, 1.24, -0.3), Vec2::new(0.36, 0.54), -28.0, -4.0),
];

/// The narrow side-panel graphics.
#[derive(Clone, Copy)]
enum Side {
    Gauges,
    List,
    Wave,
    Dial,
}

const SIDES: [Side; 4] = [Side::Gauges, Side::List, Side::Wave, Side::Dial];

// Colours, linear 0..1, painted additively, then written as sRGB.
const CYAN: Vec3 = Vec3::new(0.22, 0.78, 1.0);
const AZURE: Vec3 = Vec3::new(0.18, 0.55, 1.0);
const DIM: Vec3 = Vec3::new(0.08, 0.25, 0.5);
const AMBER: Vec3 = Vec3::new(1.0, 0.6, 0.24);
const VIOLET: Vec3 = Vec3::new(0.54, 0.36, 1.0);
const GREEN: Vec3 = Vec3::new(0.13, 0.88, 0.48);

/// Spawns the screens on every crew desk: each is two quads, a dark see-through glass
/// pane and, just in front of it, the art added on top as light (it glows and blooms
/// while the glass stays dim, as in the target's crop 07).
pub fn build(kit: &mut Kit, images: &mut Assets<Image>) {
    let mains: Vec<Handle<Image>> = SUBJECTS.iter().map(|(_, bytes)| images.add(crate::space::decode_png(bytes))).collect();
    let sides: Vec<Handle<Image>> = SIDES.iter().enumerate().map(|(seed, side)| images.add(paint_side(*side, seed as u32 + 40))).collect();
    let glass_main = images.add(glass_image(512, 256));
    let glass_side = images.add(glass_image(160, 240));
    let unlit = |texture: Handle<Image>, gain: f32, alpha_mode: AlphaMode| StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::new(gain, gain, gain, 1.0)),
        base_color_texture: Some(texture),
        alpha_mode,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let glass = [kit.materials.add(unlit(glass_main, 1.0, AlphaMode::Blend)), kit.materials.add(unlit(glass_side, 1.0, AlphaMode::Blend))];
    let quad = kit.meshes.add(Rectangle::new(1.0, 1.0));
    for index in 0..DESKS.len() {
        let desk = Spot::Desk(index as u8).center();
        let textures = [mains[subject(index as u8)].clone(), sides[index % SIDES.len()].clone(), sides[(index + 2) % SIDES.len()].clone()];
        for (i, ((at, size, yaw, tilt), texture)) in SCREENS.into_iter().zip(textures).enumerate() {
            let rotation = Quat::from_rotation_y(yaw.to_radians()) * Quat::from_rotation_x(tilt.to_radians());
            let front = rotation * Vec3::Z;
            let scale = size.extend(1.0) * 0.97;
            kit.commands.spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(glass[(i > 0) as usize].clone()),
                Transform::from_translation(desk + at + front * 0.004).with_rotation(rotation).with_scale(scale),
            ));
            // The art: well over 1, added on top, so it blooms like a hologram. The
            // main screens are baked darker (exposure -1.7) to keep their colours.
            let gain = if i == 0 { MAIN_GAIN } else { ART_GAIN };
            let mut material = unlit(texture, gain, AlphaMode::Add);
            if i == 0 {
                material.uv_transform = rest_frame_uv();
            }
            kit.commands.spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(kit.materials.add(material)),
                Transform::from_translation(desk + at + front * 0.008).with_rotation(rotation).with_scale(scale),
            ));
        }
    }
}

/// The main screens hold still: the big 3D hologram over a working desk carries the
/// motion (`holo.rs`; the robot's label used to hide the screen). The bake is an atlas
/// of 8 frames, 4 columns by 2 rows (`bake_screens.sh`); the screen shows frame 3.
fn rest_frame_uv() -> bevy::math::Affine2 {
    const COLUMNS: usize = 4;
    const ROWS: usize = 2;
    const REST_FRAME: usize = 3;
    let (col, row) = (REST_FRAME % COLUMNS, REST_FRAME / COLUMNS);
    let size = Vec2::new(1.0 / COLUMNS as f32, 1.0 / ROWS as f32);
    bevy::math::Affine2::from_scale_angle_translation(size, 0.0, Vec2::new(col as f32, row as f32) * size)
}

/// How bright the side-panel art is drawn over its glass.
const ART_GAIN: f32 = 3.0;
/// How bright the baked main screens are drawn (they were rendered 1.7 stops dark).
const MAIN_GAIN: f32 = 3.2;

/// Which subject desk `desk`'s main screen (and its hologram) shows: an index into
/// `SUBJECTS`.
pub fn subject(desk: u8) -> usize {
    desk as usize % SUBJECTS.len()
}

/// The main screens' starship holograms, rendered in Blender
/// (`tools/blender/holo_screen_proto.py`, `bake_screens.sh`): the subject, the UI and
/// the beam as light only, straight on, in an atlas. One per desk, in this order.
/// The name is also the subject's 3D model, `models/holo/<name>.glb`
/// (`export_holo_subjects.sh`), shown big over the desk while it works.
pub const SUBJECTS: [(&str, &[u8]); 8] = [
    ("rocket", include_bytes!("../../../assets/textures/screens/rocket.png")),
    ("planet", include_bytes!("../../../assets/textures/screens/planet.png")),
    ("sun", include_bytes!("../../../assets/textures/screens/sun.png")),
    ("station", include_bytes!("../../../assets/textures/screens/station.png")),
    ("ship", include_bytes!("../../../assets/textures/screens/ship.png")),
    ("blackhole", include_bytes!("../../../assets/textures/screens/blackhole.png")),
    ("crate", include_bytes!("../../../assets/textures/screens/crate.png")),
    ("galaxy", include_bytes!("../../../assets/textures/screens/galaxy.png")),
];

/// The dark glass pane behind the art: deep navy, a touch lighter in the middle, a
/// faint grid, about two thirds opaque so the room shows through it.
fn glass_image(w: usize, h: usize) -> Image {
    crate::space::image(w as u32, h as u32, glass_bytes(w, h))
}

fn glass_bytes(w: usize, h: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let u = Vec2::new(x as f32 / w as f32 - 0.5, y as f32 / h as f32 - 0.45);
            let centre = (1.0 - (u.length() * 1.5).min(1.0)).powi(2);
            let mut glass = Vec3::new(0.004, 0.014, 0.045) + Vec3::new(0.004, 0.02, 0.06) * centre;
            if x.is_multiple_of(32) || y.is_multiple_of(32) {
                glass += AZURE * 0.025;
            }
            for channel in [glass.x, glass.y, glass.z] {
                bytes.push((linear_to_srgb(channel) * 255.0) as u8);
            }
            bytes.push((0.68 * 255.0) as u8);
        }
    }
    bytes
}

// ---------------------------------------------------------------- a tiny painter

/// An additive RGB canvas, painted in pixel units.
struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<Vec3>,
    seed: u32,
}

impl Canvas {
    fn new(w: usize, h: usize, seed: u32) -> Self {
        Canvas { w, h, rgb: vec![Vec3::ZERO; w * h], seed }
    }

    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn add(&mut self, x: i32, y: i32, color: Vec3, amount: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || amount <= 0.0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.rgb[i] += color * amount;
    }

    /// A soft dot: a bright core and a falloff `glow` pixels wide.
    fn dot(&mut self, cx: f32, cy: f32, r: f32, color: Vec3, glow: f32) {
        let reach = (r + glow).ceil() as i32;
        for y in (cy as i32 - reach)..=(cy as i32 + reach) {
            for x in (cx as i32 - reach)..=(cx as i32 + reach) {
                let d = Vec2::new(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy).length();
                let core = (r + 0.5 - d).clamp(0.0, 1.0);
                let halo = if glow > 0.0 { (1.0 - (d - r) / glow).clamp(0.0, 1.0).powi(2) * 0.35 } else { 0.0 };
                self.add(x, y, color, core.max(halo));
            }
        }
    }

    /// A line `width` pixels wide with a faint glow either side.
    fn line(&mut self, a: Vec2, b: Vec2, width: f32, color: Vec3) {
        let length = a.distance(b).max(0.001);
        let steps = (length * 1.5) as i32 + 1;
        for i in 0..=steps {
            let p = a.lerp(b, i as f32 / steps as f32);
            self.dot(p.x, p.y, width / 2.0, color * 0.5, width * 2.5 + 2.0);
        }
    }

    fn ring(&mut self, c: Vec2, r: f32, width: f32, color: Vec3, from: f32, to: f32) {
        let steps = ((to - from).abs() * r).max(8.0) as i32;
        let mut last = c + Vec2::from_angle(from) * r;
        for i in 1..=steps {
            let p = c + Vec2::from_angle(from + (to - from) * i as f32 / steps as f32) * r;
            self.line(last, p, width, color);
            last = p;
        }
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Vec3, amount: f32) {
        for y in y0 as i32..y1 as i32 {
            for x in x0 as i32..x1 as i32 {
                self.add(x, y, color, amount);
            }
        }
    }

    /// The monitor's UI round the focal graphic: an inner border, corner brackets, a
    /// header strip with a title bar, a status pill and tick labels, and a footer of ticks.
    fn chrome(&mut self, accent: Vec3) {
        let (w, h) = (self.w as f32, self.h as f32);
        // inner border, thin and dim, just inside the bezel
        for (a, b) in [((3.0, 3.0), (w - 3.0, 3.0)), ((3.0, h - 3.0), (w - 3.0, h - 3.0)), ((3.0, 3.0), (3.0, h - 3.0)), ((w - 3.0, 3.0), (w - 3.0, h - 3.0))] {
            self.line(Vec2::from(a), Vec2::from(b), 1.0, AZURE * 0.45);
        }
        for (x, y) in [(6.0, 6.0), (w - 6.0, 6.0), (6.0, h - 6.0), (w - 6.0, h - 6.0)] {
            let sx = if x < w / 2.0 { 1.0 } else { -1.0 };
            let sy = if y < h / 2.0 { 1.0 } else { -1.0 };
            self.line(Vec2::new(x, y), Vec2::new(x + sx * 16.0, y), 2.0, CYAN);
            self.line(Vec2::new(x, y), Vec2::new(x, y + sy * 16.0), 2.0, CYAN);
        }
        if w < 200.0 {
            // side panel: a title bar and a status dot
            self.rect(14.0, 13.0, w * 0.6, 17.0, AZURE, 0.8);
            self.dot(w - 18.0, 15.0, 3.0, accent, 4.0);
            return;
        }
        // header: title, sub-title "text", a status pill in the accent colour
        self.rect(14.0, 11.0, 14.0 + w * 0.2, 16.0, CYAN, 0.85);
        let mut x = 14.0;
        while x < 14.0 + w * 0.3 {
            let word = 6.0 + self.random() * 16.0;
            self.rect(x, 20.0, x + word, 22.5, DIM * 1.6, 0.9);
            x += word + 4.0;
        }
        self.rect(w - 74.0, 10.0, w - 14.0, 20.0, accent * 0.35, 0.9);
        self.rect(w - 70.0, 13.5, w - 40.0, 16.5, accent, 1.0);
        self.dot(w - 24.0, 15.0, 2.5, accent, 4.0);
        // header rule
        self.line(Vec2::new(10.0, 28.0), Vec2::new(w - 10.0, 28.0), 0.8, AZURE * 0.35);
        // footer: a tick scale and three small readouts
        for i in 0..48u32 {
            let x = 16.0 + i as f32 * ((w - 32.0) / 47.0);
            let tall = if i.is_multiple_of(6) { 6.0 } else { 3.0 };
            self.rect(x, h - 12.0 - tall, x + 1.0, h - 12.0, AZURE, 0.5);
        }
        for i in 0..3 {
            let x = w - 150.0 + i as f32 * 44.0;
            let end = x + 30.0 + self.random() * 8.0;
            self.rect(x, h - 30.0, end, h - 26.0, if i == 1 { accent } else { CYAN }, 0.7);
        }
    }

    /// The art as light only (black adds nothing): the focal graphic, the UI and a
    /// bright cyan edge with a soft inner glow, lightly scanlined; out to sRGB bytes.
    fn finish(self) -> Image {
        let (w, h) = (self.w, self.h);
        crate::space::image(w as u32, h as u32, self.bytes())
    }

    fn bytes(&self) -> Vec<u8> {
        let (w, h) = (self.w, self.h);
        let mut bytes = Vec::with_capacity(w * h * 4);
        for (i, rgb) in self.rgb.iter().enumerate() {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let edge = x.min(y).min(w as f32 - 1.0 - x).min(h as f32 - 1.0 - y);
            let rim = if edge < 2.0 { 0.55 } else { (1.0 - (edge - 2.0) / 12.0).clamp(0.0, 1.0).powi(2) * 0.06 };
            let scan = if (y as usize).is_multiple_of(3) { 0.8 } else { 1.0 };
            let lit = (*rgb + CYAN * rim) * scan;
            // Purer hues (less of the weakest channel), so bright glow still reads
            // as blue and cyan after the camera's tone mapping.
            let lit = (lit - Vec3::splat(lit.min_element() * 0.75)).max(Vec3::ZERO);
            // Tone-map by the brightest channel so overlapping glow keeps its hue
            // (cyan stays cyan instead of burning to white).
            let peak = lit.max_element().max(1e-6);
            let lit = lit * ((peak / (1.0 + peak * 0.6)).min(1.0) / peak);
            for channel in [lit.x, lit.y, lit.z] {
                bytes.push((linear_to_srgb(channel.min(1.0)) * 255.0) as u8);
            }
            bytes.push(255);
        }
        bytes
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

// ---------------------------------------------------------------- side panels

fn paint_side(side: Side, seed: u32) -> Image {
    side_canvas(side, seed).finish()
}

fn side_canvas(side: Side, seed: u32) -> Canvas {
    let mut c = Canvas::new(160, 240, seed * 104_729 + 3);
    match side {
        Side::Gauges => {
            for i in 0..4 {
                let x = 22.0 + i as f32 * 32.0;
                let level = 0.3 + c.random() * 0.65;
                c.rect(x, 40.0, x + 16.0, 210.0, DIM, 0.6);
                c.rect(x, 210.0 - 170.0 * level, x + 16.0, 210.0, if i == 2 { AMBER } else { CYAN }, 0.9);
            }
        }
        Side::List => {
            for i in 0..9 {
                let y = 40.0 + i as f32 * 20.0;
                c.dot(20.0, y + 3.0, 3.0, if i % 4 == 1 { GREEN } else { CYAN }, 3.0);
                let end = 72.0 + c.random() * 80.0;
                c.rect(32.0, y, end, y + 5.0, AZURE, 0.75);
            }
        }
        Side::Wave => {
            for row in 0..3 {
                let base = 60.0 + row as f32 * 64.0;
                let mut last = Vec2::new(12.0, base);
                for i in 1..40 {
                    let x = 12.0 + i as f32 * 3.5;
                    let p = Vec2::new(x, base + (i as f32 * (0.5 + row as f32 * 0.3)).sin() * 16.0 * (1.0 - row as f32 * 0.2));
                    c.line(last, p, 1.4, if row == 1 { VIOLET } else { CYAN });
                    last = p;
                }
            }
        }
        Side::Dial => {
            let centre = Vec2::new(80.0, 110.0);
            c.ring(centre, 52.0, 2.0, DIM * 2.0, 0.0, std::f32::consts::TAU);
            c.ring(centre, 52.0, 4.0, CYAN, -std::f32::consts::FRAC_PI_2, 1.6);
            c.ring(centre, 38.0, 2.0, AMBER, -std::f32::consts::FRAC_PI_2, 0.2);
            for i in 0..4 {
                let end = 54.0 + c.random() * 80.0;
                c.rect(24.0, 186.0 + i as f32 * 10.0, end, 190.0 + i as f32 * 10.0, AZURE, 0.75);
            }
        }
    }
    c.chrome(CYAN);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes every screen texture as PNG into `$AGENT_ROOM_SCREENS_DUMP`, to look at
    /// the art without running the app: `cargo test -p room-app dump_screens -- --ignored`.
    #[test]
    #[ignore]
    fn dump_screens() {
        let dir = std::env::var("AGENT_ROOM_SCREENS_DUMP").unwrap_or_else(|_| "/tmp/screens".into());
        std::fs::create_dir_all(&dir).unwrap();
        let save = |name: String, c: Canvas| {
            let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), c.w as u32, c.h as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&c.bytes()).unwrap();
        };
        for (seed, side) in SIDES.iter().enumerate() {
            save(format!("side-{seed}"), side_canvas(*side, seed as u32 + 40));
        }
        for (name, w, h) in [("glass-main", 512, 256), ("glass-side", 160, 240)] {
            let file = std::fs::File::create(format!("{dir}/{name}.png")).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&glass_bytes(w, h)).unwrap();
        }
    }
}
