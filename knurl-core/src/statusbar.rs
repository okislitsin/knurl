use core::cell::Cell;

use crate::{Area, Component, Msg, Outcome, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

// ── StatusBar ─────────────────────────────────────────────────────────────────

/// A single-row status line with left-, centre-, and right-aligned ASCII
/// segments, separated from the content above by a thin 1px rule.
///
/// Charm-calm: text in [`Style::Muted`] by default (no bright blocks). All text is
/// ASCII - the rule is the only pixel decoration.
#[derive(Debug)]
pub struct StatusBar<'a> {
    left: &'a str,
    center: &'a str,
    right: &'a str,
    style: Style,
    // Repaint gate: set when a segment's text actually changes. Chrome spans
    // the panel, so an ungated one would stretch the dirty region across the
    // full width on every frame. Starts dirty so the first frame draws.
    dirty: Cell<bool>,
}

impl<'a> StatusBar<'a> {
    pub const fn new() -> Self {
        Self {
            left: "",
            center: "",
            right: "",
            style: Style::Muted,
            dirty: Cell::new(true),
        }
    }

    pub const fn with_left(mut self, s: &'a str) -> Self {
        self.left = s;
        self
    }

    pub const fn with_center(mut self, s: &'a str) -> Self {
        self.center = s;
        self
    }

    pub const fn with_right(mut self, s: &'a str) -> Self {
        self.right = s;
        self
    }

    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the left segment. Setting it to the text it already shows is not a
    /// change - a host that re-asserts the same hint every frame costs nothing.
    pub fn set_left(&mut self, s: &'a str) {
        if s != self.left {
            self.left = s;
            self.dirty.set(true);
        }
    }

    /// Sets the centre segment; dirties only on a real change.
    pub fn set_center(&mut self, s: &'a str) {
        if s != self.center {
            self.center = s;
            self.dirty.set(true);
        }
    }

    /// Sets the right segment; dirties only on a real change.
    pub fn set_right(&mut self, s: &'a str) {
        if s != self.right {
            self.right = s;
            self.dirty.set(true);
        }
    }
}

impl Default for StatusBar<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Component for StatusBar<'a> {
    fn update(&mut self, _msg: &Msg) -> Outcome {
        // Status bars are static: the event stays available to whoever is next.
        Outcome::Ignored
    }

    fn focusable(&self) -> bool {
        // Static: it can never use an event, so the cursor does not stop here.
        false
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);

        // A thin top rule separates the bar from the content above; text sits
        // just below it (falling back to the top row if there's no headroom).
        target.fill_rect(Area::new(area.x, area.y, area.w, 1), Style::Muted);
        let ty = if area.h > 2 { area.y + 2 } else { area.y };
        let max = (area.w / cw) as usize;

        if !self.left.is_empty() {
            target.draw_text(area.x, ty, truncate(self.left, max), self.style);
        }

        if !self.right.is_empty() {
            let rw = target.text_width(self.right);
            if rw <= area.w {
                target.draw_text(area.x + area.w - rw, ty, self.right, self.style);
            }
        }

        if !self.center.is_empty() {
            let cwid = target.text_width(self.center);
            if cwid <= area.w {
                target.draw_text(area.x + (area.w - cwid) / 2, ty, self.center, self.style);
            }
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
    fn statusbar_left_and_right_segments() {
        let mut t = RecordingTarget::new(120, 12);
        StatusBar::new()
            .with_left("Ready")
            .with_right("OK")
            .view(&mut t, Area::new(0, 0, 120, 12));
        let tx = texts(&t);
        // Left flush at x=0, text below the rule (y=2).
        assert!(tx.contains(&(0, 2, "Ready".into(), Style::Muted)));
        // "OK" = 2*6 = 12px → right-aligned at x = 120 - 12 = 108.
        assert!(tx.contains(&(108, 2, "OK".into(), Style::Muted)));
        // The 1px top rule is present.
        assert!(
            t.ops()
                .iter()
                .any(|op| matches!(op, Op::Fill { area, .. } if area.h == 1 && area.y == 0))
        );
    }

    #[test]
    fn statusbar_center() {
        let mut t = RecordingTarget::new(120, 12);
        StatusBar::new()
            .with_center("MID")
            .view(&mut t, Area::new(0, 0, 120, 12));
        // "MID" = 18px → x = (120 - 18) / 2 = 51.
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 51 && s == "MID"));
    }

    /// Chrome is the worst thing to leave always-dirty: it spans the panel, so
    /// repainting it for nothing stretches the region across the whole width
    /// every frame. It dirties when its text actually changes, and not before.
    #[test]
    fn statusbar_only_dirties_when_a_segment_changes() {
        let mut bar = StatusBar::new().with_left("Ready");
        assert!(bar.dirty(), "a fresh bar owes its first paint");
        bar.mark_clean();

        bar.set_left("Ready"); // the same hint, frame after frame
        assert!(!bar.dirty());
        bar.set_center("");
        bar.set_right("");
        assert!(!bar.dirty());

        bar.set_left("Saving");
        assert!(bar.dirty());
        bar.mark_clean();
        bar.set_right("100%");
        assert!(bar.dirty());
    }

    /// ...and a clean one draws nothing, so it costs the region nothing.
    #[test]
    fn a_clean_statusbar_leaves_the_region_empty() {
        let bar = StatusBar::new().with_left("Ready");
        let mut t = RecordingTarget::new(320, 240);
        let area = Area::new(0, 228, 320, 12);
        bar.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area));
        bar.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None);
    }

    #[test]
    fn statusbar_empty_draws_only_rule() {
        let mut t = RecordingTarget::new(80, 12);
        StatusBar::new().view(&mut t, Area::new(0, 0, 80, 12));
        // Only the rule, no text.
        assert!(!t.ops().iter().any(|op| matches!(op, Op::Text { .. })));
        assert_eq!(
            t.ops()
                .iter()
                .filter(|op| matches!(op, Op::Fill { .. }))
                .count(),
            1
        );
    }
}
