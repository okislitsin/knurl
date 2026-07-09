use core::cell::Cell;

use crate::{Area, BorderStyle, Component, FormField, Msg, RenderTarget, Style};

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
fn draw_labeled_value(
    target: &mut dyn RenderTarget,
    area: Area,
    label: &str,
    label_style: Style,
    value: &str,
    value_style: Style,
) {
    let cw = target.char_width().max(1);
    let value_px = target.text_width(value);

    let label_avail = area.w.saturating_sub(value_px.saturating_add(cw));
    let label_max = (label_avail / cw) as usize;
    if label_max > 0 {
        target.draw_text(area.x, area.y, truncate(label, label_max), label_style);
    }

    if value_px <= area.w {
        let vx = area.x + area.w - value_px;
        target.draw_text(vx, area.y, value, value_style);
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
        let style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };
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
        let style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };

        // Indicator right-aligned (3-char slot); label fills the rest.
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
            self.pressed = true;
            self.dirty.set(true);
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };
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

    /// Sets the value verbatim - **not** clamped, since `min`/`max` may be set
    /// later by [`with_range`](Counter::with_range). Clamping is enforced by
    /// [`update`](Counter::update) and [`set_value`](Counter::set_value).
    pub const fn with_value(mut self, v: i32) -> Self {
        self.value = v;
        self
    }

    pub const fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min = min;
        self.max = max;
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
        let clamped = v.clamp(self.min, self.max);
        if clamped != self.value {
            self.value = clamped;
            self.dirty.set(true);
        }
    }
}

impl<'a> Component for Counter<'a> {
    fn update(&mut self, msg: &Msg) {
        let before = self.value;
        match msg {
            Msg::Up | Msg::Right => {
                self.value = self.value.saturating_add(self.step).min(self.max);
            }
            Msg::Down | Msg::Left => {
                self.value = self.value.saturating_sub(self.step).max(self.min);
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
        // Focus highlights the label; the value is highlighted only while it is
        // actually being edited (Form edit mode), so a focused-but-idle row does
        // not look like it is being changed.
        let label_style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };
        let value_style = if self.editing {
            Style::Focus
        } else {
            Style::Normal
        };
        draw_labeled_value(target, area, self.label, label_style, value, value_style);
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

    pub const fn with_range(mut self, min: i32, max: i32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    pub const fn with_step(mut self, step: i32) -> Self {
        self.step = step;
        self
    }

    /// Sets the value verbatim - **not** clamped, since `min`/`max` may be set
    /// later by [`with_range`](Slider::with_range). Clamping is enforced by
    /// [`update`](Slider::update) and [`set_value`](Slider::set_value).
    pub const fn with_value(mut self, v: i32) -> Self {
        self.value = v;
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
        let clamped = v.clamp(self.min, self.max);
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
        match msg {
            Msg::Up | Msg::Right => {
                self.value = self.value.saturating_add(self.step).min(self.max);
            }
            Msg::Down | Msg::Left => {
                self.value = self.value.saturating_sub(self.step).max(self.min);
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
        let label_style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };
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
        let label_style = if self.focused {
            Style::Focus
        } else {
            Style::Normal
        };
        let opt = match self.selected_option() {
            Some(opt) => opt.as_str(),
            None => "",
        };
        // The option highlights only while editing - mirrors Counter.
        let opt_style = if self.editing {
            Style::Focus
        } else {
            Style::Normal
        };
        draw_labeled_value(target, area, self.label, label_style, opt, opt_style);
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

    #[test]
    fn button_dirty_contract() {
        let mut b = Button::new("Go");
        b.mark_clean();
        b.update(&Msg::Up); // not Select → no change
        assert!(!b.dirty());
        b.update(&Msg::Select);
        assert!(b.dirty());
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
    fn counter_value_focus_only_when_editing() {
        // Focused but not editing: value is Normal, label is Focus.
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
                .any(|(_, _, s, st)| s == "0" && *st == Style::Normal)
        );

        // Editing: value flips to Focus.
        c.set_editing(true);
        let mut t2 = RecordingTarget::new(120, 10);
        c.view(&mut t2, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t2)
                .iter()
                .any(|(_, _, s, st)| s == "0" && *st == Style::Focus)
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
    fn picker_option_focus_only_when_editing() {
        let mut p = Picker::new("M", OPTS);
        p.set_editing(true);
        let mut t = RecordingTarget::new(120, 10);
        p.view(&mut t, Area::new(0, 0, 120, 10));
        assert!(
            texts(&t)
                .iter()
                .any(|(_, _, s, st)| s == "Alpha" && *st == Style::Focus)
        );
    }

    #[test]
    fn picker_empty_safe() {
        let mut p: Picker<'_, &str> = Picker::new("M", &[]);
        p.update(&Msg::Down);
        assert_eq!(p.selected_option(), Option::<&&str>::None);
    }
}
