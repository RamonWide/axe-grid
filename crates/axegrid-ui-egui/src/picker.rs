//! A browser for long choice lists: amp models, cabs, and any other list
//! too long for a drop-down. It is a window with a search box and the
//! choices laid out in columns, read down one column and then the next.
//! A click sets the parameter at once, so that a sound can be auditioned
//! with the window still open; a double click or Enter sets it and closes.
//!
//! Like the rest of the shell it only draws what the engine published and
//! acts by running `param.set`, the command a drop-down runs. The names are
//! the ones the unit gave; nothing here asks the unit for anything.

use std::collections::BTreeMap;
use std::ops::Range;

use axegrid_model::blocks;
use egui::{
    vec2, Align, Align2, Color32, FontId, Id, Key, Modifiers, Order, Rect, RichText, Sense, Stroke, StrokeKind,
};
use serde_json::{json, Value};

use crate::{theme, Action, Shell};

/// A list with more choices than this is browsed in the window, not in a drop-down.
pub(crate) const THRESHOLD: usize = 24;
/// Height of one entry, and the narrowest and widest a column is drawn.
const ROW_HEIGHT: f32 = 22.0;
const COLUMN_WIDTH: f32 = 190.0;
const MAX_COLUMN_WIDTH: f32 = 320.0;
/// Room kept for the scroll bar under the columns when they do not all fit.
const SCROLL_BAR: f32 = 14.0;
/// Width of the number in front of a numbered entry.
const NUMBER_WIDTH: f32 = 38.0;
/// The window's share of the app window when it first opens, and its smallest size.
const DEFAULT_SHARE: f32 = 0.7;
const MIN_SIZE: egui::Vec2 = vec2(380.0, 220.0);
/// What the unit calls a user-cab slot that holds nothing.
const EMPTY_CAB: &str = "EMPTY";
/// Frames after opening during which the current choice is scrolled to;
/// the window has its final size only on the second one.
const REVEAL_FRAMES: u8 = 2;

/// Whether a list control opens the browser rather than a drop-down, given
/// how many choices it has (if they are read) and whether it picks a cab.
pub(crate) fn wanted(choices: Option<usize>, cab: bool) -> bool {
    cab || choices.is_some_and(|count| count > THRESHOLD)
}

/// Whether an entry matches what was typed in the search box: every word
/// typed must be part of its name or of its number. Case is ignored.
pub(crate) fn matches(filter: &str, number: Option<u32>, name: &str) -> bool {
    let name = name.to_lowercase();
    let number = number.map(|number| number.to_string()).unwrap_or_default();
    filter.split_whitespace().all(|word| name.contains(&word.to_lowercase()) || number.contains(word))
}

/// How a number of entries is laid out in a space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Columns {
    pub rows: usize,
    pub columns: usize,
    pub column_width: f32,
}

/// Lays `count` entries out in columns that are filled from the top, as
/// many rows as fit the height. Columns that fit the width share it; when
/// there are more, whole columns fill the width and the rest is scrolled to.
pub(crate) fn columns(count: usize, width: f32, height: f32) -> Columns {
    // A cast from a float saturates: nonsense sizes give one row and one column.
    let fit = ((width / COLUMN_WIDTH) as usize).max(1);
    let rows_in = |height: f32| ((height / ROW_HEIGHT) as usize).max(1);
    let mut rows = rows_in(height);
    if count > rows.saturating_mul(fit) {
        rows = rows_in(height - SCROLL_BAR);
    }
    let columns = count.div_ceil(rows).max(1);
    let column_width = if columns <= fit { (width / columns as f32).min(MAX_COLUMN_WIDTH) } else { width / fit as f32 };
    Columns { rows, columns, column_width: column_width.max(COLUMN_WIDTH / 2.0) }
}

impl Columns {
    /// Where the entry at a position is drawn, from the top left of the first.
    fn cell(&self, position: usize) -> Rect {
        let (column, row) = (position / self.rows.max(1), position % self.rows.max(1));
        let min = egui::pos2(column as f32 * self.column_width, row as f32 * ROW_HEIGHT);
        Rect::from_min_size(min, vec2(self.column_width, ROW_HEIGHT))
    }
}

/// A step of the highlight made with an arrow key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Up,
    Down,
    Left,
    Right,
}

/// Where the highlight goes from position `at` among `count` entries laid
/// out in columns of `rows`. Up and down follow the list, running on from
/// one column into the next; left and right change column. `None` when
/// there is nothing to highlight.
pub(crate) fn step(at: Option<usize>, count: usize, rows: usize, step: Step) -> Option<usize> {
    let last = count.checked_sub(1)?;
    let rows = rows.max(1);
    // With nothing highlighted any arrow starts at the top.
    let Some(at) = at.filter(|&at| at <= last) else { return Some(0) };
    Some(match step {
        Step::Up => at.saturating_sub(1),
        Step::Down => (at + 1).min(last),
        Step::Left => at.checked_sub(rows).unwrap_or(at),
        Step::Right if at + rows <= last => at + rows,
        // The last column may be short: from beside its gap, go to its end.
        Step::Right if at / rows < last / rows => last,
        Step::Right => at,
    })
}

/// What the engine says of the cab list behind a cab control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CabList {
    /// Positions in the list, and how many of their names have been read.
    pub total: u32,
    pub known: u32,
    /// Factory cabs and user cabs, on a model whose cabinet memory is known.
    pub slots: Option<(u32, u32)>,
}

impl CabList {
    pub(crate) fn from_json(value: &Value) -> Option<Self> {
        let number = |key: &str| value[key].as_u64().and_then(|n| u32::try_from(n).ok());
        Some(Self {
            total: number("total")?,
            known: number("known").unwrap_or(0),
            slots: number("factory").zip(number("user")),
        })
    }
}

/// The values of a cab parameter that pick a factory cab, a user cab, and
/// whatever the list holds after the user cabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CabRanges {
    pub factory: Range<u32>,
    pub user: Range<u32>,
    pub other: Range<u32>,
}

/// Splits a cab list of `total` positions: the factory cabs come first and
/// the user cabs follow them. Nothing reaches past the end of the list.
pub(crate) fn cab_ranges(total: u32, factory: u32, user: u32) -> CabRanges {
    let factory_end = factory.min(total);
    let user_end = factory_end.saturating_add(user).min(total);
    CabRanges { factory: 0..factory_end, user: factory_end..user_end, other: user_end..total }
}

/// One choice: the raw value that selects it, the number shown in front of
/// it if it has one, and its name if that has been read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Entry<'a> {
    pub value: u32,
    pub number: Option<u32>,
    pub name: Option<&'a str>,
}

/// A group of choices with a tab of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section<'a> {
    pub title: &'static str,
    /// The values that belong here, whether or not their names are read yet.
    pub range: Range<u32>,
    pub entries: Vec<Entry<'a>>,
    /// User-cab slots: one named [`EMPTY_CAB`] holds nothing.
    pub user: bool,
}

impl Section<'_> {
    /// Positions in `entries` of those matching the search box.
    fn matching(&self, filter: &str) -> Vec<usize> {
        let hit = |entry: &Entry<'_>| matches(filter, entry.number, self.shown_name(entry).0);
        self.entries.iter().enumerate().filter(|(_, entry)| hit(entry)).map(|(at, _)| at).collect()
    }

    /// The text shown for an entry, and whether it stands for a missing name.
    fn shown_name<'e>(&self, entry: &Entry<'e>) -> (&'e str, bool) {
        match entry.name {
            None => ("…", true),
            Some(EMPTY_CAB) if self.user => ("(empty)", true),
            Some(name) if name.trim().is_empty() => ("(no name)", true),
            Some(name) => (name, false),
        }
    }
}

/// Groups the choices of a control. An ordinary list is one group. A cab
/// list on a model whose cabinet memory is known is split into the factory
/// cabs and the numbered user slots; the user slots are offered as soon as
/// the cab list's length is known, with the names read so far (`user_names`,
/// by slot number from 1), while the factory cabs wait for their names.
pub(crate) fn sections<'a>(
    choices: Option<&'a [Value]>,
    cab: Option<CabList>,
    user_names: &'a BTreeMap<u16, String>,
) -> Vec<Section<'a>> {
    let named = |value: u32| choices.and_then(|list| list.get(value as usize)).and_then(Value::as_str);
    let listed = |range: Range<u32>, numbered: bool| -> Vec<Entry<'a>> {
        let known = range.filter(|_| choices.is_some());
        known.map(|value| Entry { value, number: numbered.then_some(value + 1), name: named(value) }).collect()
    };
    let whole =
        |range: Range<u32>| vec![Section { title: "", entries: listed(range.clone(), false), range, user: false }];
    let Some(cab) = cab else {
        return whole(0..choices.map_or(0, |list| u32::try_from(list.len()).unwrap_or(u32::MAX)));
    };
    let Some((factory, user)) = cab.slots else { return whole(0..cab.total) };
    let ranges = cab_ranges(cab.total, factory, user);
    let slots: Vec<Entry<'a>> = ranges
        .user
        .clone()
        .map(|value| {
            let number = value - ranges.user.start + 1;
            let read = u16::try_from(number).ok().and_then(|slot| user_names.get(&slot)).map(String::as_str);
            Entry { value, number: Some(number), name: read.or_else(|| named(value)) }
        })
        .collect();
    let mut sections = vec![
        Section {
            title: "Factory",
            entries: listed(ranges.factory.clone(), false),
            range: ranges.factory,
            user: false,
        },
        Section { title: "User", entries: slots, range: ranges.user, user: true },
        Section { title: "Other", entries: listed(ranges.other.clone(), true), range: ranges.other, user: false },
    ];
    sections.retain(|section| !section.range.is_empty());
    sections
}

/// The control of the block editor that shows a parameter, as the engine describes it.
pub(crate) fn find_control(editor: &Value, param: u64) -> Option<&Value> {
    let shows = |control: &&Value| control["param"].as_u64() == Some(param);
    let pages = editor["pages"].as_array().into_iter().flatten();
    let rows = pages.flat_map(|page| page["rows"].as_array().into_iter().flatten());
    let mut controls = rows.flat_map(|row| row["controls"].as_array().into_iter().flatten());
    Some(&editor["model"]).filter(shows).or_else(|| controls.find(shows))
}

/// The button that stands where a drop-down would and opens the browser.
pub(crate) fn button(ui: &mut egui::Ui, text: RichText, width: f32) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(vec2(width, 0.0)))
}

/// The browser while it is open: which parameter it sets, and what the
/// user has typed and highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Picker {
    block: u16,
    param: u64,
    title: String,
    filter: String,
    tab: usize,
    /// The value under the keyboard highlight.
    highlight: Option<u32>,
    /// Frames left in which the current choice is still to be brought into view.
    reveal: u8,
}

impl Picker {
    /// A browser for the list parameter `param` of `block`, titled after `label`.
    pub(crate) fn new(block: u16, param: u64, label: &str) -> Self {
        let title = format!("{}: {}", blocks::label(block), if label.is_empty() { "choose" } else { label });
        Self { block, param, title, filter: String::new(), tab: 0, highlight: None, reveal: REVEAL_FRAMES }
    }

    fn opening(&self) -> bool {
        self.reveal == REVEAL_FRAMES
    }
}

impl Shell {
    /// Draws the browser, if one is open, and turns what is chosen in it
    /// into the command a drop-down would run.
    pub(crate) fn picker_window(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(mut picker) = self.picker.take() else { return };
        // It belongs to a control of the block being edited: gone with it.
        let view = self.view.clone();
        if !view.connected || view.state.selected_block != Some(picker.block) {
            return;
        }
        let Some(control) = find_control(&view.editor, picker.param) else { return };
        let cab = CabList::from_json(&control["cab_list"]);
        let choices = control["choices"].as_array().map(Vec::as_slice);
        if cab.is_none() && choices.is_none() {
            return;
        }
        // What was just chosen, until the unit has answered.
        let asked = self.unconfirmed.get(&format!("{}/{}", picker.block, picker.param)).copied();
        let current = asked.or(control["value"].as_u64()).and_then(|value| u32::try_from(value).ok());
        let sections = sections(choices, cab, &view.state.cab_names);
        if picker.opening() {
            picker.highlight = current;
            picker.tab =
                sections.iter().position(|s| current.is_some_and(|value| s.range.contains(&value))).unwrap_or(0);
        }
        picker.tab = picker.tab.min(sections.len().saturating_sub(1));

        // Taken before the search box sees them: it has the keyboard.
        let (steps, enter, escape) = ctx.input_mut(|input| {
            let mut pressed = |key| input.consume_key(Modifiers::NONE, key);
            let arrows = [
                (Key::ArrowUp, Step::Up),
                (Key::ArrowDown, Step::Down),
                (Key::ArrowLeft, Step::Left),
                (Key::ArrowRight, Step::Right),
            ];
            let steps: Vec<Step> = arrows.into_iter().filter(|(key, _)| pressed(*key)).map(|(_, step)| step).collect();
            (steps, pressed(Key::Enter), pressed(Key::Escape))
        });

        // Everything else is out of reach while the browser is open; a click there closes it.
        let screen = ctx.content_rect();
        let backdrop = egui::Area::new(Id::new("picker-backdrop")).order(Order::Middle).fixed_pos(screen.min);
        let backdrop = backdrop.show(ctx, |ui| {
            let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0_f32, Color32::from_black_alpha(120));
            response.clicked() || response.secondary_clicked()
        });
        ctx.move_to_top(backdrop.response.layer_id);

        let mut open = true;
        let mut close = backdrop.inner || escape;
        let mut chosen = None;
        let size = (screen.size() * DEFAULT_SHARE).max(MIN_SIZE);
        let window = egui::Window::new(picker.title.clone())
            .id(Id::new("choice-picker"))
            .order(Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .min_size(MIN_SIZE)
            .default_size(size)
            .default_pos(screen.center() - size / 2.0);
        window.show(ctx, |ui| {
            let mut shown = Vec::new();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Find").small().color(theme::TEXT_DIM));
                let field =
                    egui::TextEdit::singleline(&mut picker.filter).desired_width(200.0).hint_text("name or number");
                let edit = ui.add(field);
                if !edit.has_focus() {
                    edit.request_focus();
                }
                shown = sections.iter().map(|section| section.matching(&picker.filter)).collect::<Vec<_>>();
                if sections.len() > 1 {
                    ui.separator();
                    for (index, section) in sections.iter().enumerate() {
                        let found = shown.get(index).map_or(0, Vec::len);
                        let searching = !picker.filter.trim().is_empty();
                        let title =
                            if searching { format!("{} ({found})", section.title) } else { section.title.to_owned() };
                        if ui.selectable_label(picker.tab == index, title).clicked() {
                            picker.tab = index;
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let keys = "Click to hear · double-click or Enter to choose · Esc to close";
                    ui.label(RichText::new(keys).small().color(theme::TEXT_DIM));
                });
            });
            // The cab list is read a few names at a time while the cab block is open.
            if let Some(cab) = cab.filter(|cab| cab.known < cab.total) {
                ui.horizontal(|ui| {
                    if self.lists_failed == Some(picker.block) {
                        ui.label(RichText::new("The cab names could not be read from the unit.").color(theme::DANGER));
                        if ui.small_button("Try again").clicked() {
                            self.lists_failed = None;
                        }
                    } else {
                        ui.spinner();
                        let progress = format!("Reading cab names from the unit: {} of {}", cab.known, cab.total);
                        ui.label(RichText::new(progress).small().color(theme::TEXT_DIM));
                    }
                });
            }
            ui.separator();
            let (Some(section), Some(shown)) = (sections.get(picker.tab), shown.get(picker.tab)) else { return };
            let value_at =
                |position: usize| shown.get(position).and_then(|&at| section.entries.get(at)).map(|e| e.value);
            let position_of = |value: Option<u32>| (0..shown.len()).find(|&position| value_at(position) == value);
            let space = ui.available_size();
            let grid = columns(shown.len(), space.x, space.y);

            let mut reveal = (picker.reveal > 0).then_some(Some(Align::Center));
            picker.reveal = picker.reveal.saturating_sub(1);
            for &arrow in &steps {
                let next = step(position_of(picker.highlight), shown.len(), grid.rows, arrow);
                picker.highlight = next.and_then(value_at).or(picker.highlight);
                reveal = Some(None);
            }
            if enter {
                // The highlighted entry, or the first one found when it is not among them.
                let target = position_of(picker.highlight).or(Some(0)).and_then(value_at);
                (chosen, close) = (target, target.is_some() || close);
            }
            if shown.is_empty() {
                let nothing = if section.entries.is_empty() { "Nothing to show yet." } else { "Nothing matches." };
                ui.label(RichText::new(nothing).color(theme::TEXT_DIM));
            }

            // The wheel moves along the columns, the only way there is to go.
            ui.style_mut().always_scroll_the_only_direction = true;
            let list = egui::ScrollArea::horizontal().id_salt(("picker-list", picker.tab)).auto_shrink(false);
            list.show_viewport(ui, |ui, viewport| {
                let extent = vec2(grid.columns as f32 * grid.column_width, grid.rows as f32 * ROW_HEIGHT);
                let (area, _) = ui.allocate_exact_size(extent, Sense::hover());
                let place = |position: usize| grid.cell(position).translate(area.min.to_vec2());
                let first = (viewport.min.x / grid.column_width).max(0.0) as usize;
                let last = ((viewport.max.x / grid.column_width).ceil().max(0.0) as usize).min(grid.columns);
                for position in first.saturating_mul(grid.rows)..last.saturating_mul(grid.rows).min(shown.len()) {
                    let Some(entry) = shown.get(position).and_then(|&at| section.entries.get(at)) else { continue };
                    let cell = place(position).shrink2(vec2(2.0, 1.0));
                    let response = ui.interact(cell, ui.id().with(("entry", entry.value)), Sense::click());
                    let (name, missing) = section.shown_name(entry);
                    let is_current = current == Some(entry.value);
                    let lit = picker.highlight == Some(entry.value);
                    let painter = ui.painter().with_clip_rect(cell.intersect(ui.clip_rect()));
                    let (fill, ink) = match (is_current, lit || response.hovered()) {
                        (true, _) => (theme::ACCENT, theme::ON_ACCENT),
                        (false, true) => (theme::RAISED_HOT, theme::TEXT),
                        (false, false) => (Color32::TRANSPARENT, theme::TEXT),
                    };
                    painter.rect_filled(cell, 4.0_f32, fill);
                    if lit {
                        painter.rect_stroke(cell, 4.0_f32, Stroke::new(1.5_f32, theme::ACCENT), StrokeKind::Inside);
                    }
                    let dim = if is_current { ink } else { theme::TEXT_DIM };
                    let mut left = cell.left() + 6.0;
                    if let Some(number) = entry.number {
                        let at = egui::pos2(left + NUMBER_WIDTH - 8.0, cell.center().y);
                        painter.text(at, Align2::RIGHT_CENTER, number, FontId::monospace(12.0), dim);
                        left += NUMBER_WIDTH;
                    }
                    let at = egui::pos2(left, cell.center().y);
                    painter.text(
                        at,
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(13.0),
                        if missing { dim } else { ink },
                    );
                    if response.clicked() {
                        (chosen, picker.highlight) = (Some(entry.value), Some(entry.value));
                    }
                    close |= response.double_clicked();
                    response.on_hover_text(name);
                }
                if let (Some(align), Some(position)) = (reveal, position_of(picker.highlight)) {
                    ui.scroll_to_rect(place(position), align);
                }
            });
        });

        // The same command, with the same parameters, as a drop-down runs.
        if let Some(value) = chosen.filter(|&value| current != Some(value)) {
            actions.push(("param.set", json!({ "block": picker.block, "param": picker.param, "value": value })));
        }
        if open && !close {
            self.picker = Some(picker);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lists_and_cabs_open_the_browser() {
        assert!(!wanted(Some(3), false));
        assert!(!wanted(Some(THRESHOLD), false));
        assert!(wanted(Some(THRESHOLD + 1), false));
        // A list that has not been read is not known to be long.
        assert!(!wanted(None, false));
        // A cab control always does, also while its names are being read.
        assert!(wanted(None, true) && wanted(Some(2), true));
    }

    #[test]
    fn the_search_matches_names_and_numbers_whatever_the_case() {
        assert!(matches("", None, "Anything"));
        assert!(matches("   ", Some(4), ""));
        assert!(matches("crunch", None, "Brit CRUNCH 2"));
        assert!(matches("BRIT", None, "brit crunch"));
        assert!(!matches("clean", None, "Brit Crunch"));
        // Every word typed has to be found, in any order.
        assert!(matches("2 brit", None, "Brit Crunch 2"));
        assert!(!matches("brit 3", None, "Brit Crunch 2"));
        // A number matches the entry's own number as well as its name.
        assert!(matches("41", Some(41), "My cab"));
        assert!(matches("4", Some(141), "My cab"));
        assert!(!matches("41", Some(7), "My cab"));
        assert!(!matches("41", None, "My cab"));
        assert!(matches("ÉCLAT", None, "éclat"));
    }

    #[test]
    fn entries_fill_columns_from_the_top() {
        // Ten rows fit; three columns would, and 25 entries need exactly three.
        let fits = columns(25, 3.0 * COLUMN_WIDTH, 10.0 * ROW_HEIGHT);
        assert_eq!((fits.rows, fits.columns), (10, 3));
        assert_eq!(fits.column_width, COLUMN_WIDTH);
        // Few entries: the columns share the width, up to a limit.
        let few = columns(12, 3.0 * COLUMN_WIDTH, 10.0 * ROW_HEIGHT);
        assert_eq!((few.rows, few.columns), (10, 2));
        assert_eq!(few.column_width, 1.5 * COLUMN_WIDTH);
        assert_eq!(columns(3, 1000.0, 10.0 * ROW_HEIGHT).column_width, MAX_COLUMN_WIDTH);
        // Too many: a row makes way for the scroll bar, and whole columns fill the width.
        let many = columns(280, 3.5 * COLUMN_WIDTH, 10.0 * ROW_HEIGHT);
        assert_eq!((many.rows, many.columns), (9, 32));
        assert_eq!(many.column_width, 3.5 * COLUMN_WIDTH / 3.0);
        // Every entry has a place.
        assert!(many.rows * many.columns >= 280);
    }

    #[test]
    fn a_layout_survives_nonsense_sizes() {
        for (width, height) in [(0.0, 0.0), (-50.0, -50.0), (f32::NAN, f32::NAN), (1.0, 1.0e9)] {
            let grid = columns(40, width, height);
            assert!(grid.rows >= 1 && grid.columns >= 1 && grid.column_width > 0.0, "{width} {height}: {grid:?}");
            assert!(grid.rows.saturating_mul(grid.columns) >= 40);
        }
        let none = columns(0, 600.0, 400.0);
        assert_eq!(none.columns, 1);
    }

    #[test]
    fn reading_down_a_column_follows_the_list() {
        let grid = Columns { rows: 4, columns: 3, column_width: 100.0 };
        assert_eq!(grid.cell(0).min, egui::pos2(0.0, 0.0));
        assert_eq!(grid.cell(3).min, egui::pos2(0.0, 3.0 * ROW_HEIGHT));
        // The fifth entry starts the second column.
        assert_eq!(grid.cell(4).min, egui::pos2(100.0, 0.0));
        assert_eq!(grid.cell(9).min, egui::pos2(200.0, ROW_HEIGHT));
        assert_eq!(grid.cell(9).size(), vec2(100.0, ROW_HEIGHT));
    }

    #[test]
    fn arrows_move_the_highlight_within_the_list() {
        // Ten entries in columns of four: 0-3, 4-7, 8-9.
        let go = |at, arrow| step(Some(at), 10, 4, arrow);
        assert_eq!(go(5, Step::Up), Some(4));
        assert_eq!(go(5, Step::Down), Some(6));
        // Up and down run on from one column into the next.
        assert_eq!((go(3, Step::Down), go(4, Step::Up)), (Some(4), Some(3)));
        assert_eq!((go(0, Step::Up), go(9, Step::Down)), (Some(0), Some(9)));
        assert_eq!((go(5, Step::Left), go(5, Step::Right)), (Some(1), Some(9)));
        assert_eq!((go(1, Step::Left), go(8, Step::Right)), (Some(1), Some(8)));
        // Beside the gap under a short last column: to its last entry.
        assert_eq!(go(6, Step::Right), Some(9));
        assert_eq!(go(2, Step::Right), Some(6));
    }

    #[test]
    fn arrows_cope_with_no_highlight_and_no_entries() {
        for arrow in [Step::Up, Step::Down, Step::Left, Step::Right] {
            assert_eq!(step(None, 0, 4, arrow), None);
            assert_eq!(step(Some(3), 0, 4, arrow), None);
            assert_eq!(step(None, 10, 4, arrow), Some(0));
            // A highlight left over from a longer list starts again at the top.
            assert_eq!(step(Some(40), 10, 4, arrow), Some(0));
            // No rows is treated as one.
            assert!(step(Some(1), 3, 0, arrow).is_some_and(|at| at < 3));
        }
        assert_eq!(step(Some(0), 1, 4, Step::Right), Some(0));
    }

    #[test]
    fn user_cabs_follow_the_factory_cabs() {
        let xl = cab_ranges(1217, 189, 1024);
        assert_eq!((xl.factory, xl.user, xl.other), (0..189, 189..1213, 1213..1217));
        // A list shorter than the table says is never reached past.
        let short = cab_ranges(200, 189, 1024);
        assert_eq!((short.factory, short.user, short.other), (0..189, 189..200, 200..200));
        let tiny = cab_ranges(5, 189, 1024);
        assert_eq!((tiny.factory, tiny.user.is_empty(), tiny.other.is_empty()), (0..5, true, true));
        let huge = cab_ranges(u32::MAX, 189, u32::MAX);
        assert_eq!(huge.user.end, u32::MAX);
    }

    #[test]
    fn the_cab_list_is_read_from_what_the_engine_says() {
        let list = CabList::from_json(&json!({ "total": 1217, "known": 40, "factory": 189, "user": 1024 }));
        assert_eq!(list, Some(CabList { total: 1217, known: 40, slots: Some((189, 1024)) }));
        // A model whose cabinet memory is not known has no split.
        let plain = CabList::from_json(&json!({ "total": 90, "known": 90, "factory": null, "user": null }));
        assert_eq!(plain.map(|list| list.slots), Some(None));
        assert_eq!(CabList::from_json(&Value::Null), None);
        assert_eq!(CabList::from_json(&json!({ "total": -1 })), None);
    }

    #[test]
    fn an_ordinary_list_is_one_group() {
        let choices = [json!("One"), json!("Two"), json!("Three")];
        let names = BTreeMap::new();
        let groups = sections(Some(&choices), None, &names);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].range, 0..3);
        assert_eq!(groups[0].entries[2], Entry { value: 2, number: None, name: Some("Three") });
        assert_eq!(groups[0].matching("t"), vec![1, 2]);
        // A list that is not there yet is an empty group, not a crash.
        assert!(sections(None, None, &names)[0].entries.is_empty());
    }

    #[test]
    fn cabs_are_split_into_factory_and_user() {
        // Two factory cabs, three user slots and one entry after them.
        let choices = [json!("F one"), json!("F two"), json!("Mine"), json!("EMPTY"), json!("EMPTY"), json!("")];
        let cab = CabList { total: 6, known: 6, slots: Some((2, 3)) };
        let names = BTreeMap::from([(1, "Mine".to_owned()), (2, "EMPTY".to_owned())]);
        let groups = sections(Some(&choices), Some(cab), &names);
        assert_eq!(groups.iter().map(|g| g.title).collect::<Vec<_>>(), ["Factory", "User", "Other"]);
        assert_eq!(
            groups[0].entries,
            [
                Entry { value: 0, number: None, name: Some("F one") },
                Entry { value: 1, number: None, name: Some("F two") }
            ]
        );
        // A user slot is numbered from 1 and selects the position after the factory cabs.
        assert_eq!(groups[1].entries[0], Entry { value: 2, number: Some(1), name: Some("Mine") });
        assert_eq!(groups[1].entries[2], Entry { value: 4, number: Some(3), name: Some("EMPTY") });
        assert_eq!(groups[1].shown_name(&groups[1].entries[2]), ("(empty)", true));
        assert_eq!(groups[1].shown_name(&groups[1].entries[0]), ("Mine", false));
        assert_eq!(groups[2].entries, [Entry { value: 5, number: Some(6), name: Some("") }]);
        assert_eq!(groups[2].shown_name(&groups[2].entries[0]), ("(no name)", true));
        // The search finds a slot by its number or its name, and an empty one as "empty".
        assert_eq!(groups[1].matching("3"), vec![2]);
        assert_eq!(groups[1].matching("mine"), vec![0]);
        assert_eq!(groups[1].matching("empty"), vec![1, 2]);
        // Only a user slot named EMPTY is an empty one.
        let factory = Section { user: false, ..groups[1].clone() };
        assert_eq!(factory.shown_name(&groups[1].entries[2]), ("EMPTY", false));
    }

    #[test]
    fn user_slots_are_offered_while_the_names_are_being_read() {
        let cab = CabList { total: 1217, known: 191, slots: Some((189, 1024)) };
        let names = BTreeMap::from([(1, "Mine".to_owned())]);
        let groups = sections(None, Some(cab), &names);
        assert_eq!(groups.iter().map(|g| g.title).collect::<Vec<_>>(), ["Factory", "User", "Other"]);
        // The factory cabs wait for their names; their tab still knows its values.
        assert!(groups[0].entries.is_empty() && groups[0].range == (0..189));
        assert_eq!(groups[1].entries.len(), 1024);
        assert_eq!(groups[1].entries[0], Entry { value: 189, number: Some(1), name: Some("Mine") });
        assert_eq!(groups[1].entries[1023], Entry { value: 1212, number: Some(1024), name: None });
        assert_eq!(groups[1].shown_name(&groups[1].entries[1023]), ("…", true));
        assert!(groups[2].entries.is_empty());
        // Without the model's table there is nothing to split by, and nothing to offer yet.
        let unknown = sections(None, Some(CabList { total: 90, known: 3, slots: None }), &names);
        assert_eq!((unknown.len(), unknown[0].range.clone(), unknown[0].entries.len()), (1, 0..90, 0));
    }

    #[test]
    fn a_control_is_found_in_the_title_row_or_on_a_page() {
        let editor = json!({
            "model": { "param": 0, "label": "Type" },
            "pages": [{ "rows": [{ "controls": [{ "kind": "spacer" }, { "param": 7, "label": "Cab" }] }] }],
        });
        assert_eq!(find_control(&editor, 0).map(|c| &c["label"]), Some(&json!("Type")));
        assert_eq!(find_control(&editor, 7).map(|c| &c["label"]), Some(&json!("Cab")));
        assert!(find_control(&editor, 8).is_none());
        assert!(find_control(&Value::Null, 0).is_none());
    }

    #[test]
    fn a_new_browser_is_named_after_its_control() {
        let picker = Picker::new(106, 0, "Type");
        assert!(picker.title.ends_with(": Type") && picker.opening());
        assert!(Picker::new(106, 0, "").title.ends_with(": choose"));
    }
}
