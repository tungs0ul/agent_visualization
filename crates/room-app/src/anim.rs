//! Everything that moves (PLAN.md 4.2–4.4): character poses and blinking, walking,
//! speech bubbles, and a sub-agent walking its report over to Claude.
//!
//! The app only redraws while [`Animating`] is true, so all of this costs nothing while
//! Claude is idle.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use bevy::text::{LineBreak, TextBounds, TextLayoutInfo};
use bevy_prototype_lyon::prelude::*;
use room_proto::describe::truncate;

use crate::model::Pose;
use crate::scene::{TEXT, UiFont, rounded};

/// Walking speed in room units per second.
const WALK_SPEED: f32 = 340.0;
/// Long walks across the office are capped at this, so agents never dawdle.
const MAX_WALK_SECONDS: f32 = 2.2;
/// Distance between the dots of a walk's path.
const TRAIL_SPACING: f32 = 22.0;
/// Path dots lie on the floor: above the zone rugs, below every character and desk.
const TRAIL_Z: f32 = -5.0;
const BUBBLE_FONT: f32 = 17.0;
const BUBBLE_MAX_CHARS: usize = 42;
/// Longer sentences wrap onto a second line instead of making a very wide bubble.
const BUBBLE_TEXT_MAX_WIDTH: f32 = 260.0;
/// Space between the text and the bubble's edge.
const BUBBLE_PAD: Vec2 = Vec2::new(12.0, 5.0);
/// A bubble is never smaller than this; its bottom edge stays where the tail starts.
const BUBBLE_MIN_SIZE: Vec2 = Vec2::new(44.0, 32.0);
/// Typewriter speed of speech bubbles.
const CHARS_PER_SECOND: f32 = 45.0;
/// After this long a bubble dims; it stays until the agent says something else.
const BUBBLE_FRESH: f32 = 4.0;
const BUBBLE_DIM_ALPHA: f32 = 0.9;
/// How long the paper takes to pass from the sub-agent's hand into Claude's.
const HAND_OVER_SECONDS: f32 = 0.45;
const PAPER_Z: f32 = 60.0;
/// A finished sub-agent lingers this long after handing over, then walks out.
const LINGER_SECONDS: f32 = 0.7;

const BUBBLE_FILL: Color = Color::WHITE;
pub(crate) const BUBBLE_EDGE: Color = Color::srgb(0.886, 0.835, 0.765);

/// Per-character offset so characters don't breathe and blink in sync.
#[derive(Component)]
pub struct Phase(pub f32);

/// One animated piece of a character.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Body,
    Head,
    Eye,
    ArmLeft,
    ArmRight,
    Badge,
    /// The report, held up while walking it over to Claude.
    Paper,
    /// An open book, held while reading files.
    Book,
}

/// The character root a part belongs to.
#[derive(Component)]
pub struct RigOf(pub Entity);

/// The part's transform at rest; animation is applied on top of it.
#[derive(Component)]
pub struct Rest(pub Transform);

#[derive(Clone, Copy)]
pub enum Arrive {
    Sit,
    /// Stand at a spot in a zone of the office.
    Stand(Place),
    /// Hand the report to Claude, whose hand is at `to`.
    HandOver { to: Vec2 },
    /// Leave the room: despawn the character and its desk.
    Despawn { desk: Entity },
}

/// Where in the office an agent can be (PLAN.md 4.4b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Desk,
    Library,
    Servers,
    WebBay,
    Approval,
    Claude,
    Door,
    /// The room for work in other repositories (PLAN.md M4d).
    Away,
}

impl Place {
    pub fn name(self) -> &'static str {
        match self {
            Place::Desk => "Desk",
            Place::Library => "Library",
            Place::Servers => "Servers",
            Place::WebBay => "Window",
            Place::Approval => "Approval",
            Place::Claude => "Claude",
            Place::Door => "Door",
            Place::Away => "Other repos",
        }
    }
}

/// Where a sub-agent is now, and since when (seconds since the app started).
#[derive(Component, Clone, Copy)]
pub struct Station {
    pub place: Place,
    pub since: f32,
}

/// A walk in two straight legs through `via`, so characters go around their desk
/// (which stands in front of the seat) instead of through it.
#[derive(Component)]
pub struct Walk {
    from: Vec2,
    via: Vec2,
    to: Vec2,
    elapsed: f32,
    duration: f32,
    then: Arrive,
    /// Dots showing the way, with how far along the walk each one lies; each dot
    /// disappears once the walker passes it.
    trail: Vec<(Entity, f32)>,
    trail_drawn: bool,
}

impl Walk {
    pub fn new(from: Vec2, via: Vec2, to: Vec2, then: Arrive) -> Self {
        let length = from.distance(via) + via.distance(to);
        let duration = (length / WALK_SPEED).clamp(0.5, MAX_WALK_SECONDS);
        Self { from, via, to, elapsed: 0.0, duration, then, trail: Vec::new(), trail_drawn: false }
    }

    fn carries_report(&self) -> bool {
        matches!(self.then, Arrive::HandOver { .. })
    }

    /// Where this walk ends.
    pub fn destination(&self) -> Place {
        match self.then {
            Arrive::Sit => Place::Desk,
            Arrive::Stand(place) => place,
            Arrive::HandOver { .. } => Place::Claude,
            Arrive::Despawn { .. } => Place::Door,
        }
    }

    /// Position at `eased` ∈ [0, 1] of the way.
    ///
    /// The character's z never changes: Bevy 0.20 retains 2D draw items and computes
    /// their sort key once, when they are first queued, so changing z later doesn't
    /// re-sort them. Walkers keep seat depth and pass behind other desks.
    fn at(&self, eased: f32) -> Vec2 {
        let first = self.from.distance(self.via);
        let total = first + self.via.distance(self.to);
        let travelled = eased * total;
        if travelled <= first {
            self.from.lerp(self.via, travelled / first.max(f32::EPSILON))
        } else {
            let second = (travelled - first) / (total - first).max(f32::EPSILON);
            self.via.lerp(self.to, second)
        }
    }
}

/// A finished sub-agent: once it stands still (report handed over), it lingers a
/// moment and walks out the door.
#[derive(Component)]
pub struct Done {
    pub desk: Entity,
    pub door: Vec2,
    lingered: f32,
}

impl Done {
    pub fn new(desk: Entity, door: Vec2) -> Self {
        Self { desk, door, lingered: 0.0 }
    }
}

/// A character's color (for the report sheet and its pulse).
#[derive(Component, Clone, Copy)]
pub struct Accent(pub Color);

/// Points at the speech bubble that belongs to a character.
#[derive(Component)]
pub struct BubbleRef(pub Entity);

#[derive(Component)]
pub struct Bubble {
    text: String,
    revealed: f32,
    age: f32,
    background: Entity,
    tail: Entity,
    label: Entity,
    /// The not yet typed rest of the sentence: laid out but transparent, so the label's
    /// layout size is the whole sentence's from the first letter on.
    rest: Entity,
    /// Current size of the background shape.
    size: Vec2,
    /// Height above the character's feet at normal size (see `keep_text_readable`).
    base_y: f32,
}

/// The report passing from the sub-agent's hand into Claude's.
#[derive(Component)]
pub struct HandOver {
    from: Vec2,
    to: Vec2,
    elapsed: f32,
    accent: Color,
}

#[derive(Component)]
pub struct Pulse {
    age: f32,
    color: Color,
}

/// True while anything on screen moves by itself (read by the redraw pacing).
#[derive(Resource, Default, PartialEq)]
pub struct Animating(pub bool);

/// Spawns an empty, hidden speech bubble above a character and returns its root.
pub fn spawn_bubble(commands: &mut Commands, character: Entity, height: f32, font: &UiFont) -> Entity {
    let root = commands
        .spawn((Transform::from_xyz(0.0, height, 30.0), Visibility::Hidden, ChildOf(character)))
        .id();
    let background = commands
        .spawn((rounded(Vec2::new(40.0, 32.0), 11.0, BUBBLE_FILL, Some((BUBBLE_EDGE, 1.5))), ChildOf(root)))
        .id();
    let tail_shape = shapes::Polygon {
        points: vec![Vec2::new(-6.0, 0.0), Vec2::new(6.0, 0.0), Vec2::new(0.0, -8.0)],
        closed: true,
    };
    let tail = commands
        .spawn((
            ShapeBuilder::with(&tail_shape).fill(BUBBLE_FILL).build(),
            Transform::from_xyz(0.0, -15.5, 1.0),
            ChildOf(root),
        ))
        .id();
    let label = commands
        .spawn((
            Text2d::default(),
            TextFont::from_font_size(BUBBLE_FONT).with_font(font.0.clone()),
            TextColor(TEXT),
            TextLayout::new(Justify::Center, LineBreak::WordOrCharacter),
            TextBounds::new_horizontal(BUBBLE_TEXT_MAX_WIDTH),
            Transform::from_xyz(0.0, 0.0, 2.0),
            ChildOf(root),
        ))
        .id();
    let rest = commands
        .spawn((
            TextSpan::default(),
            TextFont::from_font_size(BUBBLE_FONT).with_font(font.0.clone()),
            TextColor(Color::NONE),
            ChildOf(label),
        ))
        .id();
    commands.entity(root).insert(Bubble {
        text: String::new(),
        revealed: 0.0,
        age: 0.0,
        background,
        tail,
        label,
        rest,
        size: Vec2::ZERO,
        base_y: height,
    });
    root
}

/// Makes a character say something: restarts its bubble with the new text.
pub fn say(bubbles: &mut Query<&mut Bubble>, bubble: Entity, text: &str) {
    if let Ok(mut bubble) = bubbles.get_mut(bubble) {
        bubble.text = truncate(text.trim(), BUBBLE_MAX_CHARS);
        bubble.revealed = 0.0;
        bubble.age = 0.0;
    }
}

/// An open book: two pages with a few lines, in the reader's color.
pub fn book(accent: Color) -> Shape {
    let page = |x: f32| shapes::Rectangle {
        extents: Vec2::new(17.0, 22.0),
        origin: RectangleOrigin::CustomCenter(Vec2::new(x, 0.0)),
        radii: Some(BorderRadii::single(2.0)),
    };
    let line = |x: f32, y: f32| shapes::Rectangle {
        extents: Vec2::new(10.0, 0.1),
        origin: RectangleOrigin::CustomCenter(Vec2::new(x, y)),
        radii: None,
    };
    ShapeBuilder::with(&page(-9.0))
        .add(&page(9.0))
        .add(&line(-9.0, 5.0))
        .add(&line(-9.0, 0.0))
        .add(&line(9.0, 5.0))
        .add(&line(9.0, 0.0))
        .fill(Color::WHITE)
        .stroke((accent, 1.5))
        .build()
}

/// The report sheet: white paper with a few lines in the agent's color.
pub fn paper(accent: Color) -> Shape {
    let sheet = shapes::Rectangle {
        extents: Vec2::new(14.0, 18.0),
        origin: RectangleOrigin::Center,
        radii: Some(BorderRadii::single(2.0)),
    };
    // Text lines: hairline rectangles, drawn by the accent stroke.
    let line = |y: f32| shapes::Rectangle { extents: Vec2::new(8.0, 0.1), origin: RectangleOrigin::CustomCenter(Vec2::new(0.0, y)), radii: None };
    ShapeBuilder::with(&sheet)
        .add(&line(4.0))
        .add(&line(0.0))
        .add(&line(-4.0))
        .fill(Color::WHITE)
        .stroke((accent, 1.2))
        .build()
}

/// Poses, breathing, blinking, typing arms, walking bob, the waiting badge.
pub fn animate_parts(
    time: Res<Time>,
    rigs: Query<(&Pose, &Phase, Option<&Walk>)>,
    mut parts: Query<(&Part, &Rest, &RigOf, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (part, rest, rig, mut transform) in &mut parts {
        let Ok((pose, phase, walk)) = rigs.get(rig.0) else { continue };
        let walking = walk.is_some();
        let carrying = walk.is_some_and(Walk::carries_report);
        let t = now + phase.0;
        let mut next = rest.0;
        let bob = if walking { 3.0 * (t * 10.0).sin().abs() } else { 0.0 };
        match part {
            Part::Body => {
                next.translation.y += bob;
                next.scale.y *= 1.0 + 0.025 * (t * 2.2).sin();
            }
            Part::Head => {
                next.translation.y += bob + 0.8 * (t * 2.2).sin();
                let tilt = match pose {
                    Pose::Reading => 0.12 * (t * 1.4).sin(),
                    Pose::Thinking => 0.06 * (t * 0.9).sin(),
                    Pose::Web => 0.08,
                    Pose::Delegating | Pose::Reporting => -0.1,
                    _ => 0.03 * (t * 0.7).sin(),
                };
                next.rotation = Quat::from_rotation_z(tilt);
            }
            Part::Eye => {
                // Blink for 0.12 s roughly every 3.7 s.
                if t % 3.7 < 0.12 {
                    next.scale.y *= 0.15;
                }
                match pose {
                    Pose::Reading => next.translation.x += 1.8 * (t * 3.0).sin(),
                    Pose::Web => next.translation.y += 1.5,
                    Pose::Typing | Pose::Terminal => next.translation.y -= 1.2,
                    _ => {}
                }
            }
            Part::ArmLeft | Part::ArmRight => {
                let side = if *part == Part::ArmLeft { 0.0 } else { PI };
                next.translation.y += bob;
                if carrying && *part == Part::ArmRight {
                    // Holding the report out in front.
                    next.translation += Vec3::new(-2.0, 8.0, 0.0);
                    next.rotation = Quat::from_rotation_z(0.6);
                } else if walking {
                    next.translation.y += 3.0 * (t * 10.0 + side).sin();
                } else if matches!(pose, Pose::Typing | Pose::Terminal) {
                    next.translation.y += 2.5 * (t * 16.0 + side).sin();
                } else if matches!(pose, Pose::Delegating | Pose::Reporting) && *part == Part::ArmRight {
                    next.translation.y += 14.0;
                    next.rotation = Quat::from_rotation_z(0.5);
                }
            }
            Part::Book => {
                if *pose == Pose::Reading && !walking {
                    // Turning pages: a slow tilt.
                    next.rotation = Quat::from_rotation_z(0.06 * (t * 1.3).sin());
                } else {
                    next.scale = Vec3::ZERO;
                }
            }
            Part::Paper => {
                if carrying {
                    next.translation.y += bob;
                } else {
                    next.scale = Vec3::ZERO;
                }
            }
            Part::Badge => {
                if *pose == Pose::Waiting {
                    next.translation.y += 4.0 * (t * 5.0).sin().abs();
                } else {
                    next.scale = Vec3::ZERO;
                }
            }
        }
        if *transform != next {
            *transform = next;
        }
    }
}

pub fn walk(
    mut commands: Commands,
    time: Res<Time>,
    mut walkers: Query<(Entity, &mut Transform, &mut Walk, &Accent)>,
) {
    for (entity, mut transform, mut walk, accent) in &mut walkers {
        if !walk.trail_drawn {
            walk.trail_drawn = true;
            walk.trail = draw_trail(&mut commands, &walk, accent.0);
        }
        walk.elapsed += time.delta_secs();
        let progress = (walk.elapsed / walk.duration).min(1.0);
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let at = walk.at(eased);
        transform.translation.x = at.x;
        transform.translation.y = at.y;
        walk.trail.retain(|&(dot, along)| {
            let ahead = along > eased;
            if !ahead {
                commands.entity(dot).despawn();
            }
            ahead
        });
        if progress < 1.0 {
            continue;
        }
        for (dot, _) in walk.trail.drain(..) {
            commands.entity(dot).despawn();
        }
        let arrived = Station { place: walk.destination(), since: time.elapsed_secs() };
        match walk.then {
            Arrive::Sit | Arrive::Stand(_) => {
                commands.entity(entity).remove::<Walk>().insert(arrived);
            }
            Arrive::HandOver { to } => {
                commands.entity(entity).remove::<Walk>().insert(arrived);
                let from = transform.translation.truncate() + Vec2::new(0.0, 24.0);
                commands.spawn((
                    paper(accent.0),
                    Transform::from_translation(from.extend(PAPER_Z)),
                    HandOver { from, to, elapsed: 0.0, accent: accent.0 },
                ));
            }
            Arrive::Despawn { desk } => {
                commands.entity(entity).despawn();
                commands.entity(desk).despawn();
            }
        }
    }
}

/// Faint dots along the whole walk, in the walker's color.
fn draw_trail(commands: &mut Commands, walk: &Walk, accent: Color) -> Vec<(Entity, f32)> {
    let total = walk.from.distance(walk.via) + walk.via.distance(walk.to);
    let dots = (total / TRAIL_SPACING) as usize;
    let dot = ShapeBuilder::with(&shapes::Circle { radius: 2.6, center: Vec2::ZERO })
        .fill(accent.with_alpha(0.45))
        .build();
    (1..dots)
        .map(|i| {
            let along = i as f32 / dots as f32;
            let at = walk.at(along);
            let entity = commands.spawn((dot.clone(), Transform::from_translation(at.extend(TRAIL_Z)))).id();
            (entity, along)
        })
        .collect()
}

/// The small "to Library" line under a walking agent.
#[derive(Component)]
pub struct Chip;

pub fn update_chips(
    walkers: Query<Option<&Walk>>,
    mut chips: Query<(&RigOf, &mut Text2d, &mut Visibility), With<Chip>>,
) {
    for (rig, mut text, mut visibility) in &mut chips {
        let Ok(walk) = walkers.get(rig.0) else { continue };
        match walk {
            Some(walk) => {
                let label = format!("to {}", walk.destination().name());
                if text.0 != label {
                    text.0 = label;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// Finished sub-agents walk out once they stand still (report delivered).
pub fn leave_when_done(
    mut commands: Commands,
    time: Res<Time>,
    mut finished: Query<(Entity, &Transform, &mut Done), Without<Walk>>,
) {
    for (entity, transform, mut done) in &mut finished {
        done.lingered += time.delta_secs();
        if done.lingered >= LINGER_SECONDS {
            let here = transform.translation.truncate();
            let walk = Walk::new(here, here, done.door, Arrive::Despawn { desk: done.desk });
            commands.entity(entity).remove::<Done>().insert(walk);
        }
    }
}

/// Bubble size for text whose laid-out size is `text` (in room units).
fn bubble_size(text: Vec2) -> Vec2 {
    (text + 2.0 * BUBBLE_PAD).max(BUBBLE_MIN_SIZE).ceil()
}

/// Typewriter reveal, sizing to the text, dimming after a few seconds.
///
/// The label always lays out the whole sentence (the untyped rest is transparent), so
/// the bubble is sized from Bevy's real text layout up front and doesn't grow while
/// typing. The layout lands a frame after the text changes.
pub fn update_bubbles(
    time: Res<Time>,
    mut bubbles: Query<(&mut Bubble, &mut Visibility)>,
    mut shapes: Query<(&mut Shape, &mut Transform)>,
    mut labels: Query<(&mut Text2d, &mut TextColor, &TextLayoutInfo, &mut Transform), Without<Shape>>,
    mut rests: Query<&mut TextSpan>,
) {
    let dt = time.delta_secs();
    for (mut bubble, mut visibility) in &mut bubbles {
        if bubble.text.is_empty() {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);
        let total = bubble.text.chars().count();
        if bubble.revealed >= total as f32 && bubble.age > BUBBLE_FRESH + 0.5 {
            continue;
        }
        bubble.revealed = (bubble.revealed + CHARS_PER_SECOND * dt).min(total as f32);
        bubble.age += dt;
        let alpha = if bubble.age <= BUBBLE_FRESH {
            1.0
        } else {
            1.0 - (1.0 - BUBBLE_DIM_ALPHA) * ((bubble.age - BUBBLE_FRESH) / 0.5).min(1.0)
        };

        let shown = bubble.revealed.ceil() as usize;
        let split = bubble.text.char_indices().nth(shown).map_or(bubble.text.len(), |(i, _)| i);
        let (typed, rest) = bubble.text.split_at(split);
        if let Ok(mut span) = rests.get_mut(bubble.rest)
            && span.0 != rest
        {
            span.0 = rest.to_owned();
        }
        let Ok((mut text, mut color, layout, mut label_transform)) = labels.get_mut(bubble.label) else {
            continue;
        };
        if text.0 != typed {
            text.0 = typed.to_owned();
        }
        // Write only on change: any write re-shapes the text (that cost a third of a
        // CPU core at 30 fps).
        let wanted = TEXT.with_alpha(alpha);
        if color.0 != wanted {
            color.0 = wanted;
        }
        if layout.size == Vec2::ZERO || layout.scale_factor <= 0.0 {
            continue;
        }
        let size = bubble_size(layout.size / layout.scale_factor);
        if size == bubble.size && alpha >= 1.0 {
            continue;
        }
        bubble.size = size;
        // Taller bubbles (wrapped text) grow upwards so the bottom edge meets the tail.
        let centre = (size.y - BUBBLE_MIN_SIZE.y) / 2.0;
        label_transform.translation.y = centre;
        if let Ok((mut shape, mut transform)) = shapes.get_mut(bubble.background) {
            *shape = rounded(
                size,
                9.0,
                BUBBLE_FILL.with_alpha(alpha),
                Some((BUBBLE_EDGE.with_alpha(alpha), 1.5)),
            );
            transform.translation.y = centre;
        }
        if let Ok((mut shape, _)) = shapes.get_mut(bubble.tail) {
            shape.fill = Some(Fill::color(BUBBLE_FILL.with_alpha(alpha)));
        }
    }
}

/// Bubbles and walk chips live in the room, so zooming out would shrink their text
/// until it can't be read. Scale them up by the camera's room units per screen pixel
/// so they never look smaller than their normal size. Zooming in still enlarges them.
/// `units_per_pixel` is computed by `scene::follow_action` for the frame being drawn.
pub fn keep_text_readable(
    zoom: Res<crate::scene::Zoom>,
    mut bubbles: Query<(&Bubble, &mut Transform), Without<Chip>>,
    mut chips: Query<&mut Transform, (With<Chip>, Without<Bubble>)>,
) {
    let scale = zoom.units_per_pixel.clamp(1.0, MAX_TEXT_SCALE);
    for (bubble, mut transform) in &mut bubbles {
        // Grow upwards from the tail's tip (8 + 15.5 below the centre), so the bubble
        // never covers the head.
        let y = bubble.base_y + BUBBLE_TAIL_TIP * (scale - 1.0);
        if transform.scale.x != scale || transform.translation.y != y {
            transform.scale = Vec3::splat(scale);
            transform.translation.y = y;
        }
    }
    for mut transform in &mut chips {
        if transform.scale.x != scale {
            transform.scale = Vec3::splat(scale);
        }
    }
}

/// Beyond this the bubbles would cover most of the room.
const MAX_TEXT_SCALE: f32 = 4.0;

/// Distance from a bubble's centre down to the tip of its tail.
const BUBBLE_TAIL_TIP: f32 = 23.5;

/// The paper moves in a small arc from one hand to the other; a ring pulses on arrival.
pub fn hand_over(mut commands: Commands, time: Res<Time>, mut sheets: Query<(Entity, &mut Transform, &mut HandOver)>) {
    for (entity, mut transform, mut sheet) in &mut sheets {
        sheet.elapsed += time.delta_secs();
        let s = (sheet.elapsed / HAND_OVER_SECONDS).min(1.0);
        let eased = s * s * (3.0 - 2.0 * s);
        let lift = 18.0 * (s * PI).sin();
        let at = sheet.from.lerp(sheet.to, eased) + Vec2::new(0.0, lift);
        transform.translation = at.extend(PAPER_Z);
        transform.rotation = Quat::from_rotation_z(0.4 * (s * PI).sin());
        if s >= 1.0 {
            commands.entity(entity).despawn();
            commands.spawn((
                ShapeBuilder::with(&shapes::Circle { radius: 30.0, center: Vec2::ZERO })
                    .stroke((sheet.accent, 3.0))
                    .build(),
                Transform::from_translation(sheet.to.extend(PAPER_Z - 1.0)),
                Pulse { age: 0.0, color: sheet.accent },
            ));
        }
    }
}

pub fn pulse(mut commands: Commands, time: Res<Time>, mut rings: Query<(Entity, &mut Pulse, &mut Shape, &mut Transform)>) {
    const LIFE: f32 = 0.7;
    for (entity, mut ring, mut shape, mut transform) in &mut rings {
        ring.age += time.delta_secs();
        let progress = ring.age / LIFE;
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        shape.stroke = Some(Stroke::new(ring.color.with_alpha(1.0 - progress), 3.0));
        transform.scale = Vec3::splat(0.6 + 1.6 * progress);
    }
}

/// Anything that is moving across the screen right now.
type InMotion = Or<(With<Walk>, With<Done>, With<HandOver>, With<Pulse>)>;

/// Keep redrawing while any character works or anything is in motion.
pub fn detect_motion(
    mut animating: ResMut<Animating>,
    poses: Query<&Pose>,
    bubbles: Query<&Bubble>,
    moving: Query<(), InMotion>,
) {
    let busy = poses.iter().any(|pose| *pose != Pose::Idle)
        || !moving.is_empty()
        || bubbles.iter().any(|b| !b.text.is_empty() && b.age <= BUBBLE_FRESH + 0.5);
    animating.set_if_neq(Animating(busy));
}

/// A stable offset in [0, 2π) from an agent id.
pub fn phase_for(key: &str) -> f32 {
    let hash = key.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    (hash % 1000) as f32 / 1000.0 * TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_fits_its_text_with_padding() {
        let text = Vec2::new(301.4, 23.0);
        let size = bubble_size(text);
        assert!(size.x >= text.x + 2.0 * BUBBLE_PAD.x);
        assert!(size.y >= text.y + 2.0 * BUBBLE_PAD.y);
        assert_eq!(size, size.ceil());
    }

    #[test]
    fn short_text_keeps_the_minimum_bubble() {
        assert_eq!(bubble_size(Vec2::new(5.0, 10.0)), BUBBLE_MIN_SIZE);
    }

    #[test]
    fn wrapped_text_makes_a_taller_bubble() {
        let one_line = bubble_size(Vec2::new(200.0, 23.0));
        let two_lines = bubble_size(Vec2::new(BUBBLE_TEXT_MAX_WIDTH, 46.0));
        assert!(two_lines.y > one_line.y);
        assert!(two_lines.x <= BUBBLE_TEXT_MAX_WIDTH + 2.0 * BUBBLE_PAD.x);
    }
}
