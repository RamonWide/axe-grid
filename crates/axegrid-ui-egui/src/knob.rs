//! The block editor's controls, painted from scratch: a rotary knob (an arc
//! that fills with the value, a round cap and a pointer), a vertical fader
//! and an on/off switch. Drag up or down, or turn the mouse wheel over a
//! control, to change it; hold Shift for fine steps. Double-click the value
//! under a knob or a fader to type one.

use std::f32::consts::PI;

use egui::{pos2, vec2, Align, Align2, FontId, Key, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

use crate::theme;

/// Space one control takes at full size, label and value included.
pub const CELL: Vec2 = vec2(86.0, 100.0);
/// Narrowest and shortest a cell may get before the page scrolls instead.
pub const MIN_CELL: Vec2 = vec2(54.0, 80.0);
/// Height kept for the label, enough for two lines.
pub const LABEL_BAND: f32 = 26.0;
const LABEL_FONT: f32 = 10.5;
/// Height of the value under a control, which is also where one is typed.
const VALUE_BAND: f32 = 18.0;
const RADIUS: f32 = 23.0;
const START_ANGLE: f32 = 0.75 * PI;
const SWEEP: f32 = 1.5 * PI;
const ARC_STEPS: usize = 32;
/// Points of drag for the full range, coarse and fine.
const DRAG_RANGE: f32 = 180.0;
const FINE_DRAG_RANGE: f32 = 1800.0;
/// Ranges up to this size move one step per wheel notch; larger ones move
/// a hundredth (a thousandth with Shift) of the range.
const COUNTED_RANGE: u32 = 2000;

fn arc(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    (0..=ARC_STEPS)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / ARC_STEPS as f32;
            center + radius * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

/// Where a drag of `dy` points takes a value of `raw` out of `max`.
pub fn dragged_value(raw: u32, max: u32, dy: f32, fine: bool) -> u32 {
    let range = if fine { FINE_DRAG_RANGE } else { DRAG_RANGE };
    let moved = raw as f32 - dy * max as f32 / range;
    moved.round().clamp(0.0, max as f32) as u32
}

/// Where `notches` of the mouse wheel take a value of `raw` out of `max`.
pub fn wheeled_value(raw: u32, max: u32, notches: i32, fine: bool) -> u32 {
    let step = match (max <= COUNTED_RANGE, fine) {
        (true, _) => 1,
        (false, false) => (max / 100).max(1),
        (false, true) => (max / 1000).max(1),
    };
    let moved = i64::from(raw.min(max)) + i64::from(notches) * i64::from(step);
    moved.clamp(0, i64::from(max)) as u32
}

/// How far the wheel turned this frame, before any smoothing: positive is up.
pub fn wheel_turned(ui: &Ui) -> f32 {
    ui.input(|input| {
        input
            .events
            .iter()
            .map(|event| match event {
                egui::Event::MouseWheel { delta, .. } => delta.y,
                _ => 0.0,
            })
            .sum()
    })
}

/// Wheel notches turned over `response` this frame: +1 up, -1 down. The
/// scroll is used up, so nothing behind the control scrolls with it.
pub fn wheel(ui: &Ui, response: &Response) -> i32 {
    if !response.hovered() {
        return 0;
    }
    let turned = wheel_turned(ui);
    if turned == 0.0 {
        return 0;
    }
    ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
    if turned > 0.0 {
        1
    } else {
        -1
    }
}

/// New value from dragging or wheeling over a continuous control.
fn adjusted(ui: &Ui, response: &Response, raw: u32, max: u32) -> Option<u32> {
    let fine = ui.input(|input| input.modifiers.shift);
    let mut next = raw.min(max);
    if response.dragged() {
        next = dragged_value(next, max, response.drag_delta().y, fine);
    }
    let notches = wheel(ui, response);
    if notches != 0 {
        next = wheeled_value(next, max, notches, fine);
    }
    (next != raw).then_some(next)
}

/// Draws a control's name across the top of its cell, on up to two lines.
/// A name too long even for that ends in an ellipsis and shows in full on hover.
pub fn label(ui: &Ui, rect: Rect, text: &str, hot: bool) {
    let ink = if hot { theme::TEXT } else { theme::TEXT_DIM };
    let font = FontId::proportional(LABEL_FONT);
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, ink, rect.width() - 8.0);
    job.halign = Align::Center;
    job.wrap.max_rows = 2;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let top = rect.top() + (LABEL_BAND - galley.size().y).max(0.0) / 2.0;
    ui.painter().with_clip_rect(rect).galley(pos2(rect.center().x, top), galley, ink);
}

fn value_text(ui: &Ui, rect: Rect, text: &str) {
    let at = pos2(rect.center().x, rect.bottom() - VALUE_BAND / 2.0);
    ui.painter().with_clip_rect(rect).text(at, Align2::CENTER_CENTER, text, FontId::monospace(11.0), theme::TEXT);
}

/// What the user did to a continuous control this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Change {
    /// Dragged or wheeled to this raw value.
    Raw(u32),
    /// Typed this value, in the unit the control displays.
    Typed(f64),
}

/// The number a typed value stands for. A unit after it is ignored, and a
/// decimal comma is read as a point.
pub fn typed_number(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    let end = text.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(text.len());
    text[..end].parse::<f64>().ok().filter(|number| number.is_finite())
}

/// The part of a display text worth starting an edit from: its number.
fn editable(text: &str) -> String {
    typed_number(text).map_or_else(String::new, |_| {
        let end = text.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(text.len());
        text[..end].to_owned()
    })
}

/// The value under a control. A double-click turns it into a box to type
/// in: Enter applies what was typed, Escape or a click elsewhere drops it.
fn value_field(ui: &mut Ui, rect: Rect, text: &str) -> Option<f64> {
    let band = Rect::from_min_max(pos2(rect.left() + 2.0, rect.bottom() - VALUE_BAND), rect.max - vec2(2.0, 0.0));
    let id = ui.id().with(("value", rect.min.x.to_bits(), rect.min.y.to_bits()));
    let editing: Option<String> = ui.data(|data| data.get_temp(id));
    let Some(mut typed) = editing else {
        value_text(ui, rect, text);
        let spot = ui.interact(band, id, Sense::click());
        if spot.on_hover_text("Double-click to type a value").double_clicked() {
            ui.data_mut(|data| data.insert_temp(id, editable(text)));
            ui.data_mut(|data| data.insert_temp(id.with("focus"), true));
        }
        return None;
    };
    let field = egui::TextEdit::singleline(&mut typed)
        .font(FontId::monospace(11.0))
        .horizontal_align(Align::Center)
        .margin(egui::Margin::symmetric(2, 1));
    let response = ui.put(band, field);
    if ui.data_mut(|data| data.remove_temp::<bool>(id.with("focus"))).is_some() {
        response.request_focus();
    }
    if response.lost_focus() {
        ui.data_mut(|data| data.remove_temp::<String>(id));
        let entered = ui.input(|input| input.key_pressed(Key::Enter));
        return entered.then(|| typed_number(&typed)).flatten();
    }
    ui.data_mut(|data| data.insert_temp(id, typed));
    None
}

/// Draws a knob and returns what the user did to it, if anything.
pub fn knob(ui: &mut Ui, size: Vec2, name: &str, text: &str, raw: u32, max: u32) -> Option<Change> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let turned = adjusted(ui, &response, raw, max).map(Change::Raw);
    if !ui.is_rect_visible(rect) {
        return turned;
    }
    let hot = response.hovered() || response.dragged();
    label(ui, rect, name, hot);
    let typed = value_field(ui, rect, text).map(Change::Typed);

    let painter = ui.painter().with_clip_rect(rect);
    // The dial takes what is left between the label and the value.
    let room = rect.height() - LABEL_BAND - 18.0;
    let radius = RADIUS.min(room / 1.85).min(rect.width() / 2.0 - 4.0);
    let center = pos2(rect.center().x, rect.top() + LABEL_BAND + radius + 1.0);
    let position = raw.min(max) as f32 / max.max(1) as f32;
    let angle = START_ANGLE + SWEEP * position;
    let track = arc(center, radius, START_ANGLE, START_ANGLE + SWEEP);
    painter.add(Shape::line(track, Stroke::new(3.0_f32, theme::EDGE)));
    if position > 0.0 {
        painter.add(Shape::line(arc(center, radius, START_ANGLE, angle), Stroke::new(3.0_f32, theme::ACCENT)));
    }
    let cap = radius - 6.0;
    painter.circle_filled(center, cap, if hot { theme::RAISED_HOT } else { theme::RAISED });
    painter.circle_stroke(center, cap, Stroke::new(1.0_f32, theme::EDGE));
    let direction = vec2(angle.cos(), angle.sin());
    painter.line_segment(
        [center + direction * (cap - 9.0), center + direction * (cap - 2.0)],
        Stroke::new(2.0_f32, theme::TEXT),
    );
    response.on_hover_text(format!("{name}: {text}"));
    typed.or(turned)
}

/// Where a drag of `dy` points takes a fader whose track is `track` points
/// long: the cap follows the pointer, ten times slower when `fine`.
pub fn fader_dragged(raw: u32, max: u32, dy: f32, track: f32, fine: bool) -> u32 {
    let travel = if fine { track * 10.0 } else { track }.max(1.0);
    let moved = raw as f32 - dy * max as f32 / travel;
    moved.round().clamp(0.0, max as f32) as u32
}

/// Draws a vertical fader and returns what the user did to it, if anything.
/// The track fills whatever height the cell has.
pub fn fader(ui: &mut Ui, size: Vec2, name: &str, text: &str, raw: u32, max: u32) -> Option<Change> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let track = Rect::from_min_max(
        pos2(rect.center().x - 2.0, rect.top() + LABEL_BAND + 4.0),
        pos2(rect.center().x + 2.0, rect.bottom() - VALUE_BAND - 4.0),
    );
    let fine = ui.input(|input| input.modifiers.shift);
    let mut next = raw.min(max);
    if response.dragged() {
        next = fader_dragged(next, max, response.drag_delta().y, track.height(), fine);
    }
    let notches = wheel(ui, &response);
    if notches != 0 {
        next = wheeled_value(next, max, notches, fine);
    }
    let moved = (next != raw).then_some(Change::Raw(next));
    if !ui.is_rect_visible(rect) {
        return moved;
    }
    let hot = response.hovered() || response.dragged();
    label(ui, rect, name, hot);
    let typed = value_field(ui, rect, text).map(Change::Typed);
    let position = raw.min(max) as f32 / max.max(1) as f32;
    let y = track.bottom() - track.height() * position;
    let painter = ui.painter();
    painter.rect_filled(track, 2.0_f32, theme::EDGE);
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), y), track.max), 2.0_f32, theme::ACCENT);
    let cap = Rect::from_center_size(pos2(rect.center().x, y), vec2((rect.width() - 14.0).min(26.0), 9.0));
    painter.rect_filled(cap, 3.0_f32, if hot { theme::TEXT } else { theme::CABLE });
    response.on_hover_text(format!("{name}: {text}"));
    typed.or(moved)
}

/// Draws an on/off switch and returns the new state if it was flipped.
pub fn switch(ui: &mut Ui, size: Vec2, name: &str, on: bool) -> Option<bool> {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = response.hovered();
        label(ui, rect, name, hot);
        value_text(ui, rect, if on { "ON" } else { "OFF" });
        let body = Rect::from_center_size(pos2(rect.center().x, rect.top() + LABEL_BAND + 22.0), vec2(40.0, 20.0));
        let painter = ui.painter();
        painter.rect_filled(body, 10.0_f32, if on { theme::ACCENT } else { theme::EDGE });
        let x = if on { body.right() - 10.0 } else { body.left() + 10.0 };
        painter.circle_filled(pos2(x, body.center().y), 7.0, if hot { theme::TEXT } else { theme::CABLE });
    }
    let wheeled = match wheel(ui, &response) {
        0 => None,
        notches => Some(notches > 0).filter(|&wanted| wanted != on),
    };
    response.clicked().then_some(!on).or(wheeled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_up_raises_and_clamps() {
        assert!(dragged_value(1000, 65534, -10.0, false) > 1000);
        assert!(dragged_value(1000, 65534, 1.0, false) < 1000);
        assert_eq!(dragged_value(10, 65534, 500.0, false), 0);
        assert_eq!(dragged_value(65000, 65534, -500.0, false), 65534);
        assert_eq!(dragged_value(500, 65534, 0.0, false), 500);
    }

    #[test]
    fn a_fader_cap_follows_the_pointer() {
        // Dragging the length of the track covers the whole range, however long the track is.
        assert_eq!(fader_dragged(0, 65534, -150.0, 150.0, false), 65534);
        assert_eq!(fader_dragged(65534, 65534, 48.0, 48.0, false), 0);
        assert_eq!(fader_dragged(32767, 65534, -15.0, 150.0, false), 32767 + 6553);
        // Fine: a tenth of that. A track of no length does not divide by zero.
        assert_eq!(fader_dragged(32767, 65534, -15.0, 150.0, true), 32767 + 655);
        assert_eq!(fader_dragged(10, 100, -1.0, 0.0, false), 100);
    }

    #[test]
    fn typed_values_are_read_as_numbers() {
        assert_eq!(typed_number("-40"), Some(-40.0));
        assert_eq!(typed_number(" 1200 Hz "), Some(1200.0));
        assert_eq!(typed_number("3,5"), Some(3.5));
        assert_eq!(typed_number("+2.5 dB"), Some(2.5));
        assert_eq!(typed_number("loud"), None);
        assert_eq!(typed_number(""), None);
        // An edit starts from the number shown, without its unit.
        assert_eq!(editable("-65.2 dB"), "-65.2");
        assert_eq!(editable("OFF"), "");
    }

    #[test]
    fn fine_drag_moves_a_tenth() {
        let coarse = dragged_value(30000, 65534, -18.0, false) - 30000;
        let fine = dragged_value(30000, 65534, -18.0, true) - 30000;
        assert!((coarse as f32 / fine as f32 - 10.0).abs() < 0.1);
    }

    #[test]
    fn wheel_steps_by_range() {
        // A continuous range: a hundredth per notch, a thousandth with Shift.
        assert_eq!(wheeled_value(30000, 65534, 1, false), 30655);
        assert_eq!(wheeled_value(30000, 65534, -1, true), 29935);
        // A counted range, such as a tempo or a list: one per notch.
        assert_eq!(wheeled_value(120, 250, 1, false), 121);
        assert_eq!(wheeled_value(3, 7, -1, true), 2);
        // Ends hold.
        assert_eq!(wheeled_value(0, 7, -1, false), 0);
        assert_eq!(wheeled_value(65500, 65534, 1, false), 65534);
        assert_eq!(wheeled_value(99999, 250, 0, false), 250);
    }

    #[test]
    fn arc_runs_between_its_ends() {
        let points = arc(pos2(0.0, 0.0), 10.0, 0.0, PI);
        assert_eq!(points.len(), ARC_STEPS + 1);
        assert!((points[0].x - 10.0).abs() < 1e-4);
        assert!((points[ARC_STEPS].x + 10.0).abs() < 1e-4);
    }
}
