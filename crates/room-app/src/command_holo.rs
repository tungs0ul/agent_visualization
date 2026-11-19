//! Claude's big hologram (the user, 2026-11-19, prototype
//! `tools/blender/command_holo_proto.py`): a curved see-through wall behind its robot
//! on the command dais, in place of the old console. It shows what Claude is doing:
//! a status header, the task in large text, the current step, chips of the recent
//! tools and a small bar chart of how long they took, beside a ring emblem that
//! turns while Claude works.
//!
//! The frame, glass, beams and emblem are meshes built here once. The writing is
//! Bevy UI rendered into a texture by a camera of its own that is switched on only
//! for a few frames after the shown content changes: a still hologram costs nothing.
//! Text uses the Mac's system fonts by family name (bevy's `system_font_discovery`),
//! which cover Vietnamese, German and any other script the system has.

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{PrimitiveTopology, TextureFormat};
use bevy::text::FontSource;

use crate::model::{Model, Pose, SessionState};
use crate::world3d::{Kit, Spot};

/// The glass: radius round the dais centre, the arc (degrees, 0 = straight back,
/// negative = left), bottom and top above the floor. As in the prototype.
const R: f32 = 1.75;
const A0: f32 = -52.0;
const A1: f32 = 52.0;
const Y0: f32 = 1.3;
const Y1: f32 = 3.0;
/// The projectors on the dais's middle tier (`stations.py` `build_main_desk`): angle,
/// radius, top of the lens.
const PROJECTORS: [f32; 3] = [-40.0, 0.0, 40.0];
const PROJECTOR_R: f32 = 1.3;
const PROJECTOR_TOP: f32 = 0.35;
/// The emblem's place on the wall (angle, height) and how far in front of the glass.
const EMBLEM: (f32, f32, f32) = (-38.0, 2.22, 0.25);

/// The texture the writing is rendered into: about 640 px per metre of glass.
const TEX_W: u32 = 2048;
const TEX_H: u32 = 1088;
/// Frames the text camera renders after a change (layout and glyphs settle in one;
/// the rest is margin).
const RENDER_FRAMES: u8 = 3;
/// Recent tools shown as chips, and calls shown as bars.
const CHIPS: usize = 5;
const BARS: usize = 10;

/// The fonts, best first; the system falls back per character for scripts they lack.
const FONTS: &str = "'Avenir Next', 'Helvetica Neue', 'Arial Unicode MS', sans-serif";

const WHITE: Color = Color::srgb(0.86, 0.97, 1.0);
const CYAN: Color = Color::srgb(0.35, 0.85, 1.0);
const DIM: Color = Color::srgba(0.3, 0.6, 1.0, 0.75);
const AMBER: Color = Color::srgb(1.0, 0.62, 0.25);
const RED: Color = Color::srgb(1.0, 0.42, 0.36);

/// A point on the wall: `a` degrees round the dais centre, `y` above the floor, `r`
/// from the centre.
fn on_arc(a: f32, y: f32, r: f32) -> Vec3 {
    let a = a.to_radians();
    Spot::Command.center() + Vec3::new(r * a.sin(), y, -r * a.cos())
}

/// The top middle of the glass: what must stay visible over the dais (the HUD's
/// command panel and the plan board keep clear of it).
pub fn top() -> Vec3 {
    on_arc(0.0, Y1, R)
}

/// A ribbon on the cylinder of radius `r` from `a0` to `a1` degrees and `y0` to `y1`
/// metres, facing the dais centre. UV: u left to right, v top to bottom.
fn arc_strip(r: f32, a0: f32, a1: f32, y0: f32, y1: f32, segs: u32) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let centre = Spot::Command.center();
    for i in 0..=segs {
        let t = i as f32 / segs as f32;
        let a = a0 + (a1 - a0) * t;
        for (y, v) in [(y0, 1.0), (y1, 0.0)] {
            positions.push((on_arc(a, y, r) - centre).to_array());
            let rad = a.to_radians();
            normals.push([-rad.sin(), 0.0, rad.cos()]);
            uvs.push([t, v]);
        }
        if i > 0 {
            let k = 2 * i;
            indices.extend_from_slice(&[k - 2, k, k - 1, k - 1, k, k + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// A soft light beam from a projector's lens fanning up to the bottom of the glass:
/// bright at the lens, fading on the way up (vertex colours).
fn beam(a: f32) -> Mesh {
    let centre = Spot::Command.center();
    let lens = on_arc(a, PROJECTOR_TOP, PROJECTOR_R) - centre;
    let side = Vec3::new(a.to_radians().cos(), 0.0, a.to_radians().sin()) * 0.04;
    let positions = vec![
        (lens - side).to_array(),
        (lens + side).to_array(),
        (on_arc(a + 16.0, Y0, R - 0.01) - centre).to_array(),
        (on_arc(a - 16.0, Y0, R - 0.01) - centre).to_array(),
    ];
    let normal = [-a.to_radians().sin(), 0.0, a.to_radians().cos()];
    let low = [1.0, 1.0, 1.0, 1.0];
    let high = [1.0, 1.0, 1.0, 0.35];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![low, low, high, high])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// One of the emblem's rings, its resting turn and the axis it turns about.
#[derive(Component)]
pub struct EmblemRing {
    rest: Quat,
    axis: Vec3,
    speed: f32,
}

/// The camera that renders the writing into the texture.
#[derive(Component)]
pub struct HoloTextCamera;

/// The parts of the writing that change.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Status,
    Title,
    Task,
    Step,
    ChipLabel(usize),
    ChipLine(usize),
    Bar(usize),
}

/// Builds the hologram once: glass, frame, beams and emblem in the room, and the
/// texture with its camera and writing.
pub fn build(kit: &mut Kit, images: &mut Assets<Image>) {
    let centre = Spot::Command.center();
    let at = Transform::from_translation(centre);
    let unlit = |color: LinearRgba, alpha_mode: AlphaMode| StandardMaterial {
        base_color: Color::LinearRgba(color),
        unlit: true,
        alpha_mode,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    // Dark see-through glass.
    let glass = kit.materials.add(unlit(LinearRgba::new(0.008, 0.04, 0.13, 0.5), AlphaMode::Blend));
    kit.mesh(arc_strip(R, A0, A1, Y0, Y1, 64), glass, at);

    // The writing, added on the glass as light (premultiplied: the UI texture is).
    let image = images.add(Image::new_target_texture(TEX_W, TEX_H, TextureFormat::Bgra8UnormSrgb, None));
    let writing = kit.materials.add(StandardMaterial {
        base_color_texture: Some(image.clone()),
        ..unlit(LinearRgba::rgb(2.4, 2.4, 2.4), AlphaMode::Premultiplied)
    });
    kit.mesh(arc_strip(R - 0.012, A0, A1, Y0, Y1, 64), writing, at);

    // Glowing edges, and brighter corner brackets.
    let edge = kit.glow(LinearRgba::rgb(0.3, 2.6, 4.0));
    let bracket = kit.glow(LinearRgba::rgb(0.6, 5.0, 8.0));
    for y in [Y0, Y1] {
        kit.mesh(arc_strip(R - 0.005, A0, A1, y - 0.008, y + 0.008, 96), edge.clone(), at);
    }
    for a in [A0, A1] {
        kit.mesh(arc_strip(R - 0.005, a - 0.35, a + 0.35, Y0, Y1, 1), edge.clone(), at);
    }
    for (a, s) in [(A0, 1.0), (A1, -1.0)] {
        for (y, t) in [(Y0, 1.0), (Y1, -1.0)] {
            let (b0, b1) = if s > 0.0 { (a, a + 5.0) } else { (a - 5.0, a) };
            kit.mesh(arc_strip(R - 0.012, b0, b1, y - 0.016, y + 0.016, 8), bracket.clone(), at);
            let (h0, h1) = if t > 0.0 { (y, y + 0.17) } else { (y - 0.17, y) };
            kit.mesh(arc_strip(R - 0.012, a - 0.7, a + 0.7, h0, h1, 1), bracket.clone(), at);
        }
    }

    // Soft beams from the projectors up to the glass.
    let light = kit.materials.add(unlit(LinearRgba::rgb(0.03, 0.16, 0.42), AlphaMode::Add));
    for a in PROJECTORS {
        kit.mesh(beam(a), light.clone(), at);
    }

    // The emblem: three rings round an orange core, facing the dais centre.
    let (ea, ey, ein) = EMBLEM;
    let emblem = kit
        .commands
        .spawn((Transform::from_translation(on_arc(ea, ey, R - ein)).with_rotation(Quat::from_rotation_y(-ea.to_radians())), Visibility::default()))
        .id();
    let ring_glows = [LinearRgba::rgb(0.6, 5.0, 8.0), LinearRgba::rgb(0.3, 2.6, 4.0), LinearRgba::rgb(0.1, 0.9, 4.0)];
    let rings = [
        (0.26, Quat::from_rotation_x(PI / 2.0), Vec3::Y, 0.5),
        (0.22, Quat::from_rotation_x(70f32.to_radians()) * Quat::from_rotation_z(25f32.to_radians()), Vec3::new(1.0, 1.0, 0.0).normalize(), -0.8),
        (0.18, Quat::from_rotation_x(110f32.to_radians()) * Quat::from_rotation_y(-0.35), Vec3::new(-0.4, 1.0, 0.6).normalize(), 1.2),
    ];
    for ((radius, rest, axis, speed), glow) in rings.into_iter().zip(ring_glows) {
        let material = kit.glow(glow);
        let ring = kit.mesh(Torus { minor_radius: 0.007, major_radius: radius }, material, Transform::from_rotation(rest));
        kit.commands.entity(ring).insert((EmblemRing { rest, axis, speed }, ChildOf(emblem)));
    }
    let core = kit.glow(LinearRgba::rgb(6.0, 2.6, 0.7));
    let core = kit.mesh(Sphere::new(0.06).mesh().ico(3).unwrap(), core, Transform::IDENTITY);
    kit.commands.entity(core).insert(ChildOf(emblem));

    spawn_writing(kit.commands, image);
}

/// Horizontal place on the texture of a point `a` degrees round the wall.
fn u(a: f32) -> f32 {
    (a - A0) / (A1 - A0) * TEX_W as f32
}

/// Vertical place on the texture of a point `y` metres above the floor.
fn v(y: f32) -> f32 {
    (Y1 - y) / (Y1 - Y0) * TEX_H as f32
}

fn font(size: f32) -> TextFont {
    TextFont { font: FontSource::families(FONTS), font_size: FontSize::Px(size), weight: FontWeight::MEDIUM, ..default() }
}

fn line(left: f32, top: f32, width: f32, height: f32, color: Color) -> (Node, BackgroundColor) {
    (
        Node { position_type: PositionType::Absolute, left: Val::Px(left), top: Val::Px(top), width: Val::Px(width), height: Val::Px(height), ..default() },
        BackgroundColor(color),
    )
}

/// A text box at a fixed place on the texture; text that doesn't fit is cut off.
fn text_box(left: f32, top: f32, width: f32, height: f32, justify: JustifyContent) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(width),
        height: Val::Px(height),
        justify_content: justify,
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    }
}

fn spawn_writing(commands: &mut Commands, image: Handle<Image>) {
    let camera = commands
        .spawn((
            HoloTextCamera,
            Camera2d,
            Camera { order: -2, is_active: true, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            RenderTarget::Image(image.into()),
            RenderLayers::none(),
        ))
        .id();
    let (w, h) = (TEX_W as f32, TEX_H as f32);
    let root = commands
        .spawn((Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, UiTargetCamera(camera)))
        .id();
    let mut add = |bundle: (Node, BackgroundColor)| {
        commands.spawn((bundle, ChildOf(root)));
    };
    // Faint scanlines, every seventh a little brighter, and the header rule.
    for k in 1..28 {
        let color = if k % 7 == 0 { Color::srgba(0.2, 0.6, 1.0, 0.22) } else { Color::srgba(0.2, 0.6, 1.0, 0.1) };
        add(line(w * 0.01, h * k as f32 / 28.0, w * 0.98, 2.0, color));
    }
    add(line(u(A0 + 3.0), v(Y1 - 0.2), u(A1 - 3.0) - u(A0 + 3.0), 4.0, Color::srgba(0.35, 0.85, 1.0, 0.85)));

    let header_top = v(Y1 - 0.04);
    let header_h = v(Y1 - 0.2) - header_top;
    commands.spawn((Slot::Status, text_box(u(-49.0), header_top, w * 0.4, header_h, JustifyContent::Start), Text::new(""), font(50.0), TextColor(AMBER), TextLayout::no_wrap(), ChildOf(root)));
    commands.spawn((Slot::Title, text_box(u(49.0) - w * 0.55, header_top, w * 0.55, header_h, JustifyContent::End), Text::new(""), font(46.0), TextColor(CYAN), TextLayout::no_wrap(), ChildOf(root)));
    // The task, large, over the emblem and the chart; two lines at most.
    commands.spawn((
        Slot::Task,
        text_box(u(-46.0), v(2.78), u(46.0) - u(-46.0), v(2.28) - v(2.78), JustifyContent::Center),
        Text::new(""),
        font(100.0),
        TextColor(WHITE),
        TextLayout::new(Justify::Center, LineBreak::WordBoundary),
        ChildOf(root),
    ));
    // The current step, between the emblem and the chart.
    commands.spawn((
        Slot::Step,
        text_box(u(-26.0), v(2.2), u(26.0) - u(-26.0), v(1.98) - v(2.2), JustifyContent::Center),
        Text::new(""),
        font(62.0),
        TextColor(CYAN),
        TextLayout::no_wrap(),
        ChildOf(root),
    ));
    // Tool chips along the bottom: a rule over and under each label.
    for i in 0..CHIPS {
        let a = -30.0 + 15.0 * i as f32;
        let (left, width) = (u(a - 6.0), u(a + 6.0) - u(a - 6.0));
        for y in [1.78, 1.6] {
            commands.spawn((Slot::ChipLine(i), line(left, v(y), width, 5.0, Color::NONE), ChildOf(root)));
        }
        commands.spawn((
            Slot::ChipLabel(i),
            text_box(left - 20.0, v(1.77), width + 40.0, v(1.61) - v(1.77), JustifyContent::Center),
            Text::new(""),
            font(44.0),
            TextColor(CYAN),
            TextLayout::no_wrap(),
            ChildOf(root),
        ));
    }
    // The bar chart, right of the step: how long each recent call took.
    for i in 0..BARS {
        let a = 30.0 + 1.6 * i as f32;
        commands.spawn((Slot::Bar(i), line(u(a), v(1.88), u(a + 0.9) - u(a), 0.0, Color::NONE), ChildOf(root)));
    }
}

/// What the hologram shows. Rendered again only when this changes.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Shown {
    pub status: String,
    pub status_color: [u8; 3],
    pub title: String,
    pub task: String,
    pub step: String,
    /// Recent tools, oldest first: label and whether it is the live one.
    pub chips: Vec<(String, bool)>,
    /// Recent calls, oldest first: height 0..1 and whether it still runs.
    pub bars: Vec<(u8, bool)>,
}

fn rgb(color: Color) -> [u8; 3] {
    color.to_srgba().to_u8_array_no_alpha()
}

/// One line, spaces collapsed.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A tool's short name: `mcp__server__tool` shows as `tool`.
fn tool_name(name: &str) -> String {
    let short = name.rsplit("__").next().unwrap_or(name);
    room_proto::describe::truncate(short, 12)
}

impl Shown {
    pub fn of(session: Option<&SessionState>) -> Self {
        let Some(session) = session else {
            return Shown {
                status: "○ STANDBY".into(),
                status_color: rgb(DIM),
                title: "CLAUDE · MAIN AGENT".into(),
                task: "No session yet".into(),
                step: "Start Claude Code to begin".into(),
                ..default()
            };
        };
        let busy = session.main_pose != Pose::Idle;
        let (status, color) = if session.ended {
            ("■ ENDED", DIM)
        } else if session.main_pose == Pose::Waiting {
            ("● NEEDS YOU", AMBER)
        } else if session.main_failed {
            ("● ERROR", RED)
        } else if busy {
            ("● WORKING", AMBER)
        } else if session.is_busy() {
            ("● CREW WORKING", CYAN)
        } else {
            ("○ IDLE", DIM)
        };
        let model = session.model.as_deref().map(crate::log::short_model).and_then(|m| m.split(['-', '[']).next()).filter(|m| !m.is_empty());
        let title = match model {
            Some(model) => format!("CLAUDE · MAIN AGENT · {}", model.to_uppercase()),
            None => "CLAUDE · MAIN AGENT".into(),
        };
        let task = match session.prompts.last().map(|prompt| one_line(prompt)).filter(|prompt| !prompt.is_empty()) {
            Some(prompt) => room_proto::describe::truncate(&prompt, 96),
            None => "Waiting for a task".into(),
        };
        let step = match &session.main_doing {
            Some(doing) if busy => format!("▸ {}", room_proto::describe::truncate(&one_line(doing), 44)),
            _ if session.prompts.is_empty() => "▸ Ready".into(),
            _ if busy => "▸ Thinking".into(),
            _ => "▸ Waiting for your next prompt".into(),
        };
        // Runs of the same tool count as one chip ("Read ×3").
        let calls = &session.main_work.calls;
        let mut runs: Vec<(String, usize, bool)> = Vec::new();
        for call in calls {
            let running = call.outcome == crate::work::Outcome::Running;
            match runs.last_mut() {
                Some((name, count, live)) if *name == call.name => {
                    *count += 1;
                    *live = running;
                }
                _ => runs.push((call.name.clone(), 1, running)),
            }
        }
        let last = runs.len().saturating_sub(1);
        let chips = runs
            .iter()
            .enumerate()
            .skip(runs.len().saturating_sub(CHIPS))
            .map(|(index, (name, count, running))| {
                let label = if *count > 1 { format!("{} ×{count}", tool_name(name)) } else { tool_name(name) };
                (label, *running || (busy && index == last))
            })
            .collect();
        let bars = calls
            .iter()
            .skip(calls.len().saturating_sub(BARS))
            .map(|call| match &call.outcome {
                crate::work::Outcome::Running => (60, true),
                crate::work::Outcome::Done { took, .. } | crate::work::Outcome::Failed { took, .. } => {
                    let ms = took.as_millis() as f32;
                    ((((1.0 + ms).ln() / (1.0 + 60_000f32).ln()).clamp(0.08, 1.0) * 100.0) as u8, false)
                }
            })
            .collect();
        Shown { status: status.into(), status_color: rgb(color), title, task, step, chips, bars }
    }

    /// The task's font size: large, smaller for long tasks (which wrap to two lines).
    fn task_size(&self) -> f32 {
        match self.task.chars().count() {
            0..=30 => 104.0,
            31..=48 => 84.0,
            _ => 66.0,
        }
    }
}

/// Keeps the writing up to date. It compares what to show with what is shown and,
/// only on a change, rewrites the text and lets the text camera render a few frames;
/// otherwise the camera stays off and the texture keeps its last picture.
#[allow(clippy::type_complexity)]
pub fn update(
    model: Res<Model>,
    mut view: ResMut<crate::view3d::View3d>,
    mut shown: Local<Option<Shown>>,
    mut frames: Local<u8>,
    mut camera: Query<&mut Camera, With<HoloTextCamera>>,
    mut slots: Query<(&Slot, Option<&mut Text>, Option<&mut TextColor>, Option<&mut TextFont>, Option<&mut Node>, Option<&mut BackgroundColor>)>,
) {
    let Ok(mut camera) = camera.single_mut() else { return };
    if model.is_changed() || shown.is_none() {
        let wanted = Shown::of(model.shown());
        if shown.as_ref() != Some(&wanted) {
            write(&wanted, &mut slots);
            *shown = Some(wanted);
            *frames = RENDER_FRAMES;
        }
    }
    let active = *frames > 0;
    if camera.is_active != active {
        camera.is_active = active;
    }
    if active {
        *frames -= 1;
        // Make sure the frames are drawn even when the room dozes.
        view.moving = true;
    }
}

#[allow(clippy::type_complexity)]
fn write(shown: &Shown, slots: &mut Query<(&Slot, Option<&mut Text>, Option<&mut TextColor>, Option<&mut TextFont>, Option<&mut Node>, Option<&mut BackgroundColor>)>) {
    let srgb = |[r, g, b]: [u8; 3]| Color::srgb_u8(r, g, b);
    for (slot, text, color, font, node, background) in slots.iter_mut() {
        let set = |text: Option<Mut<Text>>, value: &str| {
            if let Some(mut text) = text
                && text.0 != value
            {
                text.0 = value.to_owned();
            }
        };
        match *slot {
            Slot::Status => {
                set(text, &shown.status);
                if let Some(mut color) = color {
                    color.0 = srgb(shown.status_color);
                }
            }
            Slot::Title => set(text, &shown.title),
            Slot::Task => {
                set(text, &shown.task);
                if let Some(mut font) = font {
                    font.font_size = FontSize::Px(shown.task_size());
                }
            }
            Slot::Step => set(text, &shown.step),
            Slot::ChipLabel(i) => {
                let chip = shown.chips.get(i);
                set(text, chip.map_or("", |(label, _)| label));
                if let Some(mut color) = color {
                    color.0 = if chip.is_some_and(|(_, live)| *live) { AMBER } else { CYAN };
                }
            }
            Slot::ChipLine(i) => {
                if let Some(mut background) = background {
                    background.0 = match shown.chips.get(i) {
                        Some((_, true)) => AMBER,
                        Some(_) => CYAN.with_alpha(0.85),
                        None => DIM.with_alpha(0.25),
                    };
                }
            }
            Slot::Bar(i) => {
                let bar = shown.bars.get(i);
                let height = bar.map_or(0.04, |(h, _)| *h as f32 / 100.0) * (v(1.88) - v(2.2));
                if let Some(mut node) = node {
                    node.top = Val::Px(v(1.88) - height);
                    node.height = Val::Px(height);
                }
                if let Some(mut background) = background {
                    background.0 = match bar {
                        Some((_, true)) => AMBER,
                        Some(_) if i % 4 == 3 => AMBER.with_alpha(0.8),
                        Some(_) => CYAN.with_alpha(0.9),
                        None => DIM.with_alpha(0.3),
                    };
                }
            }
        }
    }
}

/// Turns the emblem's rings while Claude works, and only then.
pub fn turn_emblem(model: Res<Model>, time: Res<Time>, mut angle: Local<f32>, mut rings: Query<(&EmblemRing, &mut Transform)>) {
    let busy = model.shown().is_some_and(|session| session.main_pose != Pose::Idle && !session.ended);
    if !busy {
        return;
    }
    *angle += time.delta_secs();
    for (ring, mut at) in &mut rings {
        at.rotation = Quat::from_axis_angle(ring.axis, *angle * ring.speed) * ring.rest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_session_shows_standby() {
        let shown = Shown::of(None);
        assert!(shown.status.contains("STANDBY"));
        assert!(shown.chips.is_empty());
    }

    #[test]
    fn long_tasks_are_cut_and_get_smaller() {
        let mut session = SessionState::default();
        session.prompts.push("Sửa lỗi phân tích cú pháp\nund füge Tests hinzu ".repeat(10));
        let shown = Shown::of(Some(&session));
        assert!(shown.task.chars().count() <= 96);
        assert!(shown.task.ends_with('…'));
        assert!(!shown.task.contains('\n'));
        assert_eq!(shown.task_size(), 66.0);
    }

    #[test]
    fn runs_of_one_tool_make_one_chip() {
        let mut session = SessionState { main_pose: Pose::Reading, ..Default::default() };
        for (id, name) in ["Grep", "Read", "Read", "Read", "Edit", "Bash", "mcp__github__get_issue"].iter().enumerate() {
            let tool = room_proto::event::ToolCall { name: (*name).into(), input: serde_json::json!({}) };
            session.main_work.on(&room_proto::event::EventKind::ToolStarted { tool_use_id: id.to_string(), tool });
        }
        let shown = Shown::of(Some(&session));
        let labels: Vec<&str> = shown.chips.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(labels, ["Grep", "Read ×3", "Edit", "Bash", "get_issue"]);
        assert!(shown.chips.last().unwrap().1, "the live tool is lit");
        assert_eq!(shown.status, "● WORKING");
        assert_eq!(shown.bars.len(), 7);
    }

    #[test]
    fn the_wall_is_behind_the_robot() {
        let robot = Spot::Command.work_spot(0).0;
        assert!(top().z < robot.z - 1.5);
        assert!((on_arc(A0, Y0, R).x + on_arc(A1, Y0, R).x).abs() < 1e-4);
    }
}
