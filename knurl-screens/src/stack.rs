//! The scrolling window behind the two screens that stack widgets by hand.
//!
//! A hand-drawn page has no widget to scroll it: the screen keeps an offset (a
//! [`ScrollZone`](knurl::ScrollZone) on the chain moves it) and paints a window
//! of rows itself. [`Stack`] is that window - the offset, the row loop and the
//! overflow indicator - so the two screens are left with nothing but the rows
//! they actually draw.
//!
//! ## Why it is a type and not a function
//!
//! It used to be a function that cleared the window on every single frame, and
//! the screens above it built their rows inside `draw`. Both halves of the
//! documented anti-pattern, in the demo that documents it: a spinner tick on
//! the Indicators screen reported **84%** of a 320x240 panel dirty, where the
//! same animation with its widgets in fields reports 4%.
//!
//! So the window behaves like any other container: it clears **only when the
//! rows it shows change** - the offset moved, or the screen was entered - and
//! on every other frame it hands each row its rectangle and lets the widget's
//! own dirty gate decide. The rows above it live in the screen's fields, where
//! they can stay clean.

use knurl::{Area, Component, RenderTarget, ScrollZone, Scrollbar};

/// Pixels the scroll indicator wants on the right.
const BAR_W: u16 = 4;

/// A scrolled window over a fixed number of rows, drawn by the screen.
pub struct Stack {
    /// First visible row - what the [`ScrollZone`] on the chain moves.
    scroll: usize,
    /// Rows that fitted last frame; the ceiling the zone stops at.
    visible: usize,
    /// The offset the window was last painted at. `None` means it owes a full
    /// repaint - a fresh screen, or one just entered.
    painted: Option<usize>,
    /// Kept as a field for the same reason the rows are: rebuilt per frame it
    /// would repaint its column forever.
    bar: Scrollbar,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            scroll: 0,
            visible: 1,
            painted: None,
            bar: Scrollbar::new(),
        }
    }

    /// The zone that gives the window to the focus chain.
    pub fn zone(&mut self, total: usize) -> ScrollZone<'_> {
        let max = total.saturating_sub(self.visible);
        ScrollZone::new(&mut self.scroll, max)
    }

    /// Demands a full repaint - what a screen calls from `on_enter`, since the
    /// repaint cascade walks *zones* and the rows inside a window are not ones.
    pub fn mark_dirty(&mut self) {
        self.painted = None;
    }

    /// Paints rows `scroll..` of a `total`-row stack into `area`, one text line
    /// each.
    ///
    /// `draw_row` is handed the row index, its rectangle, and whether the
    /// window **shifted** this frame. On a shifted frame the slot now shows a
    /// different row, so the caller marks that row's widget dirty; on every
    /// other frame the widget decides for itself and a still picture costs
    /// nothing.
    pub fn rows(
        &mut self,
        target: &mut dyn RenderTarget,
        area: Area,
        total: usize,
        mut draw_row: impl FnMut(&mut dyn RenderTarget, usize, Area, bool),
    ) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let lh = target.line_height().max(1);
        let visible = (area.h / lh) as usize;
        self.visible = visible;

        let shifted = self.painted != Some(self.scroll);
        self.painted = Some(self.scroll);
        if shifted {
            // The rows are about to move under the slots: wipe the window once,
            // rather than trusting every row to cover its predecessor exactly.
            // Which also wipes the indicator, so it is owed a paint as well -
            // its own gate would otherwise stay clean and leave a blank column.
            target.clear(area);
            self.bar.mark_dirty();
        }

        let overflow = total > visible && area.w > BAR_W;
        let w = if overflow { area.w - BAR_W } else { area.w };

        for r in 0..visible {
            let i = self.scroll + r;
            if i >= total {
                break;
            }
            draw_row(
                target,
                i,
                Area::new(area.x, area.y + r as u16 * lh, w, lh),
                shifted,
            );
        }

        if overflow {
            self.bar.set(total, visible, self.scroll);
            self.bar
                .view(target, Area::new(area.x + area.w - 3, area.y, 3, area.h));
        }
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}
