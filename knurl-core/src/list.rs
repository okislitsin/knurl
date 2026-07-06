use core::cell::Cell;

use crate::{Area, Component, Marker, Msg, RenderTarget, Style};

// ── ListModel (data provider) ──────────────────────────────────────────────────

/// The data behind a [`List`]: a count and indexed access to borrowed item text.
///
/// This decouples the widget from its data - `List` talks to a `ListModel`, not
/// a concrete slice, so an app can back a list with its own store (a fixed
/// array, a ring buffer of log lines, …) without copying into the widget.
///
/// `get_item` returns a **borrowed** `&str`, which is all stored/static content
/// needs. Generated or streaming content (UART logs and the like) is *not* this
/// trait's job - that belongs to the `Pager` streaming model instead.
pub trait ListModel {
    /// Number of items in the list.
    fn item_count(&self) -> usize;
    /// The text of item `i`. Callers only index `0..item_count()`.
    fn get_item(&self, i: usize) -> &str;
}

/// Backwards-compatible static impl: a `&[&str]` is the simplest model, so
/// `List::new(items)` works for any slice.
impl ListModel for [&str] {
    fn item_count(&self) -> usize {
        self.len()
    }

    fn get_item(&self, i: usize) -> &str {
        self[i]
    }
}

/// Array impl so an inline literal - `List::new(&["a", "b"])` - works directly
/// (under a generic `M`, `&[&str; N]` is inferred as the array type rather than
/// coerced to a slice, so the array needs its own impl).
impl<const N: usize> ListModel for [&str; N] {
    fn item_count(&self) -> usize {
        N
    }

    fn get_item(&self, i: usize) -> &str {
        self[i]
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max_chars` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

// Geometry of the built-in scroll indicator (a thin track + thumb at the right
// edge, drawn only when the list overflows). The standalone `Scrollbar` widget
// (in the `info` module) is a separate component; here the indicator is baked
// directly into `List`.
const SCROLLBAR_W: u16 = 3;
const SCROLLBAR_GAP: u16 = 1;
const TRACK_W: u16 = 1;
const MIN_THUMB_PX: u16 = 3;

// ── List ──────────────────────────────────────────────────────────────────────

/// A vertically scrolling interactive list, pixel-laid-out and backed by a
/// [`ListModel`].
///
/// All widget state (`selected`/`offset`/`focused`) is stack-allocated; no heap
/// allocation. The model is borrowed (`&'a M`), so `List` is generic over any
/// `ListModel` - the default `M = [&str]` keeps `List::new(&["a", "b"])` ergonomic.
///
/// > A trait *object* (`&dyn ListModel`) cannot be built from a `[&str]` slice
/// > (it is unsized - a trait object needs a thin self pointer), so the borrow is
/// > a generic `&'a M` rather than `&dyn`. This still fully decouples the widget
/// > from the concrete data type and additionally accepts arrays, slices and
/// > custom sized models with one impl.
///
/// ## Elm cycle note
/// The visible-row count is captured from `area.h / line_height` on each
/// [`view`](List::view) call and consumed by the next [`update`](List::update) to
/// compute scroll offsets. In the standard embedded loop - **render, then handle
/// input** - this is always in sync.
pub struct List<'a, M: ListModel + ?Sized = [&'a str]> {
    model: &'a M,
    selected: usize,
    offset: usize,
    focused: bool,
    marker: Marker,
    // Interior mutability: view(&self) records the visible-row count so the
    // following update(&mut self) can scroll without knowing render dimensions.
    page_size: Cell<usize>,
    // Repaint gate: set when selection/scroll/focus changes, cleared after a
    // paint. Starts dirty so the first frame always draws.
    dirty: Cell<bool>,
}

impl<'a, M: ListModel + ?Sized> List<'a, M> {
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            selected: 0,
            offset: 0,
            focused: false,
            marker: Marker::ARROW,
            // usize::MAX → "everything fits" until the first view() call.
            page_size: Cell::new(usize::MAX),
            dirty: Cell::new(true),
        }
    }

    /// Sets the selection marker drawn beside each item.
    pub fn with_marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Index of the currently highlighted item.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Text of the currently highlighted item, or `""` for an empty list.
    pub fn selected_item(&self) -> &str {
        if self.selected < self.model.item_count() {
            self.model.get_item(self.selected)
        } else {
            ""
        }
    }

    /// First visible item index (scroll offset).
    pub fn offset(&self) -> usize {
        self.offset
    }
}

impl<'a, M: ListModel + ?Sized> Component for List<'a, M> {
    fn update(&mut self, msg: &Msg) {
        let n = self.model.item_count();
        if n == 0 {
            return;
        }
        let page = self.page_size.get().max(1);
        match msg {
            Msg::Down if self.selected + 1 < n => {
                self.selected += 1;
                // Scroll forward: keep the selection inside the window.
                if self.selected >= self.offset + page {
                    self.offset = self.selected + 1 - page;
                }
                self.dirty.set(true);
            }
            Msg::Up if self.selected > 0 => {
                self.selected -= 1;
                // Scroll backward: keep the selection inside the window.
                if self.selected < self.offset {
                    self.offset = self.selected;
                }
                self.dirty.set(true);
            }
            // Select and the clamped ends change nothing - leave the gate clean
            // so an idle frame is skipped. The caller reads selected().
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let visible = (area.h / line_h) as usize;

        // Cache the visible-row count for the next update() call.
        self.page_size.set(visible.max(1));

        let n = self.model.item_count();
        if area.w == 0 || area.h == 0 || visible == 0 || n == 0 {
            return;
        }

        // Reserve space on the right for the scroll indicator only when needed.
        let overflowing = n > visible;
        let bar_w = if overflowing { SCROLLBAR_W + SCROLLBAR_GAP } else { 0 };
        let content_w = area.w.saturating_sub(bar_w);

        // The leftmost columns are reserved for the selection-marker prefix.
        let cw = target.char_width().max(1);
        let prefix_px = self.marker.width() as u16 * cw;
        let text_px = content_w.saturating_sub(prefix_px);
        let max_chars = (text_px / cw) as usize;

        for row in 0..visible {
            let item_idx = self.offset + row;
            if item_idx >= n {
                break;
            }

            let y = area.y.saturating_add(row as u16 * line_h);
            let is_sel = item_idx == self.selected;
            // Charm look: the selected row is Focus, the rest are dimmed (Muted).
            let style = if is_sel { Style::Focus } else { Style::Muted };
            let prefix = if is_sel { self.marker.selected } else { self.marker.unselected };

            if !prefix.is_empty() {
                target.draw_text(area.x, y, prefix, style);
            }
            if max_chars > 0 {
                let text = truncate_str(self.model.get_item(item_idx), max_chars);
                target.draw_text(area.x.saturating_add(prefix_px), y, text, style);
            }
        }

        if overflowing {
            self.draw_scroll_indicator(target, area, n, visible);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.dirty.set(true);
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

impl<'a, M: ListModel + ?Sized> List<'a, M> {
    /// Draws the thin track + thumb at the right edge via `fill_rect`. The track
    /// is `TRACK_W` px wide (centred in the `SCROLLBAR_W` band); the thumb spans
    /// the full band, so it reads as a distinct handle even on monochrome (where
    /// `fill_rect` ignores the style and the wider thumb is what shows).
    fn draw_scroll_indicator(
        &self,
        target: &mut dyn RenderTarget,
        area: Area,
        n: usize,
        visible: usize,
    ) {
        let band_x = area.x + area.w - SCROLLBAR_W;
        let track_x = band_x + (SCROLLBAR_W - TRACK_W) / 2;
        target.fill_rect(Area::new(track_x, area.y, TRACK_W, area.h), Style::Muted);

        let track_h = area.h;
        let thumb_h = (((track_h as usize * visible) / n) as u16)
            .max(MIN_THUMB_PX)
            .min(track_h);
        let max_off = n - visible;
        let progress = ((track_h - thumb_h) as usize * self.offset)
            .checked_div(max_off)
            .unwrap_or(0) as u16;
        let thumb_y = area.y + progress;
        target.fill_rect(Area::new(band_x, thumb_y, SCROLLBAR_W, thumb_h), Style::Focus);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use alloc::vec::Vec;

    const ITEMS: &[&str] = &["Alpha", "Beta", "Gamma", "Delta", "Epsilon"];

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.
    // So a 30px-tall area shows 3 rows at y = 0, 10, 20; the marker "> " (2 chars)
    // is 12px wide, item text starts at x = 12.

    /// Collects the recorded text ops as `(x, y, text, style)`.
    fn texts(t: &RecordingTarget) -> impl Iterator<Item = (u16, u16, &str, Style)> {
        t.ops().iter().filter_map(|op| match op {
            Op::Text { x, y, text, style } => Some((*x, *y, text.as_str(), *style)),
            _ => None,
        })
    }

    fn fills(t: &RecordingTarget) -> impl Iterator<Item = (Area, Style)> + '_ {
        t.ops().iter().filter_map(|op| match op {
            Op::Fill { area, style } => Some((*area, *style)),
            _ => None,
        })
    }

    // ── ListModel ──────────────────────────────────────────────────────────────

    #[test]
    fn slice_model_basics() {
        let m: &[&str] = ITEMS;
        assert_eq!(m.item_count(), 5);
        assert_eq!(m.get_item(0), "Alpha");
        assert_eq!(m.get_item(4), "Epsilon");
    }

    /// A tiny custom model: items computed/stored outside the widget.
    struct Digits;
    impl ListModel for Digits {
        fn item_count(&self) -> usize {
            3
        }
        fn get_item(&self, i: usize) -> &str {
            ["one", "two", "three"][i]
        }
    }

    #[test]
    fn custom_model_drives_list() {
        let m = Digits;
        let list = List::new(&m);
        assert_eq!(list.selected_item(), "one");
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        let drawn: Vec<_> = texts(&t).collect();
        // 3 items fit in 3 rows; first item selected → Focus + "> ".
        assert!(drawn.contains(&(12, 0, "one", Style::Focus)));
        assert!(drawn.contains(&(12, 10, "two", Style::Muted)));
    }

    // ── Rendering (pixel positions + selection styles) ──────────────────────────

    #[test]
    fn renders_visible_rows_at_pixel_positions() {
        let list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30); // 3 rows visible
        list.view(&mut t, Area::new(0, 0, 120, 30));

        let drawn: Vec<_> = texts(&t).collect();
        // Selected row 0: marker "> " at (0,0) Focus, "Alpha" at (12,0) Focus.
        assert!(drawn.contains(&(0, 0, "> ", Style::Focus)));
        assert!(drawn.contains(&(12, 0, "Alpha", Style::Focus)));
        // Row 1: unselected marker "  " at (0,10) Muted, "Beta" at (12,10) Muted.
        assert!(drawn.contains(&(0, 10, "  ", Style::Muted)));
        assert!(drawn.contains(&(12, 10, "Beta", Style::Muted)));
        // Row 2: "Gamma" at y=20.
        assert!(drawn.contains(&(12, 20, "Gamma", Style::Muted)));
        // Delta/Epsilon are below the fold → not drawn.
        assert!(!drawn.iter().any(|&(_, _, s, _)| s == "Delta" || s == "Epsilon"));
    }

    #[test]
    fn renders_at_nonzero_origin() {
        let list = List::new(ITEMS);
        let mut t = RecordingTarget::new(200, 60);
        list.view(&mut t, Area::new(20, 10, 120, 30));
        let drawn: Vec<_> = texts(&t).collect();
        // Marker at (20,10); text at (20 + 12, 10).
        assert!(drawn.contains(&(20, 10, "> ", Style::Focus)));
        assert!(drawn.contains(&(32, 10, "Alpha", Style::Focus)));
        // Second row at y = 10 + line_height(10) = 20.
        assert!(drawn.contains(&(32, 20, "Beta", Style::Muted)));
    }

    #[test]
    fn none_marker_starts_text_at_origin() {
        let list = List::new(ITEMS).with_marker(Marker::NONE);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        // No prefix op; text begins at area.x.
        let drawn: Vec<_> = texts(&t).collect();
        assert!(drawn.contains(&(0, 0, "Alpha", Style::Focus)));
        assert!(!drawn.iter().any(|&(_, _, s, _)| s == "> " || s == "  "));
    }

    #[test]
    fn text_truncated_to_pixel_width() {
        // Width 30px, no scrollbar (single item fits): 30/6 = 5 chars; minus the
        // 2-char marker (12px) leaves 18px = 3 chars of text.
        let list = List::new(&["ABCDEFGH"]);
        let mut t = RecordingTarget::new(30, 10);
        list.view(&mut t, Area::new(0, 0, 30, 10));
        let drawn: Vec<_> = texts(&t).collect();
        assert!(drawn.contains(&(12, 0, "ABC", Style::Focus)));
    }

    // ── Scrolling - selected always visible ─────────────────────────────────────

    #[test]
    fn scroll_down_keeps_selection_visible() {
        let mut list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        list.view(&mut t, Area::new(0, 0, 120, 30)); // page_size ← 3

        list.update(&Msg::Down);
        list.update(&Msg::Down);
        assert_eq!(list.offset(), 0); // 0,1,2 still inside

        list.update(&Msg::Down); // selected → 3, window must advance
        assert_eq!(list.selected(), 3);
        assert_eq!(list.offset(), 1); // window [1,2,3]

        let mut t2 = RecordingTarget::new(120, 30);
        list.view(&mut t2, Area::new(0, 0, 120, 30));
        let drawn: Vec<_> = texts(&t2).collect();
        // Beta(row0), Gamma(row1), Delta(row2, selected).
        assert!(drawn.contains(&(12, 0, "Beta", Style::Muted)));
        assert!(drawn.contains(&(12, 10, "Gamma", Style::Muted)));
        assert!(drawn.contains(&(0, 20, "> ", Style::Focus)));
        assert!(drawn.contains(&(12, 20, "Delta", Style::Focus)));
    }

    #[test]
    fn scroll_up_retreats_window() {
        let mut list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        list.update(&Msg::Down);
        list.update(&Msg::Down);
        list.update(&Msg::Down);
        assert_eq!(list.offset(), 1);
        list.update(&Msg::Up); // 3→2, inside
        assert_eq!(list.offset(), 1);
        list.update(&Msg::Up); // 2→1, inside
        assert_eq!(list.offset(), 1);
        list.update(&Msg::Up); // 1→0, leaves top
        assert_eq!(list.selected(), 0);
        assert_eq!(list.offset(), 0);
    }

    #[test]
    fn scroll_clamped_at_both_ends() {
        let mut list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        for _ in 0..20 {
            list.update(&Msg::Down);
        }
        assert_eq!(list.selected(), 4);
        assert_eq!(list.offset(), 2); // window [2,3,4]
        for _ in 0..20 {
            list.update(&Msg::Up);
        }
        assert_eq!(list.selected(), 0);
        assert_eq!(list.offset(), 0);
    }

    // ── Scroll indicator ────────────────────────────────────────────────────────

    #[test]
    fn scroll_indicator_present_only_when_overflowing() {
        // 5 items, 3 visible → overflows → track + thumb fills present.
        let list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        let f: Vec<_> = fills(&t).collect();
        // Two fills: a Muted track and a Focus thumb, both at the right edge.
        assert!(f.iter().any(|&(a, st)| st == Style::Muted && a.w == TRACK_W && a.h == 30));
        let thumb = f.iter().find(|&&(_, st)| st == Style::Focus);
        let (ta, _) = thumb.expect("thumb fill present");
        assert_eq!(ta.w, SCROLLBAR_W);
        assert_eq!(ta.x, 120 - SCROLLBAR_W); // flush right
        // At offset 0 the thumb sits at the top.
        assert_eq!(ta.y, 0);
    }

    #[test]
    fn scroll_indicator_thumb_moves_down() {
        let mut list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        for _ in 0..4 {
            list.update(&Msg::Down); // to the last item, offset 2 (max)
        }
        let mut t2 = RecordingTarget::new(120, 30);
        list.view(&mut t2, Area::new(0, 0, 120, 30));
        let thumb_y = fills(&t2)
            .find(|&(_, st)| st == Style::Focus)
            .map(|(a, _)| a.y)
            .unwrap();
        // At max offset the thumb is flush with the bottom of the track.
        let thumb_h = (30 * 3 / 5).max(MIN_THUMB_PX as usize) as u16;
        assert_eq!(thumb_y, 30 - thumb_h);
    }

    #[test]
    fn no_indicator_when_everything_fits() {
        let list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 60); // 6 rows ≥ 5 items
        list.view(&mut t, Area::new(0, 0, 120, 60));
        assert_eq!(fills(&t).count(), 0);
    }

    // ── Edge cases & accessors ──────────────────────────────────────────────────

    #[test]
    fn empty_list_renders_nothing() {
        let list = List::new(&[] as &[&str]);
        let mut t = RecordingTarget::new(120, 40);
        list.view(&mut t, Area::new(0, 0, 120, 40));
        // Partial-redraw: view() clears its own area but draws no items.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }

    #[test]
    fn single_item_navigates_safely() {
        let mut list = List::new(&["Only"]);
        list.update(&Msg::Up);
        assert_eq!(list.selected(), 0);
        list.update(&Msg::Down);
        assert_eq!(list.selected(), 0);
        assert_eq!(list.selected_item(), "Only");
    }

    #[test]
    fn selected_item_tracks_selection() {
        let mut list = List::new(ITEMS);
        assert_eq!(list.selected_item(), "Alpha");
        list.update(&Msg::Down);
        assert_eq!(list.selected_item(), "Beta");
    }

    #[test]
    fn focus_blur_toggle() {
        let mut list = List::new(ITEMS);
        assert!(!list.focused);
        list.focus();
        assert!(list.focused);
        list.blur();
        assert!(!list.focused);
    }

    // ── Dirty contract / frame gate ───────────────────────────────────────────

    #[test]
    fn starts_dirty_then_cleans() {
        let list = List::new(ITEMS);
        // A fresh widget is dirty so the first frame always paints.
        assert!(list.dirty());
        list.mark_clean();
        assert!(!list.dirty());
    }

    #[test]
    fn state_change_marks_dirty() {
        let mut list = List::new(ITEMS);
        list.mark_clean();
        list.update(&Msg::Down); // selection moved
        assert!(list.dirty());
    }

    #[test]
    fn noop_update_stays_clean() {
        let mut list = List::new(ITEMS);
        list.mark_clean();
        // Select changes nothing in a List; Up at the top row is clamped.
        list.update(&Msg::Select);
        list.update(&Msg::Up);
        assert!(!list.dirty());
    }

    /// The frame gate: paint only when dirty. A clean component records no ops -
    /// exactly what lets the simulator skip the clear+view of an idle frame.
    #[test]
    fn gate_skips_paint_when_clean() {
        fn paint_if_dirty(c: &dyn Component, t: &mut RecordingTarget, area: Area) -> bool {
            if c.dirty() {
                c.view(t, area);
                c.mark_clean();
                true
            } else {
                false
            }
        }

        let mut list = List::new(ITEMS);
        let area = Area::new(0, 0, 120, 30);

        // First gated frame: dirty → it paints and records ops.
        let mut t1 = RecordingTarget::new(120, 30);
        assert!(paint_if_dirty(&list, &mut t1, area));
        assert!(!t1.ops().is_empty());

        // A no-op update leaves it clean → the gate skips, zero ops recorded.
        list.update(&Msg::Select);
        let mut t2 = RecordingTarget::new(120, 30);
        assert!(!paint_if_dirty(&list, &mut t2, area));
        assert!(t2.ops().is_empty());

        // A real move re-dirties → it paints again.
        list.update(&Msg::Down);
        let mut t3 = RecordingTarget::new(120, 30);
        assert!(paint_if_dirty(&list, &mut t3, area));
        assert!(!t3.ops().is_empty());
    }
}
