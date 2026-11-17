//! Themes (PLAN.md M14): Light, Dark and Spaceship, or Auto (Light or Dark as macOS is).
//!
//! The room is drawn in the Light colours (`scene.rs`). Rather than threading a palette
//! through every spawn function, `recolor` maps each Light colour to its counterpart in
//! the current theme: for new shapes and texts as they appear, and for everything when
//! the theme changes. Colours not in the table (agent accents, white paper, the chibi
//! details) stay as they are.

use bevy::prelude::*;
use bevy::window::WindowThemeChanged;
use bevy_prototype_lyon::prelude::*;

use crate::scene::{self, rgb};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Choice {
    /// Light or Dark, following the macOS appearance.
    #[default]
    Auto,
    Light,
    Dark,
    Space,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Space => "Spaceship",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Auto => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Space,
            Self::Space => Self::Auto,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Space => "space",
        }
    }

    fn parse(text: &str) -> Self {
        match text.trim() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            "space" => Self::Space,
            _ => Self::Auto,
        }
    }
}

/// What is drawn: a column of `PALETTE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Light = 0,
    Dark = 1,
    Space = 2,
}

#[derive(Resource)]
pub struct Theme {
    pub choice: Choice,
    /// The system appearance is dark (for Auto).
    system_dark: bool,
    /// What the room currently shows; `None` before the first frame.
    shown: Option<Look>,
}

impl Theme {
    pub fn load() -> Self {
        let choice = path().and_then(|path| std::fs::read_to_string(path).ok()).map_or(Choice::Auto, |text| Choice::parse(&text));
        Self { choice, system_dark: system_is_dark(), shown: None }
    }

    pub fn look(&self) -> Look {
        match self.choice {
            Choice::Auto if self.system_dark => Look::Dark,
            Choice::Auto | Choice::Light => Look::Light,
            Choice::Dark => Look::Dark,
            Choice::Space => Look::Space,
        }
    }

    /// Auto, Light, Dark, Spaceship, …; remembered in `~/.agent-room/theme.txt`.
    pub fn cycle(&mut self) {
        self.choice = self.choice.next();
        if let Some(path) = path() {
            let _ = std::fs::write(path, self.choice.key());
        }
    }
}

fn path() -> Option<std::path::PathBuf> {
    room_proto::state_dir().map(|dir| dir.join("theme.txt"))
}

/// `defaults read -g AppleInterfaceStyle` says "Dark" in dark mode and fails in light.
fn system_is_dark() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "Dark")
}

/// Light, Dark, Spaceship. Light is what `scene.rs` and `anim.rs` draw with. White
/// (window frames, bubbles, paper) is left alone on purpose: bubbles stay readable.
const PALETTE: [[Color; 3]; 31] = [
    [scene::BACKDROP, rgb(0x1B, 0x1C, 0x21), rgb(0x04, 0x06, 0x10)],
    [scene::FLOOR, rgb(0x2A, 0x2C, 0x33), rgb(0x1A, 0x21, 0x2E)],
    [scene::FLOOR_EDGE, rgb(0x3A, 0x3D, 0x46), rgb(0x2C, 0x3A, 0x50)],
    [scene::WALL, rgb(0x23, 0x25, 0x2B), rgb(0x10, 0x16, 0x24)],
    [scene::WINDOW_GLASS, rgb(0x27, 0x3F, 0x5A), rgb(0x07, 0x0B, 0x24)],
    [scene::RUG, rgb(0x32, 0x35, 0x3E), rgb(0x1D, 0x28, 0x3B)],
    [scene::RUG_EDGE, rgb(0x41, 0x45, 0x4F), rgb(0x2F, 0xC4, 0xEE)],
    [scene::DOOR_COLOR, rgb(0x4A, 0x42, 0x37), rgb(0x2A, 0x39, 0x51)],
    [scene::DESK, rgb(0x6B, 0x54, 0x40), rgb(0x28, 0x32, 0x46)],
    [scene::DESK_EDGE, rgb(0x54, 0x41, 0x2F), rgb(0x2E, 0xC3, 0xED)],
    [scene::LAPTOP, rgb(0x14, 0x16, 0x1B), rgb(0x0C, 0x18, 0x2A)],
    [scene::LEAF, rgb(0x5C, 0x8F, 0x58), rgb(0x3B, 0xE0, 0x93)],
    [scene::LEAF_DARK, rgb(0x45, 0x6E, 0x43), rgb(0x1F, 0xA3, 0x6A)],
    // Pot and sofa share a colour.
    [scene::POT, rgb(0x8A, 0x5A, 0x44), rgb(0x2B, 0x3A, 0x56)],
    [scene::ALERT, rgb(0xE0, 0x5A, 0x53), rgb(0xFF, 0x4D, 0x6D)],
    [scene::ZONE_LABEL, rgb(0xB9, 0xAF, 0xA0), rgb(0x7F, 0xDB, 0xFF)],
    [scene::SHELF, rgb(0x6E, 0x50, 0x38), rgb(0x24, 0x31, 0x47)],
    [scene::RACK, rgb(0x14, 0x17, 0x1D), rgb(0x0B, 0x10, 0x1D)],
    [scene::LED_GREEN, rgb(0x6A, 0xD2, 0x89), rgb(0x39, 0xFF, 0x88)],
    [scene::LED_BLUE, rgb(0x6E, 0xB6, 0xFE), rgb(0x2D, 0xC2, 0xEC)],
    [scene::GLOBE_SEA, rgb(0x3D, 0x6E, 0x99), rgb(0x3B, 0x6B, 0xFF)],
    [scene::SOFA_CUSHION, rgb(0x9C, 0x6A, 0x52), rgb(0x3C, 0x4F, 0x73)],
    [scene::TEXT, rgb(0xE8, 0xE2, 0xD8), rgb(0xCF, 0xE8, 0xFF)],
    [crate::anim::BUBBLE_EDGE, rgb(0xB8, 0xB0, 0xA4), rgb(0x2C, 0xC1, 0xEB)],
    // The zone floors (`scene::ZONES`).
    [rgb(0xEC, 0xEF, 0xE0), rgb(0x2E, 0x38, 0x2D), rgb(0x12, 0x26, 0x30)],
    [rgb(0xE5, 0xE9, 0xF0), rgb(0x2B, 0x32, 0x3F), rgb(0x13, 0x1D, 0x38)],
    [rgb(0xE3, 0xEE, 0xF3), rgb(0x29, 0x38, 0x40), rgb(0x10, 0x27, 0x33)],
    [rgb(0xF5, 0xE6, 0xE1), rgb(0x42, 0x30, 0x2D), rgb(0x2C, 0x16, 0x2D)],
    [rgb(0xF2, 0xE8, 0xDB), rgb(0x39, 0x33, 0x2B), rgb(0x18, 0x20, 0x33)],
    [rgb(0xEC, 0xE6, 0xF4), rgb(0x34, 0x2E, 0x3F), rgb(0x1D, 0x18, 0x33)],
    // Rack shelves.
    [rgb(0x55, 0x5E, 0x6E), rgb(0x2E, 0x33, 0x3D), rgb(0x1F, 0x3B, 0x55)],
];

fn same_rgb(a: Color, b: Color) -> bool {
    let (a, b) = (a.to_srgba(), b.to_srgba());
    (a.red - b.red).abs() < 1e-4 && (a.green - b.green).abs() < 1e-4 && (a.blue - b.blue).abs() < 1e-4
}

/// `color` (in any theme) as drawn in `look`; unknown colours are kept.
fn repaint(color: Color, look: Look) -> Color {
    let Some(row) = PALETTE.iter().find(|row| row.iter().any(|c| same_rgb(*c, color))) else { return color };
    row[look as usize].with_alpha(color.alpha())
}

fn repaint_shape(shape: &mut Shape, look: Look) {
    if let Some(fill) = shape.fill.as_mut() {
        fill.color = repaint(fill.color, look);
    }
    if let Some(stroke) = shape.stroke.as_mut() {
        stroke.color = repaint(stroke.color, look);
    }
}

/// T cycles the theme; the system appearance updates Auto.
pub fn switch(
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<bevy_egui::input::EguiWantsInput>,
    mut changed: MessageReader<WindowThemeChanged>,
    mut theme: ResMut<Theme>,
) {
    if keys.just_pressed(KeyCode::KeyT) && !egui_input.wants_any_keyboard_input() {
        theme.cycle();
    }
    if let Some(change) = changed.read().last() {
        theme.system_dark = change.theme == bevy::window::WindowTheme::Dark;
    }
}

/// Repaints new shapes and texts, and everything when the theme changes.
#[allow(clippy::type_complexity)]
pub fn recolor(
    mut theme: ResMut<Theme>,
    mut clear: ResMut<ClearColor>,
    mode: Res<crate::mini::ViewMode>,
    mut shapes: ParamSet<(Query<&mut Shape, Added<Shape>>, Query<&mut Shape>)>,
    mut texts: ParamSet<(Query<&mut TextColor, Added<TextColor>>, Query<&mut TextColor>)>,
) {
    let look = theme.look();
    if theme.shown == Some(look) {
        if look != Look::Light {
            for mut shape in &mut shapes.p0() {
                repaint_shape(&mut shape, look);
            }
            for mut color in &mut texts.p0() {
                color.0 = repaint(color.0, look);
            }
        }
        return;
    }
    theme.shown = Some(look);
    for mut shape in &mut shapes.p1() {
        repaint_shape(&mut shape, look);
    }
    for mut color in &mut texts.p1() {
        color.0 = repaint(color.0, look);
    }
    // Mini mode has its own see-through background.
    if !mode.mini {
        clear.0 = repaint(scene::BACKDROP, look);
    }
}

/// A star of the Spaceship theme: in the windows and in the dark around the room.
#[derive(Component)]
pub struct Star;

/// Spawns the stars once (hidden), and shows them only in the Spaceship theme.
pub fn stars(
    mut commands: Commands,
    theme: Res<Theme>,
    glass: Query<(Entity, &Shape), Without<Star>>,
    mut stars: Query<&mut Visibility, With<Star>>,
    mut spawned: Local<bool>,
    mut shown: Local<Option<bool>>,
) {
    let space = theme.look() == Look::Space;
    if !*spawned {
        if !space {
            return;
        }
        *spawned = true;
        let mut seed = 0x9E37_79B9_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed % 10_000) as f32 / 10_000.0
        };
        let star = |size: f32| crate::scene::circle(size, Color::srgba(0.85, 0.93, 1.0, 0.9));
        for (window, shape) in &glass {
            let is_glass = shape.fill.is_some_and(|fill| PALETTE[4].iter().any(|c| same_rgb(*c, fill.color)));
            if !is_glass {
                continue;
            }
            for _ in 0..9 {
                let at = Vec3::new((next() - 0.5) * 120.0, (next() - 0.5) * 34.0, 0.5);
                commands.spawn((Star, star(0.8 + next() * 1.4), Transform::from_translation(at), Visibility::Hidden, ChildOf(window)));
            }
        }
        // Around the room, behind the floor.
        let area = scene::ROOM_SIZE * 1.6;
        for _ in 0..160 {
            let at = Vec3::new((next() - 0.5) * area.x, (next() - 0.5) * area.y, -120.0);
            commands.spawn((Star, star(0.7 + next() * 1.6), Transform::from_translation(at), Visibility::Hidden));
        }
        return; // Shown next frame, once spawned.
    }
    if *shown == Some(space) {
        return;
    }
    *shown = Some(space);
    for mut visibility in &mut stars {
        *visibility = if space { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// The panels' egui colours for a look.
pub fn visuals(look: Look) -> bevy_egui::egui::Visuals {
    use bevy_egui::egui::{Color32, Visuals};
    match look {
        Look::Light => {
            let mut visuals = Visuals::light();
            visuals.panel_fill = Color32::from_rgb(0xFB, 0xF7, 0xF1);
            visuals
        }
        Look::Dark => {
            let mut visuals = Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(0x22, 0x23, 0x29);
            visuals.window_fill = Color32::from_rgb(0x26, 0x27, 0x2E);
            visuals
        }
        Look::Space => {
            let mut visuals = Visuals::dark();
            visuals.panel_fill = Color32::from_rgb(0x0B, 0x11, 0x20);
            visuals.window_fill = Color32::from_rgb(0x0F, 0x18, 0x2C);
            visuals.extreme_bg_color = Color32::from_rgb(0x06, 0x0A, 0x16);
            visuals.selection.bg_fill = Color32::from_rgb(0x1E, 0x7F, 0xA8);
            visuals.hyperlink_color = Color32::from_rgb(0x7F, 0xDB, 0xFF);
            visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x16, 0x24, 0x3A);
            visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x1D, 0x33, 0x52);
            visuals
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_map_between_themes_and_back() {
        let dark = repaint(scene::DESK, Look::Dark);
        assert_ne!(dark, scene::DESK);
        assert_eq!(repaint(dark, Look::Light), scene::DESK);
        assert_eq!(repaint(repaint(dark, Look::Space), Look::Light), scene::DESK);
        // Alpha survives (bubbles fade).
        assert!((repaint(crate::anim::BUBBLE_EDGE.with_alpha(0.5), Look::Dark).alpha() - 0.5).abs() < 1e-6);
        // Accents and other colours stay.
        assert_eq!(repaint(Color::WHITE, Look::Space), Color::WHITE);
        assert_eq!(repaint(scene::MAIN_ACCENT, Look::Dark), scene::MAIN_ACCENT);
    }

    #[test]
    fn every_theme_colour_is_in_one_row() {
        // Mapping back needs each colour in exactly one row (pot and sofa are one row).
        let all: Vec<Color> = PALETTE.iter().flatten().copied().collect();
        for (i, a) in all.iter().enumerate() {
            let rows = PALETTE.iter().filter(|row| row.iter().any(|c| same_rgb(*c, *a))).count();
            assert_eq!(rows, 1, "colour {i} {a:?} is in {rows} rows");
        }
    }
}
