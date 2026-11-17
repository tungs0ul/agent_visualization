//! Agent Room: a small always-on-top window that shows what Claude Code and its
//! sub-agents are doing, and lets the user answer permission prompts.
//!
//! M2: event log and permission cards (egui). The room scene arrives in M3.

mod log;
mod server;
mod ui;

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor, PrimaryWindow, WindowLevel, WindowPosition, WindowResolution};
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use room_proto::event::EventKind;

use crate::log::{PendingPermission, RoomLog, agent_label};
use crate::server::{Incoming, Server};

/// Logical size of the corner window at startup.
const START_SIZE: Vec2 = Vec2::new(420.0, 320.0);
/// Logical gap between the window and the screen's right edge.
const CORNER_MARGIN: f32 = 16.0;
/// Logical gap above the bottom edge, leaving room for the macOS Dock.
const DOCK_MARGIN: f32 = 96.0;
/// Redraw rate while a permission countdown is visible.
const COUNTDOWN_TICK: Duration = Duration::from_millis(250);

fn main() -> AppExit {
    let server = match Server::start() {
        Ok(server) => server,
        Err(err) => {
            eprintln!("agent-room: {err}");
            return AppExit::error();
        }
    };

    let mut resolution = WindowResolution::default();
    resolution.set(START_SIZE.x, START_SIZE.y);

    App::new()
        // Sleep until input or a hook event arrives (PLAN.md 6).
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(ClearColor(Color::srgb(0.96, 0.94, 0.90)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Agent Room".into(),
                resolution,
                resizable: true,
                window_level: WindowLevel::AlwaysOnTop,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(server)
        .init_resource::<RoomLog>()
        .add_systems(Startup, (spawn_camera, connect_waker))
        .add_systems(Update, (place_in_corner, receive_events, expire_permissions, pace_updates).chain())
        .add_systems(EguiPrimaryContextPass, ui::draw)
        .run()
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

/// Lets the socket thread wake the sleeping event loop when a hook event arrives.
fn connect_waker(server: Res<Server>, proxy: Res<EventLoopProxyWrapper>) {
    let proxy = (**proxy).clone();
    let _ = server.waker.set(Box::new(move || {
        let _ = proxy.send_event(WinitUserEvent::WakeUp);
    }));
}

/// Moves the window to the bottom-right corner once the primary monitor is known.
fn place_in_corner(
    mut done: Local<bool>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    if *done {
        return;
    }
    let Ok(monitor) = monitors.single() else { return };
    let scale = monitor.scale_factor as f32;
    let size = (START_SIZE * scale).as_ivec2();
    let screen = IVec2::new(monitor.physical_width as i32, monitor.physical_height as i32);
    let margin = (Vec2::new(CORNER_MARGIN, DOCK_MARGIN) * scale).as_ivec2();
    window.position = WindowPosition::At(monitor.physical_position + screen - size - margin);
    *done = true;
}

fn receive_events(server: Res<Server>, mut log: ResMut<RoomLog>) {
    let now = Instant::now();
    for Incoming { event, reply } in server.events.try_iter() {
        log.push(&event);
        if let (Some(reply), EventKind::PermissionAsked { tool }) = (reply, &event.kind) {
            log.pending.push(PendingPermission {
                agent: agent_label(&event.agent),
                tool: tool.clone(),
                cwd: event.cwd.clone(),
                shown_at: now,
                expires_at: now + room_proto::permission_timeout(),
                reply,
            });
        }
    }
}

/// The hook has given up by now and Claude Code asks in the terminal instead.
fn expire_permissions(mut log: ResMut<RoomLog>) {
    let now = Instant::now();
    if log.pending.iter().all(|p| p.expires_at > now) {
        return;
    }
    let (expired, waiting): (Vec<_>, Vec<_>) = log.pending.drain(..).partition(|p| p.expires_at <= now);
    log.pending = waiting;
    for pending in expired {
        log.note(pending.agent, format!("{}: answered in the terminal", pending.tool.name));
    }
}

/// Redraw a few times a second only while a countdown is on screen.
fn pace_updates(log: Res<RoomLog>, mut counting: Local<bool>, mut settings: ResMut<WinitSettings>) {
    let needs_ticks = !log.pending.is_empty();
    if needs_ticks == *counting {
        return;
    }
    *counting = needs_ticks;
    *settings = if needs_ticks {
        WinitSettings {
            focused_mode: UpdateMode::reactive(COUNTDOWN_TICK),
            unfocused_mode: UpdateMode::reactive_low_power(COUNTDOWN_TICK),
        }
    } else {
        WinitSettings::desktop_app()
    };
}
