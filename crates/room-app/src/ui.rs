//! egui overlay: session header, permission cards, event log.

use std::time::Instant;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use room_proto::event::ToolCall;
use room_proto::{Decision, PermissionReply};

use crate::log::RoomLog;

const DENY_MESSAGE: &str = "The user denied this in Agent Room.";

pub fn draw(mut contexts: EguiContexts, mut log: ResMut<RoomLog>) -> Result {
    let ctx = contexts.ctx_mut()?;
    let mut root = egui::Ui::new(
        ctx.clone(),
        "root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let now = Instant::now();
    let mut answers: Vec<(usize, Decision)> = Vec::new();

    egui::Panel::top("session").show(&mut root, |ui| {
        let title = match &log.session {
            Some(session) => session.cwd.clone().unwrap_or_else(|| session.id.clone()),
            None => "Waiting for Claude Code…".to_owned(),
        };
        ui.label(egui::RichText::new(title).strong());
    });

    if !log.pending.is_empty() {
        egui::Panel::top("permissions").show(&mut root, |ui| {
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
                        if ui.add_enabled(enabled, egui::Button::new("Allow")).clicked() {
                            answers.push((index, Decision::Allow));
                        }
                        if ui.add_enabled(enabled, egui::Button::new("Deny")).clicked() {
                            answers.push((index, Decision::Deny));
                        }
                        let left = pending.expires_at.saturating_duration_since(now).as_secs();
                        ui.small(format!("{left} s, then the terminal asks"));
                    });
                });
            }
        });
    }

    egui::CentralPanel::default().show(&mut root, |ui| {
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
    });

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

fn full_input(tool: &ToolCall) -> String {
    if let Some(command) = tool.input.get("command").and_then(|v| v.as_str()) {
        return command.to_owned();
    }
    serde_json::to_string_pretty(&tool.input).unwrap_or_default()
}
