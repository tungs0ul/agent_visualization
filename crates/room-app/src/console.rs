//! Station consoles (PLAN.md M19, M20): the stations are the controls. Main desk:
//! settings. Bookshelf: history. Research: chat. Writing desk: the changes. Lab: recent
//! commands. Teleporter: a new session. Wall screen: today's numbers. A console is a
//! hologram over what was clicked; one is open at a time.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::world3d::Location;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Console {
    Settings,
    History,
    Chat,
    /// What changed in the session's folder (writing desk).
    Diff,
    /// Recent commands (processing lab).
    Commands,
    /// Start a new session (teleporter).
    Launch,
    /// Today's numbers (the wall screen).
    Stats,
}

/// The holographic display on the left wall (`environment.py`, "Holo").
const WALL_SCREEN: Vec3 = Vec3::new(-9.7, 2.2, -2.6);

impl Console {
    pub const ALL: [Console; 7] =
        [Console::Settings, Console::History, Console::Chat, Console::Diff, Console::Commands, Console::Launch, Console::Stats];

    /// The workstation it belongs to; the teleporter and the wall screen are none.
    pub fn station(self) -> Option<Location> {
        match self {
            Console::Settings => Some(Location::MainDesk),
            Console::History => Some(Location::Bookshelf),
            Console::Chat => Some(Location::Research),
            Console::Diff => Some(Location::WritingDesk),
            Console::Commands => Some(Location::Lab),
            Console::Launch | Console::Stats => None,
        }
    }

    pub fn at(location: Location) -> Option<Console> {
        Console::ALL.into_iter().find(|console| console.station() == Some(location))
    }

    /// The point a click aims at, in the room.
    pub fn anchor(self) -> Vec3 {
        match (self, self.station()) {
            (_, Some(location)) => location.center() + Vec3::Y * 0.8,
            (Console::Launch, None) => crate::world3d::TELEPORTER + Vec3::Y * 0.4,
            _ => WALL_SCREEN,
        }
    }

    /// Where the hologram stands, in the room.
    pub fn hologram_at(self) -> Vec3 {
        match self.station() {
            Some(location) => location.center() + Vec3::Y * 2.2,
            None => self.anchor() + Vec3::Y * 1.4,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Console::Settings => "Settings",
            Console::History => "History",
            Console::Chat => "Chat with Claude",
            Console::Diff => "Changes",
            Console::Commands => "Commands",
            Console::Launch => "New session",
            Console::Stats => "Today",
        }
    }

    /// What its sign offers.
    pub fn hint(self) -> &'static str {
        match self {
            Console::Settings => "⚙ Settings",
            Console::History => "📖 History",
            Console::Chat => "💬 Chat",
            Console::Diff => "± Changes",
            Console::Commands => "▶ Commands",
            Console::Launch => "✚ New session",
            Console::Stats => "📊 Today",
        }
    }

    /// Key that opens it (when no text field has focus).
    pub fn key(self) -> Option<egui::Key> {
        match self {
            Console::Settings => Some(egui::Key::S),
            Console::History => Some(egui::Key::L),
            Console::Chat => Some(egui::Key::C),
            Console::Diff => Some(egui::Key::D),
            Console::Commands => Some(egui::Key::R),
            Console::Launch => Some(egui::Key::O),
            Console::Stats => Some(egui::Key::T),
        }
    }

    /// Size of the hologram, in logical pixels (height 0: fit the content).
    fn size(self) -> egui::Vec2 {
        match self {
            Console::Settings | Console::Launch => egui::vec2(320.0, 0.0),
            Console::History | Console::Stats => egui::vec2(380.0, 420.0),
            Console::Chat | Console::Diff | Console::Commands => egui::vec2(460.0, 480.0),
        }
    }
}

/// The open console, if any.
#[derive(Resource, Default)]
pub struct OpenConsole(pub Option<Console>);

impl OpenConsole {
    /// `AGENT_ROOM_CONSOLE=<console>` (e.g. `diff`) starts with that console open
    /// (screenshot tests can't click).
    pub fn from_env() -> Self {
        let wanted = std::env::var("AGENT_ROOM_CONSOLE").unwrap_or_default();
        Self(Console::ALL.into_iter().find(|console| format!("{console:?}").eq_ignore_ascii_case(&wanted)))
    }

    /// Opens it, or closes it when it is the one already open.
    pub fn toggle(&mut self, console: Console) {
        self.0 = if self.0 == Some(console) { None } else { Some(console) };
    }

    pub fn is(&self, console: Console) -> bool {
        self.0 == Some(console)
    }
}

/// The console whose screen spot is within reach of a click, nearest first.
pub fn console_at(click: Vec2, spots: impl IntoIterator<Item = (Console, Vec2)>, reach: f32) -> Option<Console> {
    spots
        .into_iter()
        .map(|(console, spot)| (console, spot.distance(click)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(console, _)| console)
}

pub const HOLO: egui::Color32 = egui::Color32::from_rgb(90, 210, 255);

/// Draws a console as a hologram standing over its station (`spot`, logical pixels),
/// kept inside the window. Returns false when the user closed it.
pub fn hologram(ctx: &egui::Context, console: Console, spot: egui::Pos2, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let size = console.size();
    let screen = ctx.content_rect();
    let height = if size.y > 0.0 { size.y.min(screen.height() * 0.8) } else { 0.0 };
    let mut open = true;
    egui::Area::new(egui::Id::new(("console", console.title())))
        .fixed_pos(spot)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .constrain(true)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(6, 22, 40, 246))
                .stroke(egui::Stroke::new(1.0, HOLO))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::same(10))
                .shadow(egui::Shadow { blur: 18, spread: 0, color: HOLO.gamma_multiply(0.25), offset: [0, 0] })
                .show(ui, |ui| {
                    ui.set_width(size.x.min(screen.width() - 40.0));
                    if height > 0.0 {
                        ui.set_height(height);
                    }
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(console.title()).strong().color(HOLO));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("x").on_hover_text("Close (Esc)").clicked() {
                                open = false;
                            }
                        });
                    });
                    ui.add(egui::Separator::default().spacing(6.0));
                    add(ui);
                });
        });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_station_has_one_console_and_every_console_a_key() {
        for console in Console::ALL {
            if let Some(location) = console.station() {
                assert_eq!(Console::at(location), Some(console));
            }
        }
        let mut keys: Vec<_> = Console::ALL.iter().filter_map(|console| console.key()).map(|key| key.name()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), Console::ALL.len());
    }

    #[test]
    fn a_click_picks_the_nearest_station_in_reach() {
        let spots = [(Console::Settings, Vec2::new(100.0, 100.0)), (Console::History, Vec2::new(160.0, 100.0))];
        assert_eq!(console_at(Vec2::new(140.0, 100.0), spots, 50.0), Some(Console::History));
        assert_eq!(console_at(Vec2::new(400.0, 400.0), spots, 50.0), None);
        let mut open = OpenConsole::default();
        open.toggle(Console::Chat);
        assert!(open.is(Console::Chat));
        open.toggle(Console::Chat);
        assert_eq!(open.0, None);
    }
}
