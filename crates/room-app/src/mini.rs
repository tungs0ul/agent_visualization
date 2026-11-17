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

/// Desks of agents who arrive while in mini mode start hidden too.
pub fn hide_new_desks(mode: Res<ViewMode>, mut desks: Query<&mut Visibility, Added<Desk>>) {
    if mode.mini {
        for mut visibility in &mut desks {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(target_os = "macos")]
thread_local! {
    /// The frosted view while mini mode is on (AppKit objects live on the main thread).
    static GLASS: std::cell::RefCell<Option<objc2::rc::Retained<objc2_app_kit::NSVisualEffectView>>> =
        const { std::cell::RefCell::new(None) };
}

/// The backdrop behind Bevy's view: frosted in mini mode (`on`), opaque otherwise. A
/// frosted NSVisualEffectView *behind* Bevy's view. It has to be a sibling below
/// Bevy's view, in its superview: inside Bevy's view it would sit above the Metal layer
/// and blur the room itself (what `window-vibrancy` did).
#[cfg(target_os = "macos")]
fn set_glass(handle: &RawHandleWrapper, on: bool) {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindowOrderingMode,
    };
    GLASS.with_borrow_mut(|glass| {
        if let Some(old) = glass.take() {
            old.removeFromSuperview();
        }
        let raw_window_handle::RawWindowHandle::AppKit(handle) = handle.get_window_handle() else { return };
        // `apply_mode` runs on the main thread (`NonSendMarker`).
        let Some(main_thread) = MainThreadMarker::new() else { return };
        // SAFETY: the handle points at the live NSView of our window.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        // SAFETY: plain AppKit getter, on the main thread.
        let Some(container) = (unsafe { view.superview() }) else { return };
        let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(main_thread), container.bounds());
        // Mini: frosted glass showing the desktop. Full view: the window is still
        // see-through (it's created transparent), so give it the standard opaque window
        // background, or the title bar shows the desktop.
        if on {
            effect.setMaterial(NSVisualEffectMaterial::Popover);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        } else {
            effect.setMaterial(NSVisualEffectMaterial::WindowBackground);
            effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        }
        effect.setState(NSVisualEffectState::Active);
        effect.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        // Bottom of the whole stack: below Bevy's view *and* the title bar with the
        // close/minimise/zoom buttons, which it would otherwise cover.
        container.addSubview_positioned_relativeTo(&effect, NSWindowOrderingMode::Below, None);
        *glass = Some(effect);
    });
}
