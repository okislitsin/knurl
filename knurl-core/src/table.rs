use core::cell::Cell;

use crate::{
    Area, Component, Marker, Msg, RenderTarget, Style, V_SCROLL_RESERVE, draw_cursor_band,
    draw_v_scroll,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

/// Pixel gutter between columns; a 1px separator line is drawn in its middle.
const COL_GAP: u16 = 6;

// ── TableModel (data provider) ──────────────────────────────────────────────

/// Tabular data behind a [`Table`] (mirrors [`ListModel`](crate::ListModel)):
/// a row/column count and per-cell borrowed text.
///
/// The cleanest static representation is a slice/array of fixed-width rows
/// `[[&str; C]]` - the column count `C` rides in the type. Headers and pixel
/// column widths are **presentation**, passed to the widget, not the model.
pub trait TableModel {
    fn row_count(&self) -> usize;
    fn col_count(&self) -> usize;
    fn cell(&self, r: usize, c: usize) -> &str;
}

/// Static impl over a slice of `C`-column rows.
impl<const C: usize> TableModel for [[&str; C]] {
    fn row_count(&self) -> usize {
        self.len()
    }
    fn col_count(&self) -> usize {
        C
    }
    fn cell(&self, r: usize, c: usize) -> &str {
        self[r][c]
    }
}

/// Array impl so an inline `&[[…], […]]` literal works directly under generic `M`.
impl<const R: usize, const C: usize> TableModel for [[&str; C]; R] {
    fn row_count(&self) -> usize {
        R
    }
    fn col_count(&self) -> usize {
        C
    }
    fn cell(&self, r: usize, c: usize) -> &str {
        self[r][c]
    }
}

// ── Table ─────────────────────────────────────────────────────────────────────

/// A row/column table backed by a [`TableModel`], with an optional header, a
/// selectable row, and vertical scrolling.
///
/// Renders as a real pixel grid: fixed column widths, **1px vertical column
/// separators** and a **1px header underline** (via `fill_rect`) - no `|`/`-`
/// characters. The header is `Accent` and the rows `Muted`; the selected row
/// follows the focus language (see [`draw_cursor_band`]) - a full-width band
/// while the table holds focus, plain `Normal` when it does not. A [`Marker`]
/// column on the left carries the cursor when the table does *not* have focus -
/// styles alone cannot say it, since a monochrome theme draws `Normal` and
/// `Muted` in the same ink. The column is reserved either way, so the layout
/// does not shift as focus moves; [`Marker::NONE`] gives it up. Scrolls (never
/// truncates) with the built-in scroll indicator.
pub struct Table<'a, M: TableModel + ?Sized> {
    model: &'a M,
    headers: Option<&'a [&'a str]>,
    widths: &'a [u16],
    selected: usize,
    offset: usize,
    focused: bool,
    marker: Marker,
    page_size: Cell<usize>,
    // Repaint gate: set when the selection, the scroll offset or focus changes,
    // cleared after a paint. Starts dirty so the first frame always draws.
    dirty: Cell<bool>,
}

impl<'a, M: TableModel + ?Sized> Table<'a, M> {
    /// Creates a table over `model` with per-column pixel `widths` (one entry per
    /// column; a missing entry renders that column at zero width).
    pub fn new(model: &'a M, widths: &'a [u16]) -> Self {
        Self {
            model,
            headers: None,
            widths,
            selected: 0,
            offset: 0,
            focused: false,
            marker: Marker::ARROW,
            page_size: Cell::new(usize::MAX),
            dirty: Cell::new(true),
        }
    }

    pub fn with_headers(mut self, headers: &'a [&'a str]) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Sets the cursor marker drawn in the column left of the cells
    /// ([`Marker::NONE`] to drop the column entirely).
    pub fn with_marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Index of the currently highlighted row.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// First visible data-row index (scroll offset).
    pub fn offset(&self) -> usize {
        self.offset
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn col_width(&self, c: usize) -> u16 {
        self.widths.get(c).copied().unwrap_or(0)
    }

    /// Left pixel x of column `c`, counting from the first cell column `x0`
    /// (cumulative widths + gutters). `x0` already excludes the marker column.
    fn col_left(&self, x0: u16, c: usize) -> u16 {
        let mut x = x0;
        for i in 0..c {
            x = x.saturating_add(self.col_width(i)).saturating_add(COL_GAP);
        }
        x
    }

    /// Characters that fit in column `c` at pixel `x`: its own width, capped at
    /// whatever is left before `right` - a column wider than the space left must
    /// not run under the scroll indicator.
    fn cell_chars(&self, x: u16, c: usize, cw: u16, right: u16) -> usize {
        (self.col_width(c).min(right.saturating_sub(x)) / cw) as usize
    }

    /// Draws one row of cells across the columns at pixel-row `y`.
    fn draw_cells(
        &self,
        target: &mut dyn RenderTarget,
        x0: u16,
        right: u16,
        y: u16,
        r: usize,
        style: Style,
    ) {
        let cw = target.char_width().max(1);
        let cols = self.model.col_count();
        for c in 0..cols {
            let x = self.col_left(x0, c);
            let max = self.cell_chars(x, c, cw, right);
            if max > 0 {
                target.draw_text(x, y, truncate(self.model.cell(r, c), max), style);
            }
        }
    }
}

impl<'a, M: TableModel + ?Sized> Component for Table<'a, M> {
    fn update(&mut self, msg: &Msg) {
        let n = self.model.row_count();
        if n == 0 {
            return;
        }
        let page = self.page_size.get().max(1);
        match msg {
            Msg::Down if self.selected + 1 < n => {
                self.selected += 1;
                if self.selected >= self.offset + page {
                    self.offset = self.selected + 1 - page;
                }
                self.dirty.set(true);
            }
            Msg::Up if self.selected > 0 => {
                self.selected -= 1;
                if self.selected < self.offset {
                    self.offset = self.selected;
                }
                self.dirty.set(true);
            }
            // Everything else - Select, a clamped end, a tick - changes no
            // pixel, so the gate stays clean and an idle frame skips the table.
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let header_h = if self.headers.is_some() { line_h } else { 0 };

        let body_h = area.h.saturating_sub(header_h);
        let data_rows = (body_h / line_h) as usize;
        self.page_size.set(data_rows.max(1));

        let cols = self.model.col_count();
        let n = self.model.row_count();
        if area.w == 0 || area.h == 0 || data_rows == 0 || cols == 0 {
            return;
        }

        let overflow = n > data_rows;
        let reserve = if overflow { V_SCROLL_RESERVE } else { 0 };
        let content_right = area.x + area.w.saturating_sub(reserve);

        // The cursor-marker column, reserved whether or not the table has focus:
        // handing it back on blur would shift every cell (and the band's width)
        // as focus moves, which costs more than the two character cells.
        let cw = target.char_width().max(1);
        let prefix_px = self.marker.width() as u16 * cw;
        let x0 = area.x.saturating_add(prefix_px);

        // Vertical column separators (1px) spanning the whole table height.
        for c in 0..cols.saturating_sub(1) {
            let sep_x = self.col_left(x0, c).saturating_add(self.col_width(c)) + COL_GAP / 2;
            if sep_x < content_right {
                target.fill_rect(Area::new(sep_x, area.y, 1, area.h), Style::Muted);
            }
        }

        // Header row + underline.
        if let Some(headers) = self.headers {
            for c in 0..cols {
                let x = self.col_left(x0, c);
                let max = self.cell_chars(x, c, cw, content_right);
                if max > 0 {
                    let h = headers.get(c).copied().unwrap_or("");
                    target.draw_text(x, area.y, truncate(h, max), Style::Accent);
                }
            }
            let underline_w = content_right.saturating_sub(area.x);
            target.fill_rect(
                Area::new(area.x, area.y + line_h - 1, underline_w, 1),
                Style::Muted,
            );
        }

        // Data rows.
        for row in 0..data_rows {
            let idx = self.offset + row;
            if idx >= n {
                break;
            }
            let y = area.y + header_h + row as u16 * line_h;
            // The cursor row follows the focus language (band when focused, plain
            // when not); the rest are dimmed so the cursor still reads on an
            // unfocused table, which has no marker column to fall back on.
            let style = if idx == self.selected {
                draw_cursor_band(
                    target,
                    Area::new(area.x, y, content_right.saturating_sub(area.x), line_h),
                    self.focused,
                )
            } else {
                Style::Muted
            };
            let prefix = if idx == self.selected {
                self.marker.selected
            } else {
                self.marker.unselected
            };
            if !prefix.is_empty() {
                target.draw_text(area.x, y, prefix, style);
            }
            self.draw_cells(target, x0, content_right, y, idx, style);
        }

        if overflow {
            // Indicator spans the data region (below the header).
            let body = Area::new(area.x, area.y + header_h, area.w, body_h);
            draw_v_scroll(target, body, n, data_rows, self.offset);
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

    const HEADERS: &[&str] = &["Name", "Val"];
    const ROWS: &[[&str; 2]] = &[["Alpha", "1"], ["Beta", "2"], ["Gamma", "3"]];
    // col0 = 36px [0,36) = 6 chars, gutter 6 (sep at 39), col1 left = 42 (24px = 4 chars).
    const WIDTHS: &[u16] = &[36, 24];

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

    fn fills(t: &RecordingTarget) -> Vec<(Area, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Fill { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn table_grid_separators_and_header_rule() {
        let table = Table::new(ROWS, WIDTHS).with_headers(HEADERS);
        let mut t = RecordingTarget::new(80, 50); // header + 4 data rows
        table.view(&mut t, Area::new(0, 0, 80, 50));
        let tx = texts(&t);
        // Header cells in Accent, one marker column (2 chars = 12px) in:
        // "Name" at x = 12, "Val" at x = 12 + 42.
        assert!(tx.contains(&(12, 0, "Name".into(), Style::Accent)));
        assert!(tx.contains(&(54, 0, "Val".into(), Style::Accent)));
        // 1px vertical separator between the two columns, likewise at 39 + 12.
        assert!(
            fills(&t)
                .iter()
                .any(|(a, st)| a.w == 1 && a.x == 51 && *st == Style::Muted)
        );
        // 1px header underline along the bottom of the header row (y = 9).
        assert!(
            fills(&t)
                .iter()
                .any(|(a, st)| a.h == 1 && a.y == 9 && *st == Style::Muted)
        );
        // No '|' or '-' characters anywhere.
        assert!(!tx.iter().any(|(_, _, s, _)| s == "|" || s == "-"));
    }

    #[test]
    fn table_selected_row_is_focus() {
        let mut table = Table::new(ROWS, WIDTHS).with_headers(HEADERS);
        table.focus();
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        // Row 0 selected by default → "Alpha" at y = 10 (after header) in Focus,
        // its cell column starting after the marker at x = 12.
        assert!(
            texts(&t)
                .iter()
                .any(|(x, y, s, st)| *x == 12 && *y == 10 && s == "Alpha" && *st == Style::Focus)
        );
        // Row 1 not selected → dimmed.
        assert!(
            texts(&t)
                .iter()
                .any(|(_, y, s, st)| *y == 20 && s == "Beta" && *st == Style::Muted)
        );
    }

    #[test]
    fn table_no_headers_first_row_at_top() {
        let table = Table::new(ROWS, WIDTHS);
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        assert!(
            texts(&t)
                .iter()
                .any(|(x, y, s, _)| *x == 12 && *y == 0 && s == "Alpha")
        );
    }

    #[test]
    fn table_navigation_and_selected_row() {
        let mut table = Table::new(ROWS, WIDTHS);
        table.update(&Msg::Down);
        assert_eq!(table.selected(), 1);
    }

    #[test]
    fn table_scrolls_and_shows_indicator() {
        // 3 rows, only 2 data rows fit (no header) → overflow.
        let mut table = Table::new(ROWS, WIDTHS);
        table.focus();
        let mut t = RecordingTarget::new(80, 20); // 2 rows
        table.view(&mut t, Area::new(0, 0, 80, 20)); // page ← 2
        assert!(fills(&t).iter().any(|(_, st)| *st == Style::Focus)); // thumb present

        table.update(&Msg::Down);
        table.update(&Msg::Down); // selected 2 → offset advances
        assert_eq!(table.offset(), 1);
        let mut t2 = RecordingTarget::new(80, 20);
        table.view(&mut t2, Area::new(0, 0, 80, 20));
        // Gamma (row 2) is now visible and focused.
        assert!(
            texts(&t2)
                .iter()
                .any(|(_, _, s, st)| s == "Gamma" && *st == Style::Focus)
        );
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

    /// A focused table bands its selected row across the content width (clear of
    /// the scroll column), below the header, with the cells in the band's style.
    #[test]
    fn focused_table_bands_the_selected_row() {
        let mut table = Table::new(ROWS, WIDTHS).with_headers(HEADERS);
        table.focus();
        let mut t = RecordingTarget::new(80, 50); // header + 4 rows, no overflow
        table.view(&mut t, Area::new(0, 0, 80, 50));

        assert_eq!(bands(&t), [(Area::new(0, 10, 80, 10), Style::Focus)]);
        assert!(
            texts(&t)
                .iter()
                .any(|(_, y, s, st)| *y == 10 && s == "Alpha" && *st == Style::Focus)
        );
    }

    /// An unfocused table marks its cursor row by contrast alone - `Normal`
    /// against the `Muted` rows around it - and never inverts.
    #[test]
    fn unfocused_table_marks_the_cursor_row_without_a_band() {
        let table = Table::new(ROWS, WIDTHS).with_headers(HEADERS);
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));

        assert!(bands(&t).is_empty());
        let tx = texts(&t);
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 10 && s == "Alpha" && *st == Style::Normal)
        );
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 20 && s == "Beta" && *st == Style::Muted)
        );
    }

    /// With the scroll indicator up, the band stops short of its column.
    #[test]
    fn table_band_clears_the_scroll_column() {
        let mut table = Table::new(ROWS, WIDTHS);
        table.focus();
        let mut t = RecordingTarget::new(80, 20); // 2 rows for 3 → overflow
        table.view(&mut t, Area::new(0, 0, 80, 20));
        assert_eq!(
            bands(&t),
            [(Area::new(0, 0, 80 - V_SCROLL_RESERVE, 10), Style::Focus)]
        );
    }

    // ── Cursor marker ─────────────────────────────────────────────────────────

    /// An unfocused table shows where the cursor sits with a marker column, the
    /// way `List` does. Contrast alone cannot say it: a monochrome theme draws
    /// `Normal` and `Muted` in the same ink.
    #[test]
    fn unfocused_table_shows_the_cursor_marker() {
        let mut table = Table::new(ROWS, WIDTHS);
        table.update(&Msg::Down); // cursor on "Beta"
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));

        let tx = texts(&t);
        assert!(bands(&t).is_empty(), "an unfocused table must not invert");
        // No headers here, so the data rows sit at y = 0/10/20: the marker is on
        // the cursor row, its same-width twin on the others.
        assert!(tx.contains(&(0, 0, "  ".into(), Style::Muted)));
        assert!(tx.contains(&(0, 10, "> ".into(), Style::Normal)));
        assert!(tx.contains(&(0, 20, "  ".into(), Style::Muted)));
    }

    /// Focused, the marker is part of the band like every other glyph on the row.
    #[test]
    fn focused_table_marker_joins_the_band() {
        let mut table = Table::new(ROWS, WIDTHS);
        table.focus();
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        assert!(texts(&t).contains(&(0, 0, "> ".into(), Style::Focus)));
    }

    /// The column is reserved whether or not the table has focus, so the layout
    /// (and the band's width) does not shift as focus moves.
    #[test]
    fn table_marker_column_is_reserved_regardless_of_focus() {
        let cells = |focused: bool| {
            let mut table = Table::new(ROWS, WIDTHS);
            if focused {
                table.focus();
            }
            let mut t = RecordingTarget::new(80, 50);
            table.view(&mut t, Area::new(0, 0, 80, 50));
            texts(&t)
                .into_iter()
                .filter(|(_, _, s, _)| s == "Alpha")
                .map(|(x, ..)| x)
                .collect::<Vec<_>>()
        };
        assert_eq!(cells(false), cells(true));
        assert_eq!(cells(false), [12], "cells start one marker in");
    }

    /// `Marker::NONE` gives the column up entirely, exactly as in `List`.
    #[test]
    fn table_marker_none_starts_cells_at_the_origin() {
        let table = Table::new(ROWS, WIDTHS).with_marker(Marker::NONE);
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        let tx = texts(&t);
        assert!(tx.iter().any(|(x, _, s, _)| *x == 0 && s == "Alpha"));
        assert!(!tx.iter().any(|(_, _, s, _)| s == "> " || s == "  "));
    }

    /// Neither the marker nor the cells may run under the scroll indicator.
    #[test]
    fn table_marker_and_cells_clear_the_scroll_column() {
        let table = Table::new(ROWS, WIDTHS);
        let mut t = RecordingTarget::new(80, 20); // 2 rows for 3 → overflow
        table.view(&mut t, Area::new(0, 0, 80, 20));
        for (x, _, s, _) in texts(&t) {
            let end = x + 6 * s.chars().count() as u16;
            assert!(end <= 80 - V_SCROLL_RESERVE, "{s:?} ends at {end}");
        }
    }

    /// A custom model: cells computed outside the widget.
    struct Grid;
    impl TableModel for Grid {
        fn row_count(&self) -> usize {
            2
        }
        fn col_count(&self) -> usize {
            2
        }
        fn cell(&self, r: usize, c: usize) -> &str {
            [["r0c0", "r0c1"], ["r1c0", "r1c1"]][r][c]
        }
    }

    #[test]
    fn table_custom_model() {
        let g = Grid;
        let table = Table::new(&g, WIDTHS);
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        assert!(texts(&t).iter().any(|(_, _, s, _)| s == "r0c0"));
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 54 && s == "r1c1"));
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn table_dirty_gate() {
        let mut table = Table::new(ROWS, WIDTHS);
        assert!(table.dirty(), "a fresh widget paints its first frame");
        table.mark_clean();

        // Nothing that changes no state may dirty it.
        table.update(&Msg::Select);
        table.update(&Msg::Tick);
        table.update(&Msg::Up); // already on the first row
        assert!(!table.dirty());

        table.update(&Msg::Down);
        assert!(table.dirty());
        table.mark_clean();

        // Focus changes the picture now, so it must dirty too.
        table.focus();
        assert!(table.dirty());
        table.mark_clean();
        table.blur();
        assert!(table.dirty());
        table.mark_clean();
        table.mark_dirty();
        assert!(table.dirty());
    }

    /// The gate is what lets an idle frame skip the widget entirely.
    #[test]
    fn table_clean_view_draws_nothing() {
        let mut table = Table::new(ROWS, WIDTHS);
        let area = Area::new(0, 0, 80, 50);
        let mut t0 = RecordingTarget::new(80, 50);
        table.view(&mut t0, area);
        assert!(!t0.ops().is_empty());

        table.update(&Msg::Select); // no-op
        let mut t1 = RecordingTarget::new(80, 50);
        table.view(&mut t1, area);
        assert!(t1.ops().is_empty());
    }
}
