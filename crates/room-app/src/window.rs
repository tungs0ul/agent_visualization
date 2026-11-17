//! Window behavior (PLAN.md 5): corner placement, remembered geometry, hotkeys, and a
//! screenshot mode for checking the UI without a human at the screen.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::window::{
    Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, WindowLevel, WindowMode, WindowMoved, WindowPosition,
    WindowResized, WindowResolution,
};
use bevy_egui::input::EguiWantsInput;
use serde::{Deserialize, Serialize};

/// Logical size of the corner window on first start.
pub const START_SIZE: Vec2 = Vec2::new(420.0, 320.0);
/// Logical gap between the window and the screen's right edge.
const CORNER_MARGIN: f32 = 16.0;
/// Logical gap above the bottom edge, leaving room for the macOS Dock.
const DOCK_MARGIN: f32 = 96.0;
/// Geometry is written this long after the last move or resize.
const SAVE_DELAY: Duration = Duration::from_secs(1);
const STATE_FILE: &str = "window.json";
/// Screenshot mode: one shot every interval, at most this many.
const SHOT_INTERVAL: Duration = Duration::from_secs(2);
const MAX_SHOTS: u32 = 30;

/// The windowed geometry, kept in `~/.agent-room/window.json`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedWindow {
    /// Physical position of the window's top-left corner.
    pub x: i32,
    pub y: i32,
    /// Logical size.
    pub width: f32,
    pub height: f32,
    pub on_top: bool,
}

impl SavedWindow {
    /// A missing or broken file means defaults.
    pub fn load() -> Option<Self> {
        let text = fs::read_to_string(state_path()?).ok()?;
        serde_json::from_str::<Self>(&text)
            .ok()
            .filter(|saved| saved.width >= 200.0 && saved.height >= 150.0)
    }

    fn of(window: &Window) -> Option<Self> {
        let WindowPosition::At(position) = window.position else { return None };
        (window.mode == WindowMode::Windowed).then(|| Self {
            x: position.x,
            y: position.y,
            width: window.width(),
            height: window.height(),
            on_top: window.window_level == WindowLevel::AlwaysOnTop,
        })
    }

    fn save(&self) {
        let Some(path) = state_path() else { return };
        match serde_json::to_string(self) {
            Ok(text) => {
                if let Err(err) = fs::write(&path, text) {
                    warn!("saving {}: {err}", path.display());
                }
            }
            Err(err) => warn!("encoding window state: {err}"),
        }
    }
}

fn state_path() -> Option<PathBuf> {
    room_proto::state_dir().map(|dir| dir.join(STATE_FILE))
}

pub fn initial_window(saved: Option<SavedWindow>) -> Window {
    let size = saved.map_or(START_SIZE, |s| Vec2::new(s.width, s.height));
    let mut resolution = WindowResolution::default();
    resolution.set(size.x, size.y);
    Window {
        title: "Agent Room".into(),
        resolution,
        resizable: true,
        window_level: if saved.is_none_or(|s| s.on_top) { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal },
        position: saved.map_or(WindowPosition::Automatic, |s| WindowPosition::At(IVec2::new(s.x, s.y))),
        ..default()
    }
}

/// True until the first start has moved the window into the bottom-right corner.
#[derive(Resource)]
pub struct NeedsCornerPlacement(pub bool);

/// Moves the window to the bottom-right corner once the primary monitor is known.
pub fn place_in_corner(
    mut needed: ResMut<NeedsCornerPlacement>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if !needed.0 {
        return;
    }
    let Ok(monitor) = monitors.single() else { return };
    let scale = monitor.scale_factor as f32;
    let size = (START_SIZE * scale).as_ivec2();
    let screen = IVec2::new(monitor.physical_width as i32, monitor.physical_height as i32);
    let margin = (Vec2::new(CORNER_MARGIN, DOCK_MARGIN) * scale).as_ivec2();
    window.position = WindowPosition::At(monitor.physical_position + screen - size - margin);
    needed.0 = false;
}

/// Writes the geometry a moment after the user stops moving or resizing the window.
pub fn remember_geometry(
    mut moved: MessageReader<WindowMoved>,
    mut resized: MessageReader<WindowResized>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut changed_at: Local<Option<Instant>>,
) {
    if moved.read().count() + resized.read().count() > 0 {
        *changed_at = Some(Instant::now());
    }
    let Some(at) = *changed_at else { return };
    if at.elapsed() < SAVE_DELAY {
        return;
    }
    *changed_at = None;
    if let Some(saved) = SavedWindow::of(&window) {
        saved.save();
    }
}

/// F: full screen on/off. Esc: leave full screen. P: always-on-top on/off.
pub fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if egui_input.wants_any_keyboard_input() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        window.mode = match window.mode {
            WindowMode::Windowed => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
            _ => WindowMode::Windowed,
        };
    }
    if keys.just_pressed(KeyCode::Escape) && window.mode != WindowMode::Windowed {
        window.mode = WindowMode::Windowed;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        window.window_level = match window.window_level {
            WindowLevel::AlwaysOnTop => WindowLevel::Normal,
            _ => WindowLevel::AlwaysOnTop,
        };
        if let Some(saved) = SavedWindow::of(&window) {
            saved.save();
        }
    }
}

/// `AGENT_ROOM_SCREENSHOT_DIR=<dir>` saves `shot-001.png`, `shot-002.png`, … every two
/// seconds. Bevy reads its own frame, so this works without macOS Screen Recording
/// permission. Dev only: the app redraws continuously in this mode.
#[derive(Resource)]
pub struct ScreenshotMode {
    dir: PathBuf,
    next: Instant,
    taken: u32,
}

impl ScreenshotMode {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("AGENT_ROOM_SCREENSHOT_DIR")?);
        fs::create_dir_all(&dir).ok()?;
        Some(Self { dir, next: Instant::now() + SHOT_INTERVAL, taken: 0 })
    }
}

pub fn take_screenshots(mut commands: Commands, mode: Option<ResMut<ScreenshotMode>>) {
    let Some(mut mode) = mode else { return };
    if mode.taken >= MAX_SHOTS || Instant::now() < mode.next {
        return;
    }
    mode.taken += 1;
    mode.next += SHOT_INTERVAL;
    let path = mode.dir.join(format!("shot-{:03}.png", mode.taken));
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}
