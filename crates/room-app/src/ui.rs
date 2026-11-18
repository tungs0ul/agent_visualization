//! egui overlay: permission cards on top, the station consoles (settings, history,
//! chat; PLAN.md M19) and the pop-ups. The room has the whole window.

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
use crate::console::{self, Console, OpenConsole};
use crate::mini::{ToggleMini, ViewMode};
use crate::server::Server;
use crate::model::{Model, Who};
use bevy::ecs::system::SystemParam;
use crate::view3d::{Avatar, RoomCamera3d};

const DENY_MESSAGE: &str = "The user denied this in Agent Room.";
const ALLOW: egui::Color32 = egui::Color32::from_rgb(0x3E, 0x9B, 0x5F);
const DENY: egui::Color32 = egui::Color32::from_rgb(0xC9, 0x4F, 0x4F);
/// Share of the window the final answer covers.
const FINAL_ANSWER_SHARE: f32 = 0.86;
const AUTO: egui::Color32 = egui::Color32::from_rgb(0xE0, 0x8A, 0x1E);

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut log: ResMut<RoomLog>,
    model: Res<Model>,
    mode: Res<ViewMode>,
    mut auto: ResMut<AutoAccept>,
    mut toggle_mini: MessageWriter<ToggleMini>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut open: ResMut<OpenConsole>,
    mut camera: Single<&mut Camera, With<RoomCamera3d>>,
    in_room: InRoom,
    mut chat: Desk,
    mut markdown: ResMut<MarkdownCache>,
    mut panel: ResMut<PanelPrefs>,
    mut stats: ResMut<crate::stats::Stats>,
    mut styled: Local<bool>,
    mut was_open: Local<Option<Console>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Panels in the room's theme (PLAN.md M14).
    if !*styled {
        ctx.set_visuals(visuals());
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

    // Permission cards for every session, on top (the oldest of the shown session also
    // floats next to whoever asks, below).
    if !pending.is_empty() {
        let share = if mode.mini { 0.8 } else { 0.6 };
        egui::Panel::top("permissions").show(&mut root, |ui| {
            egui::ScrollArea::vertical()
                .max_height(ctx.viewport_rect().height() * share)
                .show(ui, |ui| permission_cards(ui, pending, now, &mut answers));
        });
    }
    if mode.mini {
        // Only the room and the cards. A small button returns to the full view (or M).
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
    } else {
        // The stations are the controls (PLAN.md M19, M20); each console has a key.
        if !ctx.egui_wants_keyboard_input() {
            for console in Console::ALL {
                if console.key().is_some_and(|key| ctx.input(|input| input.key_pressed(key))) {
                    open.toggle(console);
                }
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            open.0 = None;
        }
        if auto.0 {
            egui::Area::new("auto-badge".into())
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8.0, -8.0))
                .interactable(false)
                .show(ctx, |ui| ui.label(egui::RichText::new("Auto-accept ON").small().strong().color(AUTO)));
        }
        // The folder's changes are read each time the writing desk's console opens.
        let shown_cwd = model.shown().and_then(|session| session.cwd.clone());
        let stale = chat.diff.cwd != shown_cwd;
        if open.is(Console::Diff) && (*was_open != Some(Console::Diff) || stale) {
            match &shown_cwd {
                Some(cwd) => chat.diff.load(cwd, chat.server.waker.clone()),
                None => *chat.diff = crate::diff::DiffView::default(),
            }
        }
        *was_open = open.0;
        if let Some(console) = open.0
            && let Some(spot) = in_room.console_spot(&camera, console)
        {
            let spot = egui::pos2(spot.x, spot.y);
            let shown = model.shown().map(|session| session.id.as_str());
            let mut launched = false;
            let still_open = match console {
                Console::Settings => console::hologram(ctx, console, spot, |ui| {
                    settings(ui, &mut window, &mut auto, &mut panel, &mut stats, &mut toggle_mini);
                }),
                Console::History => console::hologram(ctx, console, spot, |ui| {
                    answer_button(ui, final_answer);
                    event_log(ui, lines, shown);
                }),
                Console::Chat => console::hologram(ctx, console, spot, |ui| chat_console(ui, &mut chat, &mut markdown, &model)),
                Console::Diff => console::hologram(ctx, console, spot, |ui| {
                    let waker = chat.server.waker.clone();
                    diff_console(ui, &mut chat.diff, shown_cwd.as_deref(), waker);
                }),
                Console::Commands => console::hologram(ctx, console, spot, |ui| commands_console(ui, &chat.activity, shown)),
                Console::Launch => console::hologram(ctx, console, spot, |ui| {
                    launched = launch_console(ui, &mut chat.chats, &model, chat.server.waker.clone());
                }),
                Console::Stats => console::hologram(ctx, console, spot, |ui| {
                    stats_console(ui, &chat.activity.day, &chat.usage, &model);
                }),
            };
            if launched {
                open.0 = Some(Console::Chat);
            } else if !still_open {
                open.0 = None;
            }
        }
    }
    let window_size = UVec2::new(window.physical_width(), window.physical_height());
    fit_camera(&mut camera, root.available_rect_before_wrap(), window.scale_factor(), window_size);

    // The oldest permission card also floats next to whoever asks (PLAN.md M4e).
    if let Some(index) = log.pending.iter().position(|card| model.shown().is_some_and(|s| s.id == card.session))
        && let Some(at) = in_room.screen_position(&camera, &log.pending[index].session, &log.pending[index].who)
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
    question_popup(ctx, &mut log, now);
    // Don't cover a console the user is using; the answer stays a click away (Last
    // answer, the trophy shelf).
    if open.0.is_none() {
        final_answer_popup(ctx, &mut log, &mut markdown, mode.mini);
    }
    Ok(())
}

/// Where characters are on screen, for cards drawn next to them.
#[derive(SystemParam)]
pub struct InRoom<'w, 's> {
    robots: Query<'w, 's, (&'static Avatar, &'static GlobalTransform)>,
    camera: Query<'w, 's, &'static GlobalTransform, With<RoomCamera3d>>,
}

impl InRoom<'_, '_> {
    /// Logical window position just above the character's head.
    fn screen_position(&self, camera: &Camera, session: &str, who: &Who) -> Option<Vec2> {
        let (_, robot) = self.robots.iter().find(|(avatar, _)| avatar.who == *who && avatar.session == session)?;
        let camera_transform = self.camera.single().ok()?;
        let head = robot.translation() + Vec3::Y * 1.1 * robot.scale().y;
        let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
        Some(camera.world_to_viewport(camera_transform, head).ok()? + offset)
    }
}

impl InRoom<'_, '_> {
    /// Logical window position where a console's hologram stands.
    fn console_spot(&self, camera: &Camera, console: Console) -> Option<Vec2> {
        let camera_transform = self.camera.single().ok()?;
        let offset = camera.logical_viewport_rect().map_or(Vec2::ZERO, |rect| rect.min);
        Some(camera.world_to_viewport(camera_transform, console.hologram_at()).ok()? + offset)
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

/// The main desk's console: the app's own switches.
fn settings(
    ui: &mut egui::Ui,
    window: &mut Window,
    auto: &mut AutoAccept,
    prefs: &mut PanelPrefs,
    stats: &mut crate::stats::Stats,
    toggle_mini: &mut MessageWriter<ToggleMini>,
) {
    let row = |ui: &mut egui::Ui, name: &str, key: &str, on: bool, hint: &str| -> bool {
        let mut clicked = false;
        ui.horizontal(|ui| {
            ui.label(name).on_hover_text(hint);
            ui.label(egui::RichText::new(key).small().weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, fill) = if on { ("On", console::HOLO.gamma_multiply(0.6)) } else { ("Off", egui::Color32::TRANSPARENT) };
                clicked = ui.add(egui::Button::new(text).fill(fill).min_size(egui::vec2(48.0, 0.0))).on_hover_text(hint).clicked();
            });
        });
        clicked
    };
    let full = window.mode != bevy::window::WindowMode::Windowed;
    if row(ui, "Full screen", "F", full, "Fill the screen; Esc leaves it") {
        window.mode = if full {
            bevy::window::WindowMode::Windowed
        } else {
            bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current)
        };
    }
    let on_top = window.window_level == bevy::window::WindowLevel::AlwaysOnTop;
    if row(ui, "Always on top", "P", on_top, "Keep the window above other apps") {
        window.window_level = if on_top { bevy::window::WindowLevel::Normal } else { bevy::window::WindowLevel::AlwaysOnTop };
        if let Some(saved) = crate::window::SavedWindow::of(window) {
            saved.save();
        }
    }
    if row(ui, "Auto-accept", "A", auto.0, "Allow permission requests without asking. Risky commands like rm -rf or git push still ask.") {
        auto.0 = !auto.0;
    }
    if row(ui, "Sound", "", prefs.sound, "Notification sounds; off plays nothing at all") {
        prefs.sound = !prefs.sound;
        prefs.save();
    }
    if row(ui, "Stats", "I", stats.shown, "This app's CPU, memory and frame rate") {
        stats.toggle();
    }
    ui.add_space(4.0);
    if ui.button("Mini widget view (M)").clicked() {
        toggle_mini.write(ToggleMini);
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
    let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    let mut open = true;
    let size = ctx.viewport_rect().size() * FINAL_ANSWER_SHARE;
    egui::Window::new("Claude")
        .id("final-answer".into())
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
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
            // Both ways: a wide table or code block scrolls inside the window instead
            // of stretching it off screen (and its Close button with it).
            egui::ScrollArea::both()
                .max_height(size.y - if mini { 40.0 } else { 56.0 })
                .max_width(size.x)
                .auto_shrink(false)
                .show(ui, |ui| {
                    egui_commonmark::CommonMarkViewer::new().show(ui, &mut markdown.0, &text);
                });
            ui.separator();
            if ui.button("Close").on_hover_text("Close (Esc)").clicked() {
                close = true;
            }
        });
    if (close || !open) && let Some(answer) = log.final_answer.as_mut() {
        answer.open = false;
    }
}

/// What the consoles show: the app's own Claude session (PLAN.md M8a), the folder's
/// changes, commands and today's numbers (M20).
#[derive(SystemParam)]
pub struct Desk<'w> {
    chats: ResMut<'w, Chats>,
    server: Res<'w, Server>,
    diff: ResMut<'w, crate::diff::DiffView>,
    activity: Res<'w, crate::activity::Activity>,
    usage: Res<'w, crate::usage::Usage>,
}

fn chat_console(ui: &mut egui::Ui, chat: &mut Desk, markdown: &mut MarkdownCache, model: &Model) {
    let waker = chat.server.waker.clone();
    let chats = &mut *chat.chats;
    if chats.session.is_none() {
        chats.refresh_past();
    }
    let (mut start, mut stop, mut send) = (false, false, false);
    let mut resume: Option<(String, Option<String>)> = None;
    // A closure, so an early return still runs the actions below.
    let mut body = |ui: &mut egui::Ui| {
        {
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
        }
    };
    body(ui);
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
#[derive(Resource, serde::Serialize, serde::Deserialize)]
pub struct PanelPrefs {
    /// Notification sounds (on by default, the user's choice; off plays nothing).
    #[serde(default = "yes")]
    pub sound: bool,
}

fn yes() -> bool {
    true
}

impl Default for PanelPrefs {
    fn default() -> Self {
        Self { sound: true }
    }
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

    fn save(&self) {
        if let (Some(path), Ok(text)) = (Self::path(), serde_json::to_string(self)) {
            let _ = std::fs::write(path, text);
        }
    }
}

/// Cache for rendered markdown (images, code highlighting state).
#[derive(Resource, Default)]
pub struct MarkdownCache(egui_commonmark::CommonMarkCache);

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
/// The writing desk: what changed in the shown session's folder.
fn diff_console(ui: &mut egui::Ui, diff: &mut crate::diff::DiffView, cwd: Option<&str>, waker: crate::server::Waker) {
    use crate::diff::DiffState;
    diff.poll();
    let Some(cwd) = cwd else {
        ui.label(egui::RichText::new("No session to look at yet.").weak());
        return;
    };
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(egui::RichText::new(cwd).small().weak()).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Refresh").clicked() {
                diff.load(cwd, waker);
            }
        });
    });
    let files = match &diff.state {
        DiffState::Empty | DiffState::Loading(_) => {
            ui.label(egui::RichText::new("Reading git…").weak().italics());
            return;
        }
        DiffState::Failed(error) => {
            ui.label(egui::RichText::new(format!("Not a git folder, or git failed: {error}")).weak());
            return;
        }
        DiffState::Ready(files) if files.is_empty() => {
            ui.label(egui::RichText::new("No changes since the last commit.").weak());
            return;
        }
        DiffState::Ready(files) => files,
    };
    let (added, removed) = files.iter().fold((0, 0), |(a, r), file| (a + file.added.unwrap_or(0), r + file.removed.unwrap_or(0)));
    ui.label(egui::RichText::new(format!("{} files · +{added} −{removed}", files.len())).small());
    ui.separator();
    let mut toggled = None;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for file in files {
            let unfolded = diff.unfolded.as_deref() == Some(file.path.as_str());
            let counts = match (file.new, file.added, file.removed) {
                (true, _, _) => egui::RichText::new("new").color(ALLOW),
                (_, Some(a), Some(r)) => egui::RichText::new(format!("+{a} −{r}")).color(egui::Color32::from_gray(170)),
                _ => egui::RichText::new("binary").weak(),
            };
            ui.horizontal(|ui| {
                let arrow = if unfolded { "▼" } else { "▶" };
                let name = egui::RichText::new(format!("{arrow} {}", file.path)).monospace().small();
                if ui.add(egui::Label::new(name).truncate().sense(egui::Sense::click())).clicked() && !file.patch.is_empty() {
                    toggled = Some(file.path.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(counts.small()));
            });
            if unfolded {
                patch_view(ui, &file.patch);
            }
        }
    });
    if let Some(path) = toggled {
        diff.unfolded = if diff.unfolded.as_ref() == Some(&path) { None } else { Some(path) };
    }
}

/// A patch with added lines green and removed lines red.
fn patch_view(ui: &mut egui::Ui, patch: &str) {
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let font = egui::FontId::new(mono.size * 0.85, mono.family);
    let mut job = egui::text::LayoutJob::default();
    for line in patch.lines() {
        let color = match line.as_bytes().first() {
            Some(b'+') => egui::Color32::from_rgb(120, 220, 140),
            Some(b'-') => egui::Color32::from_rgb(240, 120, 120),
            Some(b'@') => console::HOLO,
            _ => egui::Color32::from_gray(170),
        };
        job.append(line, 0.0, egui::TextFormat::simple(font.clone(), color));
        job.append("\n", 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job.wrap = egui::text::TextWrapping { max_width: ui.available_width() - 12.0, break_anywhere: true, ..default() };
    egui::Frame::new().fill(egui::Color32::from_rgb(4, 12, 24)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
        ui.label(job);
    });
}

/// The processing lab: the shown session's recent commands.
fn commands_console(ui: &mut egui::Ui, activity: &crate::activity::Activity, session: Option<&str>) {
    use crate::activity::Outcome;
    let Some(session) = session else {
        ui.label(egui::RichText::new("No session yet.").weak());
        return;
    };
    let mut any = false;
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (index, command) in activity.commands(session).enumerate() {
            any = true;
            let (mark, color) = match command.outcome {
                Outcome::Running => ("…", AUTO),
                Outcome::Succeeded => ("✔", ALLOW),
                Outcome::Failed => ("✖", DENY),
            };
            let took = command.took.map_or_else(|| "running".to_owned(), |took| format!("{:.1} s", took.as_secs_f32()));
            let header = egui::RichText::new(format!("{mark} {}", truncate(&command.text, 60))).monospace().small().color(color);
            egui::CollapsingHeader::new(header).id_salt(("command", index, &command.text)).show(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&command.text).monospace().small()).wrap());
                ui.label(egui::RichText::new(format!("{took} · {} ago", crate::hud::ago(command.started))).small().weak());
                if !command.output.is_empty() {
                    egui::Frame::new().fill(egui::Color32::from_rgb(4, 12, 24)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                        let tail = room_proto::event::tail(&command.output, 1_500);
                        ui.add(egui::Label::new(egui::RichText::new(tail).monospace().small()).wrap());
                    });
                }
            });
        }
        if !any {
            ui.label(egui::RichText::new("No commands in this session yet.").weak());
        }
    });
}

/// The teleporter: start a new session of the app's own in a folder. True once started.
fn launch_console(ui: &mut egui::Ui, chats: &mut Chats, model: &Model, waker: crate::server::Waker) -> bool {
    let mut folders: Vec<String> = model.sessions().iter().filter_map(|session| session.cwd.clone()).collect();
    folders.sort();
    folders.dedup();
    ui.label(egui::RichText::new("Claude beams in and works in this folder:").small());
    ui.add(egui::TextEdit::singleline(&mut chats.folder).desired_width(f32::INFINITY));
    if !folders.is_empty() {
        ui.label(egui::RichText::new("Recent folders").small().weak());
        for folder in folders {
            let name = folder.rsplit('/').next().unwrap_or(&folder).to_owned();
            if ui.selectable_label(chats.folder == folder, name).on_hover_text(&folder).clicked() {
                chats.folder = folder;
            }
        }
    }
    ui.add_space(6.0);
    if chats.session.is_some() {
        ui.label(egui::RichText::new("This ends the app's current chat session.").small().color(AUTO));
    }
    let ready = std::path::Path::new(chats.folder.trim()).is_dir();
    if ui.add_enabled(ready, colored_button("Start session", ALLOW)).clicked() {
        chats.session = None;
        chats.start(waker);
        return true;
    }
    false
}

/// The wall screen: today's numbers.
fn stats_console(ui: &mut egui::Ui, day: &crate::activity::Day, usage: &crate::usage::Usage, model: &Model) {
    let calls: u32 = day.tools.values().sum();
    let tokens = model.sessions().iter().filter_map(|session| usage.tokens(&session.id)).fold(0, |sum, t| sum + t.input + t.output);
    egui::Grid::new("today").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
        for (name, value) in [
            ("Prompts", day.prompts.to_string()),
            ("Turns finished", day.turns.to_string()),
            ("Tool calls", calls.to_string()),
            ("Tokens (sessions seen)", crate::usage::short(tokens)),
        ] {
            ui.label(name);
            ui.label(egui::RichText::new(value).strong().color(console::HOLO));
            ui.end_row();
        }
    });
    if let Some((line, _, _)) = crate::usage::limits_line(usage.limits()) {
        ui.label(egui::RichText::new(format!("Limits: {line}")).small().weak());
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Activity by hour").small().weak());
    heatmap(ui, &day.hours);
    ui.add_space(8.0);
    let mut tools: Vec<_> = day.tools.iter().collect();
    tools.sort_by(|a, b| b.1.cmp(a.1));
    let most = tools.first().map_or(1, |(_, count)| **count).max(1);
    for (name, count) in tools.into_iter().take(6) {
        ui.horizontal(|ui| {
            ui.add_sized([80.0, 14.0], egui::Label::new(egui::RichText::new(name).small()).truncate());
            let width = 180.0 * *count as f32 / most as f32;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(2.0), 8.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, console::HOLO.gamma_multiply(0.7));
            ui.label(egui::RichText::new(count.to_string()).small().weak());
        });
    }
}

/// 24 cells, brighter for busier hours, with the hour under every sixth.
fn heatmap(ui: &mut egui::Ui, hours: &[u32; 24]) {
    let busiest = hours.iter().copied().max().unwrap_or(0).max(1);
    let cell = ((ui.available_width() - 2.0) / 24.0).clamp(6.0, 14.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(cell * 24.0, cell + 12.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    for (hour, count) in hours.iter().enumerate() {
        let min = rect.min + egui::vec2(hour as f32 * cell, 0.0);
        let share = *count as f32 / busiest as f32;
        let fill = if *count == 0 { egui::Color32::from_rgb(16, 30, 48) } else { console::HOLO.gamma_multiply(0.25 + 0.75 * share) };
        painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(cell - 1.0, cell - 1.0)), 2.0, fill);
        if hour % 6 == 0 {
            painter.text(
                min + egui::vec2(0.0, cell + 1.0),
                egui::Align2::LEFT_TOP,
                hour.to_string(),
                egui::FontId::proportional(9.0),
                egui::Color32::from_gray(150),
            );
        }
    }
}

/// The panels' look: dark glass, cyan highlights, to match the space station.
fn visuals() -> egui::Visuals {
    use egui::Color32;
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(0x0B, 0x11, 0x20);
    visuals.window_fill = Color32::from_rgb(0x0F, 0x18, 0x2C);
    visuals.extreme_bg_color = Color32::from_rgb(0x06, 0x0A, 0x16);
    visuals.selection.bg_fill = Color32::from_rgb(0x1E, 0x7F, 0xA8);
    visuals.hyperlink_color = Color32::from_rgb(0x7F, 0xDB, 0xFF);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x16, 0x24, 0x3A);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x1D, 0x33, 0x52);
    visuals
}

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
