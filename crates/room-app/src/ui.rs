//! egui overlay: permission cards and the event log, beside the room when the window
//! is wide, or a one-line ticker under it when the window is small (PLAN.md 4.6).

use std::time::Instant;

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};
use room_proto::event::ToolCall;
use room_proto::{Decision, PermissionReply};

use crate::log::RoomLog;
use crate::scene::WorldCamera;

const DENY_MESSAGE: &str = "The user denied this in Agent Room.";
/// From this logical width on, the log gets its own side panel.
const WIDE_LAYOUT: f32 = 700.0;
const SIDE_PANEL_WIDTH: f32 = 320.0;
const PANEL: egui::Color32 = egui::Color32::from_rgb(0xFB, 0xF7, 0xF1);
const ALLOW: egui::Color32 = egui::Color32::from_rgb(0x3E, 0x9B, 0x5F);
const DENY: egui::Color32 = egui::Color32::from_rgb(0xC9, 0x4F, 0x4F);

pub fn draw(
    mut contexts: EguiContexts,
    mut log: ResMut<RoomLog>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Camera, With<WorldCamera>>,
    mut styled: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*styled {
        // Light panels that sit well next to the warm room.
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = PANEL;
        ctx.set_visuals(visuals);
        *styled = true;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let now = Instant::now();
    let mut answers: Vec<(usize, Decision)> = Vec::new();

    if ctx.viewport_rect().width() >= WIDE_LAYOUT {
        egui::Panel::right("side").exact_size(SIDE_PANEL_WIDTH).show(&mut root, |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(session_title(&log)).strong());
            ui.separator();
            permission_cards(ui, &log, now, &mut answers);
            event_log(ui, &log);
        });
    } else {
        if !log.pending.is_empty() {
            egui::Panel::top("permissions").show(&mut root, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.viewport_rect().height() * 0.6)
                    .show(ui, |ui| permission_cards(ui, &log, now, &mut answers));
            });
        }
        egui::Panel::bottom("ticker").show(&mut root, |ui| {
            let latest = log.lines.back();
            let text = latest.map_or_else(|| session_title(&log), |line| format!("{}: {}", line.agent, line.text));
            ui.add(egui::Label::new(text).truncate());
        });
    }
    let window_size = UVec2::new(window.physical_width(), window.physical_height());
    fit_camera(&mut camera, root.available_rect_before_wrap(), window.scale_factor(), window_size);

    // Highest index first, so removing one doesn't shift the others.
    answers.sort_by_key(|&(index, _)| std::cmp::Reverse(index));
    for (index, decision) in answers {
        let pending = log.pending.remove(index);
        let message = (decision == Decision::Deny).then(|| DENY_MESSAGE.to_owned());
        pending.reply.send(&PermissionReply { decision, message });
        let verb = match decision {
            Decision::Allow => "Allowed",
            Decision::Deny => "Denied",
        };
        log.note(pending.agent, format!("{verb} {}", pending.tool.describe().1));
    }
    Ok(())
}

fn session_title(log: &RoomLog) -> String {
    match &log.session {
        Some(session) => session.cwd.clone().unwrap_or_else(|| session.id.clone()),
        None => "Waiting for Claude Code…".to_owned(),
    }
}

fn permission_cards(ui: &mut egui::Ui, log: &RoomLog, now: Instant, answers: &mut Vec<(usize, Decision)>) {
    for (index, pending) in log.pending.iter().enumerate() {
        ui.group(|ui| {
            ui.label(egui::RichText::new(format!("{} wants to use {}", pending.agent, pending.tool.name)).strong());
            // Always the full input: a shortened command could hide what it does.
            egui::ScrollArea::vertical()
                .id_salt(("permission", index))
                .max_height(120.0)
                .show(ui, |ui| ui.label(egui::RichText::new(full_input(&pending.tool)).monospace()));
            if let Some(cwd) = &pending.cwd {
                ui.small(format!("in {cwd}"));
            }
            ui.horizontal(|ui| {
                let enabled = pending.accepts_input(now);
                if ui.add_enabled(enabled, colored_button("Allow", ALLOW)).clicked() {
                    answers.push((index, Decision::Allow));
                }
                if ui.add_enabled(enabled, colored_button("Deny", DENY)).clicked() {
                    answers.push((index, Decision::Deny));
                }
                let left = pending.expires_at.saturating_duration_since(now).as_secs();
                ui.small(format!("{left} s, then the terminal asks"));
            });
        });
    }
}

fn colored_button(text: &str, fill: egui::Color32) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE).strong()).fill(fill)
}

fn event_log(ui: &mut egui::Ui, log: &RoomLog) {
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink(false)
        .show(ui, |ui| {
            for line in &log.lines {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(&line.agent).strong());
                    ui.label(&line.text);
                });
            }
        });
}

/// Limits the room camera to the area the panels leave free (egui points are logical
/// pixels; viewports are physical).
///
/// egui lays out with last frame's window size, so right after the user shrinks the
/// window the free area can stick out of it. A viewport outside the window makes wgpu
/// reject the frame and Bevy quits, so clamp to the current size.
fn fit_camera(camera: &mut Camera, free: egui::Rect, scale: f32, window_size: UVec2) {
    let mut viewport = Viewport {
        physical_position: UVec2::new((free.min.x * scale).max(0.0) as u32, (free.min.y * scale).max(0.0) as u32),
        physical_size: UVec2::new((free.width() * scale).max(1.0) as u32, (free.height() * scale).max(1.0) as u32),
        ..default()
    };
    viewport.clamp_to_size(window_size);
    let unchanged = camera.viewport.as_ref().is_some_and(|v| {
        v.physical_position == viewport.physical_position && v.physical_size == viewport.physical_size
    });
    if !unchanged {
        camera.viewport = Some(viewport);
    }
}

fn full_input(tool: &ToolCall) -> String {
    if let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) {
        return command.to_owned();
    }
    serde_json::to_string_pretty(&tool.input).unwrap_or_default()
}
