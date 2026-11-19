//! Agent Room: a small always-on-top window that shows what Claude Code and its
//! sub-agents are doing, and lets the user answer permission prompts.
//!
//! A 3D space-station HQ where every agent is a robot walking to the station for its
//! work (`director`, `view3d`, `world3d`), with a HUD, the event log and permission
//! cards (egui) over and beside it.

mod activity;
mod assets;
mod chat;
mod compose;
mod console;
mod diff;
mod director;
mod log;
mod look;
mod mini;
mod model;
mod notify;
mod palette;
mod replay;
mod report;
mod repo;
mod server;
mod space;
mod hud;
mod hull;
mod view3d;
mod wins;
mod work;
mod world3d;
mod ui;
mod usage;
mod window;

use std::time::{Duration, Instant};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::CameraOutputMode;
use bevy::prelude::*;
use bevy::render::error_handler::{RenderErrorHandler, RenderErrorPolicy};
use bevy::render::render_resource::BlendState;
use bevy::winit::{EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};
use room_proto::event::{AgentRef, EventKind};

use crate::log::{AutoAccept, FinalAnswer, PendingPermission, PendingPlan, PendingQuestion, RoomLog, agent_label, always_asks, parse_questions};
use room_proto::{Decision, PermissionReply};
use crate::model::{Model, Who};
use crate::server::{Incoming, Server};
use crate::window::{NeedsCornerPlacement, SavedWindow, ScreenshotMode};

/// How often to redraw (focused, unfocused), from busiest to quietest. Focused, the
/// user is looking: always smooth (30 fps; the low rates looked laggy). Unfocused,
/// the widget sits beside the terminal: a 3D frame costs about 20 ms of CPU, so it
/// draws only as often as needed (V2.md phase 0). Dozing draws nothing either way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pace {
    /// A robot walks or beams, the camera glides (unfocused: 10 fps).
    Moving,
    /// Robots work in place, their clips playing (unfocused: 4 fps).
    Working,
    /// Nothing happens: the idle clip (unfocused: 4 fps), for 30 s.
    Idle,
    /// Nothing happened for a while: no redraws until input or an event.
    Dozing,
}

impl Pace {
    fn every(self) -> Option<(Duration, Duration)> {
        let ms = Duration::from_millis;
        match self {
            // Focused (the user looks at it or uses it): smooth. Unfocused (it sits
            // beside the terminal): cheap, walking still readable.
            Pace::Moving => Some((ms(33), ms(100))),
            Pace::Working => Some((ms(33), ms(250))),
            Pace::Idle => Some((ms(33), ms(250))),
            Pace::Dozing => None,
        }
    }
}

/// After this long without activity the robots doze.
const IDLE_3D_FOR: Duration = Duration::from_secs(30);
/// Redraw rate while only a permission countdown changes.
const COUNTDOWN_TICK: Duration = Duration::from_millis(250);

fn main() -> AppExit {
    replay::prune();
    let server = match Server::start() {
        Ok(server) => server,
        Err(err) => {
            eprintln!("agent-room: {err}");
            return AppExit::error();
        }
    };
    // Pauses and stops from an earlier run must not hold today's sessions.
    room_proto::hold::clear_all();
    let saved = SavedWindow::load();

    let mut app = App::new();
    app
        // Sleep until input or a hook event arrives (PLAN.md 6).
        .insert_resource(WinitSettings::desktop_app())
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.06)))
        // When the window changes size (mini mode, resizing) one frame can still mix the
        // old and new sizes; wgpu rejects it and Bevy would quit. Skip that frame instead;
        // the next one is consistent again.
        .insert_resource(RenderErrorHandler(|_, _, _| RenderErrorPolicy::Ignore))
        // The 3D models are compiled in (PLAN.md M21); must come before AssetPlugin.
        .add_plugins(assets::EmbeddedAssetsPlugin)
        .add_plugins(
            DefaultPlugins
                // No sound at all: an open output stream keeps macOS's audio service
                // busy (V2.md phase 0), and sounds made other apps' audio crackle.
                .build()
                .disable::<bevy::audio::AudioPlugin>()
                .set(WindowPlugin { primary_window: Some(window::initial_window(saved)), ..default() })
                // Three threads, not one per core: waking a big pool every frame cost more
                // than the work (V2.md phase 0: about 35 % to 22 % of a core while active).
                .set(TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(3) }),
        )
        .add_plugins(EguiPlugin::default())
        .add_message::<mini::ToggleMini>()
        .init_resource::<view3d::View3d>()
        .init_resource::<view3d::CameraView>()
        .init_resource::<notify::Toasts>()
        .init_resource::<notify::Bells>()
        .init_resource::<replay::Replay>()
        .insert_resource(look::Look::built_in())
        .init_resource::<usage::Usage>()
        .init_resource::<wins::Wins>()
        .init_resource::<view3d::BrightHolos>()
        .init_resource::<hud::Trace>()
        .init_resource::<hud::Selected>()
        .insert_resource(mini::ViewMode::from_saved(saved))
        .insert_resource(server)
        .insert_resource(NeedsCornerPlacement(saved.is_none()))
        .init_resource::<RoomLog>()
        .insert_resource(ui::PanelPrefs::load())
        .insert_resource(console::OpenConsole::from_env())
        .init_resource::<activity::Activity>()
        .init_resource::<diff::DiffView>()
        .init_resource::<AutoAccept>()
        .init_resource::<repo::RepoCache>()
        .init_resource::<ui::MarkdownCache>()
        .init_resource::<chat::Chats>()
        .init_resource::<Model>()
        .add_systems(Startup, (view3d::build, spawn_ui_camera, connect_waker, chat::init_folder))
        .add_systems(
            Update,
            (
                (window::place_in_corner, window::hotkeys, mini::toggle, mini::drag, mini::apply_mode, mini::settle_redraw, window::remember_geometry)
                    .chain(),
                (receive_events, replay::tick, chat::dev_chat, chat::read_chats, end_dead_sessions),
                settle_permissions,
                hud::title,
                (view3d::sync, look::reload, look::apply, view3d::desk_lights, view3d::teleporter, view3d::deliver, view3d::links, view3d::context_tank, view3d::celebrate, world3d::daylight, view3d::activity_holograms, view3d::light_trails, hud::trace, view3d::camera_input, view3d::frame).chain(),
                pace_updates,
                log_frame_rate.run_if(|| std::env::var_os("AGENT_ROOM_FPS_LOG").is_some()),
                window::take_screenshots,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, (ui::draw, view3d::labels, view3d::trophy_clicks, hud::draw).chain());
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
/// the permission cards and the notifications.
#[allow(clippy::too_many_arguments)]
fn receive_events(
    server: Res<Server>,
    mut model: ResMut<Model>,
    mut log: ResMut<RoomLog>,
    auto: Res<AutoAccept>,
    mode: Res<mini::ViewMode>,
    mut repos: ResMut<repo::RepoCache>,
    mut toasts: ResMut<notify::Toasts>,
    mut bells: ResMut<notify::Bells>,
    prefs: Res<ui::PanelPrefs>,
    mut usage: ResMut<usage::Usage>,
    mut wins: ResMut<wins::Wins>,
    mut activity: ResMut<activity::Activity>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut replay: ResMut<replay::Replay>,
) {
    // During a mission replay the room shows the replay; live events go on into the
    // parked live model (PLAN.md M28).
    let model = replay.live_model(&mut model);
    let now = Instant::now();
    for limits in server.limits.try_iter() {
        usage.report_limits(limits);
    }
    for Incoming { event, reply } in server.events.try_iter() {
        let internal_helper = model.is_internal_helper(&event);
        if !internal_helper {
            activity.on_event(&event);
        }
        for change in model.apply(&event) {
            trace!("{change:?}");
        }
        // Tokens: once per turn, from what the transcript gained (cheap).
        if matches!(event.kind, EventKind::TurnEnded { .. } | EventKind::SessionEnded { .. })
            && event.agent == AgentRef::Main
            && let Some(transcript) = &event.transcript_path
        {
            usage.update(&event.session_id, transcript);
        }
        // A sub-agent's own tokens, from its transcript when it stops (PLAN.md M28).
        if let (EventKind::SubagentStopped { .. }, AgentRef::Sub { id, .. }, Some(transcript)) =
            (&event.kind, &event.agent, &event.agent_transcript_path)
        {
            usage.update(&usage::agent_key(&event.session_id, id), transcript);
        }
        // The tank empties when compaction is done (PostCompact). A plugin from before
        // PostCompact was forwarded never says so: then the next prompt or turn end does.
        match &event.kind {
            EventKind::Compacting => usage.compacting(&event.session_id),
            EventKind::Compacted => usage.compacted(&event.session_id),
            EventKind::UserPrompt { .. } | EventKind::TurnEnded { .. } if usage.is_compacting(&event.session_id) => {
                usage.compacted(&event.session_id)
            }
            _ => {}
        }
        // A toast for what matters, from sessions the user works in.
        if !internal_helper && let Some(session) = model.session(&event.session_id).filter(|session| !session.headless) {
            let project = (model.live().len() > 1).then(|| session.name());
            if let Some((kind, text)) = notify::notice(&event, project.as_deref()) {
                // Claude waits for the user, who is in another app: tell macOS.
                if kind == notify::Kind::Waiting && prefs.notify && !window.focused {
                    notify::system_notification(&text);
                }
                toasts.push(kind, text);
            }
            // "Tell me when done": the user asked to hear when this session goes idle.
            if let Some(text) = bells.ring(&event, &session.name()) {
                notify::bell(&text);
            }
            // Commits and passing tests get a burst of light; finished turns, a trophy.
            if let Some(win) = wins.on_event(&event, &session.name()) {
                let who = agent_label(&event.agent);
                let text = match &project {
                    Some(project) => format!("{project} · {who}: {}", win.text()),
                    None => format!("{who}: {}", win.text()),
                };
                toasts.push(notify::Kind::Win, text);
            }
        }
        // Where everyone works (PLAN.md M4d): the session's repo from its cwd, each
        // agent's from the paths its tool calls touch.
        let who = match &event.agent {
            AgentRef::Main => Who::Main,
            AgentRef::Sub { id, .. } => Who::Sub(id.clone()),
        };
        if let Some(home) = event.cwd.as_deref().and_then(|cwd| repos.locate(std::path::Path::new(cwd))) {
            model.set_repo(&event.session_id, &who, home, true);
        }
        if let EventKind::ToolStarted { tool, .. } = &event.kind
            && let Some(repo) = repos.for_tool(tool, event.cwd.as_deref())
        {
            model.set_repo(&event.session_id, &who, repo, false);
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
            // A plan is never auto-approved: reading it is the point (PLAN.md M22).
            (Some(reply), EventKind::PermissionAsked { tool }) if tool.name == room_proto::PLAN_TOOL => {
                log.plans.push(PendingPlan::new(event.session_id.clone(), agent, tool, reply, now));
            }
            (Some(reply), EventKind::PermissionAsked { tool }) if auto.0 && !always_asks(tool, event.cwd.as_deref()) => {
                reply.send(&PermissionReply { decision: Decision::Allow, message: None, answers: None, mode: None });
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
        // Only a new prompt in the answer's own session closes it: prompts from other
        // sessions (e.g. a plugin's `claude -p` summary) must not (PLAN.md M8a status).
        let shown = model.shown().is_some_and(|session| session.id == event.session_id && !session.headless);
        match &event.kind {
            EventKind::TurnEnded { last_message: Some(text) } if shown && event.agent == AgentRef::Main => {
                // The mini widget is too small for it: it would hide the whole room. There
                // the "Last answer" button opens it.
                log.final_answer =
                    Some(FinalAnswer { session: event.session_id.clone(), text: text.clone(), open: !mode.mini });
            }
            EventKind::UserPrompt { .. } => {
                if let Some(answer) = log.final_answer.as_mut().filter(|answer| answer.session == event.session_id) {
                    answer.open = false;
                }
            }
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
    let mut gone = Vec::new();
    log.plans.retain(|card| {
        let keep = card.expires_at > now && !card.reply.is_closed();
        if !keep {
            gone.push((card.session.clone(), card.agent.clone()));
        }
        keep
    });
    for (session, agent) in gone {
        log.note(&session, agent, "Plan: answered in the terminal".to_owned());
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

/// How often to look for sessions whose `claude` process has gone.
const DEAD_CHECK_EVERY: Duration = Duration::from_secs(5);

/// Ends sessions whose `claude` process is gone (a closed terminal window or chat tab
/// never sends `SessionEnd`), so their robot and roster row leave. Runs only on frames
/// that are drawn anyway, at most every few seconds: one `kill(pid, 0)` per session.
fn end_dead_sessions(
    mut model: ResMut<Model>,
    chats: Res<chat::Chats>,
    mut bells: ResMut<notify::Bells>,
    replay: Res<replay::Replay>,
    mut last: Local<Option<Instant>>,
) {
    // A replay's sessions carry the pids of long-gone processes: leave them be.
    if replay.active.is_some() {
        return;
    }
    let now = Instant::now();
    if last.is_some_and(|at| now - at < DEAD_CHECK_EVERY) {
        return;
    }
    *last = Some(now);
    for chat in &chats.sessions {
        if let Some(id) = &chat.session_id {
            model.set_pid(id, chat.pid());
        }
    }
    for id in model.end_dead(process_alive) {
        debug!("session {id} ended: its claude process is gone");
        // A killed session sends no SessionEnd, so its bell rings here.
        if bells.0.remove(&id) {
            let name = model.session(&id).map_or_else(|| id.clone(), |session| session.name());
            notify::bell(&format!("{name}: the session ended"));
        }
    }
}

/// Whether a process with this pid exists (signal 0 only checks; nothing is sent).
/// A process of another user answers EPERM: it exists.
fn process_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let Ok(pid) = i32::try_from(pid) else { return true };
    if pid <= 0 {
        return true;
    }
    // SAFETY: `kill` with signal 0 sends nothing; it only reports whether `pid` exists.
    if unsafe { kill(pid, 0) } == 0 {
        return true;
    }
    // ESRCH is 3 on macOS and Linux.
    std::io::Error::last_os_error().raw_os_error() != Some(3)
}

/// `AGENT_ROOM_FPS_LOG=1`: prints frames drawn per second every 5 s, and why
/// (for `scripts/measure.sh` and power work).
fn log_frame_rate(
    model: Res<Model>,
    view: Res<view3d::View3d>,
    log: Res<RoomLog>,
    mut window: Local<Option<(std::time::Instant, u32, u32, u32)>>,
) {
    let now = std::time::Instant::now();
    let (start, frames, busy, moving) = window.get_or_insert((now, 0, 0, 0));
    *frames += 1;
    *busy += model.live().iter().any(|session| session.is_busy()) as u32;
    *moving += view.moving as u32;
    let elapsed = now - *start;
    if elapsed >= Duration::from_secs(5) {
        let secs = elapsed.as_secs_f32();
        eprintln!(
            "fps {:.1} (busy {busy}, moving {moving}, pending {})",
            *frames as f32 / secs,
            log.pending.len() + log.questions.len() + log.plans.len()
        );
        *window = None;
    }
}

/// Redraw only while something on screen changes by itself; sleep otherwise.
#[allow(clippy::too_many_arguments)]
fn pace_updates(
    log: Res<RoomLog>,
    model: Res<Model>,
    view: Res<view3d::View3d>,
    mut toasts: ResMut<notify::Toasts>,
    screenshots: Option<Res<ScreenshotMode>>,
    mut current: Local<Option<(Duration, Duration)>>,
    mut last_busy: Local<Option<std::time::Instant>>,
    mut settings: ResMut<WinitSettings>,
) {
    if screenshots.is_some() {
        return;
    }
    let working = model.live().iter().any(|session| session.is_busy());
    let now = std::time::Instant::now();
    if working || view.moving || last_busy.is_none() {
        *last_busy = Some(now);
    }
    let pace = if view.moving {
        Pace::Moving
    } else if working {
        Pace::Working
    } else if last_busy.is_some_and(|at| now - at < IDLE_3D_FOR) {
        Pace::Idle
    } else {
        Pace::Dozing
    };
    // Countdowns need their own ticks even when all is still.
    let tick = if !log.pending.is_empty() || !log.questions.is_empty() || !log.plans.is_empty() || toasts.tidy() {
        Some(COUNTDOWN_TICK)
    } else {
        None
    };
    let wanted = match (pace.every(), tick) {
        (Some((focused, unfocused)), Some(tick)) => Some((focused.min(tick), unfocused.min(tick))),
        (None, Some(tick)) => Some((tick, tick)),
        (every, None) => every,
    };
    if wanted == *current {
        return;
    }
    *current = wanted;
    *settings = match wanted {
        Some((focused, unfocused)) => WinitSettings {
            focused_mode: UpdateMode::reactive(focused),
            unfocused_mode: UpdateMode::reactive_low_power(unfocused),
        },
        None => WinitSettings::desktop_app(),
    };
}

#[cfg(test)]
mod tests {
    use super::process_alive;

    #[test]
    fn a_process_is_alive_until_it_exits() {
        assert!(process_alive(std::process::id()));
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!process_alive(pid));
    }
}
