//! Window behavior (PLAN.md 5): corner placement, remembered geometry, hotkeys, and a
//! screenshot mode for checking the UI without a human at the screen.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{Screenshot, save_to_disk};
use bevy::window::{
    CompositeAlphaMode, Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, WindowLevel, WindowMode, WindowMoved, WindowPosition,
    WindowResized, WindowResolution,
};
use bevy_egui::input::EguiWantsInput;
use serde::{Deserialize, Serialize};

use crate::mini::ViewMode;

/// Logical size of the corner window on first start.
pub const START_SIZE: Vec2 = Vec2::new(420.0, 320.0);
/// Logical gap between the window and the screen's right edge.
const CORNER_MARGIN: f32 = 16.0;
/// Logical gap above the bottom edge, leaving room for the macOS Dock.
const DOCK_MARGIN: f32 = 96.0;
/// Geometry is written this long after the last move or resize.
const SAVE_DELAY: Duration = Duration::from_secs(1);
const STATE_FILE: &str = "window.json";
/// Screenshot mode: one shot every interval (default), at most this many.
const SHOT_INTERVAL: Duration = Duration::from_secs(2);
const MAX_SHOTS: u32 = 80;

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
    /// Start in mini mode (the geometry above is the full window's).
    #[serde(default)]
    pub mini: bool,
}

impl SavedWindow {
    /// A missing or broken file means defaults.
    pub fn load() -> Option<Self> {
        let text = fs::read_to_string(state_path()?).ok()?;
        serde_json::from_str::<Self>(&text)
            .ok()
            .filter(|saved| saved.width >= 200.0 && saved.height >= 150.0)
    }

    pub fn of(window: &Window) -> Option<Self> {
        let WindowPosition::At(position) = window.position else { return None };
        (window.mode == WindowMode::Windowed).then(|| Self {
            x: position.x,
            y: position.y,
            width: window.width(),
            height: window.height(),
            on_top: window.window_level == WindowLevel::AlwaysOnTop,
            mini: false,
        })
    }

    pub fn save(&self) {
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
    // Starting in mini mode: create the window at mini size right away. macOS ignores
    // resizes requested while the window is still appearing, which left Bevy drawing for
    // a size the window never got (`mini::apply_mode` then only moves it).
    let mini = saved.is_some_and(|s| s.mini);
    let size = if mini { crate::mini::MINI_SIZE } else { saved.map_or(START_SIZE, |s| Vec2::new(s.width, s.height)) };
    let mut resolution = WindowResolution::default();
    resolution.set(size.x, size.y);
    Window {
        title: "Agent Room".into(),
        resolution,
        resizable: true,
        // Always created see-through so mini mode can show the glass behind it; the
        // full view paints an opaque background over it (`mini::apply_mode`).
        transparent: true,
        composite_alpha_mode: CompositeAlphaMode::PostMultiplied,
        decorations: !mini,
        window_level: if mini || saved.is_none_or(|s| s.on_top) { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal },
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
    mode: Res<ViewMode>,
    mut changed_at: Local<Option<Instant>>,
) {
    // The mini window's place is fixed; only the full window's geometry is remembered.
    if mode.mini {
        moved.clear();
        resized.clear();
        return;
    }
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
    mode: Res<ViewMode>,
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
    if keys.just_pressed(KeyCode::KeyP) && !mode.mini {
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
///
/// `AGENT_ROOM_SCREENSHOT_EVERY_MS=<ms>` changes the interval (to catch animations).
/// `AGENT_ROOM_SCREENSHOT_RESIZE=<w>x<h>` also resizes the window (logical px) after the
/// second shot, to test resizing without a human dragging the corner.
#[derive(Resource)]
pub struct ScreenshotMode {
    dir: PathBuf,
    every: Duration,
    next: Instant,
    taken: u32,
    resize_to: Option<Vec2>,
}

impl ScreenshotMode {
    pub fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("AGENT_ROOM_SCREENSHOT_DIR")?);
        fs::create_dir_all(&dir).ok()?;
        let resize_to = std::env::var("AGENT_ROOM_SCREENSHOT_RESIZE").ok().and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some(Vec2::new(w.parse().ok()?, h.parse().ok()?))
        });
        let every = std::env::var("AGENT_ROOM_SCREENSHOT_EVERY_MS")
            .ok()
            .and_then(|ms| ms.parse().ok())
            .map_or(SHOT_INTERVAL, Duration::from_millis);
        Some(Self { dir, every, next: Instant::now() + every, taken: 0, resize_to })
    }
}

pub fn take_screenshots(
    mut commands: Commands,
    mode: Option<ResMut<ScreenshotMode>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut toggle_mini: MessageWriter<crate::mini::ToggleMini>,
) {
    let Some(mut mode) = mode else { return };
    // `AGENT_ROOM_SCREENSHOT_TOGGLE_MINI=1`: switch to mini mode after the 2nd shot and
    // back after the 4th, to test the switch without a key press.
    if matches!(mode.taken, 2 | 4) && Instant::now() >= mode.next && std::env::var_os("AGENT_ROOM_SCREENSHOT_TOGGLE_MINI").is_some() {
        toggle_mini.write(crate::mini::ToggleMini);
    }
    if mode.taken >= MAX_SHOTS || Instant::now() < mode.next {
        return;
    }
    if mode.taken == 2
        && let Some(size) = mode.resize_to.take()
    {
        window.resolution.set(size.x, size.y);
    }
    mode.taken += 1;
    let every = mode.every;
    mode.next += every;
    let path = mode.dir.join(format!("shot-{:03}.png", mode.taken));
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}
