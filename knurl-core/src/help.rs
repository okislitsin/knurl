use core::cell::Cell;

use crate::{Area, Component, Msg, RenderTarget, Scrollbar, Style, V_SCROLL_RESERVE};

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
#[derive(Debug)]
pub struct Help<'a> {
    items: &'a [(&'a str, &'a str)],
    key_w: u16,
    offset: usize,
    focused: bool,
    page_size: Cell<usize>,
}

impl<'a> Help<'a> {
    pub fn new(items: &'a [(&'a str, &'a str)]) -> Self {
        Self {
            items,
            key_w: 48, // key column width, in pixels (≈8 chars)
            offset: 0,
            focused: false,
            page_size: Cell::new(usize::MAX),
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
    fn update(&mut self, msg: &Msg) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let page = self.page_size.get().max(1);
        match msg {
            Msg::Down if self.offset + page < n => self.offset += 1,
            Msg::Up if self.offset > 0 => self.offset -= 1,
            _ => {}
        }
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

        let overflow = n > rows;
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

    fn focus(&mut self) {
        self.focused = true;
    }

    fn blur(&mut self) {
        self.focused = false;
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

        help.update(&Msg::Down);
        assert_eq!(help.offset(), 1);
        let mut t2 = RecordingTarget::new(120, 20);
        help.view(&mut t2, Area::new(0, 0, 120, 20));
        assert!(texts(&t2).iter().any(|(_, y, s, _)| *y == 0 && s == "b"));
    }

    #[test]
    fn help_scroll_clamps() {
        let mut help = Help::new(&[("a", "A"), ("b", "B")]);
        help.update(&Msg::Up);
        assert_eq!(help.offset(), 0);
    }

    #[test]
    fn help_empty_safe() {
        let mut help = Help::new(&[]);
        help.update(&Msg::Down);
        assert_eq!(help.offset(), 0);
        let mut t = RecordingTarget::new(120, 30);
        help.view(&mut t, Area::new(0, 0, 120, 30));
        // Partial-redraw: view() clears its own area but draws no content.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }
}
