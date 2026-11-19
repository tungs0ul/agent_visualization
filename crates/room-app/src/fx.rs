//! The ship's energy (PLAN.md M29, the user's starship brief): branching lightning in
//! the command core and the ceiling reactor, energy pulses along the floor routes to
//! working desks, sparks at a desk when its robot starts a tool, a green pulse when it
//! finishes and a red discharge when it fails, and particles rising in the core.
//!
//! Everything here follows real agent state and stops when nobody works, so the view
//! still dozes when idle (V2.md power budget). Settings > Effects turns it all off.

use bevy::prelude::*;

use crate::model::Who;
use crate::world3d::{DeskLight, DeskState, Spot};

/// How long one discharge lasts, seconds (the brief: 100-400 ms).
const BOLT_LIFE: (f32, f32) = (0.1, 0.4);
/// Energy pulses run along the floor at this speed, m/s.
const PULSE_SPEED: f32 = 7.0;
/// A working desk gets a pulse this often, seconds.
const PULSE_EVERY: f32 = 0.9;
/// The reactor ring above the command core: height and radius.
const REACTOR_HEIGHT: f32 = 4.6;
const REACTOR_RADIUS: f32 = 1.5;
/// The core: radius, bottom and top (matching `world3d::command_core`).
const CORE_RADIUS: f32 = 1.3;
const CORE_BOTTOM: f32 = 0.4;
const CORE_TOP: f32 = 2.8;
const PARTICLES: usize = 28;

/// Shared meshes and materials, made once.
#[derive(Resource)]
pub struct Look {
    segment: Handle<Mesh>,
    core: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    pulse: Handle<Mesh>,
    cyan: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
}

/// One segment of a discharge; gone when its time is up.
#[derive(Component)]
pub struct Bolt {
    left: f32,
}

#[derive(Component)]
pub struct Pulse {
    from: Vec3,
    to: Vec3,
    travelled: f32,
}

#[derive(Component)]
pub struct Rising {
    speed: f32,
}

#[derive(Component)]
pub struct ReactorRing {
    speed: f32,
}

/// When each emitter fires next, and what it last saw.
#[derive(Resource, Default)]
pub struct Timers {
    core: f32,
    reactor: f32,
    floor: f32,
    pulses: Vec<f32>,
    /// Each desk's state last frame, to flash on a change.
    desks: Vec<DeskState>,
    /// Tool calls seen per agent, to spark when one starts.
    calls: std::collections::HashMap<(String, Who), usize>,
    seed: u32,
}

impl Timers {
    fn random(&mut self) -> f32 {
        let mut x = self.seed.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x % 10_000) as f32 / 10_000.0
    }
}

fn additive(materials: &mut Assets<StandardMaterial>, color: LinearRgba) -> Handle<StandardMaterial> {
    // Unlit materials draw their base colour (emissive is ignored), so the glow goes
    // there; values above 1 feed the bloom.
    materials.add(StandardMaterial { base_color: Color::LinearRgba(color), alpha_mode: AlphaMode::Add, unlit: true, ..default() })
}

/// Builds what is always there: the reactor rings with their energy nodes, the beam
/// from the core up to it, and the core's particles (hidden until someone works).
pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let look = Look {
        segment: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        core: additive(&mut materials, LinearRgba::rgb(6.0, 9.0, 14.0)),
        glow: additive(&mut materials, LinearRgba::rgb(0.3, 1.6, 4.0)),
        red: additive(&mut materials, LinearRgba::rgb(10.0, 1.2, 1.6)),
        pulse: meshes.add(Sphere::new(0.09)),
        cyan: additive(&mut materials, LinearRgba::rgb(1.0, 5.0, 9.0)),
        green: additive(&mut materials, LinearRgba::rgb(1.0, 9.0, 4.0)),
    };
    let command = Spot::Command.center();
    let reactor = command + Vec3::Y * REACTOR_HEIGHT;
    // Two rings turning against each other, and six energy nodes on the outer one.
    let ring_glow = additive(&mut materials, LinearRgba::rgb(0.8, 3.5, 7.0));
    for (radius, speed) in [(REACTOR_RADIUS, 0.4), (REACTOR_RADIUS * 0.72, -0.6)] {
        commands.spawn((
            Mesh3d(meshes.add(Torus::new(radius - 0.05, radius + 0.05))),
            MeshMaterial3d(ring_glow.clone()),
            Transform::from_translation(reactor),
            ReactorRing { speed },
        ));
    }
    let node = meshes.add(Sphere::new(0.12));
    for index in 0..6 {
        let angle = index as f32 / 6.0 * std::f32::consts::TAU;
        let at = reactor + Vec3::new(angle.cos(), 0.0, angle.sin()) * REACTOR_RADIUS;
        commands.spawn((Mesh3d(node.clone()), MeshMaterial3d(look.cyan.clone()), Transform::from_translation(at)));
    }
    // The beam: a soft additive column from the core's top to the reactor.
    let beam = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(LinearRgba::rgb(0.06, 0.28, 0.7)),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let length = REACTOR_HEIGHT - CORE_TOP;
    commands.spawn((
        Mesh3d(meshes.add(Cylinder::new(0.22, length).mesh().resolution(24).without_caps().build())),
        MeshMaterial3d(beam),
        Transform::from_translation(command + Vec3::Y * (CORE_TOP + length / 2.0)),
    ));
    let particle = meshes.add(Sphere::new(0.025));
    for index in 0..PARTICLES {
        let angle = index as f32 * 2.399;
        let r = CORE_RADIUS * 0.85 * ((index % 7) as f32 / 7.0).sqrt();
        let at = command + Vec3::new(angle.cos() * r, CORE_BOTTOM + (index as f32 * 0.37) % (CORE_TOP - CORE_BOTTOM), angle.sin() * r);
        commands.spawn((
            Mesh3d(particle.clone()),
            MeshMaterial3d(look.cyan.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
            Rising { speed: 0.5 + (index % 5) as f32 * 0.15 },
        ));
    }
    commands.insert_resource(look);
    commands.insert_resource(Timers { seed: 0x1234_5678, ..default() });
}

/// A branching lightning path from `a` to `b`: the line split again and again with
/// its midpoints pushed sideways, plus a couple of short forks off the main path.
fn bolt_path(a: Vec3, b: Vec3, timers: &mut Timers, depth: u32, out: &mut Vec<(Vec3, Vec3, bool)>, main: bool) {
    let mut points = vec![a, b];
    let mut spread = a.distance(b) * 0.22;
    for _ in 0..depth {
        let mut next = Vec::with_capacity(points.len() * 2);
        for pair in points.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let jitter = Vec3::new(timers.random() - 0.5, timers.random() - 0.5, timers.random() - 0.5) * 2.0 * spread;
            next.push(p);
            next.push((p + q) / 2.0 + jitter);
        }
        next.push(points[points.len() - 1]);
        points = next;
        spread *= 0.55;
    }
    for pair in points.windows(2) {
        out.push((pair[0], pair[1], main));
    }
    if main && points.len() > 2 {
        for _ in 0..2 {
            let start = points[1 + (timers.random() * (points.len() - 2) as f32) as usize];
            let reach = a.distance(b) * (0.2 + timers.random() * 0.25);
            let dir = Vec3::new(timers.random() - 0.5, timers.random() - 0.6, timers.random() - 0.5).normalize_or_zero();
            bolt_path(start, start + dir * reach, timers, depth.saturating_sub(2), out, false);
        }
    }
}

/// Spawns one discharge between two points: a white-hot core and a cyan glow around
/// it on every segment, thinner on the forks.
fn spawn_bolt(commands: &mut Commands, look: &Look, timers: &mut Timers, a: Vec3, b: Vec3, red: bool) {
    let mut segments = Vec::new();
    bolt_path(a, b, timers, 5, &mut segments, true);
    let life = BOLT_LIFE.0 + timers.random() * (BOLT_LIFE.1 - BOLT_LIFE.0);
    for (p, q, main) in segments {
        let length = p.distance(q);
        if length < 1e-4 {
            continue;
        }
        let rotation = Quat::from_rotation_arc(Vec3::Z, (q - p) / length);
        let width = if main { 0.045 } else { 0.025 };
        let core = if red { look.red.clone() } else { look.core.clone() };
        for (thickness, material) in [(width, core), (width * 3.5, look.glow.clone())] {
            commands.spawn((
                Mesh3d(look.segment.clone()),
                MeshMaterial3d(material),
                Transform::from_translation((p + q) / 2.0).with_rotation(rotation).with_scale(Vec3::new(thickness, thickness, length)),
                Bolt { left: life },
            ));
        }
    }
}

/// A random point on a ring.
fn on_ring(timers: &mut Timers, centre: Vec3, radius: f32) -> Vec3 {
    let angle = timers.random() * std::f32::consts::TAU;
    centre + Vec3::new(angle.cos(), 0.0, angle.sin()) * radius
}

fn spawn_pulse(commands: &mut Commands, look: &Look, from: Vec3, to: Vec3, green: bool) {
    let material = if green { look.green.clone() } else { look.cyan.clone() };
    let heading = Quat::from_rotation_arc(Vec3::Z, (to - from).normalize_or_zero());
    commands.spawn((
        Mesh3d(look.pulse.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(from).with_rotation(heading).with_scale(Vec3::new(1.0, 1.0, 3.5)),
        Pulse { from, to, travelled: 0.0 },
    ));
}

/// Fires discharges and pulses for what is happening now, moves them, and lets the
/// view doze when nobody works.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn energy(
    mut commands: Commands,
    time: Res<Time>,
    model: Res<crate::model::Model>,
    replay: Res<crate::replay::Replay>,
    prefs: Res<crate::ui::PanelPrefs>,
    look: Option<Res<Look>>,
    mut timers: ResMut<Timers>,
    mut view: ResMut<crate::view3d::View3d>,
    desks: Query<&DeskLight>,
    avatars: Query<&crate::view3d::Avatar>,
    mut bolts: Query<(Entity, &mut Bolt)>,
    mut pulses: Query<(Entity, &mut Pulse, &mut Transform), (Without<Rising>, Without<ReactorRing>)>,
    mut particles: Query<(&Rising, &mut Transform, &mut Visibility), (Without<Pulse>, Without<ReactorRing>)>,
    mut rings: Query<(&ReactorRing, &mut Transform), (Without<Pulse>, Without<Rising>)>,
) {
    let Some(look) = look else { return };
    let dt = time.delta_secs().min(0.1);
    let busy = prefs.quality.effects()
        && (model.live().iter().any(|session| session.is_busy()) || replay.active.as_ref().is_some_and(|play| play.playing));
    let command = Spot::Command.center();

    // Discharges fade out on their own, busy or not.
    for (entity, mut bolt) in &mut bolts {
        bolt.left -= dt;
        if bolt.left <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            view.moving = true;
        }
    }
    // Pulses run to their desk and vanish there.
    for (entity, mut pulse, mut at) in &mut pulses {
        pulse.travelled += PULSE_SPEED * dt;
        let length = pulse.from.distance(pulse.to);
        if pulse.travelled >= length {
            commands.entity(entity).despawn();
            continue;
        }
        at.translation = pulse.from + (pulse.to - pulse.from) * (pulse.travelled / length);
        view.moving = true;
    }
    for (_, _, mut visibility) in &mut particles {
        let wanted = if busy { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    if !busy {
        return;
    }
    view.moving = true;
    for (ring, mut at) in &mut rings {
        at.rotate_y(ring.speed * dt);
    }
    for (rising, mut at, _) in &mut particles {
        at.translation.y += rising.speed * dt;
        if at.translation.y > command.y + CORE_TOP {
            at.translation.y = command.y + CORE_BOTTOM;
        }
    }

    // The core crackles: arcs up its glass wall, at random intervals.
    timers.core -= dt;
    if timers.core <= 0.0 {
        let low = on_ring(&mut timers, command + Vec3::Y * CORE_BOTTOM, CORE_RADIUS);
        let high = on_ring(&mut timers, command + Vec3::Y * (CORE_TOP - 0.2), CORE_RADIUS);
        spawn_bolt(&mut commands, &look, &mut timers, low, high, false);
        timers.core = 0.35 + timers.random() * 0.9;
    }
    // The reactor discharges between its nodes, out of step with the core.
    timers.reactor -= dt;
    if timers.reactor <= 0.0 {
        let reactor = command + Vec3::Y * REACTOR_HEIGHT;
        let a = on_ring(&mut timers, reactor, REACTOR_RADIUS);
        let b = on_ring(&mut timers, reactor, REACTOR_RADIUS);
        spawn_bolt(&mut commands, &look, &mut timers, a, b, false);
        timers.reactor = 0.5 + timers.random() * 1.2;
    }

    // The floor crackles round the teleporter, arcs running out along the routes.
    timers.floor -= dt;
    if timers.floor <= 0.0 {
        let pad = crate::world3d::teleporter_spot() + Vec3::Y * 0.06;
        let a = on_ring(&mut timers, pad, 1.7);
        let out = (a - pad).normalize_or_zero();
        let b = a + out * (1.0 + timers.random() * 1.2) + Vec3::Y * 0.02;
        spawn_bolt(&mut commands, &look, &mut timers, a, b, false);
        timers.floor = 0.5 + timers.random() * 1.1;
    }

    // Desks: a pulse from the core to each working desk now and then; a green pulse
    // back when its robot finishes, a red discharge when it fails.
    let count = crate::world3d::DESKS.len();
    timers.pulses.resize(count, 0.0);
    timers.desks.resize(count, DeskState::Free);
    let from = command + Vec3::Y * 0.06;
    for desk in &desks {
        let index = desk.desk as usize;
        let at = Spot::Desk(desk.desk).center() + Vec3::new(0.0, 0.06, 0.3);
        let before = timers.desks[index];
        timers.desks[index] = desk.shown;
        match desk.shown {
            DeskState::Working => {
                timers.pulses[index] -= dt;
                if timers.pulses[index] <= 0.0 {
                    timers.pulses[index] = PULSE_EVERY * (0.7 + timers.random() * 0.6);
                    spawn_pulse(&mut commands, &look, from, at, false);
                }
            }
            DeskState::Done if before != DeskState::Done => spawn_pulse(&mut commands, &look, at, from, true),
            DeskState::Failed if before != DeskState::Failed => {
                for _ in 0..3 {
                    let a = on_ring(&mut timers, at, 1.5);
                    let b = on_ring(&mut timers, at + Vec3::Y * 0.8, 0.9);
                    spawn_bolt(&mut commands, &look, &mut timers, a, b, true);
                }
            }
            _ => {}
        }
    }

    // Sparks round the base of a station when its robot starts a tool call.
    for session in model.live() {
        let agents = std::iter::once((Who::Main, session.main_work.calls.len()))
            .chain(session.agents.iter().map(|(id, info)| (Who::Sub(id.clone()), info.work.calls.len())));
        for (who, calls) in agents {
            let seen = timers.calls.insert((session.id.clone(), who.clone()), calls).unwrap_or(calls);
            if calls <= seen {
                continue;
            }
            let spot = match &who {
                Who::Main => Some(Spot::Command),
                Who::Sub(_) => avatars
                    .iter()
                    .find(|avatar| avatar.session == session.id && avatar.who == who)
                    .and_then(|avatar| avatar.desk)
                    .map(Spot::Desk),
            };
            let Some(spot) = spot else { continue };
            let (base, radius) = match spot {
                Spot::Command => (spot.center() + Vec3::Y * 0.4, CORE_RADIUS + 0.4),
                Spot::Desk(_) => (spot.center() + Vec3::new(0.0, 0.08, 0.3), 1.45),
            };
            let a = on_ring(&mut timers, base, radius);
            let b = on_ring(&mut timers, base, radius);
            spawn_bolt(&mut commands, &look, &mut timers, a, b + Vec3::Y * 0.3, false);
        }
    }
}
