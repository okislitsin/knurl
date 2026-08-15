use core::cell::Cell;

use crate::{Area, BorderStyle, Component, FormField, Msg, RenderTarget, Style, draw_cursor_band};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

/// Formats `v` as a decimal string into a stack buffer - no allocation.
fn fmt_i32(buf: &mut [u8; 12], v: i32) -> &str {
    let neg = v < 0;
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        let mut n = (v as i64).unsigned_abs(); // correct for i32::MIN
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        if neg {
            i -= 1;
            buf[i] = b'-';
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap()
}

/// Draws `value` right-aligned within `area` (pixels), with `label` on the left
/// truncated to fit, leaving a one-character gap before the value.
///
/// `row` is the style the whole row is drawn in - [`Style::Focus`] on a focus
/// band, [`Style::Normal`] otherwise (see
/// [`draw_cursor_band`](crate::draw_cursor_band)); label and value share it, so a
/// focused row is one block instead of a highlight around the label.
///
/// While `editing`, the value **cuts out** of that block: its cell is cleared
/// back to the screen ground and the value drawn plain on it, so the number
/// being changed reads as a chip inside the highlight. That is the one edit cue
/// that survives monochrome - restyling the value cannot say it, because on a
/// band every style the theme inverts is the same ink.
fn draw_labeled_value(
    target: &mut dyn RenderTarget,
    area: Area,
    label: &str,
    value: &str,
    row: Style,
    editing: bool,
) {
    let cw = target.char_width().max(1);
    let value_px = target.text_width(value);

    let label_avail = area.w.saturating_sub(value_px.saturating_add(cw));
    let label_max = (label_avail / cw) as usize;
    if label_max > 0 {
        target.draw_text(area.x, area.y, truncate(label, label_max), row);
    }

    if value_px <= area.w {
        let vx = area.x + area.w - value_px;
        if editing {
            target.clear(Area::new(vx, area.y, value_px, area.h));
            target.draw_text(vx, area.y, value, Style::Normal);
        } else {
            target.draw_text(vx, area.y, value, row);
        }
    }
}

/// Clamps `v` into `[min, max]`. A hand-rolled `Ord::clamp`, because that one is
/// neither `const` (so builders could not use it) nor total: it panics when a
/// caller passes `min > max`, and a widget has no business panicking over a
/// range someone typed backwards. Here the floor simply wins.
const fn clamp_i32(v: i32, min: i32, max: i32) -> i32 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// The indicator box (a 3-character-wide square slot) at the area's left, and the
/// pixel x where label text begins (4 characters in: 3 for the box, 1 gap) - the
/// pixel analogue of the old `[x] Label` cell layout.
fn indicator_slot(area: Area, cw: u16) -> (Area, u16) {
    let ind = Area::new(area.x, area.y, 3 * cw, area.h);
    (ind, area.x + 4 * cw)
}

// ── Checkbox ────────────────────────────────────────────────────────────────

/// A labelled checkbox toggled with [`Msg::Select`].
#[derive(Debug)]
pub struct Checkbox<'a> {
    label: &'a str,
    checked: bool,
    focused: bool,
    dirty: Cell<bool>,
}

impl<'a> Checkbox<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            checked: false,
            focused: false,
            dirty: Cell::new(true),
        }
    }

    pub const fn with_checked(mut self, c: bool) -> Self {
        self.checked = c;
        self
    }

    pub fn is_checked(&self) -> bool {
        self.checked
    }

    pub fn set_checked(&mut self, c: bool) {
        if c != self.checked {
            self.checked = c;
            self.dirty.set(true);
        }
    }

    pub fn toggle(&mut self) {
        self.checked = !self.checked;
        self.dirty.set(true);
    }
}

impl<'a> Component for Checkbox<'a> {
    fn update(&mut self, msg: &Msg) {
        if let Msg::Select = msg {
            self.checked = !self.checked;
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);
        // Focused: the whole row is a band, box included - not just the label.
        let style = draw_cursor_band(target, area, self.focused);
        let (ind, label_x) = indicator_slot(area, cw);
        target.draw_check(ind, self.checked, style);

        let avail = area.w.saturating_sub(4 * cw);
        let max = (avail / cw) as usize;
        if max > 0 {
            target.draw_text(label_x, area.y, truncate(self.label, max), style);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

// ── Toggle ──────────────────────────────────────────────────────────────────

/// A labelled ON/OFF switch, shown as a label with a pixel checkbox indicator on
/// the right (filled = on). Toggled with [`Msg::Select`] (or `Left`/`Right`).
#[derive(Debug)]
pub struct Toggle<'a> {
    label: &'a str,
    on: bool,
    focused: bool,
    dirty: Cell<bool>,
}

impl<'a> Toggle<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            on: false,
            focused: false,
            dirty: Cell::new(true),
        }
    }

    pub const fn with_on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn set_on(&mut self, on: bool) {
        if on != self.on {
            self.on = on;
            self.dirty.set(true);
        }
    }

    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.dirty.set(true);
    }
}

impl<'a> Component for Toggle<'a> {
    fn update(&mut self, msg: &Msg) {
        let before = self.on;
        match msg {
            Msg::Select => self.on = !self.on,
            Msg::Right => self.on = true,
            Msg::Left => self.on = false,
            _ => {}
        }
        if self.on != before {
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);
        let style = draw_cursor_band(target, area, self.focused);

        // Indicator right-aligned (3-char slot); label fills the rest. Both sit
        // inside the band, so the gap between them is highlighted too.
        let ind_w = 3 * cw;
        if area.w >= ind_w {
            let ind = Area::new(area.x + area.w - ind_w, area.y, ind_w, area.h);
            target.draw_check(ind, self.on, style);
        }
        let label_avail = area.w.saturating_sub(ind_w + cw);
        let max = (label_avail / cw) as usize;
        if max > 0 {
            target.draw_text(area.x, area.y, truncate(self.label, max), style);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

// ── Button ──────────────────────────────────────────────────────────────────

/// A focusable, momentary action item: [`Msg::Select`] latches a one-shot
/// "pressed" flag, read (and cleared) via [`take_pressed`](Button::take_pressed).
///
/// Unlike [`Checkbox`]/[`Toggle`], a `Button` carries no persistent value - it
/// just reports "activated since you last asked", the same poll-after-update
/// convention every other widget uses (see [`List::selected`](crate::List::selected)),
/// specialised for a momentary action instead of a state.
#[derive(Debug)]
pub struct Button<'a> {
    label: &'a str,
    focused: bool,
    pressed: bool,
    dirty: Cell<bool>,
}

impl<'a> Button<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            focused: false,
            pressed: false,
            dirty: Cell::new(true),
        }
    }

    /// Returns whether the button was pressed since the last call, clearing
    /// the flag (`core::mem::take`) so a press is only ever reported once.
    pub fn take_pressed(&mut self) -> bool {
        core::mem::take(&mut self.pressed)
    }
}

impl<'a> Component for Button<'a> {
    fn update(&mut self, msg: &Msg) {
        if let Msg::Select = msg {
            // No dirty: `pressed` is a latch the app polls, not something the
            // button draws, so a press changes no pixel. (It is also cleared by
            // `take_pressed` before the next frame, so there would be nothing
            // left to render by the time a repaint ran.)
            self.pressed = true;
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let style = draw_cursor_band(target, area, self.focused);
        target.draw_text(area.x, area.y, self.label, style);
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

impl<'a> FormField for Button<'a> {} // momentary (editable = false)

// ── Counter ─────────────────────────────────────────────────────────────────

/// A labelled integer value bounded to `[min, max]`, adjusted in `step` units.
#[derive(Debug)]
pub struct Counter<'a> {
    label: &'a str,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    focused: bool,
    editing: bool,
    dirty: Cell<bool>,
}

impl<'a> Counter<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            value: 0,
            min: 0,
            max: 100,
            step: 1,
            focused: false,
            editing: false,
            dirty: Cell::new(true),
        }
    }

    /// Sets the value, clamped into the range set **so far** (the default
    /// `0..=100` unless [`with_range`](Counter::with_range) came first).
    ///
    /// Every entry point clamps - this one, [`set_value`](Counter::set_value)
    /// and [`update`](Counter::update) - so the value is always inside the range
    /// and `Up`/`Down` are symmetric. It used to be exempt "because the range
    /// may be set later", which let a value sit below `min`, where `Up` could
    /// only walk up from it one step at a time. `with_range` re-clamps for that
    /// case, so either builder order lands in range.
    pub const fn with_value(mut self, v: i32) -> Self {
        self.value = clamp_i32(v, self.min, self.max);
        self
    }

    /// Sets the bounds, pulling the current value into them.
    pub const fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min = min;
        self.max = max;
        self.value = clamp_i32(self.value, min, max);
        self
    }

    pub const fn with_step(mut self, step: i32) -> Self {
        self.step = step;
        self
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    /// Sets the value, clamped into `[min, max]`.
    pub fn set_value(&mut self, v: i32) {
        let clamped = clamp_i32(v, self.min, self.max);
        if clamped != self.value {
            self.value = clamped;
            self.dirty.set(true);
        }
    }
}

impl<'a> Component for Counter<'a> {
    fn update(&mut self, msg: &Msg) {
        let before = self.value;
        // Both directions clamp against *both* bounds, so a step never leaves
        // the range from either end.
        match msg {
            Msg::Up | Msg::Right => {
                self.value = clamp_i32(self.value.saturating_add(self.step), self.min, self.max);
            }
            Msg::Down | Msg::Left => {
                self.value = clamp_i32(self.value.saturating_sub(self.step), self.min, self.max);
            }
            _ => {}
        }
        if self.value != before {
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let mut buf = [0u8; 12];
        let value = fmt_i32(&mut buf, self.value);
        // Focus bands the whole row, value included; edit mode then cuts the
        // value back out of the band, so a focused-but-idle row never looks
        // like it is being changed.
        let row = draw_cursor_band(target, area, self.focused);
        draw_labeled_value(target, area, self.label, value, row, self.editing);
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

// ── Slider ──────────────────────────────────────────────────────────────────

/// A labelled integer value shown as a pixel track + fill (via
/// [`draw_bar`](RenderTarget::draw_bar)), bounded to `[min, max]` and adjusted in
/// `step` units.
///
/// **Edit-mode cue works on monochrome too:** while editing, a thin 1px frame is
/// drawn around the bar (the colour fill alone would be invisible on a 1-bit
/// panel). On colour the fill also switches to `Focus`.
///
/// **Focus** bands the row like every other field, except that the band stops
/// at the bar, which keeps its own ground - see the note in
/// [`draw`](Slider::draw).
#[derive(Debug)]
pub struct Slider<'a> {
    label: &'a str,
    value: i32,
    min: i32,
    max: i32,
    step: i32,
    label_w: u16,
    focused: bool,
    editing: bool,
    dirty: Cell<bool>,
}

impl<'a> Slider<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            value: 0,
            min: 0,
            max: 100,
            step: 10,
            // Pixel width reserved for the label column (≈8 chars at 6px).
            label_w: 48,
            focused: false,
            editing: false,
            dirty: Cell::new(true),
        }
    }

    /// Sets the bounds, pulling the current value into them.
    pub const fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min = min;
        self.max = max;
        self.value = clamp_i32(self.value, min, max);
        self
    }

    pub const fn with_step(mut self, step: i32) -> Self {
        self.step = step;
        self
    }

    /// Sets the value, clamped into the range set **so far** (the default
    /// `0..=100` unless [`with_range`](Slider::with_range) came first). Mirrors
    /// [`Counter::with_value`] - every entry point clamps, so `Up`/`Down` are
    /// symmetric and either builder order lands in range.
    pub const fn with_value(mut self, v: i32) -> Self {
        self.value = clamp_i32(v, self.min, self.max);
        self
    }

    /// Sets the label column width, in **pixels**.
    pub const fn with_label_width(mut self, px: u16) -> Self {
        self.label_w = px;
        self
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    /// Sets the value, clamped into `[min, max]`.
    pub fn set_value(&mut self, v: i32) {
        let clamped = clamp_i32(v, self.min, self.max);
        if clamped != self.value {
            self.value = clamped;
            self.dirty.set(true);
        }
    }

    fn permille(&self) -> u16 {
        let span = (self.max - self.min).max(1) as u32;
        let pos = (self.value - self.min).max(0) as u32;
        (pos * 1000 / span).min(1000) as u16
    }
}

impl<'a> Component for Slider<'a> {
    fn update(&mut self, msg: &Msg) {
        let before = self.value;
        // Symmetric with Counter: both directions clamp against both bounds.
        match msg {
            Msg::Up | Msg::Right => {
                self.value = clamp_i32(self.value.saturating_add(self.step), self.min, self.max);
            }
            Msg::Down | Msg::Left => {
                self.value = clamp_i32(self.value.saturating_sub(self.step), self.min, self.max);
            }
            _ => {}
        }
        if self.value != before {
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }

        let cw = target.char_width().max(1);
        // The band stops at the bar - the one field where it does not run the
        // full row. A monochrome `draw_bar` is ink: it ignores the style on
        // purpose (Phase 1), so track and fill come out `On` whatever is asked
        // for, and an inverted band under them would leave `On` on `On`. Giving
        // the bar its own ground keeps it (and its edit frame) legible, and the
        // band still ends on a straight column rather than hugging the label
        // text, so the row does not read as ragged.
        let label_style = draw_cursor_band(
            target,
            Area::new(area.x, area.y, self.label_w.min(area.w), area.h),
            self.focused,
        );
        let lw = self.label_w.min(area.w);
        let label_max = (lw / cw) as usize;
        if label_max > 0 {
            target.draw_text(area.x, area.y, truncate(self.label, label_max), label_style);
        }

        let bar_x = area.x.saturating_add(self.label_w);
        let bar_w = area.w.saturating_sub(self.label_w);
        if bar_w == 0 {
            return;
        }
        let bar = Area::new(bar_x, area.y, bar_w, area.h);
        let permille = self.permille();

        if self.editing {
            // Monochrome-safe edit cue: a crisp frame around the bar, with the
            // fill inset inside it. Colour additionally tints the fill `Focus`.
            target.draw_box(bar, BorderStyle::Single);
            let inner = bar.inner_by(1).unwrap_or(bar);
            target.draw_bar(inner, permille, Style::Focus);
        } else {
            target.draw_bar(bar, permille, Style::Accent);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

// ── PickerItem ──────────────────────────────────────────────────────────────

pub trait PickerItem {
    fn as_str(&self) -> &str;
}

impl PickerItem for &str {
    fn as_str(&self) -> &str {
        self
    }
}

// ── Picker ──────────────────────────────────────────────────────────────────

/// A labelled inline picker that cycles through a fixed list of options. The
/// option is shown right-aligned (like [`Counter`]); it highlights `Focus` only
/// while editing.
#[derive(Debug)]
pub struct Picker<'a, T: PickerItem = &'a str> {
    label: &'a str,
    options: &'a [T],
    selected: usize,
    wrap: bool,
    focused: bool,
    editing: bool,
    dirty: Cell<bool>,
}

impl<'a, T: PickerItem> Picker<'a, T> {
    pub fn new(label: &'a str, options: &'a [T]) -> Self {
        Self {
            label,
            options,
            selected: 0,
            wrap: true,
            focused: false,
            editing: false,
            dirty: Cell::new(true),
        }
    }

    pub const fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Sets the selected index, clamped into `[0, len - 1]` (no-op with no options).
    pub fn with_selected(mut self, idx: usize) -> Self {
        self.set_selected(idx);
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The currently selected option, or `""` when there are no options.
    pub fn selected_option(&self) -> Option<&T> {
        self.options.get(self.selected)
    }

    /// Sets the selected index, clamped into `[0, len - 1]`.
    pub fn set_selected(&mut self, idx: usize) {
        let n = self.options.len();
        if n > 0 {
            let clamped = idx.min(n - 1);
            if clamped != self.selected {
                self.selected = clamped;
                self.dirty.set(true);
            }
        }
    }
}

impl<'a, T: PickerItem> Component for Picker<'a, T> {
    fn update(&mut self, msg: &Msg) {
        let n = self.options.len();
        if n == 0 {
            return;
        }
        let before = self.selected;
        match msg {
            Msg::Down => {
                self.selected = if self.selected + 1 < n {
                    self.selected + 1
                } else if self.wrap {
                    0
                } else {
                    self.selected
                };
            }
            Msg::Up => {
                self.selected = if self.selected > 0 {
                    self.selected - 1
                } else if self.wrap {
                    n - 1
                } else {
                    0
                };
            }
            _ => {}
        }
        if self.selected != before {
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let opt = match self.selected_option() {
            Some(opt) => opt.as_str(),
            None => "",
        };
        // Band on focus, option cut back out while editing - mirrors Counter.
        let row = draw_cursor_band(target, area, self.focused);
        draw_labeled_value(target, area, self.label, opt, row, self.editing);
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
    }

    fn blur(&mut self) {
        self.focused = false;
        self.dirty.set(true);
    }

    fn dirty(&self) -> bool {
        self.dirty.get()
    }

    fn mark_clean(&self) {
        self.dirty.set(false);
    }

    fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

// ── FormField ─────────────────────────────────────────────────────────────────

impl<'a> FormField for Checkbox<'a> {} // momentary (editable = false)

impl<'a> FormField for Toggle<'a> {} // momentary

impl<'a> FormField for Counter<'a> {
    fn editable(&self) -> bool {
        true
    }

    fn set_editing(&mut self, editing: bool) {
        if editing != self.editing {
            self.editing = editing;
            self.dirty.set(true);
        }
    }
}

impl<'a> FormField for Slider<'a> {
    fn editable(&self) -> bool {
        true
    }

    fn set_editing(&mut self, editing: bool) {
        if editing != self.editing {
            self.editing = editing;
            self.dirty.set(true);
        }
    }
}

impl<'a, T: PickerItem> FormField for Picker<'a, T> {
    fn editable(&self) -> bool {
        true
    }

    fn set_editing(&mut self, editing: bool) {
        if editing != self.editing {
            self.editing = editing;
            self.dirty.set(true);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use alloc::vec::Vec;

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.

    fn texts(t: &RecordingTarget) -> Vec<(u16, u16, alloc::string::String, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.clone(), *style)),
                _ => None,
            })
            .collect()
    }

    fn has_text(t: &RecordingTarget, want: &str) -> bool {
        texts(t).iter().any(|(_, _, s, _)| s == want)
    }

    fn bands(t: &RecordingTarget) -> Vec<(Area, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Band { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect()
    }

    // ── Focus language (the band across a whole field row) ─────────────────────

    /// Every field that highlights on focus does it the same way: a band across
    /// the **whole row it was given**, with its indicator and value inside the
    /// block rather than beside it.
    #[test]
    fn focused_fields_band_their_whole_row() {
        let area = Area::new(0, 0, 120, 10);
        let expect = [(area, Style::Focus)];

        let mut cb = Checkbox::new("WiFi");
        cb.focus();
        let mut t = RecordingTarget::new(120, 10);
        cb.view(&mut t, area);
        assert_eq!(bands(&t), expect, "Checkbox");
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "WiFi" && *st == Style::Focus)
        );

        let mut tg = Toggle::new("Sound");
        tg.focus();
        let mut t = RecordingTarget::new(120, 10);
        tg.view(&mut t, area);
        assert_eq!(bands(&t), expect, "Toggle");

        let mut b = Button::new("Go");
        b.focus();
        let mut t = RecordingTarget::new(120, 10);
        b.view(&mut t, area);
        assert_eq!(bands(&t), expect, "Button");

        let mut c = Counter::new("Vol");
        c.focus();
        let mut t = RecordingTarget::new(120, 10);
        c.view(&mut t, area);
        assert_eq!(bands(&t), expect, "Counter");
        // The value is inside the band too - that is the whole point.
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "0" && *st == Style::Focus)
        );

        let mut p = Picker::new("Mode", OPTS);
        p.focus();
        let mut t = RecordingTarget::new(120, 10);
        p.view(&mut t, area);
        assert_eq!(bands(&t), expect, "Picker");
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "Alpha" && *st == Style::Focus)
        );

        // The Slider is the one exception - its band stops at the bar; see
        // `slider_band_stops_at_the_bar`.
    }

    /// An unfocused field draws no band at all.
    #[test]
    fn unfocused_fields_draw_no_band() {
        let area = Area::new(0, 0, 120, 10);
        let mut t = RecordingTarget::new(120, 10);
        Checkbox::new("WiFi").view(&mut t, area);
        Toggle::new("Sound").view(&mut t, area);
        Button::new("Go").view(&mut t, area);
        Counter::new("Vol").view(&mut t, area);
        Slider::new("Vol").view(&mut t, area);
        Picker::new("Mode", OPTS).view(&mut t, area);
        assert!(bands(&t).is_empty());
    }

    /// The value being edited **cuts out** of the band: its cell is cleared back
    /// to the screen ground and the number drawn plain on it. A colour swap
    /// could not say this - on a monochrome band both states are the same ink.
    #[test]
    fn edited_value_cuts_out_of_the_band() {
        let area = Area::new(0, 0, 120, 10);
        let mut c = Counter::new("Vol").with_value(60);
        c.focus();

        // Focused, idle: the value sits inside the band, no cut-out.
        let mut t0 = RecordingTarget::new(120, 10);
        c.view(&mut t0, area);
        assert!(
            texts(&t0)
                .iter()
                .any(|(_, _, s, st)| s == "60" && *st == Style::Focus)
        );
        assert_eq!(
            t0.ops()
                .iter()
                .filter(|op| matches!(op, Op::Clear { .. }))
                .count(),
            1,
            "only view()'s own clear of the field area"
        );

        // Editing: the value cell is cleared out of the band and drawn plain.
        c.set_editing(true);
        let mut t1 = RecordingTarget::new(120, 10);
        c.view(&mut t1, area);
        assert!(
            texts(&t1)
                .iter()
                .any(|(_, _, s, st)| s == "60" && *st == Style::Normal)
        );
        // "60" is 12px wide, right-aligned → the cut-out is that cell exactly.
        assert!(
            t1.ops().contains(&Op::Clear {
                area: Area::new(108, 0, 12, 10)
            }),
            "the value cell must be cleared out of the band"
        );
    }

    /// The slider's band stops at the bar, which keeps its own ground:
    /// `draw_bar` is ink on monochrome (it ignores the style on purpose), so an
    /// inverted band under it would leave the bar `On` on `On` - invisible.
    #[test]
    fn slider_band_stops_at_the_bar() {
        let area = Area::new(0, 0, 120, 10);
        let mut s = Slider::new("Vol").with_range(0, 100).with_value(50);
        s.focus();
        let mut t = RecordingTarget::new(120, 10);
        s.view(&mut t, area);

        // label_w = 48 → the band is x = 0..48, the bar cell x = 48..120.
        assert_eq!(bands(&t), [(Area::new(0, 0, 48, 10), Style::Focus)]);
        assert_eq!(bars(&t), [(Area::new(48, 0, 72, 10), 500, Style::Accent)]);
    }

    // ── Checkbox ──────────────────────────────────────────────────────────────

    #[test]
    fn checkbox_draws_indicator_and_label() {
        let mut t = RecordingTarget::new(120, 10);
        Checkbox::new("WiFi").view(&mut t, Area::new(0, 0, 120, 10));
        // Symbolic default of draw_check records "[ ]" at the area origin…
        assert!(texts(&t).contains(&(0, 0, "[ ]".into(), Style::Normal)));
        // …and the label starts 4 chars in (24px).
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 24 && s == "WiFi"));
    }

    #[test]
    fn checkbox_select_checks() {
        let mut c = Checkbox::new("WiFi");
        c.update(&Msg::Select);
        assert!(c.is_checked());
        let mut t = RecordingTarget::new(120, 10);
        c.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(has_text(&t, "[x]"));
    }

    #[test]
    fn checkbox_focus_styles_focus() {
        let mut c = Checkbox::new("X");
        c.focus();
        let mut t = RecordingTarget::new(120, 10);
        c.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "[ ]" && *st == Style::Focus)
        );
    }

    #[test]
    fn checkbox_set_toggle_methods() {
        let mut c = Checkbox::new("WiFi");
        c.set_checked(true);
        assert!(c.is_checked());
        c.toggle();
        assert!(!c.is_checked());
    }

    // ── Toggle ────────────────────────────────────────────────────────────────

    #[test]
    fn toggle_draws_label_and_indicator() {
        let mut t = RecordingTarget::new(120, 10);
        Toggle::new("Sound").view(&mut t, Area::new(0, 0, 120, 10));
        assert!(has_text(&t, "Sound"));
        assert!(has_text(&t, "[ ]")); // off
        // Indicator right-aligned: 3 chars = 18px → x = 120 - 18 = 102.
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 102 && s == "[ ]"));
    }

    #[test]
    fn toggle_select_turns_on() {
        let mut tg = Toggle::new("Sound");
        tg.update(&Msg::Select);
        assert!(tg.is_on());
        let mut t = RecordingTarget::new(120, 10);
        tg.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(has_text(&t, "[x]"));
    }

    #[test]
    fn toggle_left_right() {
        let mut tg = Toggle::new("Sound");
        tg.update(&Msg::Right);
        assert!(tg.is_on());
        tg.update(&Msg::Left);
        assert!(!tg.is_on());
    }

    // ── Button ────────────────────────────────────────────────────────────────

    #[test]
    fn button_draws_label() {
        let mut t = RecordingTarget::new(120, 10);
        Button::new("< Back").view(&mut t, Area::new(0, 0, 120, 10));
        assert!(has_text(&t, "< Back"));
    }

    #[test]
    fn button_select_sets_and_take_pressed_clears() {
        let mut b = Button::new("Go");
        assert!(!b.take_pressed());
        b.update(&Msg::Select);
        assert!(b.take_pressed());
        // Consumed - a second call without another Select reports false.
        assert!(!b.take_pressed());
    }

    #[test]
    fn button_focus_styles_focus() {
        let mut b = Button::new("Go");
        b.focus();
        let mut t = RecordingTarget::new(120, 10);
        b.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "Go" && *st == Style::Focus)
        );
    }

    /// A press draws nothing - `pressed` is a latch the app polls, not a state
    /// the button renders - so it must not dirty the gate either.
    #[test]
    fn button_press_does_not_dirty() {
        let mut b = Button::new("Go");
        b.mark_clean();
        b.update(&Msg::Up); // not Select → no change
        assert!(!b.dirty());
        b.update(&Msg::Select);
        assert!(b.take_pressed(), "the press is still latched");
        assert!(!b.dirty(), "a press changes no pixel");

        // Focus does change the picture (the band), so that still dirties.
        b.focus();
        assert!(b.dirty());
    }

    // ── Clamping ──────────────────────────────────────────────────────────────

    /// Values are clamped into the range wherever they enter, so `Up` and `Down`
    /// are symmetric. Before, `with_value` let a value sit below `min` and only
    /// `Down` could reach the bounds - `Up` walked up from wherever it was.
    #[test]
    fn counter_clamps_at_every_entry_point() {
        // Below the floor at construction → pulled up to `min`.
        let c = Counter::new("X").with_range(10, 20).with_value(0);
        assert_eq!(c.value(), 10);
        // …and one step up lands inside the range, not at 1.
        let mut c = c;
        c.update(&Msg::Up);
        assert_eq!(c.value(), 11);

        // Above the ceiling, either builder order.
        assert_eq!(Counter::new("X").with_range(0, 5).with_value(9).value(), 5);
        assert_eq!(Counter::new("X").with_value(9).with_range(0, 5).value(), 5);

        // set_value agrees.
        let mut c = Counter::new("X").with_range(0, 5);
        c.set_value(-3);
        assert_eq!(c.value(), 0);
        c.set_value(99);
        assert_eq!(c.value(), 5);
    }

    #[test]
    fn slider_clamps_at_every_entry_point() {
        let s = Slider::new("X").with_range(10, 20).with_value(0);
        assert_eq!(s.value(), 10);
        let mut s = s;
        s.update(&Msg::Down);
        assert_eq!(s.value(), 10, "already at the floor");
        s.update(&Msg::Up);
        assert_eq!(s.value(), 20, "step 10 from the floor");

        assert_eq!(Slider::new("X").with_value(99).with_range(0, 5).value(), 5);
    }

    // ── Counter ───────────────────────────────────────────────────────────────

    #[test]
    fn counter_value_right_aligned() {
        let mut t = RecordingTarget::new(120, 10);
        Counter::new("Vol").view(&mut t, Area::new(0, 0, 120, 10));
        assert!(texts(&t).iter().any(|(x, _, s, _)| s == "Vol" && *x == 0));
        // "0" is 1 char (6px) → right-aligned at x = 120 - 6 = 114.
        assert!(texts(&t).iter().any(|(x, _, s, _)| s == "0" && *x == 114));
    }

    #[test]
    fn counter_increment_clamps() {
        let mut c = Counter::new("X").with_range(0, 3).with_step(2);
        c.update(&Msg::Up);
        assert_eq!(c.value(), 2);
        c.update(&Msg::Up);
        assert_eq!(c.value(), 3); // 4 clamped
    }

    #[test]
    fn counter_editing_reads_apart_from_focused() {
        // Focused but not editing: label and value are both inside the band.
        let mut c = Counter::new("V");
        c.focus();
        let mut t = RecordingTarget::new(120, 10);
        c.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "V" && *st == Style::Focus)
        );
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "0" && *st == Style::Focus)
        );

        // Editing: the value cuts back out of the band, so an idle focused row
        // never looks like it is being changed.
        c.set_editing(true);
        let mut t2 = RecordingTarget::new(120, 10);
        c.view(&mut t2, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t2)
                .iter()
                .any(|(_, _, s, st)| s == "0" && *st == Style::Normal)
        );
        assert!(
            texts(&t2)
                .iter()
                .any(|(_, _, s, st)| s == "V" && *st == Style::Focus)
        );
    }

    #[test]
    fn counter_dirty_only_on_real_change() {
        let mut c = Counter::new("X").with_range(0, 3).with_step(2);
        assert!(c.dirty()); // starts dirty
        c.mark_clean();

        // No-op messages leave it clean.
        c.update(&Msg::Select);
        c.update(&Msg::Tick);
        assert!(!c.dirty());

        // A real increment dirties it.
        c.update(&Msg::Up);
        assert!(c.dirty());
        c.mark_clean();

        // At the clamp ceiling (3), another Up changes nothing → stays clean.
        c.update(&Msg::Up); // 2 -> 3
        c.mark_clean();
        c.update(&Msg::Up); // clamped at 3, no change
        assert!(!c.dirty());
    }

    #[test]
    fn checkbox_dirty_contract() {
        let mut c = Checkbox::new("WiFi");
        c.mark_clean();
        c.update(&Msg::Up); // not a toggle → no change
        assert!(!c.dirty());
        c.update(&Msg::Select); // toggles → dirty
        assert!(c.dirty());
    }

    // ── Slider ────────────────────────────────────────────────────────────────

    fn bars(t: &RecordingTarget) -> Vec<(Area, u16, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Bar {
                    area,
                    fill_permille,
                    style,
                } => Some((*area, *fill_permille, *style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn slider_draws_bar_no_brackets() {
        let mut t = RecordingTarget::new(120, 10);
        Slider::new("Vol")
            .with_range(0, 100)
            .with_value(50)
            .view(&mut t, Area::new(0, 0, 120, 10));
        // A bar at 50% - and no "[", "#", "-" characters anywhere.
        let b = bars(&t);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].1, 500);
        assert!(!has_text(&t, "[") && !has_text(&t, "#") && !has_text(&t, "-"));
    }

    #[test]
    fn slider_edit_draws_frame_on_mono() {
        let mut s = Slider::new("Vol").with_range(0, 100).with_value(50);
        s.set_editing(true);
        let mut t = RecordingTarget::new(120, 10);
        s.view(&mut t, Area::new(0, 0, 120, 10));
        // Edit cue: a Single box around the bar (visible on a 1-bit panel)…
        assert!(t.ops().iter().any(|op| matches!(
            op,
            Op::Box {
                border: BorderStyle::Single,
                ..
            }
        )));
        // …and the fill is tinted Focus.
        assert!(bars(&t).iter().any(|(_, _, st)| *st == Style::Focus));
    }

    #[test]
    fn slider_not_editing_no_frame() {
        let mut t = RecordingTarget::new(120, 10);
        Slider::new("Vol")
            .with_value(50)
            .view(&mut t, Area::new(0, 0, 120, 10));
        assert!(!t.ops().iter().any(|op| matches!(op, Op::Box { .. })));
        assert!(bars(&t).iter().all(|(_, _, st)| *st == Style::Accent));
    }

    #[test]
    fn slider_increment_clamps() {
        let mut s = Slider::new("X")
            .with_range(0, 100)
            .with_step(10)
            .with_value(95);
        s.update(&Msg::Up);
        assert_eq!(s.value(), 100);
        s.update(&Msg::Up);
        assert_eq!(s.value(), 100);
    }

    #[test]
    fn slider_set_editing_toggles_flag() {
        let mut s = Slider::new("X");
        assert!(!s.editing);
        s.set_editing(true);
        assert!(s.editing);
    }

    // ── Picker ────────────────────────────────────────────────────────────────

    const OPTS: &[&str] = &["Alpha", "Beta", "Gamma"];

    #[test]
    fn picker_renders_current_right_aligned() {
        let mut t = RecordingTarget::new(120, 10);
        Picker::new("Mode", OPTS).view(&mut t, Area::new(0, 0, 120, 10));
        assert!(texts(&t).iter().any(|(x, _, s, _)| s == "Mode" && *x == 0));
        // "Alpha" = 5 chars = 30px → right-aligned at x = 90.
        assert!(
            texts(&t)
                .iter()
                .any(|(x, _, s, _)| s == "Alpha" && *x == 90)
        );
    }

    #[test]
    fn picker_next_and_wrap() {
        let mut p = Picker::new("M", OPTS);
        p.update(&Msg::Down);
        assert_eq!(p.selected_option(), Some(&"Beta"));
        let mut p2 = Picker::new("M", OPTS);
        p2.update(&Msg::Up);
        assert_eq!(p2.selected(), 2); // wrapped
    }

    #[test]
    fn picker_editing_cuts_the_option_out_of_the_band() {
        let mut p = Picker::new("M", OPTS);
        p.focus();
        p.set_editing(true);
        let mut t = RecordingTarget::new(120, 10);
        p.view(&mut t, Area::new(0, 0, 120, 10));
        // Label inside the band, option cut out of it and drawn plain.
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "M" && *st == Style::Focus)
        );
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "Alpha" && *st == Style::Normal)
        );
    }

    #[test]
    fn picker_empty_safe() {
        let mut p: Picker<'_, &str> = Picker::new("M", &[]);
        p.update(&Msg::Down);
        assert_eq!(p.selected_option(), Option::<&&str>::None);
    }
}
