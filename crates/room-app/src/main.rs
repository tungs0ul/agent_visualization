//! Agent Room: a small always-on-top window that shows what Claude Code and its
//! sub-agents are doing, and lets the user answer permission prompts.
//!
//! The room scene with Claude and a seat per sub-agent (animated: poses, speech
//! bubbles, walking, report planes), plus the event log and permission cards (egui)
//! beside or below it.

mod anim;
mod camera;
mod log;
mod mini;
mod model;
mod scene;
mod server;
mod ui;
mod window;

use std::time::{Duration, Instant};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::CameraOutputMode;
use bevy::prelude::*;
use bevy::render::error_handler::{RenderErrorHandler, RenderErrorPolicy};
use bevy::render::render_resource::BlendState;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use bevy_prototype_lyon::prelude::ShapePlugin;
use room_proto::event::{AgentRef, EventKind};

use crate::anim::Animating;
use crate::log::{AutoAccept, PendingPermission, PendingQuestion, RoomLog, agent_label, always_asks, parse_questions};
use room_proto::{Decision, PermissionReply};
use crate::model::{Model, Who};
use crate::scene::{Roster, SceneChange};
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
        // When the window changes size (mini mode, resizing) one frame can still mix the
        // old and new sizes; wgpu rejects it and Bevy would quit. Skip that frame instead;
        // the next one is consistent again.
        .insert_resource(RenderErrorHandler(|_, _, _| RenderErrorPolicy::Ignore))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(window::initial_window(saved)),
            ..default()
        }))
        .add_plugins((EguiPlugin::default(), ShapePlugin))
        .add_message::<SceneChange>()
        .add_message::<mini::ToggleMini>()
        .insert_resource(mini::ViewMode::from_saved(saved))
        .insert_resource(server)
        .insert_resource(NeedsCornerPlacement(saved.is_none()))
        .init_resource::<RoomLog>()
        .init_resource::<AutoAccept>()
        .init_resource::<camera::CameraControl>()
        .init_resource::<Model>()
        .init_resource::<Roster>()
        .init_resource::<Animating>()
        .add_systems(Startup, ((scene::load_font, scene::spawn_room).chain(), spawn_ui_camera, connect_waker))
        .add_systems(
            Update,
            (
                (window::place_in_corner, window::hotkeys, mini::toggle, mini::drag, mini::apply_mode, window::remember_geometry)
                    .chain(),
                receive_events,
                scene::apply_changes,
                scene::apply_pending,
                mini::hide_new_desks,
                scene::route_agents,
                (anim::walk, anim::leave_when_done, anim::animate_parts, anim::update_bubbles, anim::update_chips),
                (anim::hand_over, anim::pulse),
                anim::detect_motion,
                camera::camera_input,
                scene::follow_action,
                settle_permissions,
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

/// Every hook event: into the model (which tells the scene what to show), the log,
/// and the permission cards.
fn receive_events(
    server: Res<Server>,
    mut model: ResMut<Model>,
    mut log: ResMut<RoomLog>,
    auto: Res<AutoAccept>,
    mut scene: MessageWriter<SceneChange>,
) {
    let now = Instant::now();
    for Incoming { event, reply } in server.events.try_iter() {
        let internal_helper = model.is_internal_helper(&event);
        for change in model.apply(&event) {
            trace!("{change:?}");
            scene.write(SceneChange(change));
        }
        let report_from = match &event.kind {
            EventKind::ReportReceived { from_agent } => model
                .agent_type(&event.session_id, from_agent)
                .map(|agent_type| AgentRef::Sub { id: from_agent.clone(), agent_type: agent_type.to_owned() }),
            _ => None,
        };
        match report_from {
            Some(sender) => log.note(
                &event.session_id,
                agent_label(&event.agent),
                format!("Got the report from {}", agent_label(&sender)),
            ),
            None if internal_helper => {}
            None => log.push(&event),
        }
        let agent = agent_label(&event.agent);
        match (reply, &event.kind) {
            (Some(reply), EventKind::PermissionAsked { tool }) if auto.0 && !always_asks(tool) => {
                reply.send(&PermissionReply { decision: Decision::Allow, message: None, answers: None });
                log.note(&event.session_id, agent, format!("Auto-allowed {}", tool.describe().1));
            }
            (Some(reply), EventKind::PermissionAsked { tool }) => log.pending.push(PendingPermission {
                session: event.session_id.clone(),
                who: match &event.agent {
                    AgentRef::Main => Who::Main,
                    AgentRef::Sub { id, .. } => Who::Sub(id.clone()),
                },
                agent,
                tool: tool.clone(),
                cwd: event.cwd.clone(),
                shown_at: now,
                expires_at: now + room_proto::permission_timeout(),
                reply,
                reason: String::new(),
            }),
            (Some(reply), EventKind::ToolStarted { tool, .. }) if tool.name == room_proto::QUESTION_TOOL => {
                log.questions.push(PendingQuestion {
                    session: event.session_id.clone(),
                    agent,
                    questions: parse_questions(&tool.input),
                    shown_at: now,
                    expires_at: now + room_proto::permission_timeout(),
                    reply,
                });
            }
            _ => {}
        }
        // Claude's final answer pops up in the middle (PLAN.md M9).
        let shown = model.shown().is_some_and(|session| session.id == event.session_id);
        match &event.kind {
            EventKind::TurnEnded { last_message: Some(text) } if shown && event.agent == AgentRef::Main => {
                log.final_answer = Some(text.clone());
            }
            EventKind::UserPrompt { .. } if shown => log.final_answer = None,
            _ => {}
        }
    }
}

/// Removes cards nobody can answer here any more: the hook timed out (Claude Code asks
/// in the terminal) or Claude Code stopped waiting (Esc, interrupted turn).
fn settle_permissions(mut log: ResMut<RoomLog>) {
    let now = Instant::now();
    // Questions: same rules; the terminal shows them once the hook gives up.
    let before = log.questions.len();
    let mut gone = Vec::new();
    log.questions.retain(|card| {
        let keep = card.expires_at > now && !card.reply.is_closed();
        if !keep {
            gone.push((card.session.clone(), card.agent.clone()));
        }
        keep
    });
    if log.questions.len() != before {
        for (session, agent) in gone {
            log.note(&session, agent, "Question: answered in the terminal".to_owned());
        }
    }
    if log.pending.is_empty() {
        return;
    }
    let mut index = 0;
    while index < log.pending.len() {
        let card = &log.pending[index];
        // The hook gives up at the same moment the countdown ends, so a connection that
        // closes in the last second is a timeout, not a cancel.
        let timed_out = card.expires_at <= now + Duration::from_secs(1);
        let outcome = if card.expires_at <= now {
            "answered in the terminal"
        } else if card.reply.is_closed() {
            if timed_out { "answered in the terminal" } else { "cancelled" }
        } else {
            index += 1;
            continue;
        };
        let card = log.pending.remove(index);
        log.note(&card.session, card.agent, format!("{}: {outcome}", card.tool.name));
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
    } else if !log.pending.is_empty() || !log.questions.is_empty() {
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
