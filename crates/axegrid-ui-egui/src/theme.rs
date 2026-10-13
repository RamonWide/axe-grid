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

/// How bright `color` looks, from 0 (black) to 1 (white): the relative
/// luminance of the web's contrast guidelines. Transparency is ignored.
pub fn luminance(color: Color32) -> f32 {
    let linear = |channel: u8| {
        let level = f32::from(channel) / 255.0;
        if level <= 0.04045 {
            level / 12.92
        } else {
            ((level + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// How far apart two colours read, from 1 (the same) to 21 (black on white).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The dark ink or `light`, whichever reads better on `fill`.
fn ink_on(fill: Color32, light: Color32) -> Color32 {
    if contrast(fill, ON_ACCENT) >= contrast(fill, light) {
        ON_ACCENT
    } else {
        light
    }
}

/// The colour for text drawn on `fill`: dark on a light fill such as the
/// accent yellow, white on a dark one. Every text that sits on a coloured
/// fill takes its colour from here, so the two cannot drift apart.
pub fn text_on(fill: Color32) -> Color32 {
    ink_on(fill, Color32::WHITE)
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKDROP;
    visuals.faint_bg_color = RAISED;
    // No `override_text_color`: it would also override the colour a selected
    // widget asks for, and put light text on the accent fill. Each widget
    // state names the text colour that suits its own fill instead.
    visuals.override_text_color = None;
    visuals.widgets.noninteractive.fg_stroke.color = ink_on(PANEL, TEXT);
    visuals.widgets.open.fg_stroke.color = ink_on(visuals.widgets.open.bg_fill, TEXT);
    visuals.window_stroke = Stroke::new(1.0_f32, EDGE);
    visuals.window_corner_radius = RADIUS;
    visuals.menu_corner_radius = RADIUS;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, text_on(ACCENT));
    visuals.hyperlink_color = ACCENT;
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    for (state, fill) in
        [(&mut widgets.inactive, RAISED), (&mut widgets.hovered, RAISED_HOT), (&mut widgets.active, EDGE)]
    {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.fg_stroke.color = ink_on(fill, TEXT);
        state.corner_radius = RADIUS;
    }
    widgets.noninteractive.corner_radius = RADIUS;
    widgets.open.corner_radius = RADIUS;
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
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
    fn text_is_dark_on_light_fills_and_light_on_dark_ones() {
        let light = |fill| luminance(text_on(fill)) > 0.5;
        for fill in [ACCENT, LAMP_ON, Color32::YELLOW, Color32::WHITE, TEXT, CABLE] {
            assert!(!light(fill), "{fill:?} wants dark text");
            assert_eq!(text_on(fill), ON_ACCENT);
        }
        for fill in [BACKDROP, PANEL, RAISED, RAISED_HOT, EDGE, Color32::BLACK] {
            assert!(light(fill), "{fill:?} wants light text");
        }
        // Whatever the fill, the text chosen for it stays readable.
        let fills = Category::ALL.map(Category::color);
        for fill in fills.into_iter().chain([ACCENT, LAMP_ON, DANGER, PANEL, RAISED, RAISED_HOT, EDGE]) {
            assert!(contrast(fill, text_on(fill)) >= 4.0, "{fill:?}");
        }
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
    }

    #[test]
    fn no_widget_state_puts_light_text_on_the_accent() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let visuals = ctx.global_style().visuals.clone();
        assert_eq!(visuals.override_text_color, None);
        assert_eq!(visuals.selection.stroke.color, text_on(visuals.selection.bg_fill));
        assert_eq!(visuals.selection.stroke.color, ON_ACCENT);
        let widgets = &visuals.widgets;
        for state in [&widgets.noninteractive, &widgets.inactive, &widgets.hovered, &widgets.active, &widgets.open] {
            assert!(contrast(state.fg_stroke.color, state.bg_fill) >= 4.5);
            assert!(contrast(state.fg_stroke.color, state.weak_bg_fill) >= 4.5);
        }
    }

    #[test]
    fn muting_moves_toward_the_panel() {
        let color = Category::Amp.color();
        assert!(muted(color).r() < color.r());
        assert_eq!(muted(PANEL), PANEL);
    }
}
