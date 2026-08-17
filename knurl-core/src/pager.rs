use core::cell::Cell;
use core::fmt::Write;

use crate::{
    Area, Component, DataGate, Msg, Outcome, RenderTarget, Scrollbar, Style, V_SCROLL_RESERVE,
};

// ── LinesModel (data provider for text) ─────────────────────────────────────

/// The text behind a [`Pager`] (mirrors [`ListModel`](crate::ListModel)): a line
/// count and per-line access.
///
/// `get_line` returns a borrowed `&str` - fine for *stored* lines (e.g. a ring
/// buffer of recent UART lines). Content that is **generated on demand** can't
/// return a borrow, so it overrides [`write_line`](LinesModel::write_line) to
/// stream the line into a writer instead; the [`Pager`] always renders through
/// `write_line`, so a generated model can leave `get_line` returning `""`.
pub trait LinesModel {
    /// Number of lines.
    fn line_count(&self) -> usize;
    /// The text of line `i` (callers only index `0..line_count()`).
    fn get_line(&self, i: usize) -> &str;
    /// Writes line `i` into `out`. The default forwards [`get_line`]; models that
    /// generate or borrow-internally (a `RefCell` ring buffer) override this so
    /// they need not return a borrow.
    ///
    /// [`get_line`]: LinesModel::get_line
    fn write_line(&self, i: usize, out: &mut dyn Write) {
        let _ = out.write_str(self.get_line(i));
    }

    /// A number that changes whenever the text does - the same self-service a
    /// [`ListModel`](crate::ListModel::revision) offers, and the one that
    /// matters most here: a log gains a line without any input at all, and a
    /// [`Pager`] not in follow mode has nothing else to notice it by.
    ///
    /// The default is a constant ("I keep no revision"), so a `const` array of
    /// lines costs nothing and behaves as before.
    fn revision(&self) -> u32 {
        0
    }
}

/// Backwards-compatible static impl: a `&[&str]` is the simplest line source.
impl LinesModel for [&str] {
    fn line_count(&self) -> usize {
        self.len()
    }
    fn get_line(&self, i: usize) -> &str {
        self[i]
    }
}

/// Array impl so an inline literal works directly under the generic `M`.
impl<const N: usize> LinesModel for [&str; N] {
    fn line_count(&self) -> usize {
        N
    }
    fn get_line(&self, i: usize) -> &str {
        self[i]
    }
}

// ── Stack line buffer ───────────────────────────────────────────────────────

/// Max bytes captured from one line for rendering (longer lines are clipped at a
/// char boundary - only the *display* is bounded; the model is untouched).
const LINE_CAP: usize = 256;

/// A `core::fmt::Write` sink into a fixed stack buffer - captures a model line as
/// a `&str` for drawing, with no heap. Copies whole chars only, so the buffer is
/// always valid UTF-8.
struct LineBuf {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl LineBuf {
    fn new() -> Self {
        Self {
            buf: [0; LINE_CAP],
            len: 0,
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Write for LineBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            let cl = c.len_utf8();
            if self.len + cl <= LINE_CAP {
                c.encode_utf8(&mut self.buf[self.len..]);
                self.len += cl;
            } else {
                break;
            }
        }
        Ok(())
    }
}

/// A `Write` sink that only counts chars - for measuring a line's wrapped height
/// without buffering it (so length is not bounded by [`LINE_CAP`]).
struct CharCounter(usize);

impl Write for CharCounter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0 += s.chars().count();
        Ok(())
    }
}

/// Returns the `[start, end)` char range of `s` as a `&str` (char-boundary safe).
fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let b0 = s
        .char_indices()
        .nth(start)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let b1 = s.char_indices().nth(end).map(|(i, _)| i).unwrap_or(s.len());
    &s[b0..b1]
}

// ── Pager ───────────────────────────────────────────────────────────────────

/// A scrollable read-only viewport over many lines of text - knurl's answer to
/// Bubble Tea's `viewport`/pager, and the showcase for the provider pattern over
/// **real-time data** (its motivating case is viewing live UART output).
///
/// - Backed by a [`LinesModel`] (`&'a M`), like [`List`](crate::List).
/// - Vertical scroll by line; **long lines wrap** to the area width (never
///   truncated - the "don't truncate" law).
/// - A built-in pixel scroll indicator ([`Scrollbar`]) appears on overflow.
/// - **Follow / tail mode**: when on, the view stays pinned to the bottom as the
///   model grows (`tail -f`). Scrolling `Up` disengages follow; scrolling back
///   `Down` to the bottom re-engages it.
///
/// All state is stack-only (offset, follow flag, cached page/wrap dimensions).
///
/// ## Focus
/// A pager has **no cursor** - it scrolls, it does not select - so the focus
/// language (a band under the cursor row, see
/// [`draw_cursor_band`](crate::draw_cursor_band)) has nothing to highlight here,
/// and the widget keeps no `focused` flag: `focus()`/`blur()` are the trait's
/// no-ops. Which widget the encoder drives is the composing screen's business,
/// and it is the composition layer that will show it.
pub struct Pager<'a, M: LinesModel + ?Sized = [&'a str]> {
    model: &'a M,
    offset: usize,
    follow: bool,
    // Cached from view() so the next update() can clamp/tail without the target.
    page_rows: Cell<usize>,
    wrap_cols: Cell<usize>,
    // Repaint gate: set when the offset or the follow flag actually moves.
    // Starts dirty so the first frame always draws. It also carries the model
    // revision that was painted - which is the only way a pager sitting still
    // over a growing log notices the new lines (see `LinesModel::revision`).
    gate: DataGate,
}

impl<'a, M: LinesModel + ?Sized> Pager<'a, M> {
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            offset: 0,
            follow: false,
            page_rows: Cell::new(usize::MAX),
            wrap_cols: Cell::new(usize::MAX),
            gate: DataGate::new(),
        }
    }

    /// Starts in follow/tail mode (pinned to the bottom as the model grows).
    pub fn with_follow(mut self, follow: bool) -> Self {
        self.follow = follow;
        self
    }

    /// Enables or disables follow/tail mode.
    pub fn set_follow(&mut self, follow: bool) {
        self.follow = follow;
    }

    /// Whether follow/tail mode is engaged.
    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// First visible line index (scroll offset).
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Total number of lines.
    pub fn len(&self) -> usize {
        self.model.line_count()
    }

    /// Whether the pager holds no lines.
    pub fn is_empty(&self) -> bool {
        self.model.line_count() == 0
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// Number of wrapped display rows line `i` occupies at `cols` width (a blank
    /// line still occupies one row).
    fn line_rows(&self, i: usize, cols: usize) -> usize {
        let mut c = CharCounter(0);
        self.model.write_line(i, &mut c);
        let chars = c.0.max(1);
        chars.div_ceil(cols.max(1))
    }

    /// Largest `offset` that still pins the final line's bottom to the view
    /// bottom, accounting for wrapping. Uses the cached page/wrap dimensions.
    fn max_offset(&self) -> usize {
        let cols = self.wrap_cols.get().max(1);
        let rows = self.page_rows.get().max(1);
        let n = self.model.line_count();
        if n == 0 {
            return 0;
        }
        let mut acc = 0;
        let mut i = n;
        while i > 0 {
            let r = self.line_rows(i - 1, cols);
            if acc + r > rows {
                break;
            }
            acc += r;
            i -= 1;
        }
        i.min(n - 1)
    }

    /// Total wrapped rows of all lines, and the wrapped rows before `offset`.
    fn row_geometry(&self, cols: usize) -> (usize, usize) {
        let n = self.model.line_count();
        let mut total = 0;
        let mut before = 0;
        for i in 0..n {
            let r = self.line_rows(i, cols);
            if i < self.offset {
                before += r;
            }
            total += r;
        }
        (total, before)
    }
}

impl<'a, M: LinesModel + ?Sized> Component for Pager<'a, M> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        // Tail mode ticks constantly, so the gate has to be honest about it: a
        // tick that drags the view down repaints, one that finds no new lines
        // does not - otherwise a UART pager would repaint on every tick forever.
        let before = (self.offset, self.follow);
        let max = self.max_offset();
        if self.offset > max {
            self.offset = max;
        }
        // The outcome is measured from *after* the clamp: re-clamping onto a
        // shrunken log is housekeeping the pager does whatever arrives, not
        // something the incoming event paid for.
        let settled = (self.offset, self.follow);
        // Only these three are the pager's to spend. Anything else (a Char, a
        // Select) is handed straight back, however the log moved underneath.
        let addressed = matches!(msg, Msg::Up | Msg::Down | Msg::Tick);
        match msg {
            Msg::Up => {
                self.follow = false;
                self.offset = self.offset.saturating_sub(1);
            }
            Msg::Down => {
                if self.offset < max {
                    self.offset += 1;
                }
                // Reaching the bottom re-engages tail mode.
                if max > 0 && self.offset >= max {
                    self.follow = true;
                }
            }
            _ => {}
        }
        // While following, stay pinned to the (possibly grown) bottom.
        if self.follow {
            self.offset = self.max_offset();
        }
        if (self.offset, self.follow) != before {
            self.gate.mark_dirty();
        }
        // Scrolling that moved the view - or dropped out of / re-entered tail
        // mode - used the event up; a `Tick` counts only while following, and
        // only when new lines actually dragged the view down. At the top or the
        // bottom with nothing left to do, the event goes back to the container.
        if addressed && (self.offset, self.follow) != settled {
            Outcome::Consumed
        } else {
            Outcome::Ignored
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let cw = target.char_width().max(1);
        let rows = (area.h / line_h) as usize;
        self.page_rows.set(rows.max(1));

        let n = self.model.line_count();
        if area.w == 0 || area.h == 0 || rows == 0 || n == 0 {
            self.wrap_cols.set(((area.w / cw) as usize).max(1));
            return;
        }

        // Decide overflow at full width; reserve for the scrollbar only if it
        // overflows (narrowing only ever increases wrapping, so the decision is
        // stable).
        let full_cols = ((area.w / cw) as usize).max(1);
        let (total_full, _) = self.row_geometry(full_cols);
        let bar = total_full > rows && area.w >= V_SCROLL_RESERVE;
        let cols = if bar {
            (((area.w - V_SCROLL_RESERVE) / cw) as usize).max(1)
        } else {
            full_cols
        };
        self.wrap_cols.set(cols);

        // Render wrapped lines from `offset` until the view fills.
        let mut row = 0usize;
        let mut idx = self.offset;
        while row < rows && idx < n {
            let mut lb = LineBuf::new();
            self.model.write_line(idx, &mut lb);
            let s = lb.as_str();
            let total_chars = s.chars().count();
            if total_chars == 0 {
                row += 1; // blank line keeps its row
            } else {
                let mut done = 0;
                while done < total_chars && row < rows {
                    let end = (done + cols).min(total_chars);
                    let y = area.y.saturating_add(row as u16 * line_h);
                    target.draw_text(area.x, y, char_slice(s, done, end), Style::Normal);
                    done = end;
                    row += 1;
                }
            }
            idx += 1;
        }

        if bar {
            let (total, before) = self.row_geometry(cols);
            let mut sb = Scrollbar::new();
            sb.set(total, rows, before);
            sb.view(target, Area::new(area.x + area.w - 3, area.y, 3, area.h));
        }
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

    const LINES: &[&str] = &[
        "line one",
        "line two",
        "line three",
        "line four",
        "line five",
        "line six",
    ];

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.

    fn texts(t: &RecordingTarget) -> Vec<(u16, u16, alloc::string::String)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
                _ => None,
            })
            .collect()
    }

    fn fills(t: &RecordingTarget) -> usize {
        t.ops()
            .iter()
            .filter(|op| matches!(op, Op::Fill { .. }))
            .count()
    }

    #[test]
    fn renders_from_top() {
        let p = Pager::new(LINES);
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        p.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        assert!(tx.contains(&(0, 0, "line one".into())));
        assert!(tx.contains(&(0, 10, "line two".into())));
        assert!(tx.contains(&(0, 20, "line three".into())));
    }

    #[test]
    fn scroll_down_advances_and_clamps() {
        let mut p = Pager::new(LINES); // 6 lines
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        p.view(&mut t, Area::new(0, 0, 120, 30)); // caches rows=3, cols=20
        let _ = p.update(&Msg::Down);
        assert_eq!(p.offset(), 1);
        for _ in 0..20 {
            let _ = p.update(&Msg::Down);
        }
        // max_offset = 6 - 3 = 3 (each short line is one row).
        assert_eq!(p.offset(), 3);
    }

    #[test]
    fn scroll_up_clamps_at_top() {
        let mut p = Pager::new(LINES);
        let mut t = RecordingTarget::new(120, 30);
        p.view(&mut t, Area::new(0, 0, 120, 30));
        let _ = p.update(&Msg::Down);
        let _ = p.update(&Msg::Down);
        for _ in 0..10 {
            let _ = p.update(&Msg::Up);
        }
        assert_eq!(p.offset(), 0);
    }

    #[test]
    fn long_line_wraps_to_width() {
        // One 25-char line, width 60px / 6 = 10 cols → 3 wrapped rows.
        let long: &[&str] = &["0123456789abcdefghijABCDE"];
        let p = Pager::new(long);
        let mut t = RecordingTarget::new(60, 40); // 4 rows, no overflow (3 ≤ 4)
        p.view(&mut t, Area::new(0, 0, 60, 40));
        let tx = texts(&t);
        // Three chunks of ≤10 chars on consecutive rows - nothing truncated.
        assert!(tx.contains(&(0, 0, "0123456789".into())));
        assert!(tx.contains(&(0, 10, "abcdefghij".into())));
        assert!(tx.contains(&(0, 20, "ABCDE".into())));
    }

    #[test]
    fn scrollbar_shown_on_overflow_only() {
        let p = Pager::new(LINES); // 6 lines into 3 rows → overflow
        let mut t = RecordingTarget::new(120, 30);
        p.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(fills(&t) >= 2); // scrollbar track + thumb

        let two: &[&str] = &["a", "b"];
        let p2 = Pager::new(two);
        let mut t2 = RecordingTarget::new(120, 40); // fits
        p2.view(&mut t2, Area::new(0, 0, 120, 40));
        assert_eq!(fills(&t2), 0);
    }

    // ── Follow / tail mode (the UART case) ──────────────────────────────────────

    /// A growing line source - `line_count` can be bumped between frames, as a
    /// ring buffer of UART lines would grow.
    struct Stream {
        count: Cell<usize>,
    }
    impl LinesModel for Stream {
        fn line_count(&self) -> usize {
            self.count.get()
        }
        fn get_line(&self, _i: usize) -> &str {
            "log line"
        }
    }

    #[test]
    fn follow_pins_to_bottom_as_it_grows() {
        let s = Stream {
            count: Cell::new(10),
        };
        let mut p = Pager::new(&s).with_follow(true);
        let mut t = RecordingTarget::new(60, 30); // 3 rows, "log line" = 8 ≤ 10 cols
        p.view(&mut t, Area::new(0, 0, 60, 30)); // cache rows=3
        let _ = p.update(&Msg::Tick); // follow → pin to bottom
        assert_eq!(p.offset(), 7); // 10 - 3

        // The stream grows by 5 lines, then a tick.
        s.count.set(15);
        let _ = p.update(&Msg::Tick);
        assert_eq!(p.offset(), 12); // 15 - 3 - still pinned to the new bottom
        assert!(p.is_following());
    }

    #[test]
    fn scrolling_up_disengages_follow() {
        let s = Stream {
            count: Cell::new(10),
        };
        let mut p = Pager::new(&s).with_follow(true);
        let mut t = RecordingTarget::new(60, 30);
        p.view(&mut t, Area::new(0, 0, 60, 30));
        let _ = p.update(&Msg::Tick);
        assert_eq!(p.offset(), 7);
        let _ = p.update(&Msg::Up); // leaves tail
        assert!(!p.is_following());
        assert_eq!(p.offset(), 6);
        // Growth no longer drags the view down.
        s.count.set(20);
        let _ = p.update(&Msg::Tick);
        assert_eq!(p.offset(), 6);
    }

    #[test]
    fn empty_pager_safe() {
        let mut p = Pager::new(&[] as &[&str]);
        let _ = p.update(&Msg::Down);
        let _ = p.update(&Msg::Up);
        assert_eq!(p.offset(), 0);
        assert!(p.is_empty());
        let mut t = RecordingTarget::new(60, 30);
        p.view(&mut t, Area::new(0, 0, 60, 30));
        // Partial-redraw: view() clears its own area but draws no lines.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn pager_dirty_gate() {
        let mut p = Pager::new(LINES);
        let mut t = RecordingTarget::new(120, 30); // 3 rows
        p.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(!p.dirty(), "view() leaves it clean");

        let _ = p.update(&Msg::Up); // already at the top, and follow is already off
        let _ = p.update(&Msg::Tick);
        assert!(!p.dirty());

        let _ = p.update(&Msg::Down); // scrolls
        assert!(p.dirty());
        p.mark_clean();

        for _ in 0..10 {
            let _ = p.update(&Msg::Down); // reaches the bottom, engaging follow
        }
        p.mark_clean();
        let _ = p.update(&Msg::Down); // pinned at the bottom, nothing moves
        assert!(!p.dirty());

        p.mark_dirty();
        assert!(p.dirty());
    }

    /// Tail mode: a tick that drags the view down must dirty; one that does not
    /// must leave it clean, or a UART pager would repaint every tick forever.
    #[test]
    fn pager_follow_dirties_only_when_the_bottom_moves() {
        let s = Stream {
            count: Cell::new(10),
        };
        let mut p = Pager::new(&s).with_follow(true);
        let mut t = RecordingTarget::new(60, 30);
        p.view(&mut t, Area::new(0, 0, 60, 30));
        let _ = p.update(&Msg::Tick); // pins to the bottom
        p.mark_clean();

        let _ = p.update(&Msg::Tick); // nothing new arrived
        assert!(!p.dirty());

        s.count.set(15); // five new lines
        let _ = p.update(&Msg::Tick);
        assert!(p.dirty());
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    #[test]
    fn pager_spends_a_scroll_and_hands_back_an_edge() {
        let mut p = Pager::new(LINES);
        let mut t = RecordingTarget::new(120, 30); // 3 of 6 lines
        p.view(&mut t, Area::new(0, 0, 120, 30));

        assert_eq!(p.update(&Msg::Up), Outcome::Ignored, "Up at the top");
        assert_eq!(p.update(&Msg::Down), Outcome::Consumed, "a line of scroll");
        while !p.is_following() {
            assert_eq!(p.update(&Msg::Down), Outcome::Consumed);
        }
        assert_eq!(p.update(&Msg::Down), Outcome::Ignored, "Down at the bottom");
    }

    /// Tail mode is the one place a `Tick` is the pager's to spend: it is
    /// Consumed only when new lines actually dragged the view down.
    #[test]
    fn pager_tick_counts_only_while_the_tail_moves() {
        let s = Stream {
            count: Cell::new(10),
        };
        let mut p = Pager::new(&s).with_follow(true);
        let mut t = RecordingTarget::new(60, 30);
        p.view(&mut t, Area::new(0, 0, 60, 30));
        let _ = p.update(&Msg::Tick); // pins to the bottom

        assert_eq!(
            p.update(&Msg::Tick),
            Outcome::Ignored,
            "nothing new arrived"
        );
        s.count.set(15); // five new lines
        assert_eq!(p.update(&Msg::Tick), Outcome::Consumed, "the tail moved");

        // A Tick outside tail mode is nobody's: it belongs to the container.
        let mut still = Pager::new(LINES);
        let mut t2 = RecordingTarget::new(120, 30);
        still.view(&mut t2, Area::new(0, 0, 120, 30));
        assert_eq!(still.update(&Msg::Tick), Outcome::Ignored);
    }

    // ── A log that keeps a revision ─────────────────────────────────────────

    /// The same live stream, now saying when it grew.
    struct Log {
        count: Cell<usize>,
    }

    impl LinesModel for Log {
        fn line_count(&self) -> usize {
            self.count.get()
        }
        fn get_line(&self, _i: usize) -> &str {
            "log line"
        }
        // The line count *is* the revision here - nothing extra to store.
        fn revision(&self) -> u32 {
            self.count.get() as u32
        }
    }

    /// The case the revision exists for. A log short enough to fit has nothing
    /// to scroll, so follow mode never moves the offset and the pager's own
    /// state never changes - and before the revision, a line appended here was
    /// simply never drawn.
    #[test]
    fn a_pager_shows_a_new_line_that_moved_nothing_else() {
        let log = Log {
            count: Cell::new(1),
        };
        let p = Pager::new(&log).with_follow(true);
        let area = Area::new(0, 0, 60, 30); // 3 rows for 1 line
        let mut t = RecordingTarget::new(64, 32);

        p.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area));
        assert_eq!(texts(&t).len(), 1);

        p.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None, "an idle log costs nothing");

        log.count.set(2); // a line arrives; the offset stays at 0
        let mut after = RecordingTarget::new(64, 32);
        p.view(&mut after, area);
        assert!(
            after.take_dirty_rect().is_some(),
            "the new line was never drawn"
        );
        assert_eq!(texts(&after).len(), 2, "both lines on the panel");
        assert_eq!(p.offset(), 0, "and nothing scrolled to make it happen");
    }
}
