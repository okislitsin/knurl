use core::cell::Cell;

use crate::{Align, Area, Component, Msg, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max_chars` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    s.char_indices()
        .nth(max_chars)
        .map(|(i, _)| &s[..i])
        .unwrap_or(s)
}

// ── Title ───────────────────────────────────────────────────────────────────

/// A non-interactive heading: text rendered with [`Style::Accent`] and a
/// horizontal alignment within its area.
///
/// Holds a borrowed `&str` - no heap allocation required. Alignment is computed
/// in pixels from the target's [`text_width`](RenderTarget::text_width).
#[derive(Debug)]
pub struct Title<'a> {
    text: &'a str,
    style: Style,
    align: Align,
    dirty: Cell<bool>,
}

impl<'a> Title<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            style: Style::Accent,
            align: Align::Left,
            dirty: Cell::new(true),
        }
    }

    /// Replaces the heading text and marks it for repaint.
    pub fn set_text(&mut self, text: &'a str) {
        self.text = text;
        self.dirty.set(true);
    }

    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub const fn with_align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn text(&self) -> &str {
        self.text
    }

    pub fn style(&self) -> Style {
        self.style
    }

    pub fn align(&self) -> Align {
        self.align
    }
}

impl<'a> Component for Title<'a> {
    fn update(&mut self, _msg: &Msg) {
        // Titles are static; nothing to update.
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }

        // Truncate to what fits across the area's pixel width, then place the
        // (possibly truncated) text by measuring it in pixels.
        let cw = target.char_width().max(1);
        let max_chars = (area.w / cw) as usize;
        let text = truncate_str(self.text, max_chars);
        let text_w = target.text_width(text);

        let x = match self.align {
            Align::Left => area.x,
            Align::Center => area.x.saturating_add(area.w.saturating_sub(text_w) / 2),
            Align::Right => area.x.saturating_add(area.w.saturating_sub(text_w)),
        };

        target.draw_text(x, area.y, text, self.style);
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

// ── Separator ─────────────────────────────────────────────────────────────────

/// A thin horizontal rule: a [`fill_rect`](RenderTarget::fill_rect) line spanning
/// the area's width, vertically centred, `thickness` pixels tall.
#[derive(Debug)]
pub struct Separator {
    thickness: u16,
    style: Style,
    dirty: Cell<bool>,
}

impl Separator {
    pub const fn new() -> Self {
        Self {
            thickness: 1,
            style: Style::Muted,
            dirty: Cell::new(true),
        }
    }

    /// Sets the rule thickness in pixels (default 1).
    pub const fn with_thickness(mut self, thickness: u16) -> Self {
        self.thickness = thickness;
        self
    }

    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl Default for Separator {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Separator {
    fn update(&mut self, _msg: &Msg) {
        // Separators are static; nothing to update.
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }

        // Clamp the rule to the available height and centre it vertically.
        let t = self.thickness.max(1).min(area.h);
        let y = area.y.saturating_add((area.h - t) / 2);
        target.fill_rect(Area::new(area.x, y, area.w, t), self.style);
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

// ── Spacer ──────────────────────────────────────────────────────────────────

/// Empty space - draws nothing. Useful as a layout filler.
#[derive(Debug)]
pub struct Spacer;

impl Spacer {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Spacer {
    fn update(&mut self, _msg: &Msg) {
        // Nothing to update.
    }

    fn view(&self, _target: &mut dyn RenderTarget, _area: Area) {
        // Spacers occupy layout space but render nothing.
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{Op, RecordingTarget};

    // ── Title ─────────────────────────────────────────────────────────────────

    #[test]
    fn title_left_default() {
        // char_width = 6 (default mock metric).
        let mut t = RecordingTarget::new(60, 10);
        Title::new("Hi").view(&mut t, Area::new(0, 0, 60, 10));
        assert_eq!(t.first_text(), Some((0, 0, "Hi", Style::Accent)));
    }

    #[test]
    fn title_center() {
        let mut t = RecordingTarget::new(60, 10);
        Title::new("Hi")
            .with_align(Align::Center)
            .view(&mut t, Area::new(0, 0, 60, 10));
        // text_width("Hi") = 2 * 6 = 12; (60 - 12) / 2 = 24.
        assert_eq!(t.first_text(), Some((24, 0, "Hi", Style::Accent)));
    }

    #[test]
    fn title_right() {
        let mut t = RecordingTarget::new(60, 10);
        Title::new("Hi")
            .with_align(Align::Right)
            .view(&mut t, Area::new(0, 0, 60, 10));
        // 60 - 12 = 48.
        assert_eq!(t.first_text(), Some((48, 0, "Hi", Style::Accent)));
    }

    #[test]
    fn title_truncates_to_pixel_width() {
        // Width 30 px / 6 px per char = 5 chars fit.
        let mut t = RecordingTarget::new(30, 10);
        Title::new("ABCDEFGH").view(&mut t, Area::new(0, 0, 30, 10));
        assert_eq!(t.first_text(), Some((0, 0, "ABCDE", Style::Accent)));
    }

    // ── Separator ─────────────────────────────────────────────────────────────

    #[test]
    fn separator_fills_width_as_thin_line() {
        let mut t = RecordingTarget::new(64, 10);
        Separator::new().view(&mut t, Area::new(0, 0, 64, 10));
        // Partial-redraw: view() clears the widget's own area first, then draws
        // the 1px line vertically centred in the 10px row: y = (10 - 1) / 2 = 4.
        assert_eq!(
            t.ops(),
            &[
                Op::Clear {
                    area: Area::new(0, 0, 64, 10)
                },
                Op::Fill {
                    area: Area::new(0, 4, 64, 1),
                    style: Style::Muted
                },
            ]
        );
    }

    #[test]
    fn separator_custom_thickness_and_style() {
        let mut t = RecordingTarget::new(64, 10);
        Separator::new()
            .with_thickness(2)
            .with_style(Style::Accent)
            .view(&mut t, Area::new(0, 0, 64, 10));
        // Self-clear, then a 2px line centred: y = (10 - 2) / 2 = 4.
        assert_eq!(
            t.ops(),
            &[
                Op::Clear {
                    area: Area::new(0, 0, 64, 10)
                },
                Op::Fill {
                    area: Area::new(0, 4, 64, 2),
                    style: Style::Accent
                },
            ]
        );
    }

    // ── Spacer ────────────────────────────────────────────────────────────────

    #[test]
    fn spacer_renders_nothing() {
        let mut t = RecordingTarget::new(64, 20);
        Spacer::new().view(&mut t, Area::new(0, 0, 64, 20));
        assert!(t.ops().is_empty());
    }
}
