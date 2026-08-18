use core::cell::Cell;

use crate::{
    Area, Component, Marker, Msg, Outcome, RenderTarget, Style, V_SCROLL_RESERVE, draw_cursor_band,
    draw_v_scroll,
};

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
/// tracked - `cursor` (navigation) and `selected` (chosen), and the row shows
/// both: a [`Marker`] column carries the **cursor**, the dial
/// ([`draw_radio`](RenderTarget::draw_radio)) beside it the **choice** -
/// `> (*) Option`. They answer different questions, and on monochrome the dial
/// cannot stand in for the cursor. The marker column is reserved whether or not
/// the group has focus, so nothing shifts as focus moves; [`Marker::NONE`] gives
/// it up.
///
/// Otherwise pixel-laid-out like [`List`](crate::List): visible rows =
/// `area.h / line_height`, rows `Muted` with the cursor row following the focus
/// language (see [`draw_cursor_band`]), and the same built-in scroll indicator on
/// overflow.
///
/// ## Elm cycle note
/// The visible-row count is captured from `area.h / line_height` on each
/// [`view`](Radio::view) call and consumed by the next [`update`](Radio::update) to
/// compute scroll offsets. In the standard embedded loop - **render, then handle
/// input** - this is always in sync. Before the first frame it is `usize::MAX`
/// ("everything fits"), so an `update` that arrives ahead of any `view` moves
/// the cursor without scrolling the window under it.
#[derive(Debug)]
pub struct Radio<'a> {
    options: &'a [&'a str],
    selected: usize,
    cursor: usize,
    offset: usize,
    focused: bool,
    marker: Marker,
    // Interior mutability: view(&self) records the visible-row count so the
    // following update(&mut self) can scroll without knowing render dimensions.
    page_size: Cell<usize>,
    // Repaint gate: set when the cursor, the chosen option, the scroll offset or
    // focus changes. Starts dirty so the first frame always draws.
    dirty: Cell<bool>,
}

impl<'a> Radio<'a> {
    pub fn new(options: &'a [&'a str]) -> Self {
        Self {
            options,
            selected: 0,
            cursor: 0,
            offset: 0,
            focused: false,
            marker: Marker::ARROW,
            // usize::MAX → "everything fits" until the first view() call.
            page_size: Cell::new(usize::MAX),
            dirty: Cell::new(true),
        }
    }

    /// Sets the cursor marker drawn left of the dial ([`Marker::NONE`] to drop
    /// the column entirely).
    pub fn with_marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
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
    fn update(&mut self, msg: &Msg) -> Outcome {
        let n = self.options.len();
        if n == 0 {
            // No options: every event stays available to the container.
            return Outcome::Ignored;
        }
        let page = self.page_size.get().max(1);
        // Choosing the option already chosen, or a clamped end, moves nothing -
        // comparing the three indices afterwards keeps those frames clean.
        let before = (self.cursor, self.offset, self.selected);
        let outcome = match msg {
            Msg::Down if self.cursor + 1 < n => {
                self.cursor += 1;
                if self.cursor >= self.offset + page {
                    self.offset = self.cursor + 1 - page;
                }
                Outcome::Consumed
            }
            Msg::Up if self.cursor > 0 => {
                self.cursor -= 1;
                if self.cursor < self.offset {
                    self.offset = self.cursor;
                }
                Outcome::Consumed
            }
            // The chosen option is the dial's own state, not an app-level
            // action, so a pick is Consumed even when it re-picks the same one.
            Msg::Select => {
                self.selected = self.cursor;
                Outcome::Consumed
            }
            // A clamped end: the cursor has run out of options, so the event
            // goes back to the container.
            _ => Outcome::Ignored,
        };
        if (self.cursor, self.offset, self.selected) != before {
            self.dirty.set(true);
        }
        outcome
    }

    /// A widget that stacks rows needs a row: given less, it can paint nothing
    /// at all, and `view` must not call that a paint (see
    /// [`min_size`](Component::min_size)).
    fn min_size(&self, target: &dyn RenderTarget) -> (u16, u16) {
        (1, target.line_height().max(1))
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let visible = (area.h / line_h) as usize;
        self.page_size.set(visible.max(1));

        let n = self.options.len();
        if area.w == 0 || area.h == 0 || visible == 0 || n == 0 {
            return;
        }

        // Reserve the right-hand column for the scroll indicator only when the
        // group overflows - exactly as List/Form do.
        let overflowing = n > visible;
        let content_w = area
            .w
            .saturating_sub(if overflowing { V_SCROLL_RESERVE } else { 0 });

        let cw = target.char_width().max(1);
        // Cursor marker, then a 3-char dial slot + 1-char gap (the old "(*) "
        // prefix), then the label.
        let prefix_px = self.marker.width() as u16 * cw;
        let ind_x = area.x.saturating_add(prefix_px);
        let ind_w = 3 * cw;
        let label_x = ind_x + 4 * cw;
        let text_max = (content_w.saturating_sub(prefix_px + 4 * cw) / cw) as usize;

        for row in 0..visible {
            let idx = self.offset + row;
            if idx >= n {
                break;
            }
            let y = area.y.saturating_add(row as u16 * line_h);
            let style = if idx == self.cursor {
                draw_cursor_band(
                    target,
                    Area::new(area.x, y, content_w, line_h),
                    self.focused,
                )
            } else {
                Style::Muted
            };

            let prefix = if idx == self.cursor {
                self.marker.selected
            } else {
                self.marker.unselected
            };
            if !prefix.is_empty() {
                target.draw_text(area.x, y, prefix, style);
            }

            target.draw_radio(
                Area::new(ind_x, y, ind_w, area.h.min(line_h)),
                idx == self.selected,
                style,
            );

            if text_max > 0 {
                target.draw_text(label_x, y, truncate(self.options[idx], text_max), style);
            }
        }

        if overflowing {
            draw_v_scroll(target, area, n, visible, self.offset);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true); // focus decides whether the cursor row bands
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
        let mut radio = Radio::new(OPTS);
        radio.focus(); // the cursor row inverts only for the focused widget
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        // Row 0 chosen + cursor → "(*)" Focus, "Alpha" Focus. Both sit one
        // marker column (2 chars = 12px) further right than before.
        assert!(tx.contains(&(12, 0, "(*)".into(), Style::Focus)));
        assert!(
            tx.iter()
                .any(|(x, y, s, st)| *x == 36 && *y == 0 && s == "Alpha" && *st == Style::Focus)
        );
        // Row 1 not chosen, not cursor → "( )" Muted, "Beta" Muted at y=10.
        assert!(tx.contains(&(12, 10, "( )".into(), Style::Muted)));
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 10 && s == "Beta" && *st == Style::Muted)
        );
    }

    #[test]
    fn radio_cursor_moves_without_select() {
        let mut radio = Radio::new(OPTS);
        let _ = radio.update(&Msg::Down);
        assert_eq!(radio.cursor(), 1);
        assert_eq!(radio.selected(), 0);
    }

    #[test]
    fn radio_select_sets_chosen() {
        let mut radio = Radio::new(OPTS);
        radio.focus();
        let _ = radio.update(&Msg::Down);
        let _ = radio.update(&Msg::Select);
        assert_eq!(radio.selected(), 1);
        assert_eq!(radio.selected_option(), "Beta");

        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        // Beta now chosen "(*)" at row 1; Alpha no longer chosen "( )" at row 0.
        // The dial column starts at x = 12, after the marker.
        assert!(texts(&t).contains(&(12, 10, "(*)".into(), Style::Focus)));
        assert!(texts(&t).iter().any(|(_, y, s, _)| *y == 0 && s == "( )"));
    }

    #[test]
    fn radio_scroll_keeps_cursor_visible() {
        let mut radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 10); // 1 row visible
        radio.view(&mut t, Area::new(0, 0, 120, 10)); // page ← 1
        let _ = radio.update(&Msg::Down); // cursor leaves window → scrolls

        let mut t2 = RecordingTarget::new(120, 10);
        radio.view(&mut t2, Area::new(0, 0, 120, 10));
        assert!(texts(&t2).iter().any(|(_, _, s, _)| s == "Beta"));
    }

    // ── Focus language (band vs bare cursor) ───────────────────────────────────

    fn bands(t: &RecordingTarget) -> Vec<(Area, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Band { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect()
    }

    /// A focused radio group bands its cursor row; the dial and the option text
    /// draw in the band's style, so the row is one block.
    #[test]
    fn focused_radio_bands_the_cursor_row() {
        let mut radio = Radio::new(OPTS);
        radio.focus();
        let mut t = RecordingTarget::new(120, 30); // 3 rows for 3 options
        radio.view(&mut t, Area::new(0, 0, 120, 30));

        assert_eq!(bands(&t), [(Area::new(0, 0, 120, 10), Style::Focus)]);
        assert!(texts(&t).contains(&(12, 0, "(*)".into(), Style::Focus)));
    }

    /// An unfocused group keeps the cursor legible without inverting it.
    #[test]
    fn unfocused_radio_marks_the_cursor_row_without_a_band() {
        let radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));

        assert!(bands(&t).is_empty());
        let tx = texts(&t);
        assert!(tx.contains(&(12, 0, "(*)".into(), Style::Normal)));
        assert!(tx.contains(&(12, 10, "( )".into(), Style::Muted)));
    }

    // ── Cursor marker ─────────────────────────────────────────────────────────

    /// The dial says which option is *chosen*; it cannot also say where the
    /// cursor is. An unfocused group needs the marker column for that - on
    /// monochrome `Normal` and `Muted` are the same ink.
    #[test]
    fn unfocused_radio_shows_the_cursor_marker() {
        let mut radio = Radio::new(OPTS);
        let _ = radio.update(&Msg::Down); // cursor on "Beta", "Alpha" still chosen
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));

        let tx = texts(&t);
        assert!(bands(&t).is_empty());
        assert!(tx.contains(&(0, 10, "> ".into(), Style::Normal)));
        assert!(tx.contains(&(0, 0, "  ".into(), Style::Muted)));
        // Marker first, then the dial, then the label: `> (*) Alpha`.
        assert!(tx.contains(&(12, 0, "(*)".into(), Style::Muted)));
        assert!(
            tx.iter()
                .any(|(x, y, s, _)| *x == 36 && *y == 0 && s == "Alpha")
        );
    }

    #[test]
    fn focused_radio_marker_joins_the_band() {
        let mut radio = Radio::new(OPTS);
        radio.focus();
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(texts(&t).contains(&(0, 0, "> ".into(), Style::Focus)));
    }

    #[test]
    fn radio_marker_none_starts_the_dial_at_the_origin() {
        let radio = Radio::new(OPTS).with_marker(Marker::NONE);
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        assert!(tx.contains(&(0, 0, "(*)".into(), Style::Normal)));
        assert!(!tx.iter().any(|(_, _, s, _)| s == "> " || s == "  "));
    }

    #[test]
    fn radio_empty_safe() {
        let mut radio = Radio::new(&[]);
        let _ = radio.update(&Msg::Down);
        let _ = radio.update(&Msg::Select);
        assert_eq!(radio.selected_option(), "");
        let mut t = RecordingTarget::new(120, 30);
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        // Partial-redraw: view() clears its own area but draws no options.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn radio_dirty_gate() {
        let mut radio = Radio::new(OPTS);
        assert!(radio.dirty());
        radio.mark_clean();

        let _ = radio.update(&Msg::Up); // cursor already at the top
        let _ = radio.update(&Msg::Tick);
        let _ = radio.update(&Msg::Select); // row 0 is already the chosen one
        assert!(!radio.dirty());

        let _ = radio.update(&Msg::Down);
        assert!(radio.dirty());
        radio.mark_clean();
        let _ = radio.update(&Msg::Select); // chooses row 1 - a real change
        assert!(radio.dirty());
        radio.mark_clean();

        radio.focus();
        assert!(radio.dirty());
        radio.mark_clean();
        radio.blur();
        assert!(radio.dirty());
    }

    #[test]
    fn radio_clean_view_draws_nothing() {
        let mut radio = Radio::new(OPTS);
        let area = Area::new(0, 0, 120, 30);
        let mut t0 = RecordingTarget::new(120, 30);
        radio.view(&mut t0, area);
        assert!(!t0.ops().is_empty());

        let _ = radio.update(&Msg::Up); // clamped
        let mut t1 = RecordingTarget::new(120, 30);
        radio.view(&mut t1, area);
        assert!(t1.ops().is_empty());
    }

    /// A group taller than its area reserves the indicator column and draws the
    /// same track + thumb as List/Form.
    #[test]
    fn radio_overflow_draws_the_scroll_indicator() {
        let radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 20); // 2 rows for 3 options
        radio.view(&mut t, Area::new(0, 0, 120, 20));

        let fills: Vec<_> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Fill { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect();
        assert!(fills.iter().any(|&(a, st)| st == Style::Muted && a.w == 1));
        assert!(
            fills
                .iter()
                .any(|&(a, st)| st == Style::Focus && a.w == 3 && a.x == 117)
        );
        // …and the option text stops short of that column.
        for (x, _, s, _) in texts(&t) {
            let end = x + 6 * s.chars().count() as u16;
            assert!(
                end <= 120 - V_SCROLL_RESERVE,
                "{s:?} runs under the indicator"
            );
        }
    }

    #[test]
    fn radio_no_indicator_when_everything_fits() {
        let radio = Radio::new(OPTS);
        let mut t = RecordingTarget::new(120, 30); // 3 rows for 3 options
        radio.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(!t.ops().iter().any(|op| matches!(op, Op::Fill { .. })));
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    #[test]
    fn radio_spends_a_step_and_hands_back_an_edge() {
        let mut r = Radio::new(OPTS);
        assert_eq!(
            r.update(&Msg::Up),
            Outcome::Ignored,
            "Up at the first option"
        );
        assert_eq!(
            r.update(&Msg::Down),
            Outcome::Consumed,
            "a step in the middle"
        );
        while r.cursor() + 1 < OPTS.len() {
            assert_eq!(r.update(&Msg::Down), Outcome::Consumed);
        }
        assert_eq!(
            r.update(&Msg::Down),
            Outcome::Ignored,
            "Down at the last option"
        );
    }

    /// Choosing is the dial's own state, not an app-level action - so it is
    /// Consumed, and stays Consumed when it re-picks the option already chosen.
    #[test]
    fn radio_select_is_consumed_even_when_it_changes_nothing() {
        let mut r = Radio::new(OPTS);
        assert_eq!(r.update(&Msg::Select), Outcome::Consumed);
        assert_eq!(r.update(&Msg::Select), Outcome::Consumed);
        assert_eq!(r.selected(), 0);
    }

    #[test]
    fn empty_radio_hands_back_everything() {
        let none: &[&str] = &[];
        let mut r = Radio::new(none);
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Tick] {
            assert_eq!(
                r.update(&msg),
                Outcome::Ignored,
                "{msg:?} on an empty group"
            );
        }
    }
}
