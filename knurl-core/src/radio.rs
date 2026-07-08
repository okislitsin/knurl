use core::cell::Cell;

use crate::{Area, Component, Msg, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── Radio ─────────────────────────────────────────────────────────────────────

/// A vertically scrolling radio group: one option is chosen at a time.
///
/// A plain navigable widget (not a `FormField`): `Up`/`Down` move the cursor
/// (scrolling as needed), `Select` chooses the cursor row. Two indices are
/// tracked - `cursor` (navigation) and `selected` (chosen). Pixel-laid-out like
/// [`List`](crate::List): visible rows = `area.h / line_height`, marker via
/// [`draw_radio`](RenderTarget::draw_radio), cursor row `Focus` and the rest
/// `Muted`.
#[derive(Debug)]
pub struct Radio<'a> {
    options: &'a [&'a str],
    selected: usize,
    cursor: usize,
    offset: usize,
    focused: bool,
    // Interior mutability: view(&self) records the visible-row count so the
    // following update(&mut self) can scroll without knowing render dimensions.
    page_size: Cell<usize>,
}

impl<'a> Radio<'a> {
    pub fn new(options: &'a [&'a str]) -> Self {
        Self {
            options,
            selected: 0,
            cursor: 0,
            offset: 0,
            focused: false,
            page_size: Cell::new(usize::MAX),
        }
    }

    /// Index of the currently chosen option.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Text of the chosen option, or `""` for an empty group.
    pub fn selected_option(&self) -> &'a str {
        self.options.get(self.selected).copied().unwrap_or("")
    }

    /// Position of the navigation cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }
}

impl<'a> Component for Radio<'a> {
    fn update(&mut self, msg: &Msg) {
        let n = self.options.len();
        if n == 0 {
            return;
        }
        let page = self.page_size.get().max(1);
        match msg {
            Msg::Down if self.cursor + 1 < n => {
                self.cursor += 1;
                if self.cursor >= self.offset + page {
                    self.offset = self.cursor + 1 - page;
                }
            }
            Msg::Up if self.cursor > 0 => {
                self.cursor -= 1;
                if self.cursor < self.offset {
                    self.offset = self.cursor;
                }
            }
            Msg::Select => self.selected = self.cursor,
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let visible = (area.h / line_h) as usize;
        self.page_size.set(visible.max(1));

        if area.w == 0 || area.h == 0 || visible == 0 || self.options.is_empty() {
            return;
        }

        let cw = target.char_width().max(1);
        // A 3-char marker slot + 1-char gap, like the old "(*) " prefix.
        let ind_w = 3 * cw;
        let label_x = area.x + 4 * cw;
        let text_max = (area.w.saturating_sub(4 * cw) / cw) as usize;

        for row in 0..visible {
            let idx = self.offset + row;
            if idx >= self.options.len() {
                break;
            }
            let y = area.y.saturating_add(row as u16 * line_h);
            let style = if idx == self.cursor {
                Style::Focus
            } else {
                Style::Muted
            };

            target.draw_radio(
                Area::new(area.x, y, ind_w, area.h.min(line_h)),
                idx == self.selected,
                style,
            );

            if text_max > 0 {
                target.draw_text(label_x, y, truncate(self.options[idx], text_max), style);
            }
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

    const OPTS: &[&str] = &["Alpha", "Beta", "Gamma"];

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
    fn radio_draws_markers_and_styles() {
        let radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        // Row 0 chosen + cursor → "(*)" Focus, "Alpha" Focus at x=24.
        assert!(tx.contains(&(0, 0, "(*)".into(), Style::Focus)));
        assert!(
            tx.iter()
                .any(|(x, y, s, st)| *x == 24 && *y == 0 && s == "Alpha" && *st == Style::Focus)
        );
        // Row 1 not chosen, not cursor → "( )" Muted, "Beta" Muted at y=10.
        assert!(tx.contains(&(0, 10, "( )".into(), Style::Muted)));
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 10 && s == "Beta" && *st == Style::Muted)
        );
    }

    #[test]
    fn radio_cursor_moves_without_select() {
        let mut radio = Radio::new(OPTS);
        radio.update(&Msg::Down);
        assert_eq!(radio.cursor(), 1);
        assert_eq!(radio.selected(), 0);
    }

    #[test]
    fn radio_select_sets_chosen() {
        let mut radio = Radio::new(OPTS);
        radio.update(&Msg::Down);
        radio.update(&Msg::Select);
        assert_eq!(radio.selected(), 1);
        assert_eq!(radio.selected_option(), "Beta");

        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        // Beta now chosen "(*)" at row 1; Alpha no longer chosen "( )" at row 0.
        assert!(texts(&t).contains(&(0, 10, "(*)".into(), Style::Focus)));
        assert!(texts(&t).iter().any(|(_, y, s, _)| *y == 0 && s == "( )"));
    }

    #[test]
    fn radio_scroll_keeps_cursor_visible() {
        let mut radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 10); // 1 row visible
        radio.view(&mut t, Area::new(0, 0, 120, 10)); // page ← 1
        radio.update(&Msg::Down); // cursor leaves window → scrolls

        let mut t2 = RecordingTarget::new(120, 10);
        radio.view(&mut t2, Area::new(0, 0, 120, 10));
        assert!(texts(&t2).iter().any(|(_, _, s, _)| s == "Beta"));
    }

    #[test]
    fn radio_empty_safe() {
        let mut radio = Radio::new(&[]);
        radio.update(&Msg::Down);
        radio.update(&Msg::Select);
        assert_eq!(radio.selected_option(), "");
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        // Partial-redraw: view() clears its own area but draws no options.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }
}
