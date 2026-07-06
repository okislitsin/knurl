use core::cell::Cell;

use crate::{Area, BorderStyle, Component, Msg, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── Dialog ────────────────────────────────────────────────────────────────────

/// A modal dialog: a rounded pixel box with an ASCII title, a message, and a row
/// of selectable buttons. The focused button is drawn `Style::Focus` inside a
/// thin outline box; `Select` confirms it. Encoder-navigated (Up/Down/Left/Right).
#[derive(Debug)]
pub struct Dialog<'a> {
    title: &'a str,
    message: &'a str,
    buttons: &'a [&'a str],
    selected: usize,
    border: BorderStyle,
    confirmed: bool,
    // Repaint gate: set on button-selection change. A modal also needs a full
    // repaint when it opens (it draws over screen content) - the app calls
    // [`mark_dirty`](Component::mark_dirty) then.
    dirty: Cell<bool>,
}

impl<'a> Dialog<'a> {
    pub fn new(title: &'a str, message: &'a str, buttons: &'a [&'a str]) -> Self {
        Self {
            title,
            message,
            buttons,
            selected: 0,
            border: BorderStyle::Rounded,
            confirmed: false,
            dirty: Cell::new(true),
        }
    }

    pub const fn with_border(mut self, b: BorderStyle) -> Self {
        self.border = b;
        self
    }

    /// Sets the selected button, clamped into `[0, len - 1]` (no-op with no buttons).
    pub fn with_selected(mut self, idx: usize) -> Self {
        let n = self.buttons.len();
        if n > 0 {
            self.selected = idx.min(n - 1);
        }
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Text of the highlighted button, or `""` when there are no buttons.
    pub fn selected_button(&self) -> &'a str {
        self.buttons.get(self.selected).copied().unwrap_or("")
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Clears the confirmed flag.
    pub fn reset(&mut self) {
        self.confirmed = false;
    }
}

impl<'a> Component for Dialog<'a> {
    fn update(&mut self, msg: &Msg) {
        let n = self.buttons.len();
        match msg {
            Msg::Up | Msg::Left if self.selected > 0 => {
                self.selected -= 1;
                self.dirty.set(true);
            }
            Msg::Down | Msg::Right if n > 0 && self.selected + 1 < n => {
                self.selected += 1;
                self.dirty.set(true);
            }
            // Confirm changes no on-screen pixels of the dialog itself; the app
            // reads is_confirmed() and typically closes the modal.
            Msg::Select => self.confirmed = true,
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        target.draw_box(area, self.border);
        let Some(inner) = area.inner_by(self.border.thickness()) else {
            return;
        };
        let cw = target.char_width().max(1);
        let line_h = target.line_height().max(1);
        let max_chars = (inner.w / cw) as usize;

        // Title (Accent) on the first inner row.
        target.draw_text(inner.x, inner.y, truncate(self.title, max_chars), Style::Accent);

        // Message (Normal) on the second row, if there's room.
        if inner.h >= 2 * line_h {
            target.draw_text(
                inner.x,
                inner.y + line_h,
                truncate(self.message, max_chars),
                Style::Normal,
            );
        }

        // Buttons on the last inner row, laid out horizontally; the focused one is
        // boxed and drawn Focus.
        let by = inner.y + inner.h - line_h;
        let right = inner.x + inner.w;
        let mut x = inner.x;
        for (i, b) in self.buttons.iter().enumerate() {
            if x >= right {
                break;
            }
            let label = truncate(b, ((right - x) / cw) as usize);
            let cell_w = target.text_width(label) + 2 * cw;
            let focused = i == self.selected;
            if focused {
                target.draw_box(Area::new(x, by, cell_w.min(right - x), line_h), BorderStyle::Single);
            }
            let style = if focused { Style::Focus } else { Style::Normal };
            target.draw_text(x + cw, by, label, style);
            x = x.saturating_add(cell_w).saturating_add(cw);
        }
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use alloc::vec::Vec;

    const BTNS: &[&str] = &["Yes", "No"];

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.
    // Rounded border thickness = 1 → inner = (1, 1, w-2, h-2).

    fn texts(t: &RecordingTarget) -> Vec<(u16, u16, alloc::string::String, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.clone(), *style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn dialog_renders_box_title_message_buttons() {
        let d = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t = RecordingTarget::new(120, 50);
        d.view(&mut t, Area::new(0, 0, 120, 50));
        // Rounded modal box over the whole area.
        assert!(t.ops().contains(&Op::Box {
            area: Area::new(0, 0, 120, 50),
            border: BorderStyle::Rounded,
        }));
        let tx = texts(&t);
        // Title at inner origin (1,1) Accent; message a line below (y=11) Normal.
        assert!(tx.contains(&(1, 1, "Confirm".into(), Style::Accent)));
        assert!(tx.contains(&(1, 11, "Sure?".into(), Style::Normal)));
        // Buttons on the last inner row (by = 1 + 48 - 10 = 39). "Yes" focused.
        assert!(tx.iter().any(|(_, y, s, st)| *y == 39 && s == "Yes" && *st == Style::Focus));
        assert!(tx.iter().any(|(_, y, s, st)| *y == 39 && s == "No" && *st == Style::Normal));
        // The focused button is boxed (a Single box on the button row).
        assert!(t.ops().iter().any(|op| matches!(op, Op::Box { area, border: BorderStyle::Single } if area.y == 39)));
    }

    #[test]
    fn dialog_navigate_and_confirm() {
        let mut d = Dialog::new("t", "m", BTNS);
        d.update(&Msg::Down);
        assert_eq!(d.selected(), 1);
        assert_eq!(d.selected_button(), "No");
        d.update(&Msg::Up);
        assert_eq!(d.selected(), 0);
        d.update(&Msg::Select);
        assert!(d.is_confirmed());
        d.reset();
        assert!(!d.is_confirmed());
    }

    #[test]
    fn dialog_clamp() {
        let mut d = Dialog::new("t", "m", BTNS);
        d.update(&Msg::Up);
        assert_eq!(d.selected(), 0);
        let mut d2 = Dialog::new("t", "m", BTNS).with_selected(1);
        d2.update(&Msg::Down);
        assert_eq!(d2.selected(), 1);
    }
}
