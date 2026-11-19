//! Now and then a comet or a meteor crosses the view outside the window, one at a time,
//! every 2 to 5 minutes (sprites rendered in Blender, `tools/blender/comets.py`).
//!
//! Power: nothing here asks for frames between visitors. `fly` only looks at the clock
//! on frames the app draws anyway (every frame while focused; while dozing unfocused,
//! the 60 s low-power wake-up of `WinitSettings::desktop_app`), so a visitor may start
//! up to a minute late, which nobody can tell. While one is on screen, `on_screen` makes
//! `pace_updates` draw at the walking rate; when it has gone the app dozes again.
//!
//! `AGENT_ROOM_VISITOR=comet|meteor` sends one a second after start (screenshots, tests).

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::space::{decode_png, through_window};

/// Pause between two visitors, seconds.
const GAP: (f32, f32) = (120.0, 300.0);
/// The window glass spans x -7.5..7.5; a visitor starts and ends beyond it.
const GLASS_HALF_WIDTH: f32 = 7.5;
/// Sprites are 4:1 (2048x512), the head at the right end.
const ASPECT: f32 = 4.0;
/// Brightness of the light the sprite adds (as the planets').
const GAIN: f32 = 1.3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Comet,
    Meteor,
}

impl Kind {
    fn from_env() -> Option<Self> {
        match std::env::var("AGENT_ROOM_VISITOR").ok()?.as_str() {
            "comet" => Some(Kind::Comet),
            "meteor" => Some(Kind::Meteor),
            _ => None,
        }
    }
}

/// One crossing, in window-glass coordinates (see `space::through_window`).
#[derive(Clone, Copy, Debug)]
struct Pass {
    from: Vec2,
    to: Vec2,
    /// Sprite length on the glass.
    length: f32,
    /// Depth behind the glass.
    z: f32,
    seconds: f32,
}

impl Pass {
    /// A random crossing: left to right or back, slightly slanted, head first. Comets are
    /// far (behind the small planet, in front of the nebula), long and slow; meteors
    /// nearer (in front of the big planet), shorter and faster.
    fn random(kind: Kind, mut next: impl FnMut() -> f32) -> Self {
        let (length, z, seconds) = match kind {
            Kind::Comet => (5.0 + next() * 2.5, -100.0, 8.5 + next() * 1.5),
            Kind::Meteor => (2.6 + next() * 1.2, -36.0, 6.0 + next() * 1.0),
        };
        // Off the glass by the whole sprite, so it enters and leaves unseen.
        let reach = GLASS_HALF_WIDTH + length + 0.5;
        let high = 2.9 + next() * 1.0;
        let low = 1.0 + next() * 1.2;
        // Mostly falling, sometimes rising (as the approved preview).
        let (start_y, end_y) = if next() < 0.7 { (high, low) } else { (low, high) };
        let (from, to) = if next() < 0.5 {
            (Vec2::new(-reach, start_y), Vec2::new(reach, end_y))
        } else {
            (Vec2::new(reach, start_y), Vec2::new(-reach, end_y))
        };
        Pass { from, to, length, z, seconds }
    }

    /// Where the sprite's middle is on the glass at `t` (0..1).
    fn at(&self, t: f32) -> Vec2 {
        self.from.lerp(self.to, t)
    }

    /// Angle of the motion on the glass, counter-clockwise from +x: the sprite's head
    /// (+x) is turned this way, so it leads.
    fn heading(&self) -> f32 {
        let d = self.to - self.from;
        d.y.atan2(d.x)
    }

    /// Fades in over the first and out over the last 12 %.
    fn fade(t: f32) -> f32 {
        let edge = (t.min(1.0 - t) / 0.12).clamp(0.0, 1.0);
        edge * edge * (3.0 - 2.0 * edge)
    }
}

/// When the next visitor comes, and whether one is on screen (for `pace_updates`).
#[derive(Resource)]
pub struct Visitors {
    pub on_screen: bool,
    next_at: Instant,
    next_kind: Option<Kind>,
    seed: u32,
    comet: Option<Handle<Image>>,
    meteor: Option<Handle<Image>>,
    quad: Option<Handle<Mesh>>,
}

impl Default for Visitors {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x9E37_79B9, |d| d.subsec_nanos() | 1);
        let mut visitors =
            Visitors { on_screen: false, next_at: Instant::now(), next_kind: None, seed, comet: None, meteor: None, quad: None };
        match Kind::from_env() {
            Some(kind) => {
                visitors.next_at = Instant::now() + Duration::from_secs(1);
                visitors.next_kind = Some(kind);
            }
            None => visitors.next_at = Instant::now() + visitors.gap(),
        }
        visitors
    }
}

impl Visitors {
    /// Xorshift, 0..1.
    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed % 10_000) as f32 / 10_000.0
    }

    fn gap(&mut self) -> Duration {
        Duration::from_secs_f32(GAP.0 + self.random() * (GAP.1 - GAP.0))
    }
}

/// The visitor crossing now.
#[derive(Component)]
pub struct Visitor {
    pass: Pass,
    started: Instant,
    material: Handle<StandardMaterial>,
}

/// Sends a visitor when one is due, moves it across and removes it when it has gone.
pub fn fly(
    mut commands: Commands,
    mut visitors: ResMut<Visitors>,
    mut flying: Query<(Entity, &Visitor, &mut Transform)>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = Instant::now();
    if flying.is_empty() && now >= visitors.next_at {
        // Comets are rarer: about one in three.
        let kind = visitors.next_kind.take().unwrap_or_else(|| if visitors.random() < 0.34 { Kind::Comet } else { Kind::Meteor });
        let pass = Pass::random(kind, || visitors.random());
        let gap = visitors.gap();
        visitors.next_at = now + Duration::from_secs_f32(pass.seconds) + gap;
        let texture = match kind {
            Kind::Comet => visitors
                .comet
                .get_or_insert_with(|| images.add(decode_png(include_bytes!("../../../assets/textures/space/comet.png"))))
                .clone(),
            Kind::Meteor => visitors
                .meteor
                .get_or_insert_with(|| images.add(decode_png(include_bytes!("../../../assets/textures/space/meteor.png"))))
                .clone(),
        };
        let quad = visitors.quad.get_or_insert_with(|| meshes.add(Rectangle::new(ASPECT, 1.0))).clone();
        let material = materials.add(StandardMaterial {
            base_color: Color::LinearRgba(LinearRgba::new(0.0, 0.0, 0.0, 0.0)),
            base_color_texture: Some(texture),
            alpha_mode: AlphaMode::Premultiplied,
            unlit: true,
            ..default()
        });
        commands.spawn((
            Visitor { pass, started: now, material: material.clone() },
            Mesh3d(quad),
            MeshMaterial3d(material),
            place(&pass, 0.0),
        ));
        debug!("visitor: {kind:?} for {:.1} s, next in {:.0} s", pass.seconds, (visitors.next_at - now).as_secs_f32());
        visitors.on_screen = true;
        return;
    }
    let mut any = false;
    for (entity, visitor, mut transform) in &mut flying {
        let t = (now - visitor.started).as_secs_f32() / visitor.pass.seconds;
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        any = true;
        *transform = place(&visitor.pass, t);
        if let Some(material) = materials.get_mut(&visitor.material) {
            // Premultiplied: fading scales the added light and the hiding alike.
            let f = Pass::fade(t);
            material.base_color = Color::LinearRgba(LinearRgba::new(GAIN * f, GAIN * f, GAIN * f, f));
        }
    }
    visitors.on_screen = any;
}

/// The sprite at `t`: behind the glass point, sized to look `length` long on the glass,
/// facing the camera and turned in that plane so the head leads.
fn place(pass: &Pass, t: f32) -> Transform {
    let glass = pass.at(t);
    let (at, reach) = through_window(glass.x, glass.y, pass.z);
    // As `space::planet_sprite`: face the camera, then half a turn about Y so the front
    // (not the culled back) shows unmirrored; local +x is then screen right.
    let facing = Transform::from_translation(at).looking_at(crate::view3d::FULL_EYE, Vec3::Y).rotation;
    let rotation = facing * Quat::from_rotation_y(PI) * Quat::from_rotation_z(pass.heading());
    Transform::from_translation(at).with_rotation(rotation).with_scale(Vec3::splat(pass.length / ASPECT * reach))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passes(kind: Kind) -> Vec<Pass> {
        let mut visitors = Visitors { seed: 12345, ..Visitors::default() };
        (0..200).map(|_| Pass::random(kind, || visitors.random())).collect()
    }

    #[test]
    fn a_visitor_enters_and_leaves_outside_the_window() {
        for kind in [Kind::Comet, Kind::Meteor] {
            for pass in passes(kind) {
                for end in [pass.from, pass.to] {
                    // Even the tail end of the sprite is off the glass.
                    assert!(end.x.abs() - pass.length / 2.0 > GLASS_HALF_WIDTH, "{pass:?}");
                }
                assert!(pass.from.x.signum() != pass.to.x.signum());
                // Slanted, but only slightly.
                assert!(pass.heading().sin().abs() < 0.25, "{pass:?}");
                assert!((0.0..=1.0).contains(&Pass::fade(0.5)) && Pass::fade(0.0) == 0.0 && Pass::fade(1.0) == 0.0);
            }
        }
    }

    #[test]
    fn the_head_leads_along_the_motion() {
        for pass in passes(Kind::Meteor) {
            let a = place(&pass, 0.3);
            let b = place(&pass, 0.6);
            // The sprite's +x (its head) points where it goes.
            let head = a.rotation * Vec3::X;
            assert!(head.dot((b.translation - a.translation).normalize()) > 0.99, "{pass:?}");
            // And its front faces the camera (not culled).
            let front = a.rotation * Vec3::Z;
            assert!(front.dot((crate::view3d::FULL_EYE - a.translation).normalize()) > 0.99);
        }
    }

    #[test]
    fn meteors_are_smaller_faster_and_nearer_than_comets() {
        let (comets, meteors) = (passes(Kind::Comet), passes(Kind::Meteor));
        let max = |list: &[Pass], f: fn(&Pass) -> f32| list.iter().map(f).fold(f32::MIN, f32::max);
        let min = |list: &[Pass], f: fn(&Pass) -> f32| list.iter().map(f).fold(f32::MAX, f32::min);
        assert!(max(&meteors, |p| p.length) < min(&comets, |p| p.length));
        assert!(max(&meteors, |p| p.seconds) < min(&comets, |p| p.seconds));
        assert!(min(&meteors, |p| p.z) > max(&comets, |p| p.z));
    }
}
