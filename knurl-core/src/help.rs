use core::cell::Cell;

use crate::{Area, Component, Msg, Outcome, RenderTarget, Scrollbar, Style, V_SCROLL_RESERVE};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── Help ──────────────────────────────────────────────────────────────────────

/// A read-only key/action cheat sheet in two columns, scrolling when the list
/// overflows the area (never truncated vertically - the "don't truncate" law).
///
/// Keys are drawn `Style::Accent` on the left, actions `Style::Muted` on the
/// right. `Up`/`Down` scroll (no selection). All text is ASCII; a built-in pixel
/// scroll indicator appears on overflow.
///
/// ## Focus
/// Like [`Pager`](crate::Pager), a cheat sheet has **no cursor** - there is
/// nothing for the focus band (see [`draw_cursor_band`](crate::draw_cursor_band))
/// to sit under - so it keeps no `focused` flag and leaves `focus()`/`blur()` at
/// the trait's no-ops.
#[derive(Debug)]
pub struct Help<'a> {
    items: &'a [(&'a str, &'a str)],
    key_w: u16,
    offset: usize,
    page_size: Cell<usize>,
    // Repaint gate: set when the scroll offset actually moves. Starts dirty so
    // the first frame always draws.
    dirty: Cell<bool>,
}

impl<'a> Help<'a> {
    pub fn new(items: &'a [(&'a str, &'a str)]) -> Self {
        Self {
            items,
            key_w: 48, // key column width, in pixels (≈8 chars)
            offset: 0,
            page_size: Cell::new(usize::MAX),
            dirty: Cell::new(true),
        }
    }

    /// Sets the key column width, in pixels.
    pub const fn with_key_width(mut self, px: u16) -> Self {
        self.key_w = px;
        self
    }

    /// First visible row index (scroll offset).
    pub fn offset(&self) -> usize {
        self.offset
    }
}

impl<'a> Component for Help<'a> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        let n = self.items.len();
        if n == 0 {
            return Outcome::Ignored;
        }
        let page = self.page_size.get().max(1);
        match msg {
            Msg::Down if self.offset + page < n => {
                self.offset += 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            Msg::Up if self.offset > 0 => {
                self.offset -= 1;
                self.dirty.set(true);
                Outcome::Consumed
            }
            // A clamped end scrolls nothing, so the frame is skipped - and the
            // event is handed back for the container to route elsewhere.
            _ => Outcome::Ignored,
        }
    }

    /// A widget that stacks rows needs a row: given less, it can paint nothing
    /// at all, and `view` must not call that a paint (see
    /// [`min_size`](Component::min_size)).
    fn min_size(&self, target: &dyn RenderTarget) -> (u16, u16) {
        (1, target.line_height().max(1))
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let cw = target.char_width().max(1);
        let rows = (area.h / line_h) as usize;
        self.page_size.set(rows.max(1));

        let n = self.items.len();
        if area.w == 0 || area.h == 0 || rows == 0 || n == 0 {
            return;
        }

        // The indicator needs its own column: an area narrower than the
        // reservation has no room for one, and drawing it anyway used to
        // compute `area.x + area.w - 3` and panic on the subtraction.
        let overflow = n > rows && area.w >= V_SCROLL_RESERVE;
        let reserve = if overflow { V_SCROLL_RESERVE } else { 0 };
        let content_w = area.w.saturating_sub(reserve);
        let key_w = self.key_w.min(content_w);
        let action_w = content_w.saturating_sub(key_w);

        for row in 0..rows {
            let idx = self.offset + row;
            if idx >= n {
                break;
            }
            let y = area.y.saturating_add(row as u16 * line_h);
            let (key, action) = self.items[idx];
            target.draw_text(
                area.x,
                y,
                truncate(key, (key_w / cw) as usize),
                Style::Accent,
            );
            if action_w > 0 {
                target.draw_text(
                    area.x + key_w,
                    y,
                    truncate(action, (action_w / cw) as usize),
                    Style::Muted,
                );
            }
        }

        if overflow {
            let mut sb = Scrollbar::new();
            sb.set(n, rows, self.offset);
            sb.view(target, Area::new(area.x + area.w - 3, area.y, 3, area.h));
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

    #[test]
    fn help_renders_two_columns() {
        let help = Help::new(&[("OK", "Select"), ("Up", "Move")]).with_key_width(30);
        let mut t = RecordingTarget::new(120, 30);
        help.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        // Keys Accent at x=0; actions Muted at x = key_w = 30.
        assert!(tx.contains(&(0, 0, "OK".into(), Style::Accent)));
        assert!(tx.contains(&(30, 0, "Select".into(), Style::Muted)));
        assert!(tx.contains(&(0, 10, "Up".into(), Style::Accent)));
        assert!(tx.contains(&(30, 10, "Move".into(), Style::Muted)));
    }

    #[test]
    fn help_scroll_down_and_indicator() {
        let items = &[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")];
        let mut help = Help::new(items);
        let mut t = RecordingTarget::new(120, 20); // 2 rows → overflow
        help.view(&mut t, Area::new(0, 0, 120, 20)); // page ← 2
        // Scroll indicator present.
        assert!(t.ops().iter().any(|op| matches!(op, Op::Fill { .. })));

        let _ = help.update(&Msg::Down);
        assert_eq!(help.offset(), 1);
        let mut t2 = RecordingTarget::new(120, 20);
        help.view(&mut t2, Area::new(0, 0, 120, 20));
        assert!(texts(&t2).iter().any(|(_, y, s, _)| *y == 0 && s == "b"));
    }

    #[test]
    fn help_scroll_clamps() {
        let mut help = Help::new(&[("a", "A"), ("b", "B")]);
        let _ = help.update(&Msg::Up);
        assert_eq!(help.offset(), 0);
    }

    #[test]
    fn help_empty_safe() {
        let mut help = Help::new(&[]);
        let _ = help.update(&Msg::Down);
        assert_eq!(help.offset(), 0);
        let mut t = RecordingTarget::new(120, 30);
        help.view(&mut t, Area::new(0, 0, 120, 30));
        // Partial-redraw: view() clears its own area but draws no content.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn help_dirty_gate() {
        let items = &[("a", "A"), ("b", "B"), ("c", "C")];
        let mut help = Help::new(items);
        let mut t = RecordingTarget::new(120, 20); // 2 rows → scrollable
        help.view(&mut t, Area::new(0, 0, 120, 20));
        assert!(!help.dirty());

        let _ = help.update(&Msg::Up); // already at the top
        let _ = help.update(&Msg::Tick);
        assert!(!help.dirty());

        let _ = help.update(&Msg::Down);
        assert!(help.dirty());
        help.mark_clean();
        let _ = help.update(&Msg::Down); // clamped at the last page
        assert!(!help.dirty());
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    /// Found by the seeded sweep (`smoke.rs`, seed 176), and it panicked:
    /// a `Help` that overflows puts its indicator at `area.x + area.w - 3`, and
    /// an area narrower than three pixels made that subtraction wrap. A layout
    /// that has run out of room hands over exactly such an area.
    #[test]
    fn a_scrolling_help_squeezed_below_its_indicator_does_not_panic() {
        const ITEMS: &[(&str, &str)] = &[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")];
        let h = Help::new(ITEMS);
        let mut t = RecordingTarget::new(64, 32);
        for w in 0..6u16 {
            h.mark_dirty();
            h.view(&mut t, Area::new(0, 0, w, 20)); // two rows of four: it scrolls
        }
    }

    #[test]
    fn help_spends_a_scroll_and_hands_back_an_edge() {
        const ITEMS: &[(&str, &str)] = &[("a", "1"), ("b", "2"), ("c", "3"), ("d", "4")];
        let mut h = Help::new(ITEMS);
        let mut t = RecordingTarget::new(120, 20); // 2 rows of 4 → it scrolls
        h.view(&mut t, Area::new(0, 0, 120, 20));

        assert_eq!(h.update(&Msg::Up), Outcome::Ignored, "Up at the top");
        assert_eq!(h.update(&Msg::Down), Outcome::Consumed, "a row of scroll");
        while h.offset() + 2 < ITEMS.len() {
            assert_eq!(h.update(&Msg::Down), Outcome::Consumed);
        }
        assert_eq!(h.update(&Msg::Down), Outcome::Ignored, "Down at the bottom");
        assert_eq!(
            h.update(&Msg::Select),
            Outcome::Ignored,
            "Help has nothing to pick"
        );
    }
}
