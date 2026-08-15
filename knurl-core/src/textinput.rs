use core::cell::Cell;

use crate::{Area, Component, FormField, Msg, Outcome, RenderTarget, Style, draw_cursor_band};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── TextInput ─────────────────────────────────────────────────────────────────

/// A 3-button text entry widget driven by a scrolling "token ribbon".
///
/// The token space is the `charset` (printable **ASCII**, one byte per char)
/// followed by two implicit tokens: backspace and done. `Up`/`Down` scroll the
/// candidate token (wrapping); `Select` applies it - a printable char appends to
/// the buffer, backspace erases the last byte, done sets the [`is_done`] flag.
///
/// Pixel-rendered with ASCII glyphs only (space `_`, backspace `<`, done `#`).
/// The buffer is a fixed `[u8; N]` (each charset char is one ASCII byte); all
/// state is stack-allocated. As a [`FormField`] it captures `Select` while
/// editing, so it composes inside a [`Form`](crate::Form): Select enters edit,
/// the ribbon picks tokens, "done" exits.
///
/// [`is_done`]: TextInput::is_done
#[derive(Debug)]
pub struct TextInput<'a, const N: usize> {
    label: &'a str,
    label_w: u16,
    buf: [u8; N],
    len: usize,
    charset: &'static str,
    candidate: usize,
    done: bool,
    space_glyph: char,
    backspace_glyph: char,
    done_glyph: char,
    window: u8,
    focused: bool,
    editing: bool,
    // Repaint gate: set when the candidate token, the buffer, the done latch,
    // focus or edit mode changes. Starts dirty so the first frame always draws.
    dirty: Cell<bool>,
}

impl<'a, const N: usize> TextInput<'a, N> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            label_w: 36,
            buf: [0; N],
            len: 0,
            charset: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 ",
            candidate: 0,
            done: false,
            space_glyph: '_',
            backspace_glyph: '<',
            done_glyph: '#',
            window: 5,
            focused: false,
            editing: false,
            dirty: Cell::new(true),
        }
    }

    /// Sets the candidate character set. **Must** be printable ASCII
    /// (`0x20..=0x7E`) - the buffer stores each chosen character as `c as u8`,
    /// so anything else is silently truncated to a stray byte. Checked with a
    /// `debug_assert`, which is where a contract with no runtime cost belongs:
    /// the charset is a compile-time constant in every real use.
    pub const fn with_charset(mut self, cs: &'static str) -> Self {
        let b = cs.as_bytes();
        let mut i = 0;
        while i < b.len() {
            debug_assert!(
                b[i] >= 0x20 && b[i] <= 0x7E,
                "TextInput charset must be printable ASCII"
            );
            i += 1;
        }
        self.charset = cs;
        self
    }

    pub const fn with_window(mut self, w: u8) -> Self {
        self.window = w;
        self
    }

    /// Sets the label column width, in pixels.
    pub const fn with_label_width(mut self, px: u16) -> Self {
        self.label_w = px;
        self
    }

    pub const fn with_glyphs(mut self, space: char, backspace: char, done: char) -> Self {
        self.space_glyph = space;
        self.backspace_glyph = backspace;
        self.done_glyph = done;
        self
    }

    /// The text entered so far.
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Clears the buffer and resets the candidate and done flag.
    pub fn reset(&mut self) {
        self.len = 0;
        self.done = false;
        self.candidate = 0;
        self.dirty.set(true);
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn charset_len(&self) -> usize {
        self.charset.chars().count()
    }

    /// Total tokens: every charset char plus backspace and done.
    fn token_count(&self) -> usize {
        self.charset_len() + 2
    }

    /// The glyph displayed for the token at `idx`.
    fn glyph_at(&self, idx: usize) -> char {
        let cl = self.charset_len();
        if idx < cl {
            let c = self.charset.chars().nth(idx).unwrap_or(' ');
            if c == ' ' { self.space_glyph } else { c }
        } else if idx == cl {
            self.backspace_glyph
        } else {
            self.done_glyph
        }
    }
}

impl<'a, const N: usize> Component for TextInput<'a, N> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        let tc = self.token_count(); // always >= 2
        // A Select on a full buffer, or a backspace on an empty one, is a no-op:
        // comparing the three pieces of state afterwards catches both.
        let before = (self.candidate, self.len, self.done);
        // The ribbon is a **ring** - it has no first or last token - so Up/Down
        // always land somewhere new and never hand the event back. That is the
        // one navigable widget here without edges, and it is deliberate: the
        // way out of the ribbon is the `done` token, not running off its end.
        let outcome = match msg {
            Msg::Down => {
                self.candidate = (self.candidate + 1) % tc;
                Outcome::Consumed
            }
            Msg::Up => {
                self.candidate = (self.candidate + tc - 1) % tc;
                Outcome::Consumed
            }
            Msg::Select => {
                let cl = self.charset_len();
                if self.candidate < cl {
                    if self.len < N {
                        let c = self.charset.chars().nth(self.candidate).unwrap_or(' ');
                        self.buf[self.len] = c as u8; // ASCII
                        self.len += 1;
                    }
                    // A full buffer swallows the keystroke rather than passing
                    // it on: mid-edit, Select belongs to the ribbon either way.
                    Outcome::Consumed
                } else if self.candidate == cl {
                    if self.len > 0 {
                        self.len -= 1; // backspace
                    }
                    Outcome::Consumed
                } else {
                    self.done = true; // done
                    // "Done" is the field's own activation - the form leaves
                    // edit mode (editing_finished) and the app reads text().
                    Outcome::Activated
                }
            }
            _ => Outcome::Ignored,
        };
        if (self.candidate, self.len, self.done) != before {
            self.dirty.set(true);
        }
        outcome
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);
        let line_h = target.line_height().max(1);

        // Row 0: label + current text, banded as one block when focused. The
        // ribbon row below is deliberately left off the band - its unselected
        // tokens are `Muted`, which an inverted band would swallow.
        let row_style = draw_cursor_band(
            target,
            Area::new(area.x, area.y, area.w, line_h.min(area.h)),
            self.focused,
        );
        let lw = self.label_w.min(area.w);
        if lw > 0 {
            target.draw_text(
                area.x,
                area.y,
                truncate(self.label, (lw / cw) as usize),
                row_style,
            );
        }
        let text_x = area.x + self.label_w;
        let avail = area.w.saturating_sub(self.label_w);
        if avail > 0 {
            let text = truncate(self.text(), (avail / cw) as usize);
            target.draw_text(text_x, area.y, text, row_style);
            if self.editing {
                // The 1px underline is the monochrome edit cue for an *unbanded*
                // row; on a band it would be the band's own ink on itself, so it
                // is dropped there and the ribbon carries the cue instead.
                if !self.focused {
                    target.fill_rect(
                        Area::new(text_x, area.y + line_h - 1, avail, 1),
                        Style::Focus,
                    );
                }
                let caret_x = text_x + target.text_width(text);
                if caret_x + cw <= area.x + area.w {
                    target.draw_text(caret_x, area.y, "_", row_style);
                }
            }
        }

        // Row 1: the token ribbon - shown **only while editing**, so its presence
        // (a window of candidates centred on the highlighted one) is itself the
        // "you are inside, Up/Down pick a token" cue.
        if self.editing && area.h >= 2 * line_h {
            let tc = self.token_count();
            let visible = (self.window as usize).min(tc);
            let half = visible / 2;
            let ry = area.y + line_h;
            for k in 0..visible {
                let idx = (self.candidate + tc + k - half) % tc;
                let cx = area.x + (k as u16) * (2 * cw);
                if cx + cw > area.x + area.w {
                    break;
                }
                let g = self.glyph_at(idx);
                let style = if idx == self.candidate {
                    Style::Focus
                } else {
                    Style::Muted
                };
                let mut b = [0u8; 4];
                target.draw_text(cx, ry, g.encode_utf8(&mut b), style);
            }
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true); // focus decides whether the text row bands
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

impl<'a, const N: usize> FormField for TextInput<'a, N> {
    /// State-dependent height: the token ribbon only exists while editing, so the
    /// field claims its second row exactly then (and one row otherwise). The
    /// [`Form`](crate::Form) re-lays-out each frame, so entering edit pushes the
    /// fields below down by one row - accepted, and it wastes no space when idle.
    fn height(&self, target: &dyn RenderTarget) -> u16 {
        let line_h = target.line_height();
        if self.editing {
            line_h.saturating_mul(2)
        } else {
            line_h
        }
    }

    fn editable(&self) -> bool {
        true
    }

    fn set_editing(&mut self, editing: bool) {
        if editing != self.editing {
            // Edit mode grows the field by a row and shows the ribbon.
            self.dirty.set(true);
        }
        self.editing = editing;
        if !editing {
            // Leaving edit clears the transient "done" latch (and candidate) so a
            // later re-entry starts clean; the typed text is preserved.
            self.done = false;
            self.candidate = 0;
        }
    }

    /// `Select` must reach the widget while editing - it picks the highlighted
    /// token (a character, backspace, or "done").
    fn captures_select_while_editing(&self) -> bool {
        true
    }

    /// The "done" token sets [`is_done`](TextInput::is_done); that ends editing.
    fn editing_finished(&self) -> bool {
        self.done
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

    #[test]
    fn textinput_initial_empty() {
        let ti = TextInput::<16>::new("Name");
        assert_eq!(ti.text(), "");
        assert!(!ti.is_done());
    }

    #[test]
    fn textinput_type_scroll_backspace() {
        let mut ti = TextInput::<8>::new("X").with_charset("AB");
        let _ = ti.update(&Msg::Select); // 'A'
        assert_eq!(ti.text(), "A");
        let _ = ti.update(&Msg::Down); // candidate → 'B'
        let _ = ti.update(&Msg::Select);
        assert_eq!(ti.text(), "AB");
        let _ = ti.update(&Msg::Down); // candidate → backspace token
        let _ = ti.update(&Msg::Select);
        assert_eq!(ti.text(), "A");
    }

    #[test]
    fn textinput_done() {
        let mut ti = TextInput::<8>::new("X").with_charset("AB");
        let _ = ti.update(&Msg::Up); // candidate → done ((0 + 4 - 1) % 4 = 3)
        let _ = ti.update(&Msg::Select);
        assert!(ti.is_done());
    }

    /// The charset contract ("printable ASCII", because the buffer stores
    /// `c as u8`) is now checked instead of silently truncating: `'Я' as u8`
    /// used to land a stray byte in the buffer.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "printable ASCII")]
    fn textinput_rejects_a_non_ascii_charset() {
        let _ = TextInput::<8>::new("X").with_charset("ЙЦУ");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "printable ASCII")]
    fn textinput_rejects_a_control_charset() {
        let _ = TextInput::<8>::new("X").with_charset("A\tB");
    }

    #[test]
    fn textinput_capacity() {
        let mut ti = TextInput::<2>::new("X").with_charset("A");
        let _ = ti.update(&Msg::Select);
        let _ = ti.update(&Msg::Select);
        let _ = ti.update(&Msg::Select); // full → ignored
        assert_eq!(ti.text(), "AA");
    }

    #[test]
    fn textinput_ribbon_renders_only_while_editing() {
        let mut ti = TextInput::<8>::new("Nm").with_charset("ABC").with_window(3);
        let area = Area::new(0, 0, 120, 30); // 2+ rows tall

        // Not editing → no ribbon, no underline.
        let mut t0 = RecordingTarget::new(120, 30);
        ti.view(&mut t0, area);
        assert!(
            !t0.ops()
                .iter()
                .any(|op| matches!(op, Op::Text { y: 10, .. }))
        );

        // Editing → ribbon on row 1 (y = line_height = 10), candidate Focus.
        ti.set_editing(true);
        let _ = ti.update(&Msg::Down); // candidate → 'B'
        let mut t = RecordingTarget::new(120, 30);
        ti.view(&mut t, area);
        let tx: Vec<_> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.clone(), *style)),
                _ => None,
            })
            .collect();
        // Window of 3 centred on 'B': A(0) B(12,Focus) C(24).
        assert!(tx.contains(&(0, 10, "A".into(), Style::Muted)));
        assert!(tx.contains(&(12, 10, "B".into(), Style::Focus)));
        assert!(tx.contains(&(24, 10, "C".into(), Style::Muted)));
        // Edit underline cue present (a Fill on row 0's bottom).
        assert!(
            t.ops()
                .iter()
                .any(|op| matches!(op, Op::Fill { area, style: Style::Focus } if area.h == 1))
        );
    }

    /// A focused text input bands its **first** row only - the label and the
    /// text typed so far are one block, while the token ribbon below keeps the
    /// plain ground its `Muted` candidates need to stay readable.
    #[test]
    fn textinput_bands_only_its_text_row() {
        let mut ti = TextInput::<8>::new("Nm").with_charset("ABC").with_window(3);
        ti.focus();
        ti.set_editing(true);
        let area = Area::new(0, 0, 120, 30);
        let mut t = RecordingTarget::new(120, 30);
        ti.view(&mut t, area);

        let bands: Vec<_> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Band { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect();
        assert_eq!(bands, [(Area::new(0, 0, 120, 10), Style::Focus)]);
        // Label and typed text both draw in the band's style.
        assert!(t.ops().iter().any(|op| matches!(
            op,
            Op::Text { text, style: Style::Focus, y: 0, .. } if text == "Nm"
        )));
        // The 1px edit underline is dropped on a band - it is drawn in the same
        // ink as the band on monochrome, so it would be invisible anyway; the
        // ribbon is the edit cue there.
        assert!(
            !t.ops()
                .iter()
                .any(|op| matches!(op, Op::Fill { area, .. } if area.h == 1)),
            "no underline under a band"
        );
    }

    /// Unfocused, there is no band and nothing inverts.
    #[test]
    fn textinput_unfocused_draws_no_band() {
        let ti = TextInput::<8>::new("Nm");
        let mut t = RecordingTarget::new(120, 30);
        ti.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(!t.ops().iter().any(|op| matches!(op, Op::Band { .. })));
    }

    #[test]
    fn textinput_formfield_in_form() {
        use crate::{Counter, Form, FormField};
        let mut ti = TextInput::<8>::new("N").with_charset("AB");
        let mut ct = Counter::new("C");
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 2] = [&mut ti, &mut ct];
            form.sync_focus(&mut fields);
            let _ = form.update(&Msg::Select, &mut fields); // enter edit on TextInput
            assert!(form.is_editing());
            let _ = form.update(&Msg::Select, &mut fields); // routed into field → appends 'A'
            assert!(form.is_editing());
            assert_eq!(form.focus_index(), 0);
            let _ = form.update(&Msg::Up, &mut fields); // candidate → done token
            let _ = form.update(&Msg::Select, &mut fields); // done → form leaves edit
            assert!(!form.is_editing());
        }
        assert_eq!(ti.text(), "A");
    }

    #[test]
    fn textinput_height_is_two_rows_while_editing() {
        // line_height = 10: one row idle, two rows while editing (for the ribbon).
        let mut ti = TextInput::<8>::new("N");
        let t = RecordingTarget::new(120, 60);
        assert_eq!(FormField::height(&ti, &t), 10);
        ti.set_editing(true);
        assert_eq!(FormField::height(&ti, &t), 20);
        ti.set_editing(false);
        assert_eq!(FormField::height(&ti, &t), 10);
    }

    #[test]
    fn textinput_set_editing_resets_done() {
        let mut ti = TextInput::<8>::new("X").with_charset("AB");
        ti.set_editing(true);
        let _ = ti.update(&Msg::Up);
        let _ = ti.update(&Msg::Select);
        assert!(ti.editing_finished());
        ti.set_editing(false);
        assert!(!ti.editing_finished());
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn textinput_dirty_gate() {
        let mut ti = TextInput::<2>::new("X").with_charset("AB");
        assert!(ti.dirty());
        ti.mark_clean();

        let _ = ti.update(&Msg::Tick); // no token, no text, no change
        assert!(!ti.dirty());

        let _ = ti.update(&Msg::Down); // candidate moves - the ribbon changes
        assert!(ti.dirty());
        ti.mark_clean();

        let _ = ti.update(&Msg::Select); // appends a character
        assert!(ti.dirty());
        ti.mark_clean();

        let _ = ti.update(&Msg::Select); // buffer full (N = 2) after two appends
        ti.mark_clean();
        let _ = ti.update(&Msg::Select); // nothing left to append → no change
        assert!(!ti.dirty());

        ti.focus();
        assert!(ti.dirty());
        ti.mark_clean();
        ti.blur();
        assert!(ti.dirty());
        ti.mark_clean();

        // Entering edit grows the field by a row and shows the ribbon.
        ti.set_editing(true);
        assert!(ti.dirty());
        ti.mark_clean();
        ti.set_editing(true); // already editing → no change
        assert!(!ti.dirty());
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    /// The token ribbon is a ring, so it has no edge to hand an event back at -
    /// the way out is the `done` token, not running off the end.
    #[test]
    fn textinput_ribbon_has_no_edge() {
        let mut ti: TextInput<8> = TextInput::new("Name").with_charset("ab");
        for _ in 0..(2 + 2) * 2 {
            assert_eq!(ti.update(&Msg::Down), Outcome::Consumed);
        }
        assert_eq!(ti.update(&Msg::Up), Outcome::Consumed);
    }

    #[test]
    fn textinput_select_types_and_done_activates() {
        let mut ti: TextInput<2> = TextInput::new("Name").with_charset("ab");
        assert_eq!(ti.update(&Msg::Select), Outcome::Consumed, "typed 'a'");
        assert_eq!(
            ti.update(&Msg::Select),
            Outcome::Consumed,
            "typed 'a' again"
        );
        // The buffer is full: the keystroke is swallowed mid-edit, not passed on.
        assert_eq!(ti.update(&Msg::Select), Outcome::Consumed, "full buffer");
        assert_eq!(ti.text(), "aa");

        // Walk to the `done` token (charset len + 1) and finish.
        let _ = ti.update(&Msg::Up); // one step back from 'a' wraps onto done
        assert_eq!(ti.update(&Msg::Select), Outcome::Activated);
        assert!(ti.is_done());
    }
}
