//! Mini mode (PLAN.md M4c): a widget-sized window in the top-right corner with a
//! the camera following Claude's robot up close (`view3d::frame`).
//! Meant to sit beside the terminal or editor while you work. M toggles it.

use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, RawHandleWrapper, WindowLevel, WindowPosition};
use bevy_egui::input::EguiWantsInput;

use crate::window::SavedWindow;

/// Logical size of the mini window.
pub const MINI_SIZE: Vec2 = Vec2::new(320.0, 200.0);
/// Logical gap to the screen's right edge and below the menu bar.
const MINI_MARGIN: Vec2 = Vec2::new(12.0, 44.0);

#[derive(Resource, Default)]
pub struct ViewMode {
    pub mini: bool,
    /// The mode the window currently shows, once applied.
    applied: Option<bool>,
    /// The full window's place and size, to return to when leaving mini mode.
    full: Option<SavedWindow>,
    /// Keep drawing until then after a switch (see `settle_redraw`).
    settle_until: Option<std::time::Instant>,
}

impl ViewMode {
    /// Starts in the mode saved last time; `full` is that saved full-window geometry.
    pub fn from_saved(saved: Option<SavedWindow>) -> Self {
        Self { mini: saved.is_some_and(|s| s.mini), applied: None, full: saved, settle_until: None }
    }
}

/// Asks to switch between mini and full view (the egui button sends it).
#[derive(Message)]
pub struct ToggleMini;

/// M toggles mini mode.
pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    mut requests: MessageReader<ToggleMini>,
    mut mode: ResMut<ViewMode>,
) {
    let key = keys.just_pressed(KeyCode::KeyM) && !egui_input.wants_any_keyboard_input();
    if key || requests.read().count() % 2 == 1 {
        mode.mini = !mode.mini;
    }
}

/// The mini window has no title bar: drag it by any empty spot.
pub fn drag(
    mode: Res<ViewMode>,
    buttons: Res<ButtonInput<MouseButton>>,
    egui_input: Res<EguiWantsInput>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if mode.mini && buttons.just_pressed(MouseButton::Left) && !egui_input.wants_any_pointer_input() {
        window.start_drag_move();
    }
}

/// Applies a mode change to the window and its backdrop. Runs on the main
/// (The 3D view paints an opaque background in both modes, so the window needs no
/// see-through glass.)
pub fn apply_mode(
    mut mode: ResMut<ViewMode>,
    window: Single<(&mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
) {
    if mode.applied == Some(mode.mini) {
        return;
    }
    let (mut window, handle) = window.into_inner();
    // The native window (and its handle) appears a few frames after startup.
    if handle.is_none() {
        return;
    }
    let Ok(monitor) = monitors.single() else { return };

    if mode.mini {
        // Remember the full window, unless we start in mini mode with it already known.
        if mode.applied.is_some() || mode.full.is_none() {
            mode.full = SavedWindow::of(&window);
        }
        let scale = monitor.scale_factor as f32;
        let size = (MINI_SIZE * scale).as_ivec2();
        let margin = (MINI_MARGIN * scale).as_ivec2();
        let top_right = monitor.physical_position + IVec2::new(monitor.physical_width as i32 - size.x - margin.x, margin.y);
        window.decorations = false;
        window.window_level = WindowLevel::AlwaysOnTop;
        window.resolution.set(MINI_SIZE.x, MINI_SIZE.y);
        window.position = WindowPosition::At(top_right);
    } else {
        window.decorations = true;
        if let Some(full) = mode.full {
            window.resolution.set(full.width, full.height);
            window.position = WindowPosition::At(IVec2::new(full.x, full.y));
            window.window_level = if full.on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal };
        }
    }

    if let Some(mut saved) = mode.full {
        saved.mini = mode.mini;
        saved.save();
    }
    mode.applied = Some(mode.mini);
    mode.settle_until = Some(std::time::Instant::now() + SETTLE);
}

/// How long to keep redrawing after switching modes.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(600);

/// While the window changes size, wgpu rejects a frame or two whose depth and colour
/// textures still have different sizes (ignored by the app's `RenderErrorHandler`).
/// When idle the app draws only on events, so the rejected frame could stay on screen,
/// leaving the window blank. Keep drawing for a moment so a good frame follows.
pub fn settle_redraw(mut mode: ResMut<ViewMode>, mut redraw: MessageWriter<bevy::window::RequestRedraw>) {
    let Some(until) = mode.settle_until else { return };
    if std::time::Instant::now() < until {
        redraw.write(bevy::window::RequestRedraw);
    } else {
        mode.settle_until = None;
    }
}
