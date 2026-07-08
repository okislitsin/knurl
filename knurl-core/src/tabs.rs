use crate::{Area, Component, Msg, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

/// Pixel gap between tab titles.
const TAB_GAP: u16 = 8;

// ── Tabs ──────────────────────────────────────────────────────────────────────

/// A single-row tab strip. Each ASCII title is **underlined** so the strip reads
/// as tabs, not bare text: the active tab gets a thick `Accent` underline and
/// `Accent` text; inactive tabs get a thin `Muted` underline and `Muted` text.
#[derive(Debug)]
pub struct Tabs<'a> {
    titles: &'a [&'a str],
    selected: usize,
}

impl<'a> Tabs<'a> {
    pub fn new(titles: &'a [&'a str]) -> Self {
        Self {
            titles,
            selected: 0,
        }
    }

    /// Sets the selected tab, clamped into `[0, len - 1]` (no-op with no tabs).
    pub fn with_selected(mut self, idx: usize) -> Self {
        self.set_selected(idx);
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Title of the active tab, or `""` when there are no tabs.
    pub fn selected_title(&self) -> &'a str {
        self.titles.get(self.selected).copied().unwrap_or("")
    }

    /// Sets the selected tab, clamped into `[0, len - 1]`.
    pub fn set_selected(&mut self, idx: usize) {
        let n = self.titles.len();
        if n > 0 {
            self.selected = idx.min(n - 1);
        }
    }

    /// Advances to the next tab, stopping at the last.
    pub fn next(&mut self) {
        if self.selected + 1 < self.titles.len() {
            self.selected += 1;
        }
    }

    /// Returns to the previous tab, stopping at the first.
    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }
}

impl<'a> Component for Tabs<'a> {
    fn update(&mut self, msg: &Msg) {
        match msg {
            // Encoder rotation switches tabs; Left/Right kept for keyboards.
            Msg::Down | Msg::Right => self.next(),
            Msg::Up | Msg::Left => self.prev(),
            _ => {}
        }
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 || self.titles.is_empty() {
            return;
        }
        let cw = target.char_width().max(1);
        let line_h = target.line_height().max(1);

        let mut x = area.x;
        let right = area.x + area.w; // exclusive bound
        for (i, title) in self.titles.iter().enumerate() {
            if x >= right {
                break;
            }
            let max = ((right - x) / cw) as usize;
            let t = truncate(title, max);
            let tw = target.text_width(t);
            let active = i == self.selected;
            let style = if active { Style::Accent } else { Style::Muted };
            target.draw_text(x, area.y, t, style);

            // Underline: 2px Accent for the active tab, 1px Muted otherwise.
            let (uh, ustyle) = if active {
                (2, Style::Accent)
            } else {
                (1, Style::Muted)
            };
            let uy = area.y + line_h.saturating_sub(uh);
            target.fill_rect(Area::new(x, uy, tw, uh), ustyle);

            x = x.saturating_add(tw).saturating_add(TAB_GAP);
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

    const T: &[&str] = &["One", "Two"];

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
    fn tabs_active_and_inactive_styling_with_underline() {
        let tabs = Tabs::new(T);
        let mut t = RecordingTarget::new(120, 12);
        tabs.view(&mut t, Area::new(0, 0, 120, 12));
        let tx = texts(&t);
        // "One" active: Accent text at x=0; "Two" inactive: Muted at x = 18 + 8 = 26.
        assert!(tx.contains(&(0, 0, "One".into(), Style::Accent)));
        assert!(tx.contains(&(26, 0, "Two".into(), Style::Muted)));
        // Active underline: 2px Accent under "One" (width 18, at y = 10 - 2 = 8).
        assert!(fills(&t).contains(&(Area::new(0, 8, 18, 2), Style::Accent)));
        // Inactive underline: 1px Muted under "Two" (at y = 9).
        assert!(fills(&t).contains(&(Area::new(26, 9, 18, 1), Style::Muted)));
    }

    #[test]
    fn tabs_next_prev_and_encoder() {
        let mut tabs = Tabs::new(T);
        tabs.update(&Msg::Right);
        assert_eq!(tabs.selected(), 1);
        assert_eq!(tabs.selected_title(), "Two");
        tabs.update(&Msg::Up);
        assert_eq!(tabs.selected(), 0);
    }

    #[test]
    fn tabs_clamp() {
        let mut tabs = Tabs::new(T);
        tabs.prev();
        assert_eq!(tabs.selected(), 0);
        tabs.set_selected(1);
        tabs.next();
        assert_eq!(tabs.selected(), 1);
    }

    #[test]
    fn tabs_active_underline_moves_with_selection() {
        let tabs = Tabs::new(T).with_selected(1);
        let mut t = RecordingTarget::new(120, 12);
        tabs.view(&mut t, Area::new(0, 0, 120, 12));
        // Now "Two" carries the 2px Accent underline.
        assert!(
            fills(&t)
                .iter()
                .any(|(a, st)| a.h == 2 && a.x == 26 && *st == Style::Accent)
        );
    }
}
