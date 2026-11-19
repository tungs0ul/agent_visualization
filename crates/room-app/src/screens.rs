//! The crew desks' screen art (PROMPT_03 B; the user: "it is a star ship and you
//! display code? some display graph, some display planet"). Each desk shows starship
//! illustrations painted here once at start: a big focal graphic on translucent
//! luminous blue glass with a bright cyan edge, small gauges round it and one accent
//! colour, so no two desks look alike. They are frameless holograms as in the target
//! (crops 07, 08); the Blender desk (`stations.py`) has only their emitters.

use bevy::prelude::*;

use crate::world3d::{DESKS, Kit, Spot};

/// Where a desk's screens are, relative to the desk centre (glTF space), their size,
/// yaw and tilt in degrees. Keep in step with `stations.py` `build_desk_console`.
const SCREENS: [(Vec3, Vec2, f32, f32); 3] = [
    // the main screen's lower edge sits on the projector bar (top 0.81 m)
    (Vec3::new(0.0, 1.2, -0.355), Vec2::new(1.5, 0.75), 0.0, -6.0),
    // side panels float a few cm over their emitters at the desk ends
    (Vec3::new(-0.98, 1.1, -0.195), Vec2::new(0.36, 0.54), 28.0, -4.0),
    (Vec3::new(0.98, 1.1, -0.195), Vec2::new(0.36, 0.54), -28.0, -4.0),
];

/// The big illustrations a main screen can show.
#[derive(Clone, Copy)]
enum Art {
    StarMap,
    Planet,
    Radar,
    Ship,
    Nebula,
    Route,
    Crew,
    Cargo,
}

const ARTS: [Art; 8] = [Art::StarMap, Art::Planet, Art::Radar, Art::Ship, Art::Nebula, Art::Route, Art::Crew, Art::Cargo];

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
const ORANGE: Vec3 = Vec3::new(1.0, 0.42, 0.24);
const MAGENTA: Vec3 = Vec3::new(1.0, 0.31, 0.85);
const VIOLET: Vec3 = Vec3::new(0.54, 0.36, 1.0);
const GREEN: Vec3 = Vec3::new(0.13, 0.88, 0.48);
const WHITE: Vec3 = Vec3::new(0.85, 0.95, 1.0);

/// Spawns the art panels on every crew desk.
pub fn build(kit: &mut Kit, images: &mut Assets<Image>) {
    let mains: Vec<Handle<Image>> = ARTS.iter().enumerate().map(|(seed, art)| images.add(paint_main(*art, seed as u32))).collect();
    let sides: Vec<Handle<Image>> = SIDES.iter().enumerate().map(|(seed, side)| images.add(paint_side(*side, seed as u32 + 40))).collect();
    let quad = kit.meshes.add(Rectangle::new(1.0, 1.0));
    for index in 0..DESKS.len() {
        let desk = Spot::Desk(index as u8).center();
        let textures = [mains[(index * 3) % ARTS.len()].clone(), sides[index % SIDES.len()].clone(), sides[(index + 2) % SIDES.len()].clone()];
        for ((at, size, yaw, tilt), texture) in SCREENS.into_iter().zip(textures) {
            let material = kit.materials.add(StandardMaterial {
                // Unlit and a touch over 1: the art glows (and blooms a little) by itself.
                base_color: Color::LinearRgba(LinearRgba::new(3.2, 3.2, 3.2, 1.0)),
                base_color_texture: Some(texture),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let rotation = Quat::from_rotation_y(yaw.to_radians()) * Quat::from_rotation_x(tilt.to_radians());
            let front = rotation * Vec3::Z * 0.006;
            kit.commands.spawn((
                Mesh3d(quad.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(desk + at + front).with_rotation(rotation).with_scale(size.extend(1.0) * 0.97),
            ));
        }
    }
}

// ---------------------------------------------------------------- a tiny painter

/// An additive RGB canvas with alpha, painted in pixel units.
struct Canvas {
    w: usize,
    h: usize,
    rgb: Vec<Vec3>,
    alpha: Vec<f32>,
    seed: u32,
}

impl Canvas {
    fn new(w: usize, h: usize, seed: u32) -> Self {
        Canvas { w, h, rgb: vec![Vec3::ZERO; w * h], alpha: vec![0.0; w * h], seed }
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
        self.alpha[i] = (self.alpha[i] + amount).min(1.0);
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
            self.dot(p.x, p.y, width / 2.0, color * 0.5, width * 1.5);
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

    /// Glass behind everything (translucent luminous blue, brighter in the middle, a
    /// bright cyan edge), a faint grid and scanlines, then out to sRGB bytes.
    fn finish(self) -> Image {
        let (w, h) = (self.w, self.h);
        crate::space::image(w as u32, h as u32, self.bytes())
    }

    fn bytes(&self) -> Vec<u8> {
        let (w, h) = (self.w, self.h);
        let mut bytes = Vec::with_capacity(w * h * 4);
        for (i, (rgb, alpha)) in self.rgb.iter().zip(&self.alpha).enumerate() {
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let u = Vec2::new(x / w as f32 - 0.5, y / h as f32 - 0.45);
            let centre = (1.0 - (u.length() * 1.5).min(1.0)).powi(2);
            let mut glass = Vec3::new(0.002, 0.012, 0.05) + Vec3::new(0.004, 0.022, 0.075) * centre;
            // grid every 32 px and a scanline every third row
            let grid = ((x as usize).is_multiple_of(32) || (y as usize).is_multiple_of(32)) as u8 as f32;
            glass += AZURE * 0.04 * grid;
            // the hologram's bright edge: 2 px of cyan and a soft inner falloff
            let edge = x.min(y).min(w as f32 - 1.0 - x).min(h as f32 - 1.0 - y);
            let rim = if edge < 2.0 { 1.0 } else { (1.0 - (edge - 2.0) / 10.0).clamp(0.0, 1.0).powi(2) * 0.18 };
            glass += CYAN * rim;
            let scan = if (y as usize).is_multiple_of(3) { 0.85 } else { 1.0 };
            let lit = (glass + *rgb) * scan;
            // see-through glass, opaque where the art is
            let a = (0.72 + 0.28 * alpha + rim * 0.3).min(1.0);
            for channel in [lit.x, lit.y, lit.z] {
                let mapped = channel / (1.0 + channel * 0.35);
                bytes.push((linear_to_srgb(mapped.min(1.0)) * 255.0) as u8);
            }
            bytes.push((a * 255.0) as u8);
        }
        bytes
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// Points round an ellipse, for orbits and globe meridians.
fn ellipse(c: &mut Canvas, centre: Vec2, radii: Vec2, width: f32, color: Vec3) {
    let steps = 120;
    for i in 0..steps {
        let a0 = i as f32 / steps as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / steps as f32 * std::f32::consts::TAU;
        c.line(centre + Vec2::new(a0.cos(), a0.sin()) * radii, centre + Vec2::new(a1.cos(), a1.sin()) * radii, width, color);
    }
}

// ---------------------------------------------------------------- main screens

fn paint_main(art: Art, seed: u32) -> Image {
    main_canvas(art, seed).finish()
}

fn main_canvas(art: Art, seed: u32) -> Canvas {
    use std::f32::consts::{PI, TAU};
    let mut c = Canvas::new(512, 256, seed * 7919 + 17);
    let centre = Vec2::new(256.0, 136.0);
    let accent = match art {
        Art::StarMap | Art::Route => ORANGE,
        Art::Planet => VIOLET,
        Art::Radar | Art::Cargo => AMBER,
        Art::Ship | Art::Nebula => MAGENTA,
        Art::Crew => GREEN,
    };
    match art {
        Art::StarMap => {
            for _ in 0..160 {
                let (x, y, b) = (c.random() * 512.0, c.random() * 256.0, c.random());
                c.dot(x, y, 0.6 + b, WHITE * (0.3 + b * 0.7), 1.5);
            }
            let stops: Vec<Vec2> = (0..6).map(|i| Vec2::new(70.0 + i as f32 * 76.0, 70.0 + c.random() * 130.0)).collect();
            for pair in stops.windows(2) {
                c.line(pair[0], pair[1], 2.2, ORANGE);
            }
            for (i, stop) in stops.iter().enumerate() {
                c.ring(*stop, 7.0, 1.4, if i == 0 { CYAN } else { ORANGE }, 0.0, TAU);
                c.dot(stop.x, stop.y, 2.5, WHITE, 4.0);
            }
            c.ring(stops[5], 16.0, 1.0, AMBER, 0.0, TAU);
        }
        Art::Planet => {
            let r = 82.0;
            c.dot(centre.x, centre.y, r, AZURE * 0.12, 0.0);
            c.ring(centre, r + 4.0, 3.0, CYAN, 0.0, TAU);
            for k in 1..6 {
                ellipse(&mut c, centre, Vec2::new(r * k as f32 / 6.0, r), 1.0, AZURE * 0.7);
                let y = centre.y - r + 2.0 * r * k as f32 / 6.0;
                let half = (r * r - (y - centre.y).powi(2)).max(0.0).sqrt();
                c.line(Vec2::new(centre.x - half, y), Vec2::new(centre.x + half, y), 1.0, AZURE * 0.7);
            }
            ellipse(&mut c, centre, Vec2::new(170.0, 48.0), 1.2, VIOLET);
            c.dot(centre.x + 150.0, centre.y + 23.0, 6.0, VIOLET, 8.0);
        }
        Art::Radar => {
            for k in 1..5 {
                c.ring(centre, k as f32 * 26.0, 1.2, CYAN * 0.8, 0.0, TAU);
            }
            c.line(centre - Vec2::X * 110.0, centre + Vec2::X * 110.0, 1.0, DIM * 2.0);
            c.line(centre - Vec2::Y * 110.0, centre + Vec2::Y * 110.0, 1.0, DIM * 2.0);
            for i in 0..40 {
                let a = -0.9 + i as f32 * 0.02;
                c.line(centre, centre + Vec2::from_angle(a) * 104.0, 1.0, CYAN * (i as f32 / 40.0) * 0.6);
            }
            for _ in 0..14 {
                let (a, d) = (c.random() * TAU, 20.0 + c.random() * 80.0);
                let p = centre + Vec2::from_angle(a) * d;
                c.dot(p.x, p.y, 2.0, CYAN, 4.0);
            }
            let hit = centre + Vec2::from_angle(-0.5) * 70.0;
            c.dot(hit.x, hit.y, 4.0, AMBER, 9.0);
            c.ring(hit, 12.0, 1.4, AMBER, 0.0, TAU);
        }
        Art::Ship => {
            // A side-on wireframe of a ship, its reactor in the accent colour.
            let hull = [(90.0, 140.0), (150.0, 100.0), (330.0, 96.0), (420.0, 120.0), (440.0, 136.0), (420.0, 152.0), (330.0, 176.0), (150.0, 172.0)];
            let pts: Vec<Vec2> = hull.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
            for i in 0..pts.len() {
                c.line(pts[i], pts[(i + 1) % pts.len()], 2.0, CYAN);
            }
            for x in [190.0, 240.0, 290.0, 340.0] {
                c.line(Vec2::new(x, 100.0), Vec2::new(x, 172.0), 1.0, AZURE * 0.7);
            }
            c.line(Vec2::new(150.0, 136.0), Vec2::new(420.0, 136.0), 1.0, AZURE * 0.7);
            c.ring(Vec2::new(215.0, 136.0), 20.0, 2.0, MAGENTA, 0.0, TAU);
            c.dot(215.0, 136.0, 7.0, MAGENTA, 12.0);
            c.line(Vec2::new(70.0, 128.0), Vec2::new(90.0, 124.0), 3.0, CYAN);
            c.line(Vec2::new(70.0, 152.0), Vec2::new(90.0, 156.0), 3.0, CYAN);
            c.dot(62.0, 140.0, 5.0, AZURE, 14.0);
        }
        Art::Nebula => {
            for _ in 0..2600 {
                let a = c.random() * TAU;
                let d = (c.random() * c.random()) * 120.0;
                let swirl = a + d * 0.03;
                let p = centre + Vec2::new(swirl.cos() * d * 1.7, swirl.sin() * d * 0.9);
                let color = if d < 30.0 {
                    MAGENTA.lerp(WHITE, 1.0 - d / 30.0)
                } else if c.random() < 0.5 {
                    VIOLET
                } else {
                    AZURE
                };
                c.dot(p.x, p.y, 0.8, color * 0.25, 3.0);
            }
            c.dot(centre.x, centre.y, 6.0, WHITE, 18.0);
            for _ in 0..60 {
                let (x, y) = (c.random() * 512.0, c.random() * 256.0);
                c.dot(x, y, 0.7, WHITE * 0.7, 1.2);
            }
        }
        Art::Route => {
            for k in 0..6 {
                let y = 40.0 + k as f32 * 36.0;
                c.line(Vec2::new(30.0, y), Vec2::new(482.0, y), 0.8, DIM * 1.5);
            }
            let mut last = Vec2::new(30.0, 200.0);
            for i in 1..24 {
                let x = 30.0 + i as f32 * 19.6;
                let y = 200.0 - i as f32 * 5.0 - (i as f32 * 0.9).sin() * 22.0 - c.random() * 10.0;
                let p = Vec2::new(x, y);
                c.line(last, p, 2.4, ORANGE);
                last = p;
            }
            c.dot(last.x, last.y, 4.0, WHITE, 8.0);
            for i in 0..22 {
                let height = 10.0 + c.random() * 40.0;
                c.rect(36.0 + i as f32 * 20.5, 218.0 - height, 48.0 + i as f32 * 20.5, 218.0, AZURE, 0.6);
            }
        }
        Art::Crew => {
            for i in 0..5 {
                let x = 72.0 + i as f32 * 92.0;
                c.ring(Vec2::new(x, 100.0), 18.0, 2.0, CYAN, 0.0, TAU);
                c.ring(Vec2::new(x, 172.0), 36.0, 2.0, CYAN, PI, TAU);
                c.line(Vec2::new(x - 36.0, 172.0), Vec2::new(x + 36.0, 172.0), 2.0, CYAN);
                c.dot(x + 26.0, 84.0, 4.0, if i == 3 { AMBER } else { GREEN }, 6.0);
                c.rect(x - 30.0, 192.0, x + 30.0, 196.0, AZURE, 0.7);
                c.rect(x - 30.0, 202.0, x + 10.0, 205.0, DIM, 0.9);
            }
        }
        Art::Cargo => {
            // Isometric wireframe crates, one highlighted.
            let cube = |c: &mut Canvas, o: Vec2, s: f32, color: Vec3| {
                let (dx, dy) = (Vec2::new(s * 0.87, s * 0.5), Vec2::new(-s * 0.87, s * 0.5));
                let up = Vec2::new(0.0, -s);
                let corners = [o, o + dx, o + dx + dy, o + dy];
                for i in 0..4 {
                    c.line(corners[i], corners[(i + 1) % 4], 1.6, color);
                    c.line(corners[i] + up, corners[(i + 1) % 4] + up, 1.6, color);
                    c.line(corners[i], corners[i] + up, 1.6, color);
                }
            };
            for (i, (x, y)) in [(150.0, 150.0), (220.0, 150.0), (185.0, 170.0), (290.0, 160.0), (360.0, 140.0), (330.0, 180.0)].into_iter().enumerate() {
                cube(&mut c, Vec2::new(x, y), 36.0, if i == 4 { AMBER } else { CYAN });
            }
            c.dot(392.0, 130.0, 3.0, AMBER, 8.0);
        }
    }
    c.chrome(accent);
    c
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
        for (seed, art) in ARTS.iter().enumerate() {
            save(format!("main-{seed}"), main_canvas(*art, seed as u32));
        }
        for (seed, side) in SIDES.iter().enumerate() {
            save(format!("side-{seed}"), side_canvas(*side, seed as u32 + 40));
        }
    }
}
