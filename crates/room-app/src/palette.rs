//! The panels' colours (PLAN.md M28, from the user's "Mission Control 2.0" brief), in
//! one place: dark navy glass, cyan highlights, and one colour per status.

use bevy_egui::egui::Color32;

pub const BG: Color32 = Color32::from_rgb(0x08, 0x0F, 0x20);
pub const BG_RAISED: Color32 = Color32::from_rgb(0x10, 0x1B, 0x32);
pub const PANEL: Color32 = Color32::from_rgb(0x10, 0x1D, 0x36);
/// Panels floating over the room: `PANEL`, a little see-through.
pub const GLASS: Color32 = Color32::from_rgba_premultiplied(0x0E, 0x19, 0x2F, 0xE6);
pub const CYAN: Color32 = Color32::from_rgb(0x42, 0xD9, 0xFF);
pub const BLUE: Color32 = Color32::from_rgb(0x28, 0x7B, 0xFF);
pub const GREEN: Color32 = Color32::from_rgb(0x32, 0xE6, 0xA1);
pub const AMBER: Color32 = Color32::from_rgb(0xFF, 0xBE, 0x55);
pub const RED: Color32 = Color32::from_rgb(0xFF, 0x64, 0x7C);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF7, 0xFF);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x91, 0xA4, 0xC4);
/// Thin luminous borders.
pub const EDGE: Color32 = Color32::from_rgb(0x2F, 0x5E, 0xA8);
