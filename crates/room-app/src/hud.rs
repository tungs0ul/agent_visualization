//! The 3D view's heads-up display (PLAN.md M17, phase 5), over the workspace in the
//! style of the user's reference: a title card, the main agent's card, agent counts
//! with the turn's time, and a robot's details on click.
//!
//! Everything is derived from what the app already knows (the model, the log), so
//! there is no second source of truth; the turn trace only remembers when the
//! current turn started.

use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use room_proto::describe::truncate;
use room_proto::event::AgentRef;

use crate::log::{RoomLog, agent_label};
use crate::model::{Model, Pose, Status, Who};
use crate::view3d::RoomCamera3d;

/// When the current turn started.
#[derive(Resource, Default)]
pub struct Trace {
    session: Option<String>,
    started: Option<Instant>,
    busy: bool,
}

/// The robot whose details are open.
#[derive(Resource, Default)]
pub struct Selected(pub Option<Who>);

/// Poses of everyone still in the shown session: Claude first.
fn poses(session: &crate::model::SessionState) -> Vec<Pose> {
    std::iter::once(session.main_pose)
        .chain(session.agents.iter().filter(|(_, info)| info.status != Status::Done).map(|(_, info)| info.pose))
        .collect()
}

/// Follows the shown session: a turn starts when someone gets busy after everyone
/// was idle.
pub fn trace(model: Res<Model>, mut trace: ResMut<Trace>) {
    let Some(session) = model.shown() else { return };
    let poses = poses(session);
    let busy = poses.iter().any(|pose| *pose != Pose::Idle);
    if trace.session.as_deref() != Some(session.id.as_str()) || (busy && !trace.busy) {
        *trace = Trace { session: Some(session.id.clone()), started: busy.then(Instant::now), busy };
    }
    trace.busy = busy;
}

const GLASS: egui::Color32 = egui::Color32::from_rgba_premultiplied(10, 14, 32, 225);
const EDGE: egui::Color32 = egui::Color32::from_rgb(60, 110, 200);
const CYAN: egui::Color32 = egui::Color32::from_rgb(90, 210, 255);
const GREEN: egui::Color32 = egui::Color32::from_rgb(80, 220, 140);
const DIM: egui::Color32 = egui::Color32::from_gray(150);
const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 190, 80);
const WHITE: egui::Color32 = egui::Color32::WHITE;

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(GLASS)
        .stroke(egui::Stroke::new(1.0, EDGE))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(14, 10))
}

fn elapsed(since: Instant) -> String {
    let secs = since.elapsed().as_secs();
    if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
}

/// "12s", "3m", "2h": how long ago.
pub fn ago(at: Instant) -> String {
    let secs = at.elapsed().as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

/// Draws the HUD over the 3D view (not in mini mode, where space is short).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mode: Res<crate::mini::ViewMode>,
    mut model: ResMut<Model>,
    log: Res<RoomLog>,
    trace: Res<Trace>,
    mut selected: ResMut<Selected>,
    camera: Query<&Camera, With<RoomCamera3d>>,
    toasts: Res<crate::notify::Toasts>,
    usage: Res<crate::usage::Usage>,
) -> Result {
    let Ok(camera) = camera.single() else { return Ok(()) };
    let Some(rect) = camera.logical_viewport_rect() else { return Ok(()) };
    let area = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y), egui::pos2(rect.max.x, rect.max.y));
    let ctx = contexts.ctx_mut()?;
    // Who is blocked on the user, in both views: the most asked-for thing at a glance.
    let waiting = needs_you(&model, &log);
    if !waiting.is_empty() {
        let text = match waiting.len() {
            1 => format!("{} needs you", waiting[0]),
            n => format!("{n} sessions need you"),
        };
        let at = if mode.mini { area.min + egui::vec2(6.0, 6.0) } else { egui::pos2(area.center().x, area.max.y - 8.0) };
        let pivot = if mode.mini { egui::Align2::LEFT_TOP } else { egui::Align2::CENTER_BOTTOM };
        egui::Area::new("hud-needs-you".into()).fixed_pos(at).pivot(pivot).interactable(false).show(ctx, |ui| {
            card()
                .fill(egui::Color32::from_rgb(70, 40, 6))
                .stroke(egui::Stroke::new(1.5, AMBER))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(format!("⚠ {text}")).strong().color(AMBER));
                });
        });
    }
    if mode.mini {
        return Ok(());
    }

    // Every Claude Code session sends its events here, one room each. When more than
    // one is live, list them under the counts so the user sees them and can switch
    // (a dot shows which ones are busy).
    let shown_id = model.shown().map(|session| session.id.clone());
    let live: Vec<(String, String, bool)> = model
        .sessions()
        .into_iter()
        .filter(|session| !session.ended && !session.headless)
        .map(|session| (session.id.clone(), session.name(), session.is_busy()))
        .collect();
    let mut picked = None;
    if live.len() > 1 {
        egui::Area::new("hud-sessions".into())
            .fixed_pos(egui::pos2(area.max.x - 8.0, area.min.y + 64.0))
            .pivot(egui::Align2::RIGHT_TOP)
            .show(ctx, |ui| {
                card().show(ui, |ui| {
                    ui.label(egui::RichText::new(format!("{} Claude sessions", live.len())).small().color(DIM));
                    for (id, name, busy) in &live {
                        let current = shown_id.as_deref() == Some(id.as_str());
                        let dot = match room_proto::hold::read(id) {
                            Some(room_proto::hold::Hold::Pause) => "⏸",
                            _ if *busy => "⚡",
                            _ => "·",
                        };
                        let text = egui::RichText::new(format!("{dot} {}", truncate(name, 24)))
                            .color(if current { CYAN } else { egui::Color32::from_gray(210) });
                        if ui.selectable_label(current, text).on_hover_text("Show this session's room").clicked() && !current {
                            picked = Some(id.clone());
                        }
                    }
                });
            });
    }
    if let Some(id) = picked {
        model.show(&id);
    }

    toast_stack(ctx, area, &toasts);

    let Some(session) = model.shown() else { return Ok(()) };

    // Title, top left.
    egui::Area::new("hud-title".into()).fixed_pos(area.min + egui::vec2(8.0, 8.0)).interactable(false).show(ctx, |ui| {
        card().show(ui, |ui| {
            ui.label(egui::RichText::new("Agent Room").strong().size(16.0).color(WHITE));
            ui.label(egui::RichText::new(format!("HQ of {}", session.name())).small().color(DIM));
        });
    });

    // The main agent's card, top centre: what it was asked and what it does now.
    let prompt = log
        .lines
        .iter()
        .rev()
        .find(|line| line.session == session.id && line.agent == "Claude" && line.text.starts_with("You: "))
        .map(|line| line.text.trim_start_matches("You: ").to_owned());
    // Pause, resume and stop (PLAN.md M25): the hold file is the only state.
    use room_proto::hold::{self, Hold};
    let held = hold::read(&session.id);
    let mut set_hold = None;
    egui::Area::new("hud-main".into())
        .fixed_pos(egui::pos2(area.center().x, area.min.y + 8.0))
        .pivot(egui::Align2::CENTER_TOP)
        .show(ctx, |ui| {
            card().show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.horizontal(|ui| {
                    let (dot, state) = match held {
                        Some(Hold::Pause) => (AMBER, "paused"),
                        _ if trace.busy => (GREEN, "working"),
                        _ => (DIM, "idle"),
                    };
                    ui.label(egui::RichText::new("⚡").color(dot));
                    ui.label(egui::RichText::new("Claude").strong().color(WHITE));
                    let model = session.model.as_deref().map(|m| format!(" · {}", crate::log::short_model(m))).unwrap_or_default();
                    ui.label(egui::RichText::new(format!("· main agent{model} · {state}")).color(DIM));
                    if session.ended {
                        return;
                    }
                    if held == Some(Hold::Pause) {
                        if ui.small_button("▶ Resume").on_hover_text("Let Claude go on").clicked() {
                            set_hold = Some(None);
                        }
                    } else if trace.busy
                        && ui.small_button("⏸ Pause").on_hover_text("Claude waits before its next tool call, sub-agents too, until you resume").clicked()
                    {
                        set_hold = Some(Some(Hold::Pause));
                    }
                    if (trace.busy || held == Some(Hold::Pause))
                        && held != Some(Hold::Stop)
                        && ui.small_button("⏹ Stop").on_hover_text("End this turn before Claude's next tool call (the session stays)").clicked()
                    {
                        set_hold = Some(Some(Hold::Stop));
                    }
                    if held == Some(Hold::Stop) && trace.busy {
                        ui.label(egui::RichText::new("stopping…").small().color(AMBER));
                    }
                });
                if let Some(prompt) = &prompt {
                    ui.label(egui::RichText::new(format!("Working on: {}", truncate(prompt, 80))).color(CYAN));
                }
                if let Some(doing) = &session.main_doing {
                    ui.label(egui::RichText::new(truncate(doing, 90)).small().color(DIM));
                }
                // How full Claude's context is (the tank on the main platform).
                if usage.is_compacting(&session.id) {
                    ui.label(egui::RichText::new("Compacting the context…").small().color(AMBER));
                } else if let Some((size, _)) = usage.context(&session.id) {
                    let window = crate::usage::window(session.model.as_deref());
                    let percent = (size * 100 / window.max(1)).min(100);
                    let color = match percent {
                        81.. => egui::Color32::from_rgb(255, 110, 100),
                        51.. => AMBER,
                        _ => GREEN,
                    };
                    ui.label(egui::RichText::new(format!("Context {percent} % of {}", crate::usage::short(window))).small().color(color))
                        .on_hover_text("How full Claude's context is, after the last turn; Claude Code compacts it when full");
                }
            });
        });

    if let Some(hold) = set_hold
        && let Err(err) = hold::write(&session.id, hold)
    {
        bevy::log::warn!("hold not saved: {err}");
    }

    // Agent counts and the turn's time, top right.
    let poses = poses(session);
    let agents = poses.len();
    let active = poses.iter().filter(|pose| **pose != Pose::Idle).count();
    egui::Area::new("hud-counts".into())
        .fixed_pos(egui::pos2(area.max.x - 8.0, area.min.y + 8.0))
        .pivot(egui::Align2::RIGHT_TOP)
        .interactable(false)
        .show(ctx, |ui| {
            card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    let noun = if agents == 1 { "agent" } else { "agents" };
                    ui.label(egui::RichText::new(format!("{agents} {noun}")).color(WHITE));
                    ui.label(egui::RichText::new(format!("✔ {active} active")).color(GREEN));
                    if let Some(started) = trace.started {
                        ui.label(egui::RichText::new(format!("⏱ {}", elapsed(started))).color(DIM));
                    }
                });
                // Updated when a turn ends (the session's own transcript).
                if let Some(tokens) = usage.tokens(&session.id) {
                    let line = format!("{} in · {} out tokens", crate::usage::short(tokens.input), crate::usage::short(tokens.output));
                    ui.label(egui::RichText::new(line).small().color(DIM)).on_hover_text(
                        "Tokens Claude used in this session so far (input includes cache reads), updated after each turn",
                    );
                }
                // The account's usage limits, for every session (PLAN.md M10).
                if let Some((line, hover, fullest)) = crate::usage::limits_line(usage.limits()) {
                    let color = match fullest {
                        90.0.. => egui::Color32::from_rgb(255, 110, 100),
                        70.0.. => AMBER,
                        _ => DIM,
                    };
                    ui.label(egui::RichText::new(line).small().color(color)).on_hover_text(hover);
                }
            });
        });

    // The clicked robot's details: status, task, what it does, report, history.
    // `AGENT_ROOM_SHOW_DETAILS=1` opens Claude's (screenshot tests can't click).
    if selected.0.is_none() && std::env::var_os("AGENT_ROOM_SHOW_DETAILS").is_some() {
        selected.0 = Some(Who::Main);
    }
    let who = match &selected.0 {
        None => return Ok(()),
        Some(Who::Main) => Some((AgentRef::Main, None)),
        Some(Who::Sub(id)) => session
            .agent(id)
            .map(|info| (AgentRef::Sub { id: id.clone(), agent_type: info.agent_type.clone() }, Some(info))),
    };
    let Some((agent, info)) = who else {
        selected.0 = None;
        return Ok(());
    };
    let name = agent_label(&agent);
    let (state, doing, task, report) = match info {
        None => (session.main_pose.word().to_owned(), session.main_doing.clone(), prompt, None),
        Some(info) => (
            if info.status == Status::Done { "done".to_owned() } else { info.pose.word().to_owned() },
            info.doing.clone(),
            info.task.clone(),
            info.report.clone(),
        ),
    };
    let mut open = true;
    egui::Window::new(name.clone())
        .id("robot-details".into())
        .open(&mut open)
        .default_pos(egui::pos2(area.center().x - 180.0, area.center().y - 140.0))
        .default_width(360.0)
        .frame(card())
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("Status: {state}")).color(CYAN));
                if let Some(pid) = session.claude_pid
                    && ui.small_button("Open terminal").on_hover_text("Bring this session's terminal to the front").clicked()
                {
                    focus_terminal(pid);
                }
            });
            if let Some(task) = task {
                ui.label(egui::RichText::new(format!("Task: {}", truncate(&task, 200))).color(WHITE));
            }
            if let Some(doing) = doing {
                ui.label(egui::RichText::new(format!("Now: {doing}")).color(DIM));
            }
            if let Some(report) = report {
                ui.label(egui::RichText::new(format!("Report: {}", truncate(&report, 300))).color(GREEN));
            }
            ui.separator();
            ui.label(egui::RichText::new("History").strong().color(WHITE));
            egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                for line in log.lines.iter().filter(|line| line.session == session.id && line.agent == name) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(format!("{:>4}", ago(line.at))).small().monospace().color(DIM));
                        ui.label(egui::RichText::new(&line.text).small().color(egui::Color32::from_gray(220)));
                    });
                }
            });
        });
    if !open {
        selected.0 = None;
    }
    Ok(())
}

/// The latest notifications, bottom right, fading out.
fn toast_stack(ctx: &egui::Context, area: egui::Rect, toasts: &crate::notify::Toasts) {
    use crate::notify::{Kind, TOAST_SECS};
    if toasts.shown.is_empty() {
        return;
    }
    egui::Area::new("hud-toasts".into())
        .fixed_pos(egui::pos2(area.max.x - 8.0, area.max.y - 8.0))
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .interactable(false)
        .show(ctx, |ui| {
            for (kind, text, at) in &toasts.shown {
                let left = 1.0 - at.elapsed().as_secs_f32() / TOAST_SECS.as_secs_f32();
                let fade = (left * 4.0).clamp(0.0, 1.0);
                let accent = match kind {
                    Kind::Joined => CYAN,
                    Kind::Finished | Kind::Done | Kind::Win => GREEN,
                    Kind::Failed => egui::Color32::from_rgb(255, 110, 100),
                    Kind::Waiting => egui::Color32::from_rgb(255, 190, 80),
                };
                card().stroke(egui::Stroke::new(1.0, accent.gamma_multiply(fade))).fill(GLASS.gamma_multiply(fade)).show(ui, |ui| {
                    let text = egui::RichText::new(text).small().color(egui::Color32::WHITE.gamma_multiply(fade));
                    ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
                });
                ui.add_space(4.0);
            }
        });
}

/// Live sessions blocked on the user (a permission card, a question, or an agent
/// waiting), by project name.
pub fn needs_you(model: &Model, log: &RoomLog) -> Vec<String> {
    model
        .live()
        .into_iter()
        .filter(|session| {
            log.pending.iter().any(|card| card.session == session.id)
                || log.questions.iter().any(|card| card.session == session.id)
                || log.plans.iter().any(|card| card.session == session.id)
                || session.main_pose == Pose::Waiting
                || session.agents.iter().any(|(_, info)| info.status == Status::Waiting)
        })
        .map(|session| session.name())
        .collect()
}

/// The window title says who needs you too (seen in the Dock menu and Mission
/// Control when the window is hidden).
pub fn title(model: Res<Model>, log: Res<RoomLog>, mut window: Single<&mut Window, With<bevy::window::PrimaryWindow>>) {
    let waiting = needs_you(&model, &log).len();
    let title = match waiting {
        0 => "Agent Room".to_owned(),
        n => format!("Agent Room · {n} need{} you", if n == 1 { "s" } else { "" }),
    };
    if window.title != title {
        window.title = title;
    }
}

/// Brings the terminal app running a `claude` process to the front: walks up its
/// parent processes to the first one inside an `.app` bundle (Ghostty, Terminal,
/// iTerm, an editor) and opens that app. In the background: `ps` per step.
pub fn focus_terminal(pid: u32) {
    std::thread::spawn(move || {
        let mut pid = pid;
        for _ in 0..12 {
            let Ok(out) = std::process::Command::new("ps").args(["-o", "ppid=,comm=", "-p", &pid.to_string()]).output() else {
                return;
            };
            let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            let Some((parent, command)) = line.split_once(char::is_whitespace) else { return };
            if let Some(end) = command.find(".app/") {
                let _ = std::process::Command::new("open").arg(&command[..end + 4]).status();
                return;
            }
            match parent.trim().parse::<u32>() {
                Ok(parent) if parent > 1 => pid = parent,
                _ => return,
            }
        }
    });
}
