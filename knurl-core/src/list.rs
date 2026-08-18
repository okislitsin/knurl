use core::cell::Cell;

use crate::{
    Area, Component, DataGate, Marker, Msg, Outcome, RenderTarget, Style, V_SCROLL_RESERVE,
    draw_cursor_band, draw_v_scroll,
};

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

    /// A number that changes whenever the content does - how a [`List`] notices
    /// that its data moved without anyone telling it.
    ///
    /// This is the one question a widget can ask a model it does not own. The
    /// default is a **constant**, which says "I keep no revision": the
    /// comparison never fires, and the list repaints on its own state and when
    /// its owner calls [`mark_dirty`](Component::mark_dirty) - exactly the
    /// behaviour every model has today. A model that can spare one `u32` gets
    /// self-service instead, and nobody has to remember anything:
    ///
    /// ```
    /// # use core::cell::Cell;
    /// # use knurl_core::ListModel;
    /// struct Log { lines: [&'static str; 4], len: Cell<usize>, writes: Cell<u32> }
    ///
    /// impl Log {
    ///     fn push(&self, _line: &str) {
    ///         // ...store it, and say so:
    ///         self.writes.set(self.writes.get() + 1);
    ///     }
    /// }
    ///
    /// impl ListModel for Log {
    ///     fn item_count(&self) -> usize { self.len.get() }
    ///     fn get_item(&self, i: usize) -> &str { self.lines[i] }
    ///     fn revision(&self) -> u32 { self.writes.get() }
    /// }
    /// ```
    ///
    /// Only *different* counts, never *greater*: a model with nothing to count
    /// may hand back a cheap fingerprint of its content instead, and repeating
    /// an old value is fine. Wrapping a counter is fine too - it takes four
    /// billion writes to land back on the number that is on screen.
    ///
    /// See [`DataGate`](crate::DataGate), which is where a widget keeps the
    /// answer, and the [custom widget guide](crate::custom_widget).
    fn revision(&self) -> u32 {
        0
    }
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

// The built-in scroll indicator (a thin track + thumb at the right edge, drawn
// only when the list overflows) comes from the shared [`draw_v_scroll`] helper.
// The standalone `Scrollbar` widget (in the `info` module) is a separate
// component; here the indicator is baked directly into `List`.

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
/// input** - this is always in sync. Before the first frame it is `usize::MAX`
/// ("everything fits"), so an `update` that arrives ahead of any `view` moves
/// the cursor without scrolling the window under it.
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
    // paint. Starts dirty so the first frame always draws. It also carries the
    // model revision that was painted, so a model that keeps one moves the list
    // without being told (see `ListModel::revision`).
    gate: DataGate,
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
            gate: DataGate::new(),
        }
    }

    /// Sets the selection marker drawn beside each item.
    pub fn with_marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Index of the currently highlighted item.
    ///
    /// Clamped to the model **as it is now**: the data can shrink under the
    /// widget between two events (that is what
    /// [`revision`](ListModel::revision) exists for), and an index left past
    /// the end points at nothing - a cursor with no row under it, an empty
    /// `selected_item`, and several clicks before either comes back.
    pub fn selected(&self) -> usize {
        self.cursor()
    }

    /// Text of the currently highlighted item, or `""` for an empty list.
    pub fn selected_item(&self) -> &str {
        if self.model.item_count() == 0 {
            return "";
        }
        self.model.get_item(self.cursor())
    }

    /// First visible item index (scroll offset), clamped like
    /// [`selected`](List::selected).
    pub fn offset(&self) -> usize {
        self.window(self.page_size.get())
    }

    // ── Following a model that moved ──────────────────────────────────────

    /// The cursor, clamped to the model as it is now.
    fn cursor(&self) -> usize {
        self.selected.min(self.model.item_count().saturating_sub(1))
    }

    /// The scroll offset, clamped so a `visible`-row window still lands on the
    /// end of a model that shrank rather than past it.
    fn window(&self, visible: usize) -> usize {
        let n = self.model.item_count();
        self.offset.min(n.saturating_sub(visible.max(1)))
    }
}

impl<'a, M: ListModel + ?Sized> Component for List<'a, M> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        let n = self.model.item_count();
        if n == 0 {
            // Nothing to point at: every event stays available to the container.
            return Outcome::Ignored;
        }
        let page = self.page_size.get().max(1);
        // The model may have shrunk since the last event; catch the cursor up
        // before moving it, so `Up` from a stranded index steps off the last
        // row rather than through the rows that are no longer there.
        self.selected = self.cursor();
        self.offset = self.window(page);
        match msg {
            Msg::Down if self.selected + 1 < n => {
                self.selected += 1;
                // Scroll forward: keep the selection inside the window.
                if self.selected >= self.offset + page {
                    self.offset = self.selected + 1 - page;
                }
                self.gate.mark_dirty();
                Outcome::Consumed
            }
            Msg::Up if self.selected > 0 => {
                self.selected -= 1;
                // Scroll backward: keep the selection inside the window.
                if self.selected < self.offset {
                    self.offset = self.selected;
                }
                self.gate.mark_dirty();
                Outcome::Consumed
            }
            // Picking the highlighted item changes no pixel - the gate stays
            // clean - but it is the choice the app acts on. The caller reads
            // selected() to find out which.
            Msg::Select => Outcome::Activated,
            // A clamped end: the cursor has run out of list, so the event goes
            // back to the container to spend on the next focus zone.
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
        let visible = (area.h / line_h) as usize;

        // Cache the visible-row count for the next update() call.
        self.page_size.set(visible.max(1));

        let n = self.model.item_count();
        if area.w == 0 || area.h == 0 || visible == 0 || n == 0 {
            return;
        }
        // Read through the clamps: `draw` takes `&self`, so a model that shrank
        // since the last event is corrected for the picture here and written
        // back by the next `update`.
        let selected = self.cursor();
        let offset = self.window(visible);

        // Reserve space on the right for the scroll indicator only when needed.
        let overflowing = n > visible;
        let bar_w = if overflowing { V_SCROLL_RESERVE } else { 0 };
        let content_w = area.w.saturating_sub(bar_w);

        // The leftmost columns are reserved for the selection-marker prefix.
        let cw = target.char_width().max(1);
        let prefix_px = self.marker.width() as u16 * cw;
        let text_px = content_w.saturating_sub(prefix_px);
        let max_chars = (text_px / cw) as usize;

        for row in 0..visible {
            let item_idx = offset + row;
            if item_idx >= n {
                break;
            }

            let y = area.y.saturating_add(row as u16 * line_h);
            let is_sel = item_idx == selected;
            // Charm look: the rows around the cursor are dimmed (Muted). The
            // cursor row itself follows the focus language - a full-width band
            // when this list holds focus, a plain marked row when it does not.
            let style = if is_sel {
                draw_cursor_band(
                    target,
                    Area::new(area.x, y, content_w, line_h),
                    self.focused,
                )
            } else {
                Style::Muted
            };
            let prefix = if is_sel {
                self.marker.selected
            } else {
                self.marker.unselected
            };

            if !prefix.is_empty() {
                target.draw_text(area.x, y, prefix, style);
            }
            if max_chars > 0 {
                let text = truncate_str(self.model.get_item(item_idx), max_chars);
                target.draw_text(area.x.saturating_add(prefix_px), y, text, style);
            }
        }

        if overflowing {
            draw_v_scroll(target, area, n, visible, offset);
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.gate.mark_dirty();
    }

    fn blur(&mut self) {
        self.focused = false;
        self.gate.mark_dirty();
    }

    fn dirty(&self) -> bool {
        self.gate.is_dirty(self.model.revision())
    }

    fn mark_clean(&self) {
        self.gate.mark_clean(self.model.revision());
    }

    fn mark_dirty(&self) {
        self.gate.mark_dirty();
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

    // Geometry of the shared scroll indicator (see `crate::draw_v_scroll`), so
    // the expectations below read in the same terms as the helper.
    const SCROLLBAR_W: u16 = 3;
    const TRACK_W: u16 = 1;
    const MIN_THUMB_PX: u16 = 3;

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
        let mut list = List::new(&m);
        list.focus(); // the focus band is what makes the selected row Focus
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
        let mut list = List::new(ITEMS);
        list.focus();
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
        assert!(
            !drawn
                .iter()
                .any(|&(_, _, s, _)| s == "Delta" || s == "Epsilon")
        );
    }

    #[test]
    fn renders_at_nonzero_origin() {
        let mut list = List::new(ITEMS);
        list.focus();
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
        let mut list = List::new(ITEMS).with_marker(Marker::NONE);
        list.focus();
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
        let mut list = List::new(&["ABCDEFGH"]);
        list.focus();
        let mut t = RecordingTarget::new(30, 10);
        list.view(&mut t, Area::new(0, 0, 30, 10));
        let drawn: Vec<_> = texts(&t).collect();
        assert!(drawn.contains(&(12, 0, "ABC", Style::Focus)));
    }

    // ── Focus language (band vs bare cursor) ───────────────────────────────────

    fn bands(t: &RecordingTarget) -> impl Iterator<Item = (Area, Style)> + '_ {
        t.ops().iter().filter_map(|op| match op {
            Op::Band { area, style } => Some((*area, *style)),
            _ => None,
        })
    }

    /// A focused list bands its selected row across the full content width -
    /// marker, text and the empty space after it are one block. The band stops
    /// short of the scroll indicator's reserved column.
    #[test]
    fn focused_list_bands_the_selected_row() {
        let mut list = List::new(ITEMS);
        list.focus();
        let mut t = RecordingTarget::new(120, 30); // 3 rows, 5 items → overflow
        list.view(&mut t, Area::new(0, 0, 120, 30));

        let b: Vec<_> = bands(&t).collect();
        assert_eq!(
            b,
            [(Area::new(0, 0, 120 - V_SCROLL_RESERVE, 10), Style::Focus)],
            "exactly one band, on the selected row, clear of the scroll column"
        );
        // Everything on that row draws in the band's style.
        let drawn: Vec<_> = texts(&t).collect();
        assert!(drawn.contains(&(0, 0, "> ", Style::Focus)));
        assert!(drawn.contains(&(12, 0, "Alpha", Style::Focus)));
    }

    /// An unfocused list still shows where the cursor sits (the marker is drawn)
    /// but does not invert: no band, and the row is plain `Normal` against the
    /// `Muted` rows around it.
    #[test]
    fn unfocused_list_shows_the_marker_without_a_band() {
        let list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));

        assert_eq!(bands(&t).count(), 0, "an unfocused widget must not invert");
        let drawn: Vec<_> = texts(&t).collect();
        assert!(drawn.contains(&(0, 0, "> ", Style::Normal)));
        assert!(drawn.contains(&(12, 0, "Alpha", Style::Normal)));
        assert!(drawn.contains(&(12, 10, "Beta", Style::Muted)));
    }

    /// The band follows the cursor, and blurring takes it away again.
    #[test]
    fn band_follows_the_selection_and_leaves_on_blur() {
        let mut list = List::new(ITEMS);
        list.focus();
        let mut t0 = RecordingTarget::new(120, 30);
        list.view(&mut t0, Area::new(0, 0, 120, 30));
        let _ = list.update(&Msg::Down);

        let mut t1 = RecordingTarget::new(120, 30);
        list.view(&mut t1, Area::new(0, 0, 120, 30));
        assert_eq!(bands(&t1).map(|(a, _)| a.y).next(), Some(10));

        list.blur();
        let mut t2 = RecordingTarget::new(120, 30);
        list.view(&mut t2, Area::new(0, 0, 120, 30));
        assert_eq!(bands(&t2).count(), 0);
    }

    // ── Scrolling - selected always visible ─────────────────────────────────────

    #[test]
    fn scroll_down_keeps_selection_visible() {
        let mut list = List::new(ITEMS);
        list.focus();
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        list.view(&mut t, Area::new(0, 0, 120, 30)); // page_size ← 3

        let _ = list.update(&Msg::Down);
        let _ = list.update(&Msg::Down);
        assert_eq!(list.offset(), 0); // 0,1,2 still inside

        let _ = list.update(&Msg::Down); // selected → 3, window must advance
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
        let _ = list.update(&Msg::Down);
        let _ = list.update(&Msg::Down);
        let _ = list.update(&Msg::Down);
        assert_eq!(list.offset(), 1);
        let _ = list.update(&Msg::Up); // 3→2, inside
        assert_eq!(list.offset(), 1);
        let _ = list.update(&Msg::Up); // 2→1, inside
        assert_eq!(list.offset(), 1);
        let _ = list.update(&Msg::Up); // 1→0, leaves top
        assert_eq!(list.selected(), 0);
        assert_eq!(list.offset(), 0);
    }

    #[test]
    fn scroll_clamped_at_both_ends() {
        let mut list = List::new(ITEMS);
        let mut t = RecordingTarget::new(120, 30);
        list.view(&mut t, Area::new(0, 0, 120, 30));
        for _ in 0..20 {
            let _ = list.update(&Msg::Down);
        }
        assert_eq!(list.selected(), 4);
        assert_eq!(list.offset(), 2); // window [2,3,4]
        for _ in 0..20 {
            let _ = list.update(&Msg::Up);
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
        assert!(
            f.iter()
                .any(|&(a, st)| st == Style::Muted && a.w == TRACK_W && a.h == 30)
        );
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
            let _ = list.update(&Msg::Down); // to the last item, offset 2 (max)
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

    /// A window too narrow for the indicator band must not panic (a u16
    /// underflow on `area.w - band`) nor draw outside the area. Every case here
    /// overflows (5 items, at most 3 rows), so the indicator path always runs.
    #[test]
    fn tiny_area_draws_nothing_outside_and_never_panics() {
        for w in [0u16, 1, 2, 3, 4, 5, 12] {
            for h in [0u16, 1, 9, 10, 30] {
                let list = List::new(ITEMS);
                let mut t = RecordingTarget::new(128, 64);
                let area = Area::new(0, 0, w, h);
                list.view(&mut t, area);
                for (a, _) in fills(&t) {
                    assert!(
                        u32::from(a.x) + u32::from(a.w) <= u32::from(area.x) + u32::from(area.w)
                            && u32::from(a.y) + u32::from(a.h)
                                <= u32::from(area.y) + u32::from(area.h),
                        "fill {a:?} escapes {area:?}"
                    );
                }
            }
        }
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
        let _ = list.update(&Msg::Up);
        assert_eq!(list.selected(), 0);
        let _ = list.update(&Msg::Down);
        assert_eq!(list.selected(), 0);
        assert_eq!(list.selected_item(), "Only");
    }

    #[test]
    fn selected_item_tracks_selection() {
        let mut list = List::new(ITEMS);
        assert_eq!(list.selected_item(), "Alpha");
        let _ = list.update(&Msg::Down);
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
        let _ = list.update(&Msg::Down); // selection moved
        assert!(list.dirty());
    }

    #[test]
    fn noop_update_stays_clean() {
        let mut list = List::new(ITEMS);
        list.mark_clean();
        // Select changes nothing in a List; Up at the top row is clamped.
        let _ = list.update(&Msg::Select);
        let _ = list.update(&Msg::Up);
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
        let _ = list.update(&Msg::Select);
        let mut t2 = RecordingTarget::new(120, 30);
        assert!(!paint_if_dirty(&list, &mut t2, area));
        assert!(t2.ops().is_empty());

        // A real move re-dirties → it paints again.
        let _ = list.update(&Msg::Down);
        let mut t3 = RecordingTarget::new(120, 30);
        assert!(paint_if_dirty(&list, &mut t3, area));
        assert!(!t3.ops().is_empty());
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    /// The contract every focus container will stand on: a step inside the list
    /// is spent here, a step off either end comes back unspent.
    #[test]
    fn list_spends_a_step_and_hands_back_an_edge() {
        let mut list = List::new(ITEMS);
        assert_eq!(
            list.update(&Msg::Up),
            Outcome::Ignored,
            "Up at the first item"
        );
        assert_eq!(
            list.update(&Msg::Down),
            Outcome::Consumed,
            "a step in the middle"
        );
        while list.selected() + 1 < ITEMS.len() {
            assert_eq!(list.update(&Msg::Down), Outcome::Consumed);
        }
        assert_eq!(
            list.update(&Msg::Down),
            Outcome::Ignored,
            "Down at the last item"
        );
    }

    #[test]
    fn list_select_activates_and_a_tick_is_not_its_event() {
        let mut list = List::new(ITEMS);
        let _ = list.update(&Msg::Down);
        assert_eq!(list.update(&Msg::Select), Outcome::Activated);
        assert_eq!(list.selected(), 1, "Select picks, it does not move");
        assert_eq!(list.update(&Msg::Tick), Outcome::Ignored);
    }

    #[test]
    fn empty_list_hands_back_everything() {
        let none: &[&str] = &[];
        let mut list = List::new(none);
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Tick, Msg::Char('a')] {
            assert_eq!(
                list.update(&msg),
                Outcome::Ignored,
                "{msg:?} on an empty list"
            );
        }
    }

    // ── A model that keeps a revision ───────────────────────────────────────

    /// A store the application writes to behind a shared reference - the only
    /// shape in which a list's data can change under it.
    struct Feed {
        items: [&'static str; 3],
        len: Cell<usize>,
        writes: Cell<u32>,
    }

    impl Feed {
        fn new(len: usize) -> Self {
            Self {
                items: ["Alpha", "Beta", "Gamma"],
                len: Cell::new(len),
                writes: Cell::new(0),
            }
        }
        /// What an application does: change the data, and say so.
        fn grow(&self) {
            self.len.set(self.len.get() + 1);
            self.writes.set(self.writes.get() + 1);
        }
    }

    impl ListModel for Feed {
        fn item_count(&self) -> usize {
            self.len.get()
        }
        fn get_item(&self, i: usize) -> &str {
            self.items[i]
        }
        fn revision(&self) -> u32 {
            self.writes.get()
        }
    }

    /// The point of the revision: nobody calls anything on the widget, and the
    /// new row is on the panel on the next frame.
    #[test]
    fn a_list_repaints_when_its_model_says_the_data_moved() {
        let feed = Feed::new(1);
        let list = List::new(&feed);
        let area = Area::new(0, 0, 120, 30);
        let mut t = RecordingTarget::new(128, 64);

        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area), "the first paint");
        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None, "nothing moved, nothing to send");

        feed.grow();
        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area), "the new row was drawn");
        assert_eq!(
            texts(&t).filter(|(_, _, s, _)| *s == "Beta").count(),
            1,
            "…and it is the row that arrived"
        );

        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None, "and it settles again");
    }

    /// A model can shrink under the widget, and before this the cursor stayed
    /// where it was: `selected` reported a row that no longer existed, nothing
    /// was drawn under it, and the user had to rotate past the missing rows
    /// before it reappeared. It follows the data now, in the picture and in the
    /// getters, and the next event steps off the real last row.
    #[test]
    fn the_cursor_follows_a_model_that_shrank_under_it() {
        let feed = Feed::new(3);
        let mut list = List::new(&feed);
        let area = Area::new(0, 0, 120, 30); // three rows
        let mut t = RecordingTarget::new(128, 64);

        let _ = list.update(&Msg::Down);
        let _ = list.update(&Msg::Down);
        list.view(&mut t, area);
        assert_eq!(list.selected(), 2);

        feed.len.set(1);
        feed.writes.set(1);
        assert_eq!(list.selected(), 0, "the cursor followed the data");
        assert_eq!(list.selected_item(), "Alpha");
        assert_eq!(list.offset(), 0);

        // ...and the row is actually painted under it.
        let mut after = RecordingTarget::new(128, 64);
        list.view(&mut after, area);
        assert!(
            after
                .ops()
                .iter()
                .any(|op| matches!(op, Op::Band { .. } | Op::Text { .. })),
            "nothing under the cursor"
        );
        // The next event steps off the last real row, not through the gap.
        assert_eq!(
            list.update(&Msg::Up),
            Outcome::Ignored,
            "already at the top"
        );
        assert_eq!(list.update(&Msg::Down), Outcome::Ignored, "and at the end");
    }

    /// A model with no revision of its own keeps exactly the behaviour it had:
    /// the list gates on its own state, and its owner is what says otherwise.
    #[test]
    fn a_model_without_a_revision_costs_nothing_and_changes_nothing() {
        let list = List::new(ITEMS);
        let area = Area::new(0, 0, 120, 30);
        let mut t = RecordingTarget::new(128, 64);
        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area));
        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None);
        list.mark_dirty();
        list.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area));
    }
}
