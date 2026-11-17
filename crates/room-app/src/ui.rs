//! egui overlay: permission cards and the event log, beside the room when the window
//! is wide, or a one-line ticker under it when the window is small (PLAN.md 4.6).

use std::collections::VecDeque;
use std::time::Instant;

use bevy::camera::Viewport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};
use room_proto::describe::truncate;
use room_proto::event::ToolCall;
use room_proto::{Decision, PermissionReply};

use crate::log::{AutoAccept, FinalAnswer, LogLine, PendingPermission, Question, RoomLog, keyboard_target};
use crate::chat::{Chats, Speaker};
use crate::mini::{ToggleMini, ViewMode};
use crate::server::Server;
use crate::model::{Model, Pose, SessionState, Status, Who};
use crate::scene::{Agent, Roster};
use bevy::ecs::system::SystemParam;
use crate::scene::{self, SceneChange};
use crate::scene::WorldCamera;

const DENY_MESSAGE: &str = "The user denied this in Agent Room.";
/// From this logical width on, the log gets its own side panel.
const WIDE_LAYOUT: f32 = 700.0;
const SIDE_PANEL_WIDTH: f32 = 320.0;
/// Finished sub-agents kept in the roster list.
const MAX_FINISHED_ROWS: usize = 4;
const ALLOW: egui::Color32 = egui::Color32::from_rgb(0x3E, 0x9B, 0x5F);
const DENY: egui::Color32 = egui::Color32::from_rgb(0xC9, 0x4F, 0x4F);
/// Share of the window the final answer covers.
const FINAL_ANSWER_SHARE: f32 = 0.86;
const AUTO: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x8A, 0x1E);

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut log: ResMut<RoomLog>,
    mut model: ResMut<Model>,
    mut scene: MessageWriter<SceneChange>,
    mode: Res<ViewMode>,
    mut auto: ResMut<AutoAccept>,
    mut toggle_mini: MessageWriter<ToggleMini>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<&mut Camera, With<WorldCamera>>,
    in_room: InRoom,
    mut chat: ChatParams,
    mut markdown: ResMut<MarkdownCache>,
    mut panel: ResMut<PanelPrefs>,
    mut stats: ResMut<crate::stats::Stats>,
    mut styled: Local<Option<crate::theme::Look>>,
    theme: Res<crate::theme::Theme>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Panels in the room's theme (PLAN.md M14).
    if *styled != Some(theme.look()) {
        ctx.set_visuals(crate::theme::visuals(theme.look()));
        *styled = Some(theme.look());
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
    // Y / N answer the oldest card, unless the user is typing a deny reason.
    if !ctx.egui_wants_keyboard_input()
        && let Some(index) = keyboard_target(&log.pending, now)
    {
        ctx.input(|input| {
            if input.key_pressed(egui::Key::Y) {
                answers.push((index, Decision::Allow));
            } else if input.key_pressed(egui::Key::N) {
                answers.push((index, Decision::Deny));
            }
        });
    }
    if !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::A)) {
        auto.0 = !auto.0;
    }
    if !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::L)) {
        panel.toggle();
    }
    if !ctx.egui_wants_keyboard_input() && ctx.input(|input| input.key_pressed(egui::Key::I)) {
        stats.toggle();
    }
    if stats.shown {
        egui::Area::new("stats".into())
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(8.0, -8.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(egui::RichText::new(stats.line()).small().monospace());
                });
            });
    }
    let RoomLog { lines, pending, final_answer, .. } = &mut *log;
    let mut picked = None;

    if mode.mini {
        // Only the room and, when needed, the permission cards. A small button returns
        // to the full view (or press M).
        if !pending.is_empty() {
            egui::Panel::top("permissions").show(&mut root, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.viewport_rect().height() * 0.8)
                    .show(ui, |ui| permission_cards(ui, pending, now, &mut answers));
            });
        }
        egui::Area::new("leave-mini".into())
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if auto.0 {
                        ui.label(egui::RichText::new("AUTO").small().strong().color(AUTO));
                    }
                    answer_button(ui, final_answer);
                    if ui.small_button("Full").on_hover_text("Full view (M)").clicked() {
                        toggle_mini.write(ToggleMini);
                    }
                });
            });
    } else if ctx.viewport_rect().width() >= WIDE_LAYOUT && panel.hidden {
        // The log is hidden (PLAN.md M12): the room gets the whole window. Permission
        // cards still need a place.
        if !pending.is_empty() {
            egui::Panel::top("permissions").show(&mut root, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.viewport_rect().height() * 0.6)
                    .show(ui, |ui| permission_cards(ui, pending, now, &mut answers));
            });
        }
        egui::Area::new("show-panel".into())
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    answer_button(ui, final_answer);
                    if ui.small_button("Show log").on_hover_text("Show the side panel (L)").clicked() {
                        panel.toggle();
                    }
                });
            });
    } else if ctx.viewport_rect().width() >= WIDE_LAYOUT {
        egui::Panel::right("side").exact_size(SIDE_PANEL_WIDTH).show(&mut root, |ui| {
            // Nothing in here may grow the panel: widgets wrap or truncate to this width.
            ui.set_max_width(SIDE_PANEL_WIDTH - 16.0);
            ui.add_space(4.0);
            picked = session_picker(ui, &model);
            // Wraps instead of widening the panel when all buttons show.
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Mini").on_hover_text("Small widget view (M)").clicked() {
                    toggle_mini.write(ToggleMini);
                }
                auto_button(ui, &mut auto);
                if ui.small_button("Chat").on_hover_text("Talk to Claude here (or click Claude in the room)").clicked() {
                    if chat.chats.open {
                        chat.chats.open = false;
                    } else {
                        chat.chats.open_for(model.shown().and_then(|session| session.cwd.as_deref()));
                    }
                }
                answer_button(ui, final_answer);
                if ui.small_button("Hide").on_hover_text("Hide this panel (L)").clicked() {
                    panel.toggle();
                }
                if ui.small_button("Stats").on_hover_text("This app's CPU, memory and frame rate (I)").clicked() {
                    stats.toggle();
                }
            });
            ui.separator();
            if let Some(session) = model.shown() {
                roster(ui, session);
                ui.separator();
            }
            permission_cards(ui, pending, now, &mut answers);
            event_log(ui, lines, model.shown().map(|session| session.id.as_str()));
        });
    } else {
        if !pending.is_empty() {
            egui::Panel::top("permissions").show(&mut root, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(ctx.viewport_rect().height() * 0.6)
                    .show(ui, |ui| permission_cards(ui, pending, now, &mut answers));
            });
        }
        egui::Panel::bottom("ticker").show(&mut root, |ui| {
            let shown = model.shown().map(|session| session.id.as_str());
            let latest = lines.iter().rev().find(|line| shown.is_none_or(|s| line.session == s));
            let text = latest.map_or_else(|| session_title(&model), |line| format!("{}: {}", line.agent, line.text));
            ui.horizontal(|ui| {
                if ui.small_button("Mini").on_hover_text("Small widget view (M)").clicked() {
                    toggle_mini.write(ToggleMini);
                }
                auto_button(ui, &mut auto);
                answer_button(ui, final_answer);
                ui.add(egui::Label::new(text).truncate());
            });
        });
    }
    if let Some(id) = picked {
        for change in model.show(&id) {
            scene.write(SceneChange(change));
        }
    }
    let window_size = UVec2::new(window.physical_width(), window.physical_height());
    fit_camera(&mut camera, root.available_rect_before_wrap(), window.scale_factor(), window_size);

    // The oldest permission card also floats next to whoever asks (PLAN.md M4e).
    if let Some(index) = log.pending.iter().position(|card| model.shown().is_some_and(|s| s.id == card.session))
        && let Some(at) = in_room.screen_position(&camera, &log.pending[index].who)
    {
        let card = &log.pending[index];
        let enabled = card.accepts_input(now);
        egui::Area::new("card-in-room".into())
            .fixed_pos(egui::pos2(at.x + 36.0, at.y - 70.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(260.0);
                    ui.label(egui::RichText::new(format!("May I? {}", card.tool.name)).strong());
                    ui.add(egui::Label::new(egui::RichText::new(full_input(&card.tool)).monospace().small()).truncate());
                    ui.horizontal(|ui| {
                        if ui.add_enabled(enabled, colored_button("Allow", ALLOW)).clicked() {
                            answers.push((index, Decision::Allow));
                        }
                        if ui.add_enabled(enabled, colored_button("Deny", DENY)).clicked() {
                            answers.push((index, Decision::Deny));
                        }
                    });
                });
            });
    }
    // Highest index first, so removing one doesn't shift the others.
    answers.sort_by_key(|&(index, _)| std::cmp::Reverse(index));
    answers.dedup_by_key(|&mut (index, _)| index);
    for (index, decision) in answers.drain(..) {
        let pending = log.pending.remove(index);
        let reason = pending.reason.trim();
        let message = (decision == Decision::Deny)
            .then(|| if reason.is_empty() { DENY_MESSAGE.to_owned() } else { format!("The user denied this: {reason}") });
        pending.reply.send(&PermissionReply { decision, message, answers: None });
        let verb = match decision {
            Decision::Allow => "Allowed",
            Decision::Deny => "Denied",
        };
        log.note(&pending.session, pending.agent, format!("{verb} {}", pending.tool.describe().1));
    }
    chat_window(ctx, &mut chat, &mut markdown, &model);
    question_popup(ctx, &mut log, now);
    // The chat window already shows Claude's answers; don't cover it with the pop-up.
    if !chat.chats.open {
        final_answer_popup(ctx, &mut log, &mut markdown, mode.mini);
    }
    Ok(())
}

/// Where characters are on screen, for cards drawn next to them.
#[derive(SystemParam)]
pub struct InRoom<'w, 's> {
    roster: Res<'w, Roster>,
    characters: Query<'w, 's, &'static GlobalTransform, With<Agent>>,
    camera: Query<'w, 's, &'static GlobalTransform, With<WorldCamera>>,
}

impl InRoom<'_, '_> {
    /// Logical window position just above the character's head.
    fn screen_position(&self, camera: &Camera, who: &Who) -> Option<Vec2> {
        let character = self.characters.get(self.roster.character(who)?).ok()?;
        let camera_transform = self.camera.single().ok()?;
        camera.world_to_viewport(camera_transform, character.translation() + Vec3::new(0.0, 60.0, 0.0)).ok()
    }
}

/// Opens Claude's last answer again after it was closed (by mistake, or by the next
/// prompt). Shown only when there is a closed answer to open.
fn answer_button(ui: &mut egui::Ui, answer: &mut Option<FinalAnswer>) {
    let Some(answer) = answer.as_mut().filter(|answer| !answer.open) else { return };
    if ui.small_button("Last answer").on_hover_text("Show Claude's last answer again").clicked() {
        answer.open = true;
    }
}

/// Off: a plain button. On: amber, so it is impossible to miss (PLAN.md M9).
fn auto_button(ui: &mut egui::Ui, auto: &mut AutoAccept) {
    let button = if auto.0 {
        egui::Button::new(egui::RichText::new("Auto-accept ON").small().strong().color(egui::Color32::WHITE)).fill(AUTO)
    } else {
        egui::Button::new(egui::RichText::new("Auto-accept").small())
    };
    let hint = "Allow permission requests without asking (A). Risky commands like rm -rf or git push still ask.";
    if ui.add(button).on_hover_text(hint).clicked() {
        auto.0 = !auto.0;
    }
}

/// Claude's multiple-choice question, in the middle of the window.
fn question_popup(ctx: &egui::Context, log: &mut RoomLog, now: Instant) {
    let Some(card) = log.questions.first_mut() else { return };
    let (mut send, mut terminal) = (false, false);
    egui::Window::new(format!("{} asks", card.agent))
        .id("question".into())
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .max_width(420.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(ctx.viewport_rect().height() * 0.6).show(ui, |ui| {
                for (index, question) in card.questions.iter_mut().enumerate() {
                    let Question { text, header, options, multi, chosen, other } = question;
                    ui.label(egui::RichText::new(header.as_str()).small().weak());
                    ui.label(egui::RichText::new(text.as_str()).strong());
                    for (i, option) in options.iter().enumerate() {
                        if ui.selectable_label(chosen[i], &option.label).clicked() {
                            let pick = !chosen[i];
                            if !*multi {
                                chosen.iter_mut().for_each(|c| *c = false);
                            }
                            chosen[i] = pick;
                        }
                        if !option.description.is_empty() {
                            ui.small(&option.description);
                        }
                    }
                    ui.add(egui::TextEdit::singleline(other).hint_text("Or type an answer").id_salt(("other", index)));
                    ui.separator();
                }
            });
            ui.horizontal(|ui| {
                let ready = card.answers().is_some() && card.accepts_input(now);
                if ui.add_enabled(ready, colored_button("Send", ALLOW)).clicked() {
                    send = true;
                }
                if ui.button("Answer in terminal").clicked() {
                    terminal = true;
                }
                ui.small(format!("{} s", card.expires_at.saturating_duration_since(now).as_secs()));
            });
        });
    if send || terminal {
        let card = log.questions.remove(0);
        let note = if send {
            let answers = card.answers();
            card.reply.send(&PermissionReply { decision: Decision::Allow, message: None, answers });
            "Answered the question"
        } else {
            // Dropping the reply closes the hook; Claude Code asks in the terminal.
            "Question: answer in the terminal"
        };
        log.note(&card.session, card.agent, note.to_owned());
    }
}

/// Claude's answer at the end of a turn, rendered as markdown, until closed or the
/// next prompt. Nothing else happens at that moment, so it takes most of the window.
fn final_answer_popup(ctx: &egui::Context, log: &mut RoomLog, markdown: &mut MarkdownCache, mini: bool) {
    if !log.questions.is_empty() {
        return;
    }
    let Some(text) = log.final_answer.as_ref().filter(|answer| answer.open).map(|answer| answer.text.clone()) else {
        return;
    };
    let mut close = false;
    let size = ctx.viewport_rect().size() * FINAL_ANSWER_SHARE;
    egui::Window::new("Claude")
        .id("final-answer".into())
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .fixed_size(size)
        .show(ctx, |ui| {
            // Larger, comfortable reading text; smaller in the mini widget.
            let body = if mini { 13.0 } else { 18.0 };
            let style = ui.style_mut();
            for (text_style, size) in [
                (egui::TextStyle::Body, body),
                (egui::TextStyle::Monospace, body - 2.0),
                (egui::TextStyle::Heading, body + 6.0),
                (egui::TextStyle::Button, body),
            ] {
                if let Some(font) = style.text_styles.get_mut(&text_style) {
                    font.size = size;
                }
            }
            style.spacing.item_spacing.y = 8.0;
            egui::ScrollArea::vertical()
                .max_height(size.y - if mini { 40.0 } else { 56.0 })
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui_commonmark::CommonMarkViewer::new().show(ui, &mut markdown.0, &text);
                });
            ui.separator();
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    if close && let Some(answer) = log.final_answer.as_mut() {
        answer.open = false;
    }
}

/// The app's own Claude session (PLAN.md M8a).
#[derive(SystemParam)]
pub struct ChatParams<'w> {
    chats: ResMut<'w, Chats>,
    server: Res<'w, Server>,
}

fn chat_window(ctx: &egui::Context, chat: &mut ChatParams, markdown: &mut MarkdownCache, model: &Model) {
    let waker = chat.server.waker.clone();
    let chats = &mut *chat.chats;
    if !chats.open {
        return;
    }
    if chats.session.is_none() {
        chats.refresh_past();
    }
    let mut open = true;
    let (mut start, mut stop, mut send) = (false, false, false);
    let mut resume: Option<(String, Option<String>)> = None;
    egui::Window::new("Chat with Claude")
        .id("chat".into())
        .open(&mut open)
        .default_size(ctx.viewport_rect().size() * egui::vec2(0.45, 0.7))
        // A new window has no height on its first frame; without a minimum the
        // transcript's height below goes negative and the window collapses.
        .min_size(egui::vec2(360.0, 320.0))
        .default_pos(ctx.viewport_rect().center() - egui::vec2(200.0, 200.0))
        .show(ctx, |ui| {
            let Some(session) = chats.session.as_mut() else {
                session_chooser(ui, chats, model, &mut start, &mut resume);
                return;
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&session.cwd).small().weak());
                if ui.small_button("Stop").on_hover_text("End this session").clicked() {
                    stop = true;
                }
            });
            ui.separator();
            let input_height = 70.0;
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink(false)
                .max_height((ui.available_height() - input_height).max(160.0))
                .show(ui, |ui| {
                    for (speaker, text) in &session.transcript {
                        match speaker {
                            Speaker::You => {
                                ui.label(egui::RichText::new(format!("You: {text}")).strong());
                            }
                            Speaker::Claude => {
                                egui_commonmark::CommonMarkViewer::new().show(ui, &mut markdown.0, text);
                            }
                            Speaker::Note => {
                                ui.label(egui::RichText::new(text).weak().italics());
                            }
                        }
                        ui.add_space(6.0);
                    }
                    if session.busy {
                        ui.label(egui::RichText::new("Claude is working…").weak().italics());
                    }
                });
            let response = ui.add(
                egui::TextEdit::multiline(&mut chats.input)
                    .hint_text("Message Claude (Enter sends, Shift+Enter for a new line)")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
            if enter || ui.button("Send").clicked() {
                send = true;
            }
        });
    if !open {
        chats.open = false;
    }
    if let Some((id, cwd)) = resume {
        if let Some(cwd) = cwd {
            chats.folder = cwd;
        }
        chats.resume(&id, waker);
    } else if start {
        chats.start(waker);
    }
    if stop {
        chats.session = None;
    }
    let message = chats.input.trim().to_owned();
    if send && !message.is_empty() && let Some(session) = chats.session.as_mut() {
        session.send(&message);
        chats.input.clear();
    }
}

/// No session of the app's own yet: continue the session the room shows (if it is
/// closed), resume an earlier one, or start a new one.
fn session_chooser(
    ui: &mut egui::Ui,
    chats: &mut crate::chat::Chats,
    model: &Model,
    start: &mut bool,
    resume: &mut Option<(String, Option<String>)>,
) {
    let shown = model.shown().filter(|session| !session.headless);
    if let Some(session) = shown {
        ui.label(egui::RichText::new(truncate(&session.name(), 60)).strong());
        if session.ended {
            if ui.button("Continue this session").on_hover_text("claude --resume: Claude keeps its history").clicked() {
                *resume = Some((session.id.clone(), session.cwd.clone()));
            }
        } else if chats.has_channel(session) {
            // Typed into the terminal session through its channel (PLAN.md M8b).
            let sent: Vec<&str> =
                chats.sent.iter().filter(|(id, _)| *id == session.id).map(|(_, text)| text.as_str()).collect();
            for text in &sent[sent.len().saturating_sub(3)..] {
                ui.label(egui::RichText::new(format!("You: {}", truncate(text, 200))).weak());
            }
            let response = ui.add(
                egui::TextEdit::multiline(&mut chats.input)
                    .hint_text("Message this terminal session (Enter sends)")
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
            if enter || ui.button("Send").clicked() {
                let text = chats.input.trim().to_owned();
                if !text.is_empty() {
                    chats.send_to_terminal(session, &text);
                    chats.input.clear();
                }
            }
            ui.label(egui::RichText::new("Claude's answer shows in the room.").small().weak());
        } else {
            ui.label(
                egui::RichText::new("This session is open in a terminal. To send to it from here, start Claude with:")
                    .weak(),
            );
            ui.code(CHANNEL_COMMAND);
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(CHANNEL_COMMAND.to_owned());
            }
        }
        ui.separator();
    }
    ui.label("Folder:");
    ui.add(egui::TextEdit::singleline(&mut chats.folder).desired_width(f32::INFINITY));
    let live: Vec<&str> =
        model.sessions().into_iter().filter(|s| !s.ended && !s.headless).map(|s| s.id.as_str()).collect();
    let past: Vec<_> = chats.past.iter().filter(|p| !live.contains(&p.id.as_str())).collect();
    if !past.is_empty() {
        ui.label(egui::RichText::new("Earlier sessions here").small().weak());
        egui::ScrollArea::vertical().id_salt("past").max_height(180.0).show(ui, |ui| {
            for session in past {
                ui.horizontal(|ui| {
                    if ui.small_button("Resume").clicked() {
                        *resume = Some((session.id.clone(), None));
                    }
                    let age = session.modified.elapsed().map_or(String::new(), |age| ago(age.as_secs()));
                    ui.add(egui::Label::new(format!("{}  ·  {age}", session.title)).truncate());
                });
            }
        });
    }
    if ui.button("New session").clicked() {
        *start = true;
    }
    if let Some(error) = &chats.error {
        ui.colored_label(DENY, error);
    }
}

/// Starts a terminal session the app can send to (PLAN.md M8b).
const CHANNEL_COMMAND: &str = "claude --dangerously-load-development-channels plugin:agent-room@agent-room";

/// "5 min ago", "3 h ago", "2 d ago".
fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// UI choices remembered across starts, in `~/.agent-room/ui.json`.
#[derive(Resource, Default, serde::Serialize, serde::Deserialize)]
pub struct PanelPrefs {
    /// The side panel with the log is hidden (PLAN.md M12).
    #[serde(default)]
    pub hidden: bool,
}

impl PanelPrefs {
    fn path() -> Option<std::path::PathBuf> {
        room_proto::state_dir().map(|dir| dir.join("ui.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn toggle(&mut self) {
        self.hidden = !self.hidden;
        if let (Some(path), Ok(text)) = (Self::path(), serde_json::to_string(self)) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Cache for rendered markdown (images, code highlighting state).
#[derive(Resource, Default)]
pub struct MarkdownCache(egui_commonmark::CommonMarkCache);

/// The shown session's name; a drop-down to switch when several sessions are known.
fn session_picker(ui: &mut egui::Ui, model: &Model) -> Option<String> {
    let sessions = model.sessions();
    let title = session_title(model);
    if sessions.len() < 2 {
        ui.add(egui::Label::new(egui::RichText::new(title).strong()).truncate());
        return None;
    }
    let shown = model.shown().map(|session| session.id.as_str());
    let mut picked = None;
    egui::ComboBox::from_id_salt("session")
        .selected_text(egui::RichText::new(title).strong())
        .width(ui.available_width())
        // A long path must not widen the panel (it leaves its old edge drawn as a line).
        .truncate()
        .show_ui(ui, |ui| {
            for session in sessions {
                let state = if session.ended {
                    "ended"
                } else if session.is_busy() {
                    "working"
                } else {
                    "idle"
                };
                let label = format!("{}  ·  {state}", truncate(&session.name(), 32));
                if ui.selectable_label(shown == Some(session.id.as_str()), label).clicked() {
                    picked = Some(session.id.clone());
                }
            }
        });
    picked
}

/// Who is in the room: Claude, then each sub-agent with what it is doing.
fn roster(ui: &mut egui::Ui, session: &SessionState) {
    // "in api · main" when an agent works outside the session's own repository.
    let away = |repo: &Option<crate::repo::Repo>| -> Option<String> {
        let repo = repo.as_ref()?;
        let home = session.home_repo.as_ref().map(|home| &home.root);
        (Some(&repo.root) != home).then(|| format!("in {}", repo.label()))
    };
    let main_state = match away(&session.main_repo) {
        Some(place) => format!("{} · {place}", session.main_pose.word()),
        None => session.main_pose.word().to_owned(),
    };
    agent_row(ui, scene::MAIN_ACCENT, "Claude", &main_state, session.main_doing.as_deref(), false);
    // Working agents first, then the most recent finished ones.
    let working = session.agents.iter().filter(|(_, info)| info.status != Status::Done);
    let finished = session.agents.iter().rev().filter(|(_, info)| info.status == Status::Done).take(MAX_FINISHED_ROWS);
    for (id, info) in working.chain(finished) {
        let done = info.status == Status::Done;
        let name = format!("{} {}", info.agent_type, id.chars().take(4).collect::<String>());
        let word = if done { "done" } else { info.pose.word() };
        let state = match away(&info.repo) {
            Some(place) => format!("{word} · {place}"),
            None => word.to_owned(),
        };
        let detail = if done { info.report.as_deref() } else { info.doing.as_deref().or(info.task.as_deref()) };
        agent_row(ui, scene::accent_for(&info.agent_type), &name, &state, detail, done);
    }
}

fn agent_row(ui: &mut egui::Ui, accent: Color, name: &str, state: &str, detail: Option<&str>, faded: bool) {
    let accent = egui_color(accent);
    let strong = if faded { ui.visuals().weak_text_color() } else { ui.visuals().strong_text_color() };
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 5.0, accent);
        ui.label(egui::RichText::new(name).color(strong).strong());
        let state_color = if state == Pose::Waiting.word() { DENY } else { ui.visuals().weak_text_color() };
        ui.label(egui::RichText::new(state).color(state_color).small());
    });
    if let Some(detail) = detail {
        ui.horizontal(|ui| {
            ui.add_space(22.0);
            ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate());
        });
    }
}

fn egui_color(color: Color) -> egui::Color32 {
    let srgba = color.to_srgba();
    egui::Color32::from_rgb((srgba.red * 255.0) as u8, (srgba.green * 255.0) as u8, (srgba.blue * 255.0) as u8)
}

fn session_title(model: &Model) -> String {
    if let Some(repo) = model.shown().and_then(|session| session.home_repo.as_ref()) {
        return repo.label();
    }
    match model.shown() {
        Some(session) => session.cwd.clone().unwrap_or_else(|| session.id.clone()),
        None => "Waiting for Claude Code…".to_owned(),
    }
}

fn permission_cards(
    ui: &mut egui::Ui,
    cards: &mut [PendingPermission],
    now: Instant,
    answers: &mut Vec<(usize, Decision)>,
) {
    let keyboard = keyboard_target(cards, now);
    for (index, pending) in cards.iter_mut().enumerate() {
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
            ui.add(
                egui::TextEdit::singleline(&mut pending.reason)
                    .hint_text("Reason if you deny (optional)")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                let enabled = pending.accepts_input(now);
                let keys = keyboard == Some(index);
                let allow = if keys { "Allow  Y" } else { "Allow" };
                let deny = if keys { "Deny  N" } else { "Deny" };
                if ui.add_enabled(enabled, colored_button(allow, ALLOW)).clicked() {
                    answers.push((index, Decision::Allow));
                }
                if ui.add_enabled(enabled, colored_button(deny, DENY)).clicked() {
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

/// The shown session's log. Each line is one wrapped label that may break anywhere:
/// long paths have no spaces and would otherwise widen the panel.
fn event_log(ui: &mut egui::Ui, lines: &VecDeque<LogLine>, session: Option<&str>) {
    let strong = ui.visuals().strong_text_color();
    let normal = ui.visuals().text_color();
    let font = egui::TextStyle::Body.resolve(ui.style());
    egui::ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink(false)
        .show(ui, |ui| {
            let width = ui.available_width();
            for line in lines.iter().filter(|line| session.is_none_or(|s| line.session == s)) {
                let mut job = egui::text::LayoutJob::default();
                job.append(&line.agent, 0.0, egui::TextFormat::simple(font.clone(), strong));
                job.append(&line.text, 8.0, egui::TextFormat::simple(font.clone(), normal));
                job.wrap = egui::text::TextWrapping { max_width: width, break_anywhere: true, ..default() };
                ui.label(job);
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
