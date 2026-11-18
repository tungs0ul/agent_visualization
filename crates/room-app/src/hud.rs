//! The 3D view's heads-up display (PLAN.md M17, phase 5), over the workspace in the
//! style of the user's reference: a title card, the main agent's card, agent counts,
//! the execution timeline of the current turn, and a robot's details on click.
//!
//! Everything is derived from what the app already knows (the model, the log), so
//! there is no second source of truth; the turn trace only remembers which stages
//! the current turn has passed through.

use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use room_proto::describe::truncate;
use room_proto::event::AgentRef;

use crate::log::{RoomLog, agent_label};
use crate::model::{Model, Pose, Status, Who};
use crate::view3d::RoomCamera3d;

/// The stages a turn goes through, as the bottom timeline shows them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Reading,
    Research,
    Processing,
    Writing,
}

impl Stage {
    const ALL: [Stage; 4] = [Stage::Reading, Stage::Research, Stage::Processing, Stage::Writing];

    fn of(pose: Pose) -> Option<Stage> {
        match pose {
            Pose::Reading => Some(Stage::Reading),
            Pose::Web => Some(Stage::Research),
            Pose::Terminal => Some(Stage::Processing),
            Pose::Typing => Some(Stage::Writing),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Stage::Reading => "📄 Reading",
            Stage::Research => "🔎 Research",
            Stage::Processing => "⚙ Processing",
            Stage::Writing => "✏ Writing",
        }
    }
}

/// What the current turn has done so far.
#[derive(Resource, Default)]
pub struct Trace {
    session: Option<String>,
    started: Option<Instant>,
    used: Vec<Stage>,
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
/// was idle; every stage anyone works in is remembered until the next turn.
pub fn trace(model: Res<Model>, mut trace: ResMut<Trace>) {
    let Some(session) = model.shown() else { return };
    let poses = poses(session);
    let busy = poses.iter().any(|pose| *pose != Pose::Idle);
    if trace.session.as_deref() != Some(session.id.as_str()) || (busy && !trace.busy) {
        *trace = Trace { session: Some(session.id.clone()), started: busy.then(Instant::now), used: Vec::new(), busy };
    }
    trace.busy = busy;
    for stage in poses.into_iter().filter_map(Stage::of) {
        if !trace.used.contains(&stage) {
            trace.used.push(stage);
        }
    }
}

const GLASS: egui::Color32 = egui::Color32::from_rgba_premultiplied(10, 14, 32, 225);
const EDGE: egui::Color32 = egui::Color32::from_rgb(60, 110, 200);
const CYAN: egui::Color32 = egui::Color32::from_rgb(90, 210, 255);
const GREEN: egui::Color32 = egui::Color32::from_rgb(80, 220, 140);
const DIM: egui::Color32 = egui::Color32::from_gray(150);
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
) -> Result {
    if mode.mini {
        return Ok(());
    }
    let Ok(camera) = camera.single() else { return Ok(()) };
    let Some(rect) = camera.logical_viewport_rect() else { return Ok(()) };
    let area = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.min.y), egui::pos2(rect.max.x, rect.max.y));
    let ctx = contexts.ctx_mut()?;

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
                        let dot = if *busy { "⚡" } else { "⏸" };
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
    egui::Area::new("hud-main".into())
        .fixed_pos(egui::pos2(area.center().x, area.min.y + 8.0))
        .pivot(egui::Align2::CENTER_TOP)
        .interactable(false)
        .show(ctx, |ui| {
            card().show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.horizontal(|ui| {
                    let (dot, state) = if trace.busy { (GREEN, "working") } else { (DIM, "idle") };
                    ui.label(egui::RichText::new("⚡").color(dot));
                    ui.label(egui::RichText::new("Claude").strong().color(WHITE));
                    ui.label(egui::RichText::new(format!("· main agent · {state}")).color(DIM));
                });
                if let Some(prompt) = &prompt {
                    ui.label(egui::RichText::new(format!("Working on: {}", truncate(prompt, 80))).color(CYAN));
                }
                if let Some(doing) = &session.main_doing {
                    ui.label(egui::RichText::new(truncate(doing, 90)).small().color(DIM));
                }
            });
        });

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
                    ui.label(egui::RichText::new(format!("{agents} agents")).color(WHITE));
                    ui.label(egui::RichText::new(format!("✔ {active} active")).color(GREEN));
                    if let Some(started) = trace.started {
                        ui.label(egui::RichText::new(format!("⏱ {}", elapsed(started))).color(DIM));
                    }
                });
            });
        });

    // The turn's stages, bottom left: done, running or pending.
    let running: Vec<Stage> = poses.into_iter().filter_map(Stage::of).collect();
    egui::Area::new("hud-timeline".into())
        .fixed_pos(egui::pos2(area.min.x + 8.0, area.max.y - 8.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .interactable(false)
        .show(ctx, |ui| {
            card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, stage) in Stage::ALL.into_iter().enumerate() {
                        if i > 0 {
                            ui.label(egui::RichText::new("➡").color(DIM));
                        }
                        let (state, color) = if running.contains(&stage) {
                            ("⟳ Running", CYAN)
                        } else if trace.used.contains(&stage) {
                            ("✔ Done", GREEN)
                        } else {
                            ("⌛ Pending", DIM)
                        };
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(stage.label()).strong().color(WHITE));
                            ui.label(egui::RichText::new(state).small().color(color));
                        });
                    }
                });
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
            ui.label(egui::RichText::new(format!("Status: {state}")).color(CYAN));
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
