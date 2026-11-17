//! Camera controls (PLAN.md M4f): zoom and pan. Any manual input pauses the automatic
//! framing until a double-click on the room resets it. (Pinning an agent was dropped:
//! agents come and go too quickly for it to help.)

use bevy::input::gestures::PinchGesture;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::EguiWantsInput;

use crate::mini::ViewMode;
use crate::scene::WorldCamera;

/// The smallest and largest part of the office the camera can show.
const MIN_SIZE: Vec2 = Vec2::new(280.0, 180.0);
const MAX_SIZE: Vec2 = Vec2::new(1600.0, 1030.0);
/// Two clicks within this many seconds are a double-click.
const DOUBLE_CLICK: f32 = 0.35;
/// Pointer movement (logical px) below which a press-and-release counts as a click.
const CLICK_SLOP: f32 = 4.0;

/// Manual camera state; `manual: None` means automatic framing.
#[derive(Resource, Default)]
pub struct CameraControl {
    /// Center and size chosen by panning/zooming.
    pub manual: Option<(Vec2, Vec2)>,
    last_click: f32,
    press: Option<Vec2>,
    dragged: bool,
}

/// Pinch or ⌘/Ctrl-scroll zooms, scrolling or dragging pans, double-click resets.
#[allow(clippy::too_many_arguments)]
pub fn camera_input(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<MouseWheel>,
    mut pinch: MessageReader<PinchGesture>,
    (egui_input, mode): (Res<EguiWantsInput>, Res<ViewMode>),
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform, &Projection), With<WorldCamera>>,
    mut control: ResMut<CameraControl>,
) {
    let (camera, camera_transform, projection) = camera.into_inner();
    if egui_input.wants_any_pointer_input() {
        wheel.clear();
        pinch.clear();
        control.press = None;
        return;
    }
    let Some(viewport) = camera.logical_viewport_size() else { return };
    let Projection::Orthographic(OrthographicProjection {
        scaling_mode: bevy::camera::ScalingMode::AutoMin { min_width, min_height },
        ..
    }) = projection
    else {
        return;
    };
    let (mut center, mut size) = control
        .manual
        .unwrap_or((camera_transform.translation().truncate(), Vec2::new(*min_width, *min_height)));
    let per_pixel = (size.x / viewport.x).max(size.y / viewport.y);
    let mut moved = false;

    let zoom_key = keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::ControlLeft, KeyCode::ControlRight]);
    for event in wheel.read() {
        let delta = match event.unit {
            MouseScrollUnit::Line => Vec2::new(event.x, event.y) * 40.0,
            MouseScrollUnit::Pixel => Vec2::new(event.x, event.y),
        };
        if zoom_key {
            size *= (1.0 - delta.y * 0.002).clamp(0.5, 1.5);
        } else {
            center += Vec2::new(-delta.x, delta.y) * per_pixel;
        }
        moved = true;
    }
    for gesture in pinch.read() {
        size *= (1.0 - gesture.0).clamp(0.5, 1.5);
        moved = true;
    }

    // Drag to pan in the full view; in mini mode dragging moves the window instead.
    let cursor = window.cursor_position();
    if buttons.just_pressed(MouseButton::Left) {
        control.press = cursor;
        control.dragged = false;
    }
    if buttons.pressed(MouseButton::Left)
        && !mode.mini
        && let (Some(start), Some(now)) = (control.press, cursor)
    {
        let delta = now - start;
        if control.dragged || delta.length() > CLICK_SLOP {
            control.dragged = true;
            center += Vec2::new(-delta.x, delta.y) * per_pixel;
            control.press = Some(now);
            moved = true;
        }
    }
    if buttons.just_released(MouseButton::Left) && control.press.take().is_some() && !control.dragged {
        let now = time.elapsed_secs();
        if now - control.last_click < DOUBLE_CLICK {
            control.manual = None;
            return;
        }
        control.last_click = now;
    }

    if moved {
        control.manual = Some((center, size.clamp(MIN_SIZE, MAX_SIZE)));
    }
}
