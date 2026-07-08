use core::cell::Cell;

use crate::{Area, Component, Msg, RenderTarget, Style, V_SCROLL_RESERVE, draw_v_scroll};

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
/// characters. The selected row is `Style::Focus`, the header `Accent`, other
/// rows `Normal`. Scrolls (never truncates) with the built-in scroll indicator.
pub struct Table<'a, M: TableModel + ?Sized> {
    model: &'a M,
    headers: Option<&'a [&'a str]>,
    widths: &'a [u16],
    selected: usize,
    offset: usize,
    focused: bool,
    page_size: Cell<usize>,
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
            page_size: Cell::new(usize::MAX),
        }
    }

    pub fn with_headers(mut self, headers: &'a [&'a str]) -> Self {
        self.headers = Some(headers);
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

    /// Left pixel x of column `c` within `area` (cumulative widths + gutters).
    fn col_left(&self, area: Area, c: usize) -> u16 {
        let mut x = area.x;
        for i in 0..c {
            x = x.saturating_add(self.col_width(i)).saturating_add(COL_GAP);
        }
        x
    }

    /// Draws one row of cells across the columns at pixel-row `y`.
    fn draw_cells(
        &self,
        target: &mut dyn RenderTarget,
        area: Area,
        y: u16,
        r: usize,
        style: Style,
    ) {
        let cw = target.char_width().max(1);
        let cols = self.model.col_count();
        for c in 0..cols {
            let x = self.col_left(area, c);
            let max = (self.col_width(c) / cw) as usize;
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
            }
            Msg::Up if self.selected > 0 => {
                self.selected -= 1;
                if self.selected < self.offset {
                    self.offset = self.selected;
                }
            }
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

        // Vertical column separators (1px) spanning the whole table height.
        for c in 0..cols.saturating_sub(1) {
            let sep_x = self.col_left(area, c).saturating_add(self.col_width(c)) + COL_GAP / 2;
            if sep_x < content_right {
                target.fill_rect(Area::new(sep_x, area.y, 1, area.h), Style::Muted);
            }
        }

        // Header row + underline.
        if let Some(headers) = self.headers {
            let cw = target.char_width().max(1);
            for c in 0..cols {
                let x = self.col_left(area, c);
                let max = (self.col_width(c) / cw) as usize;
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
            let style = if idx == self.selected {
                Style::Focus
            } else {
                Style::Normal
            };
            self.draw_cells(target, area, y, idx, style);
        }

        if overflow {
            // Indicator spans the data region (below the header).
            let body = Area::new(area.x, area.y + header_h, area.w, body_h);
            draw_v_scroll(target, body, n, data_rows, self.offset);
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
        // Header cells in Accent: "Name" at x=0, "Val" at x=36.
        assert!(tx.contains(&(0, 0, "Name".into(), Style::Accent)));
        assert!(tx.contains(&(42, 0, "Val".into(), Style::Accent)));
        // 1px vertical separator between the two columns at x = 33.
        assert!(
            fills(&t)
                .iter()
                .any(|(a, st)| a.w == 1 && a.x == 39 && *st == Style::Muted)
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
        let table = Table::new(ROWS, WIDTHS).with_headers(HEADERS);
        let mut t = RecordingTarget::new(80, 50);
        table.view(&mut t, Area::new(0, 0, 80, 50));
        // Row 0 selected by default → "Alpha" at y = 10 (after header) in Focus.
        assert!(
            texts(&t)
                .iter()
                .any(|(x, y, s, st)| *x == 0 && *y == 10 && s == "Alpha" && *st == Style::Focus)
        );
        // Row 1 not selected → Normal.
        assert!(
            texts(&t)
                .iter()
                .any(|(_, y, s, st)| *y == 20 && s == "Beta" && *st == Style::Normal)
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
                .any(|(x, y, s, _)| *x == 0 && *y == 0 && s == "Alpha")
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
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 42 && s == "r1c1"));
    }
}
