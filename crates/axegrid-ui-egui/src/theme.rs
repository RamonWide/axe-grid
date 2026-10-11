//! The look of the editor: palette, widget styling and block colours.
//!
//! Everything here is this project's own design, drawn with vector
//! primitives at run time. The arrangement of the window follows what users
//! of hardware editors expect (grid on top, block editor below); the artwork
//! does not follow anyone's. See `docs/clean-room.md` before changing it.

use egui::{Color32, CornerRadius, Stroke};

pub const BACKDROP: Color32 = Color32::from_rgb(0x14, 0x17, 0x1C);
pub const PANEL: Color32 = Color32::from_rgb(0x1C, 0x20, 0x27);
pub const RAISED: Color32 = Color32::from_rgb(0x27, 0x2C, 0x35);
pub const RAISED_HOT: Color32 = Color32::from_rgb(0x32, 0x38, 0x43);
pub const EDGE: Color32 = Color32::from_rgb(0x3A, 0x40, 0x4B);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEB);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8C, 0x93, 0x9E);
pub const ACCENT: Color32 = Color32::from_rgb(0xF2, 0xB4, 0x41);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x14, 0x05);
pub const CABLE: Color32 = Color32::from_rgb(0xB9, 0xC0, 0xCA);
pub const LAMP_ON: Color32 = Color32::from_rgb(0x58, 0xC9, 0x7A);
pub const LAMP_OFF: Color32 = Color32::from_rgb(0x5A, 0x60, 0x6B);
pub const DANGER: Color32 = Color32::from_rgb(0xD9, 0x5B, 0x4F);

pub const RADIUS: CornerRadius = CornerRadius::same(6);

/// What a block does to the signal; decides its tile colour and where it
/// appears in the insert menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Amp,
    Drive,
    Dynamics,
    Tone,
    Modulation,
    Time,
    Space,
    Pitch,
    Utility,
}

impl Category {
    pub const ALL: [Self; 9] = [
        Self::Amp,
        Self::Drive,
        Self::Dynamics,
        Self::Tone,
        Self::Modulation,
        Self::Time,
        Self::Space,
        Self::Pitch,
        Self::Utility,
    ];

    pub fn of(effect_type: u8) -> Self {
        match effect_type {
            5 | 6 => Self::Amp,
            19 => Self::Drive,
            2 | 29 | 31 | 35 => Self::Dynamics,
            3 | 4 | 14 | 15 | 18 | 20 | 28 | 39 => Self::Tone,
            10 | 11 | 12 | 13 | 16 | 30 | 32 => Self::Modulation,
            8 | 9 | 27 | 38 => Self::Time,
            7 => Self::Space,
            17 | 25 | 26 | 33 => Self::Pitch,
            _ => Self::Utility,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Amp => "Amp & Cab",
            Self::Drive => "Drive",
            Self::Dynamics => "Dynamics",
            Self::Tone => "EQ & Filter",
            Self::Modulation => "Modulation",
            Self::Time => "Delay",
            Self::Space => "Reverb",
            Self::Pitch => "Pitch & Synth",
            Self::Utility => "Routing & Utility",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Self::Amp => Color32::from_rgb(0xC8, 0x55, 0x3D),
            Self::Drive => Color32::from_rgb(0xC9, 0x7E, 0x2A),
            Self::Dynamics => Color32::from_rgb(0x6F, 0x83, 0x94),
            Self::Tone => Color32::from_rgb(0x4F, 0x8C, 0x74),
            Self::Modulation => Color32::from_rgb(0x82, 0x68, 0xB0),
            Self::Time => Color32::from_rgb(0x3B, 0x7F, 0xBD),
            Self::Space => Color32::from_rgb(0x2B, 0x97, 0x9C),
            Self::Pitch => Color32::from_rgb(0xB4, 0x55, 0x9C),
            Self::Utility => Color32::from_rgb(0x60, 0x67, 0x73),
        }
    }
}

/// Mixes `color` toward the panel colour; used for bypassed blocks.
pub fn muted(color: Color32) -> Color32 {
    let mix = |a: u8, b: u8| ((u16::from(a) + 3 * u16::from(b)) / 4) as u8;
    Color32::from_rgb(mix(color.r(), PANEL.r()), mix(color.g(), PANEL.g()), mix(color.b(), PANEL.b()))
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKDROP;
    visuals.faint_bg_color = RAISED;
    visuals.override_text_color = Some(TEXT);
    visuals.window_stroke = Stroke::new(1.0_f32, EDGE);
    visuals.window_corner_radius = RADIUS;
    visuals.menu_corner_radius = RADIUS;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, ON_ACCENT);
    visuals.hyperlink_color = ACCENT;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    for (state, fill) in
        [(&mut widgets.inactive, RAISED), (&mut widgets.hovered, RAISED_HOT), (&mut widgets.active, EDGE)]
    {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.corner_radius = RADIUS;
    }
    widgets.noninteractive.corner_radius = RADIUS;
    widgets.open.corner_radius = RADIUS;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 4.0);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axegrid_model::blocks;

    #[test]
    fn every_block_has_a_category_in_the_menu() {
        for block in blocks::all() {
            assert!(Category::ALL.contains(&Category::of(block.effect_type)));
        }
        // Each category is used, so no insert submenu is empty.
        for category in Category::ALL {
            assert!(blocks::all().iter().any(|b| Category::of(b.effect_type) == category), "{category:?}");
        }
    }

    #[test]
    fn muting_moves_toward_the_panel() {
        let color = Category::Amp.color();
        assert!(muted(color).r() < color.r());
        assert_eq!(muted(PANEL), PANEL);
    }
}
