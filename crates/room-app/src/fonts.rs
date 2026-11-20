//! The panels' fonts. egui's built-in fonts (Ubuntu Light, Hack) have no Vietnamese
//! letters with two marks (ộ, ữ, ẫ, …), so those showed as boxes. Noto Sans Light and
//! Noto Sans Mono cover them; they are added as fallbacks after the built-in fonts, so
//! every letter the built-in fonts have keeps its look and only the missing ones come
//! from Noto. Licences: `assets/LICENSES.md`.

use bevy_egui::egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const NOTO_SANS_LIGHT: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Light.ttf");
const NOTO_SANS_MONO: &[u8] = include_bytes!("../../../assets/fonts/NotoSansMono-Regular.ttf");

/// egui's default fonts plus the Noto fallbacks.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, data, family) in [
        ("NotoSans-Light", NOTO_SANS_LIGHT, FontFamily::Proportional),
        ("NotoSansMono-Regular", NOTO_SANS_MONO, FontFamily::Monospace),
    ] {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(data)));
        let list = fonts.families.entry(family).or_default();
        // Right after the main font, before the emoji fonts, so letters prefer Noto.
        list.insert(1.min(list.len()), name.to_owned());
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_egui::egui::{self, FontId};

    const SAMPLE: &str = "Tôi bật mac rồi. Größe, über, Straße. ộ ữ ẫ ỹ ơ ư đ ß ü ẞ";

    fn covers(fonts: FontDefinitions, family: FontFamily) -> Vec<char> {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let _ = ctx.run(Default::default(), |_| {});
        let id = FontId::new(14.0, family);
        ctx.fonts_mut(|f| SAMPLE.chars().filter(|c| !f.has_glyph(&id, *c)).collect())
    }

    #[test]
    fn default_fonts_lack_vietnamese() {
        // The cause of the bug: without the fallbacks these letters are missing.
        assert!(!covers(FontDefinitions::default(), FontFamily::Proportional).is_empty());
    }

    #[test]
    fn panels_cover_vietnamese_and_german() {
        assert_eq!(covers(definitions(), FontFamily::Proportional), Vec::<char>::new());
        assert_eq!(covers(definitions(), FontFamily::Monospace), Vec::<char>::new());
    }
}
