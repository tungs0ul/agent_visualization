//! Station consoles (PLAN.md M19): the stations are the controls. Click the main desk
//! for settings, the bookshelf for the history log, the research console to chat with
//! Claude. A console is a hologram over its station; one is open at a time.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::world3d::Location;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Console {
    Settings,
    History,
    Chat,
}

impl Console {
    pub const ALL: [Console; 3] = [Console::Settings, Console::History, Console::Chat];

    pub fn station(self) -> Location {
        match self {
            Console::Settings => Location::MainDesk,
            Console::History => Location::Bookshelf,
            Console::Chat => Location::Research,
        }
    }

    pub fn at(location: Location) -> Option<Console> {
        Console::ALL.into_iter().find(|console| console.station() == location)
    }

    pub fn title(self) -> &'static str {
        match self {
            Console::Settings => "Settings",
            Console::History => "History",
            Console::Chat => "Chat with Claude",
        }
    }

    /// What the station's sign offers.
    pub fn hint(self) -> &'static str {
        match self {
            Console::Settings => "⚙ Settings",
            Console::History => "📖 History",
            Console::Chat => "💬 Chat",
        }
    }

    /// Key that opens it (when no text field has focus).
    pub fn key(self) -> egui::Key {
        match self {
            Console::Settings => egui::Key::S,
            Console::History => egui::Key::L,
            Console::Chat => egui::Key::C,
        }
    }

    /// Size of the hologram, in logical pixels.
    fn size(self) -> egui::Vec2 {
        match self {
            Console::Settings => egui::vec2(300.0, 0.0),
            Console::History => egui::vec2(380.0, 420.0),
            Console::Chat => egui::vec2(440.0, 480.0),
        }
    }
}

/// The open console, if any.
#[derive(Resource, Default)]
pub struct OpenConsole(pub Option<Console>);

impl OpenConsole {
    /// `AGENT_ROOM_CONSOLE=settings|history|chat` starts with that console open
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

/// The station whose screen spot is within reach of a click, nearest first.
pub fn station_at(click: Vec2, spots: impl IntoIterator<Item = (Location, Vec2)>, reach: f32) -> Option<Location> {
    spots
        .into_iter()
        .map(|(location, spot)| (location, spot.distance(click)))
        .filter(|(_, distance)| *distance <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(location, _)| location)
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
    fn each_console_lives_at_one_station() {
        for console in Console::ALL {
            assert_eq!(Console::at(console.station()), Some(console));
        }
        assert_eq!(Console::at(Location::Lab), None);
    }

    #[test]
    fn a_click_picks_the_nearest_station_in_reach() {
        let spots = [(Location::MainDesk, Vec2::new(100.0, 100.0)), (Location::Bookshelf, Vec2::new(160.0, 100.0))];
        assert_eq!(station_at(Vec2::new(140.0, 100.0), spots, 50.0), Some(Location::Bookshelf));
        assert_eq!(station_at(Vec2::new(400.0, 400.0), spots, 50.0), None);
        let mut open = OpenConsole::default();
        open.toggle(Console::Chat);
        assert!(open.is(Console::Chat));
        open.toggle(Console::Chat);
        assert_eq!(open.0, None);
    }
}
