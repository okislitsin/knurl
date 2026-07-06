use core::cell::Cell;

use crate::{Area, Component, Msg, RenderTarget, Style, draw_v_scroll};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Formats `v` as a decimal string into a stack buffer - no allocation.
fn fmt_u16(buf: &mut [u8; 6], mut v: u16) -> &str {
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    } else {
        while v > 0 {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap()
}

// ── Spinner ─────────────────────────────────────────────────────────────────

/// Frame sets for [`Spinner`], matching common terminal loaders (Bubble Tea
/// `bubbles` / ratatui throbber).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinnerStyle {
    /// `| / - \` - the classic ASCII spinner (glyphs render directly).
    #[default]
    Line,
    /// `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` - Braille dots (pixel-drawn as a dot matrix).
    Braille,
    /// `█▓▒░` - a pulsing block (pixel-drawn as a shrinking square).
    Pulse,
    /// `▰▱` - a two-step meter (pixel-drawn as a block).
    Meter,
}

impl SpinnerStyle {
    /// The frame characters for this style, in animation order.
    pub const fn frames(self) -> &'static str {
        match self {
            SpinnerStyle::Line => "|/-\\",
            SpinnerStyle::Braille => "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏",
            SpinnerStyle::Pulse => "█▓▒░",
            SpinnerStyle::Meter => "▰▱",
        }
    }
}

/// An animated activity indicator that advances one frame per [`Msg::Tick`].
///
/// The frame characters come from a [`SpinnerStyle`]; on a pixel target the
/// renderer pixel-draws each frame (Braille dot matrix, pulsing block) so it
/// reads like a terminal loader even though the ASCII font lacks those glyphs.
#[derive(Debug)]
pub struct Spinner {
    style: SpinnerStyle,
    frame: usize,
    label: Option<&'static str>,
    // Repaint gate: set on each tick that advances the frame, cleared after a
    // paint. Starts dirty so the first frame always draws.
    dirty: Cell<bool>,
}

impl Spinner {
    pub const fn new() -> Self {
        Self { style: SpinnerStyle::Line, frame: 0, label: None, dirty: Cell::new(true) }
    }

    pub const fn with_style(mut self, style: SpinnerStyle) -> Self {
        self.style = style;
        self.frame = 0;
        self
    }

    pub const fn with_label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }

    /// The current frame character.
    fn frame_char(&self) -> char {
        self.style.frames().chars().nth(self.frame).unwrap_or(' ')
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Spinner {
    fn update(&mut self, msg: &Msg) {
        if let Msg::Tick = msg {
            let n = self.style.frames().chars().count();
            if n > 0 {
                self.frame = (self.frame + 1) % n;
                // A tick that advances the frame is a visible change → repaint.
                self.dirty.set(true);
            }
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);
        target.draw_spinner(Area::new(area.x, area.y, cw, area.h), self.frame_char(), Style::Accent);
        if let Some(lbl) = self.label {
            let label_x = area.x.saturating_add(2 * cw);
            if label_x < area.x + area.w {
                target.draw_text(label_x, area.y, lbl, Style::Normal);
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

// ── ProgressBar ───────────────────────────────────────────────────────────────

/// A smooth pixel progress bar filling the full width of its area.
///
/// Renders via [`draw_bar`](RenderTarget::draw_bar), which insets the fill
/// vertically - so progress bars stacked on adjacent rows keep a gap and do not
/// visually merge.
#[derive(Debug)]
pub struct ProgressBar {
    value: u16,
    max: u16,
}

impl ProgressBar {
    pub const fn new() -> Self {
        Self { value: 0, max: 100 }
    }

    pub const fn with_max(mut self, max: u16) -> Self {
        self.max = max;
        self
    }

    pub fn value(&self) -> u16 {
        self.value
    }

    /// Sets the value, clamped into `0..=max`.
    pub fn set_value(&mut self, v: u16) {
        self.value = v.min(self.max);
    }

    fn permille(&self) -> u16 {
        if self.max == 0 {
            0
        } else {
            (self.value as u32 * 1000 / self.max as u32) as u16
        }
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ProgressBar {
    fn update(&mut self, _msg: &Msg) {
        // Driven externally via set_value().
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        target.draw_bar(area, self.permille(), Style::Accent);
    }
}

// ── LineGauge ─────────────────────────────────────────────────────────────────

/// A compact single-row gauge: a smooth fill line with the percentage
/// right-aligned, e.g. `▆▆▆▆      80%`.
#[derive(Debug)]
pub struct LineGauge {
    value: u16,
    max: u16,
}

impl LineGauge {
    pub const fn new() -> Self {
        Self { value: 0, max: 100 }
    }

    pub const fn with_max(mut self, max: u16) -> Self {
        self.max = max;
        self
    }

    pub fn value(&self) -> u16 {
        self.value
    }

    /// Sets the value, clamped into `0..=max`.
    pub fn set_value(&mut self, v: u16) {
        self.value = v.min(self.max);
    }
}

impl Default for LineGauge {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for LineGauge {
    fn update(&mut self, _msg: &Msg) {
        // Driven externally via set_value().
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let cw = target.char_width().max(1);
        let percent = if self.max == 0 {
            0
        } else {
            (self.value as u32 * 100 / self.max as u32) as u16
        };
        let permille = if self.max == 0 {
            0
        } else {
            (self.value as u32 * 1000 / self.max as u32) as u16
        };

        let mut buf = [0u8; 6];
        let ps = fmt_u16(&mut buf, percent);
        let pct_px = target.text_width(ps) + cw; // digits + '%'
        let bar_w = area.w.saturating_sub(pct_px + cw); // +1 char gap

        if bar_w > 0 {
            target.draw_bar(Area::new(area.x, area.y, bar_w, area.h), permille, Style::Accent);
        }
        if pct_px <= area.w {
            let px = area.x + area.w - pct_px;
            target.draw_text(px, area.y, ps, Style::Normal);
            target.draw_text(px + target.text_width(ps), area.y, "%", Style::Normal);
        }
    }
}

// ── Scrollbar ─────────────────────────────────────────────────────────────────

/// A standalone vertical scrollbar: a thin pixel track + rounded thumb whose
/// size and position come from `(total, window, offset)`.
///
/// The owning view refreshes the geometry with [`set`](Scrollbar::set) each
/// frame, then draws it beside a tall area (Table, Pager, …). `List` keeps its
/// own built-in indicator; this is the reusable component.
#[derive(Debug)]
pub struct Scrollbar {
    total: usize,
    window: usize,
    offset: usize,
}

impl Scrollbar {
    pub const fn new() -> Self {
        Self { total: 0, window: 0, offset: 0 }
    }

    /// Updates the scroll geometry - call once per frame from the view.
    pub fn set(&mut self, total: usize, window: usize, offset: usize) {
        self.total = total;
        self.window = window;
        self.offset = offset;
    }
}

impl Default for Scrollbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Scrollbar {
    fn update(&mut self, _msg: &Msg) {
        // Driven externally via set().
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let window = self.window.max(1);
        draw_v_scroll(target, area, self.total.max(window), window, self.offset);
    }
}

// ── Paginator ─────────────────────────────────────────────────────────────────

/// A page indicator: a row of dots (current filled, others outline - like TUI
/// `●○○`) drawn via [`draw_radio`](RenderTarget::draw_radio), or a compact
/// `current/total` numeric readout.
#[derive(Debug)]
pub struct Paginator {
    pages: usize,
    current: usize,
    numeric: bool,
}

impl Paginator {
    pub const fn new(pages: usize) -> Self {
        Self { pages, current: 0, numeric: false }
    }

    pub const fn with_numeric(mut self, numeric: bool) -> Self {
        self.numeric = numeric;
        self
    }

    /// Sets the current page, clamped into `[0, pages - 1]`.
    pub fn with_current(mut self, idx: usize) -> Self {
        self.set_current(idx);
        self
    }

    pub fn current(&self) -> usize {
        self.current
    }

    /// Sets the current page, clamped into `[0, pages - 1]`.
    pub fn set_current(&mut self, idx: usize) {
        if self.pages > 0 {
            self.current = idx.min(self.pages - 1);
        }
    }

    /// Advances to the next page, stopping at the last.
    pub fn next(&mut self) {
        if self.current + 1 < self.pages {
            self.current += 1;
        }
    }

    /// Returns to the previous page, stopping at the first.
    pub fn prev(&mut self) {
        if self.current > 0 {
            self.current -= 1;
        }
    }
}

impl Component for Paginator {
    fn update(&mut self, msg: &Msg) {
        match msg {
            Msg::Right => self.next(),
            Msg::Left => self.prev(),
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 || self.pages == 0 {
            return;
        }

        if self.numeric {
            let mut b1 = [0u8; 6];
            let cur = fmt_u16(&mut b1, self.current as u16 + 1);
            target.draw_text(area.x, area.y, cur, Style::Normal);
            let mut x = area.x.saturating_add(target.text_width(cur));
            target.draw_text(x, area.y, "/", Style::Muted);
            x = x.saturating_add(target.char_width().max(1));
            let mut b2 = [0u8; 6];
            let tot = fmt_u16(&mut b2, self.pages as u16);
            target.draw_text(x, area.y, tot, Style::Normal);
        } else {
            // One dot per page; each occupies a square sized to the row.
            let step = target.line_height().max(1);
            for i in 0..self.pages {
                let cx = area.x.saturating_add(i as u16 * step);
                if cx + step > area.x + area.w {
                    break;
                }
                let style = if i == self.current { Style::Accent } else { Style::Muted };
                target.draw_radio(Area::new(cx, area.y, step, area.h), i == self.current, style);
            }
        }
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
    // draw_spinner / draw_radio fall through to their symbolic defaults here.

    fn texts(t: &RecordingTarget) -> Vec<(u16, u16, alloc::string::String, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.clone(), *style)),
                _ => None,
            })
            .collect()
    }

    fn bars(t: &RecordingTarget) -> Vec<(Area, u16, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Bar { area, fill_permille, style } => Some((*area, *fill_permille, *style)),
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

    // ── Spinner ───────────────────────────────────────────────────────────────

    #[test]
    fn spinner_first_frame_and_tick() {
        let mut s = Spinner::new(); // Line
        let mut t = RecordingTarget::new(40, 10);
        s.view(&mut t, Area::new(0, 0, 40, 10));
        assert_eq!(t.first_text(), Some((0, 0, "|", Style::Accent)));

        s.update(&Msg::Tick);
        let mut t2 = RecordingTarget::new(40, 10);
        s.view(&mut t2, Area::new(0, 0, 40, 10));
        assert_eq!(t2.first_text(), Some((0, 0, "/", Style::Accent)));
    }

    #[test]
    fn spinner_style_selects_frame_set() {
        let s = Spinner::new().with_style(SpinnerStyle::Braille);
        let mut t = RecordingTarget::new(40, 10);
        s.view(&mut t, Area::new(0, 0, 40, 10));
        // First Braille frame.
        assert_eq!(t.first_text(), Some((0, 0, "⠋", Style::Accent)));
    }

    #[test]
    fn spinner_wraps_frames() {
        let mut s = Spinner::new().with_style(SpinnerStyle::Meter); // 2 frames
        s.update(&Msg::Tick);
        s.update(&Msg::Tick); // wraps back to frame 0
        let mut t = RecordingTarget::new(40, 10);
        s.view(&mut t, Area::new(0, 0, 40, 10));
        assert_eq!(t.first_text(), Some((0, 0, "▰", Style::Accent)));
    }

    #[test]
    fn spinner_label() {
        let mut t = RecordingTarget::new(60, 10);
        Spinner::new().with_label("Go").view(&mut t, Area::new(0, 0, 60, 10));
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 12 && s == "Go"));
    }

    #[test]
    fn spinner_dirty_on_advancing_tick() {
        let mut s = Spinner::new(); // starts dirty
        assert!(s.dirty());
        s.mark_clean();
        assert!(!s.dirty());

        // A non-Tick message is a no-op → stays clean.
        s.update(&Msg::Select);
        assert!(!s.dirty());

        // A Tick advances the frame → dirty (the animation needs a repaint).
        s.update(&Msg::Tick);
        assert!(s.dirty());
    }

    // ── ProgressBar / LineGauge ───────────────────────────────────────────────

    #[test]
    fn progressbar_fraction() {
        let mut p = ProgressBar::new();
        p.set_value(25);
        let mut t = RecordingTarget::new(80, 10);
        p.view(&mut t, Area::new(0, 0, 80, 10));
        // One bar over the full area at 25%.
        assert_eq!(bars(&t), [(Area::new(0, 0, 80, 10), 250, Style::Accent)]);
    }

    #[test]
    fn progressbar_clamp() {
        let mut p = ProgressBar::new();
        p.set_value(999);
        assert_eq!(p.value(), 100);
    }

    #[test]
    fn stacked_bars_keep_their_rows() {
        // Two bars on adjacent rows draw over their own row only; the graphics
        // draw_bar insets the fill vertically so they never merge.
        let mut a = ProgressBar::new();
        a.set_value(50);
        let mut b = ProgressBar::new();
        b.set_value(80);
        let mut t = RecordingTarget::new(80, 20);
        a.view(&mut t, Area::new(0, 0, 80, 10));
        b.view(&mut t, Area::new(0, 10, 80, 10));
        let bs = bars(&t);
        assert_eq!(bs[0].0, Area::new(0, 0, 80, 10));
        assert_eq!(bs[1].0, Area::new(0, 10, 80, 10)); // disjoint rows
    }

    #[test]
    fn linegauge_percent_right_aligned_with_bar() {
        let mut g = LineGauge::new();
        g.set_value(50);
        let mut t = RecordingTarget::new(120, 10);
        g.view(&mut t, Area::new(0, 0, 120, 10));
        // "50" + "%" right-aligned: pct_px = 2*6 + 6 = 18 → x = 120 - 18 = 102.
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 102 && s == "50"));
        assert!(texts(&t).iter().any(|(_, _, s, _)| s == "%"));
        // Bar fills the remaining width at 50%.
        assert!(bars(&t).iter().any(|(a, p, _)| a.x == 0 && *p == 500));
    }

    // ── Scrollbar ─────────────────────────────────────────────────────────────

    #[test]
    fn scrollbar_thumb_top_then_bottom() {
        let mut s = Scrollbar::new();
        s.set(10, 3, 0);
        let mut t = RecordingTarget::new(4, 30);
        s.view(&mut t, Area::new(0, 0, 4, 30));
        // Track (Muted) + thumb (Focus); thumb at the top for offset 0.
        let f = fills(&t);
        assert!(f.iter().any(|(_, st)| *st == Style::Muted));
        let thumb = f.iter().find(|(_, st)| *st == Style::Focus).unwrap();
        assert_eq!(thumb.0.y, 0);

        let mut s2 = Scrollbar::new();
        s2.set(10, 3, 7); // max offset
        let mut t2 = RecordingTarget::new(4, 30);
        s2.view(&mut t2, Area::new(0, 0, 4, 30));
        let thumb2 = fills(&t2).into_iter().find(|(_, st)| *st == Style::Focus).unwrap();
        assert!(thumb2.0.y > 0); // moved down
    }

    // ── Paginator ─────────────────────────────────────────────────────────────

    #[test]
    fn paginator_dots_current_filled() {
        let mut t = RecordingTarget::new(60, 10);
        Paginator::new(3).view(&mut t, Area::new(0, 0, 60, 10));
        // draw_radio symbolic default: current "(*)" Accent, others "( )" Muted.
        let tx = texts(&t);
        assert!(tx.iter().any(|(x, _, s, st)| *x == 0 && s == "(*)" && *st == Style::Accent));
        assert!(tx.iter().filter(|(_, _, s, st)| s == "( )" && *st == Style::Muted).count() == 2);
    }

    #[test]
    fn paginator_current_moves() {
        let mut t = RecordingTarget::new(60, 10);
        Paginator::new(3).with_current(1).view(&mut t, Area::new(0, 0, 60, 10));
        // Dot 1 (x = 10) is the filled one.
        assert!(texts(&t).iter().any(|(x, _, s, _)| *x == 10 && s == "(*)"));
    }

    #[test]
    fn paginator_numeric() {
        let mut t = RecordingTarget::new(60, 10);
        Paginator::new(5).with_numeric(true).with_current(1).view(&mut t, Area::new(0, 0, 60, 10));
        let tx = texts(&t);
        assert!(tx.iter().any(|(x, _, s, _)| *x == 0 && s == "2"));
        assert!(tx.iter().any(|(_, _, s, _)| s == "/"));
        assert!(tx.iter().any(|(_, _, s, _)| s == "5"));
    }

    #[test]
    fn paginator_next_prev_clamp_and_arrows() {
        let mut p = Paginator::new(3);
        p.update(&Msg::Right);
        assert_eq!(p.current(), 1);
        p.update(&Msg::Left);
        p.update(&Msg::Left);
        assert_eq!(p.current(), 0);
        p.set_current(2);
        p.next();
        assert_eq!(p.current(), 2);
    }
}
