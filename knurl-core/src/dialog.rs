use core::cell::Cell;

use crate::{Area, BorderStyle, Component, Msg, Outcome, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── Dialog ────────────────────────────────────────────────────────────────────

/// A modal dialog: a rounded pixel box with an ASCII title, a message, and a row
/// of selectable buttons. The focused button is drawn `Style::Focus` inside a
/// thin outline box; `Select` confirms it. Encoder-navigated (Up/Down/Left/Right).
///
/// A button row too wide for the box ends in a dimmed `>`: the buttons past it
/// are still selectable, just not on screen (see
/// [`selected_button`](Dialog::selected_button)).
///
/// A dialog **without buttons** is a notice rather than a question: it has
/// nothing to confirm, so `Select` comes straight back
/// [`Ignored`](Outcome::Ignored) and the screen that raised it is free to spend
/// the press on dismissing it.
#[derive(Debug)]
pub struct Dialog<'a> {
    title: &'a str,
    message: &'a str,
    buttons: &'a [&'a str],
    selected: usize,
    border: BorderStyle,
    /// Armed by `Select`, cleared by [`take_confirmed`](Dialog::take_confirmed) -
    /// the modal's answer to "was it *this* that the user activated?".
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
    ///
    /// This is the **logical** value - the label as the caller passed it in -
    /// not what the screen shows. In a narrow box a label is truncated to fit,
    /// and buttons past the row's width are not drawn at all (the row ends in a
    /// dimmed `>` instead); the selection still moves through every one of them,
    /// and this still returns the full label. Callers match on this, so it must
    /// stay the value they gave, whatever the layout could fit.
    pub fn selected_button(&self) -> &'a str {
        self.buttons.get(self.selected).copied().unwrap_or("")
    }

    /// Whether the dialog was confirmed since the last call, clearing the flag
    /// so one press closes the modal exactly once.
    ///
    /// The screen asks the dialog, the same way it asks a
    /// [`Button`](crate::Button::take_pressed): the [`Outcome`] says an event
    /// was spent, this says the modal is the thing that spent it. Pair it with
    /// [`selected_button`](Dialog::selected_button) for *which* answer.
    pub fn take_confirmed(&mut self) -> bool {
        core::mem::take(&mut self.confirmed)
    }
}

impl<'a> Component for Dialog<'a> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        let n = self.buttons.len();
        match msg {
            Msg::Up | Msg::Left if self.selected > 0 => {
                self.selected -= 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            Msg::Down | Msg::Right if n > 0 && self.selected + 1 < n => {
                self.selected += 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            // Confirm changes no on-screen pixels of the dialog itself - so no
            // dirty - but it is precisely the moment the app acts on: it reads
            // `take_confirmed()` / `selected_button()` and closes the modal.
            //
            // A dialog with no buttons has nothing to confirm: it is a notice,
            // not a question, and `selected_button()` would answer `""`. The
            // press is reported unspent so whoever put the notice on screen can
            // use it to dismiss the thing.
            Msg::Select if n > 0 => {
                self.confirmed = true;
                Outcome::Activated
            }
            // Either end of the button row, or a message that is not ours.
            _ => Outcome::Ignored,
        }
    }

    /// Draws the box, then title / message / button row - each row only if the
    /// inner box is tall enough for it:
    ///
    /// | Inner height         | Drawn                                 |
    /// |----------------------|---------------------------------------|
    /// | `< line_height`      | box + (clipped) title                 |
    /// | `< 2 * line_height`  | box + title                           |
    /// | `< 3 * line_height`  | box + title + buttons                 |
    /// | `>= 3 * line_height` | box + title + message + buttons       |
    ///
    /// Every row has its own threshold because they occupy fixed positions -
    /// title first, message second, buttons last - and a shorter box makes them
    /// collide. Under two rows the last row *is* the title row, so the title
    /// wins; under three the button row *is* the message row, and the buttons
    /// win there because they are what the user acts on.
    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        target.draw_box(area, self.border);
        let Some(inner) = area.inner_by(self.border.thickness()) else {
            return;
        };
        let cw = target.char_width().max(1);
        let line_h = target.line_height().max(1);
        let max_chars = (inner.w / cw) as usize;

        // Title (Accent) on the first inner row.
        target.draw_text(
            inner.x,
            inner.y,
            truncate(self.title, max_chars),
            Style::Accent,
        );

        // Message (Normal) on the second row, if there's room for it *and* the
        // button row below it (see the note on `draw`).
        if inner.h >= 3 * line_h {
            target.draw_text(
                inner.x,
                inner.y + line_h,
                truncate(self.message, max_chars),
                Style::Normal,
            );
        }

        // Buttons on the last inner row, laid out horizontally; the focused one is
        // boxed and drawn Focus. Skipped when the inner box is under two rows tall
        // (see the note on `draw`): the last row would land on the title.
        if inner.h < 2 * line_h {
            return;
        }
        let by = inner.y + inner.h - line_h;
        let right = inner.x + inner.w;
        let mut x = inner.x;
        let mut drawn = 0usize;
        for (i, b) in self.buttons.iter().enumerate() {
            if x >= right {
                break;
            }
            // The cell is `cw` of padding, the label, then `cw` of padding, so
            // the label budget is what remains after *both* - a budget measured
            // against the bare `right - x` would push the text one cell past
            // `inner`. Nothing left for a character means nothing left for a
            // readable button, so the row ends here rather than drawing an empty
            // frame.
            let budget = ((right - x).saturating_sub(2 * cw) / cw) as usize;
            if budget == 0 {
                break;
            }
            let label = truncate(b, budget);
            let cell_w = target.text_width(label) + 2 * cw;
            let focused = i == self.selected;
            if focused {
                target.draw_box(
                    Area::new(x, by, cell_w.min(right - x), line_h),
                    BorderStyle::Single,
                );
            }
            let style = if focused { Style::Focus } else { Style::Normal };
            target.draw_text(x + cw, by, label, style);
            x = x.saturating_add(cell_w).saturating_add(cw);
            drawn = i + 1;
        }

        // Buttons the row could not hold are still *reachable* - `Up`/`Down`
        // move the selection through all of them - so dropping them in silence
        // reads as "there are only these". A dimmed `>` in the row's last cell
        // says there are more, in the one cell the layout guarantees is free:
        // a label's budget comes off both its padding cells, so drawn text
        // always ends at or before `right - cw`.
        if drawn < self.buttons.len() && right >= inner.x + cw {
            target.draw_text(right - cw, by, ">", Style::Muted);
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
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 39 && s == "Yes" && *st == Style::Focus)
        );
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 39 && s == "No" && *st == Style::Normal)
        );
        // The focused button is boxed (a Single box on the button row).
        assert!(
            t.ops().iter().any(
                |op| matches!(op, Op::Box { area, border: BorderStyle::Single } if area.y == 39)
            )
        );
    }

    /// A box too short for a button row must not panic (a u16 underflow on
    /// `inner.h - line_h`) nor place a row below the area. Only the vertical
    /// axis is asserted: the button layout can still overshoot horizontally in a
    /// box narrower than one character cell, which is a separate matter.
    #[test]
    fn dialog_short_area_draws_nothing_outside_and_never_panics() {
        for h in [0u16, 1, 2, 3, 9, 10, 11, 12, 21, 22] {
            for w in [0u16, 1, 3, 6, 40] {
                let d = Dialog::new("Confirm", "Sure?", BTNS);
                let mut t = RecordingTarget::new(128, 64);
                let area = Area::new(0, 0, w, h);
                d.view(&mut t, area);
                if w == 0 || h == 0 {
                    continue; // nothing to contain
                }
                for op in t.ops() {
                    assert!(
                        op.top_y() < area.y + area.h,
                        "op {op:?} starts below {area:?}"
                    );
                }
            }
        }
    }

    /// Under two rows the button row would land on the title, so the title wins
    /// and no buttons are drawn.
    #[test]
    fn dialog_omits_buttons_when_under_two_rows() {
        // Rounded border → inner height = h - 2; line_height = 10.
        let d = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t = RecordingTarget::new(128, 64);
        d.view(&mut t, Area::new(0, 0, 120, 21)); // inner.h = 19 < 20
        let tx = texts(&t);
        assert!(tx.iter().any(|(_, y, s, _)| *y == 1 && s == "Confirm"));
        assert!(!tx.iter().any(|(_, _, s, _)| s == "Yes" || s == "No"));

        // One row taller (inner.h = 20) the buttons fit on the last row.
        let d2 = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t2 = RecordingTarget::new(128, 64);
        d2.view(&mut t2, Area::new(0, 0, 120, 22));
        assert!(texts(&t2).iter().any(|(_, y, s, _)| *y == 11 && s == "Yes"));
    }

    /// At exactly two rows the message row *is* the button row, so the message
    /// stands down; from three rows up both are drawn, on rows of their own.
    #[test]
    fn dialog_message_yields_to_the_button_row_at_two_rows() {
        // inner.h = 20 = 2 rows: title y=1, buttons y=11, no message.
        let d = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t = RecordingTarget::new(128, 64);
        d.view(&mut t, Area::new(0, 0, 120, 22));
        let tx = texts(&t);
        assert!(
            !tx.iter().any(|(_, _, s, _)| s == "Sure?"),
            "the message shares the button row"
        );
        assert!(tx.iter().any(|(_, y, s, _)| *y == 11 && s == "Yes"));

        // inner.h = 30 = 3 rows: title y=1, message y=11, buttons y=21.
        let d2 = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t2 = RecordingTarget::new(128, 64);
        d2.view(&mut t2, Area::new(0, 0, 120, 32));
        let tx2 = texts(&t2);
        assert!(tx2.iter().any(|(_, y, s, _)| *y == 11 && s == "Sure?"));
        assert!(tx2.iter().any(|(_, y, s, _)| *y == 21 && s == "Yes"));
    }

    /// A button label is laid out as `cw` padding + text + `cw` padding, so its
    /// budget must come off both - otherwise the text runs one cell past the
    /// inner box. Nothing may be drawn beyond `inner`'s right edge.
    #[test]
    fn dialog_button_text_stays_inside_the_inner_box() {
        const CW: u16 = 6; // RecordingTarget char_width
        // Long labels matter: they get truncated to the budget, so a budget that
        // ignores the padding shows up as text running past the edge.
        const LONG: &[&str] = &["Cancel", "Retry"];
        for btns in [BTNS, LONG] {
            for w in [4u16, 8, 12, 14, 20, 26, 30, 32, 40, 64, 120] {
                let d = Dialog::new("Confirm", "Sure?", btns);
                let mut t = RecordingTarget::new(128, 64);
                d.view(&mut t, Area::new(0, 0, w, 40));
                // Rounded border, thickness 1 → inner spans x = 1 ..= w - 2.
                let right = w - 1;
                for (x, _, s, _) in texts(&t) {
                    let end = x + CW * s.chars().count() as u16;
                    assert!(
                        end <= right,
                        "text {s:?} ends at {end} past inner right {right} (w = {w})"
                    );
                }
            }
        }
    }

    /// A box too narrow for every button used to drop the rest in silence. The
    /// row now ends in a `>` so the user knows there is more to turn to.
    #[test]
    fn dialog_marks_buttons_that_did_not_fit() {
        const CW: u16 = 6;
        // 40px box → inner 1..=38: "Yes" fits, "No" and "Maybe" do not.
        let btns: &[&str] = &["Yes", "No", "Maybe"];
        let d = Dialog::new("Confirm", "Sure?", btns);
        let mut t = RecordingTarget::new(128, 64);
        d.view(&mut t, Area::new(0, 0, 40, 40));

        let tx = texts(&t);
        let by = 1 + 38 - 10; // last inner row
        assert!(tx.iter().any(|(_, y, s, _)| *y == by && s == "Yes"));
        assert!(!tx.iter().any(|(_, _, s, _)| s == "No" || s == "Maybe"));
        // The overflow mark sits in the last cell of the row, dimmed.
        assert!(
            tx.iter()
                .any(|(x, y, s, st)| *y == by && s == ">" && *st == Style::Muted && *x == 39 - CW),
            "no overflow mark: {tx:?}"
        );
    }

    /// When every button fits there is nothing to announce.
    #[test]
    fn dialog_no_overflow_mark_when_all_buttons_fit() {
        let d = Dialog::new("Confirm", "Sure?", BTNS);
        let mut t = RecordingTarget::new(128, 64);
        d.view(&mut t, Area::new(0, 0, 120, 40));
        assert!(!texts(&t).iter().any(|(_, _, s, _)| s == ">"));
    }

    #[test]
    fn dialog_navigate_and_confirm() {
        let mut d = Dialog::new("t", "m", BTNS);
        let _ = d.update(&Msg::Down);
        assert_eq!(d.selected(), 1);
        assert_eq!(d.selected_button(), "No");
        let _ = d.update(&Msg::Up);
        assert_eq!(d.selected(), 0);
        assert_eq!(d.update(&Msg::Select), Outcome::Activated);
        assert_eq!(d.selected_button(), "Yes", "and it says which button");
        assert!(d.take_confirmed(), "and that it was the dialog that did it");
        assert!(!d.take_confirmed(), "the read consumed the confirmation");
    }

    #[test]
    fn dialog_clamp() {
        let mut d = Dialog::new("t", "m", BTNS);
        let _ = d.update(&Msg::Up);
        assert_eq!(d.selected(), 0);
        let mut d2 = Dialog::new("t", "m", BTNS).with_selected(1);
        let _ = d2.update(&Msg::Down);
        assert_eq!(d2.selected(), 1);
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    #[test]
    fn dialog_spends_a_step_hands_back_an_edge_and_activates_on_select() {
        let mut d = Dialog::new("t", "m", BTNS);
        assert_eq!(
            d.update(&Msg::Up),
            Outcome::Ignored,
            "Up on the first button"
        );
        assert_eq!(
            d.update(&Msg::Down),
            Outcome::Consumed,
            "moved to the next button"
        );
        assert_eq!(
            d.update(&Msg::Down),
            Outcome::Ignored,
            "Down on the last button"
        );
        // Confirming repaints nothing, and is still the app's cue - the one
        // case that proves Outcome is not the dirty flag under another name.
        assert_eq!(d.update(&Msg::Select), Outcome::Activated);
        assert_eq!(d.selected_button(), "No");
    }

    /// A dialog with no buttons is a notice: there is nothing to confirm, so
    /// the press must come back unspent instead of reporting a choice nobody
    /// made (`selected_button()` would answer `""`).
    #[test]
    fn a_dialog_with_no_buttons_confirms_nothing() {
        let mut d = Dialog::new("Saved", "Settings written", &[]);
        assert_eq!(d.update(&Msg::Select), Outcome::Ignored);
        assert!(!d.take_confirmed(), "nothing was confirmed");
        assert_eq!(d.selected_button(), "");
        // …and it is still a dialog: rotation has nowhere to go either.
        assert_eq!(d.update(&Msg::Down), Outcome::Ignored);
        assert_eq!(d.update(&Msg::Up), Outcome::Ignored);
    }
}
