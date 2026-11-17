//! Mini mode (PLAN.md M4c): a widget-sized window in the top-right corner with a
//! frosted, see-through background, showing only Claude and the agents. Meant to sit
//! beside the terminal or editor while you work. M toggles it.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, RawHandleWrapper, WindowLevel, WindowPosition};
use bevy_egui::input::EguiWantsInput;

use crate::scene::{BACKDROP, Desk};
use crate::window::SavedWindow;

/// Logical size of the mini window.
pub const MINI_SIZE: Vec2 = Vec2::new(320.0, 200.0);
/// Logical gap to the screen's right edge and below the menu bar.
const MINI_MARGIN: Vec2 = Vec2::new(12.0, 44.0);
/// Rounded corners of the glass, like macOS widgets.
#[cfg(target_os = "macos")]
const CORNER_RADIUS: f64 = 18.0;

/// Room parts hidden in mini mode, where only the characters are drawn.
#[derive(Component)]
pub struct Furniture;

#[derive(Resource, Default)]
pub struct ViewMode {
    pub mini: bool,
    /// The mode the window currently shows, once applied.
    applied: Option<bool>,
    /// The full window's place and size, to return to when leaving mini mode.
    full: Option<SavedWindow>,
}

impl ViewMode {
    /// Starts in the mode saved last time; `full` is that saved full-window geometry.
    pub fn from_saved(saved: Option<SavedWindow>) -> Self {
        Self { mini: saved.is_some_and(|s| s.mini), applied: None, full: saved }
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

type RoomParts = Or<(With<Furniture>, With<Desk>)>;

/// Applies a mode change to the window, the background and the room. Runs on the main
/// thread (`NonSendMarker`) because the glass is an AppKit view.
pub fn apply_mode(
    _main_thread: NonSendMarker,
    mut mode: ResMut<ViewMode>,
    window: Single<(&mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut clear: ResMut<ClearColor>,
    mut furniture: Query<&mut Visibility, RoomParts>,
) {
    if mode.applied == Some(mode.mini) {
        return;
    }
    let (mut window, handle) = window.into_inner();
    // The native window (and its handle) appears a few frames after startup.
    let Some(handle) = handle else { return };
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
        clear.0 = Color::NONE;
    } else {
        window.decorations = true;
        if let Some(full) = mode.full {
            window.resolution.set(full.width, full.height);
            window.position = WindowPosition::At(IVec2::new(full.x, full.y));
            window.window_level = if full.on_top { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal };
        }
        clear.0 = BACKDROP;
    }
    let shown = if mode.mini { Visibility::Hidden } else { Visibility::Inherited };
    for mut visibility in &mut furniture {
        visibility.set_if_neq(shown);
    }
    #[cfg(target_os = "macos")]
    set_glass(handle, mode.mini);
    #[cfg(not(target_os = "macos"))]
    let _ = handle;

    if let Some(mut saved) = mode.full {
        saved.mini = mode.mini;
        saved.save();
    }
    mode.applied = Some(mode.mini);
}

/// Desks of agents who arrive while in mini mode start hidden too.
pub fn hide_new_desks(mode: Res<ViewMode>, mut desks: Query<&mut Visibility, Added<Desk>>) {
    if mode.mini {
        for mut visibility in &mut desks {
            *visibility = Visibility::Hidden;
        }
    }
}

/// Liquid Glass on macOS 26+, the classic frosted popover material before that.
#[cfg(target_os = "macos")]
fn set_glass(handle: &RawHandleWrapper, on: bool) {
    use window_vibrancy::{
        LiquidGlassOptions, NSGlassEffectViewStyle, NSVisualEffectMaterial, NSVisualEffectState, apply_liquid_glass,
        apply_vibrancy, clear_liquid_glass, clear_vibrancy,
    };
    // SAFETY: `apply_mode` runs on the main thread, as AppKit requires.
    let window = unsafe { handle.get_handle() };
    if on {
        let glass = LiquidGlassOptions::new(NSGlassEffectViewStyle::Regular).radius(CORNER_RADIUS);
        if apply_liquid_glass(&window, glass).is_err()
            && let Err(err) =
                apply_vibrancy(&window, NSVisualEffectMaterial::Popover, Some(NSVisualEffectState::Active), Some(CORNER_RADIUS))
        {
            warn!("no frosted background for mini mode: {err}");
        }
    } else {
        let _ = clear_liquid_glass(&window);
        let _ = clear_vibrancy(&window);
    }
}
