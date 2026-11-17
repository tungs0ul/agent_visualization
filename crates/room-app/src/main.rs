//! Agent Room: a small always-on-top window that shows what Claude Code and its
//! sub-agents are doing, and lets the user answer permission prompts.
//!
//! The room scene with Claude and a seat per sub-agent (animated: poses, speech
//! bubbles, walking, report planes), plus the event log and permission cards (egui)
//! beside or below it.

mod anim;
mod log;
mod scene;
mod server;
mod ui;
mod window;

use std::time::{Duration, Instant};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::CameraOutputMode;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use bevy_prototype_lyon::prelude::ShapePlugin;
use room_proto::event::{AgentRef, EventKind};

use crate::anim::Animating;
use crate::log::{PendingPermission, RoomLog, agent_label};
use crate::scene::{HookEvent, Roster};
use crate::server::{Incoming, Server};
use crate::window::{NeedsCornerPlacement, SavedWindow, ScreenshotMode};

/// Redraw rate while characters move (about 30 fps: smooth enough, half the power of 60).
const FRAME: Duration = Duration::from_millis(33);
/// Redraw rate while only a permission countdown changes.
const COUNTDOWN_TICK: Duration = Duration::from_millis(250);

fn main() -> AppExit {
    let server = match Server::start() {
        Ok(server) => server,
        Err(err) => {
            eprintln!("agent-room: {err}");
            return AppExit::error();
        }
    };
    let saved = SavedWindow::load();

    let mut app = App::new();
    app
        // Sleep until input or a hook event arrives (PLAN.md 6).
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(ClearColor(scene::BACKDROP))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window::initial_window(saved)),
            ..default()
        }))
        .add_plugins((EguiPlugin::default(), ShapePlugin))
        .add_message::<HookEvent>()
        .insert_resource(server)
        .insert_resource(NeedsCornerPlacement(saved.is_none()))
        .init_resource::<RoomLog>()
        .init_resource::<Roster>()
        .init_resource::<Animating>()
        .add_systems(Startup, ((scene::load_font, scene::spawn_room).chain(), spawn_ui_camera, connect_waker))
        .add_systems(
            Update,
            (
                window::place_in_corner,
                window::hotkeys,
                window::remember_geometry,
                receive_events,
                scene::apply_events,
                scene::greet,
                (anim::walk, anim::leave_when_done, anim::animate_parts, anim::update_bubbles),
                (anim::hand_over, anim::pulse),
                anim::detect_motion,
                expire_permissions,
                pace_updates,
                window::take_screenshots,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui::draw);
    if let Some(mode) = ScreenshotMode::from_env() {
        app.insert_resource(mode).insert_resource(WinitSettings::continuous());
    }
    app.run()
}

/// egui draws with its own camera on top, so the room camera can be limited to the
/// area the panels leave free.
fn spawn_ui_camera(mut commands: Commands, mut egui_settings: ResMut<EguiGlobalSettings>) {
    egui_settings.auto_create_primary_context = false;
    commands.spawn((
        PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

/// Lets the socket thread wake the sleeping event loop when a hook event arrives.
fn connect_waker(server: Res<Server>, proxy: Res<EventLoopProxyWrapper>) {
    let proxy = (**proxy).clone();
    let _ = server.waker.set(Box::new(move || {
        let _ = proxy.send_event(WinitUserEvent::WakeUp);
    }));
}

fn receive_events(
    server: Res<Server>,
    roster: Res<Roster>,
    mut log: ResMut<RoomLog>,
    mut hook_events: MessageWriter<HookEvent>,
) {
    let now = Instant::now();
    for Incoming { event, reply } in server.events.try_iter() {
        match &event.kind {
            // The roster knows what kind of agent sent the report.
            EventKind::ReportReceived { from_agent } if roster.agent_type(from_agent).is_some() => {
                let sender = AgentRef::Sub {
                    id: from_agent.clone(),
                    agent_type: roster.agent_type(from_agent).unwrap_or_default().to_owned(),
                };
                log.note(agent_label(&event.agent), format!("Got the report from {}", agent_label(&sender)));
            }
            // Claude Code's own internal helpers stop without ever starting (PLAN.md 3.4).
            EventKind::SubagentStopped { .. }
                if matches!(&event.agent, AgentRef::Sub { id, agent_type } if agent_type.is_empty() && roster.agent_type(id).is_none()) => {}
            _ => log.push(&event),
        }
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
        hook_events.write(HookEvent(event));
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

/// Redraw only while something on screen changes by itself; sleep otherwise.
fn pace_updates(
    log: Res<RoomLog>,
    animating: Res<Animating>,
    screenshots: Option<Res<ScreenshotMode>>,
    mut current: Local<Option<Duration>>,
    mut settings: ResMut<WinitSettings>,
) {
    if screenshots.is_some() {
        return;
    }
    let wanted = if animating.0 {
        Some(FRAME)
    } else if !log.pending.is_empty() {
        Some(COUNTDOWN_TICK)
    } else {
        None
    };
    if wanted == *current {
        return;
    }
    *current = wanted;
    *settings = match wanted {
        Some(every) => WinitSettings {
            focused_mode: UpdateMode::reactive(every),
            unfocused_mode: UpdateMode::reactive_low_power(every),
        },
        None => WinitSettings::desktop_app(),
    };
}
