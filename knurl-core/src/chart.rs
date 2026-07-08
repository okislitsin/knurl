use crate::{Area, Component, Msg, RenderTarget, Style};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max` Unicode scalar values of `s` as a `&str`.
fn truncate(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map(|(i, _)| &s[..i]).unwrap_or(s)
}

/// Formats `v` as a decimal string into a stack buffer - no allocation.
fn fmt_u16(buf: &mut [u8; 5], mut v: u16) -> &str {
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

fn digits(v: u16) -> u16 {
    let mut n = 1;
    let mut x = v;
    while x >= 10 {
        x /= 10;
        n += 1;
    }
    n
}

// ── BarChartModel (data provider) ───────────────────────────────────────────

/// Data behind a [`BarChart`] (mirrors [`ListModel`](crate::ListModel)): a count,
/// per-row ASCII label and `u16` value.
pub trait BarChartModel {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn label(&self, i: usize) -> &str;
    fn value(&self, i: usize) -> u16;
}

/// Static impl over a slice of `(label, value)` pairs.
impl BarChartModel for [(&str, u16)] {
    fn len(&self) -> usize {
        <[_]>::len(self)
    }
    fn label(&self, i: usize) -> &str {
        self[i].0
    }
    fn value(&self, i: usize) -> u16 {
        self[i].1
    }
}

/// Array impl so an inline literal works directly under the generic `M`.
impl<const N: usize> BarChartModel for [(&str, u16); N] {
    fn len(&self) -> usize {
        N
    }
    fn label(&self, i: usize) -> &str {
        self[i].0
    }
    fn value(&self, i: usize) -> u16 {
        self[i].1
    }
}

// ── BarChart ────────────────────────────────────────────────────────────────

/// A horizontal bar chart: one row per datum - an ASCII label column, a smooth
/// pixel bar (via [`draw_bar`](RenderTarget::draw_bar), which insets the fill so
/// stacked rows keep a gap and don't merge), and the ASCII value right-aligned.
pub struct BarChart<'a, M: BarChartModel + ?Sized = [(&'a str, u16)]> {
    model: &'a M,
    max: u16,
    label_w: u16,
}

impl<'a, M: BarChartModel + ?Sized> BarChart<'a, M> {
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            max: 0,
            label_w: 36,
        }
    }

    /// Sets the value mapped to a full-width bar. `0` means auto-scale to the
    /// largest value in the data.
    pub const fn with_max(mut self, max: u16) -> Self {
        self.max = max;
        self
    }

    /// Sets the label column width, in pixels.
    pub const fn with_label_width(mut self, px: u16) -> Self {
        self.label_w = px;
        self
    }

    fn effective_max(&self) -> u16 {
        if self.max > 0 {
            return self.max;
        }
        let mut m = 1u16;
        for i in 0..self.model.len() {
            let v = self.model.value(i);
            if v > m {
                m = v;
            }
        }
        m
    }
}

impl<'a, M: BarChartModel + ?Sized> Component for BarChart<'a, M> {
    fn update(&mut self, _msg: &Msg) {
        // Driven by its data.
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let cw = target.char_width().max(1);
        let rows = (area.h / line_h) as usize;
        let n = self.model.len();
        if area.w == 0 || area.h == 0 || rows == 0 || n == 0 {
            return;
        }

        let effective_max = self.effective_max();
        let val_w = digits(effective_max) * cw;
        let label_w = self.label_w.min(area.w);
        let bar_x = area.x + label_w;
        let bar_w = area.w.saturating_sub(label_w + val_w + cw); // +1 char gap

        for row in 0..rows {
            if row >= n {
                break;
            }
            let y = area.y.saturating_add(row as u16 * line_h);
            let value = self.model.value(row);

            target.draw_text(
                area.x,
                y,
                truncate(self.model.label(row), (label_w / cw) as usize),
                Style::Normal,
            );

            if bar_w > 0 {
                let permille = (value as u32 * 1000 / effective_max.max(1) as u32) as u16;
                target.draw_bar(Area::new(bar_x, y, bar_w, line_h), permille, Style::Accent);
            }

            let mut buf = [0u8; 5];
            let vs = fmt_u16(&mut buf, value);
            let vw = target.text_width(vs);
            if vw <= area.w {
                target.draw_text(area.x + area.w - vw, y, vs, Style::Muted);
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

    const DATA: &[(&str, u16)] = &[("Alpha", 10), ("Beta", 5)];

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

    fn bars(t: &RecordingTarget) -> Vec<(Area, u16, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Bar {
                    area,
                    fill_permille,
                    style,
                } => Some((*area, *fill_permille, *style)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn barchart_auto_scales_and_lays_out() {
        // area 120 wide, 30 tall → 3 rows. label_w 36, max=10 → val_w=12,
        // gap=6 → bar_x=36, bar_w = 120 - 36 - 12 - 6 = 66.
        let chart = BarChart::new(DATA);
        let mut t = RecordingTarget::new(120, 30);
        chart.view(&mut t, Area::new(0, 0, 120, 30));
        let tx = texts(&t);
        let bs = bars(&t);
        // Labels left, values Muted right-aligned.
        assert!(tx.contains(&(0, 0, "Alpha".into(), Style::Normal)));
        assert!(
            tx.iter()
                .any(|(x, y, s, st)| *y == 0 && *x == 108 && s == "10" && *st == Style::Muted)
        );
        // Row 0 full (10/10 = 1000), row 1 half (5/10 = 500); on disjoint rows.
        assert!(bs.contains(&(Area::new(36, 0, 66, 10), 1000, Style::Accent)));
        assert!(bs.contains(&(Area::new(36, 10, 66, 10), 500, Style::Accent)));
    }

    #[test]
    fn barchart_fixed_max() {
        let chart = BarChart::new(DATA).with_max(20);
        let mut t = RecordingTarget::new(120, 30);
        chart.view(&mut t, Area::new(0, 0, 120, 30));
        // 10/20 = 500, 5/20 = 250.
        assert!(bars(&t).iter().any(|(_, p, _)| *p == 500));
        assert!(bars(&t).iter().any(|(_, p, _)| *p == 250));
    }

    #[test]
    fn barchart_no_braces_or_hash() {
        let chart = BarChart::new(DATA);
        let mut t = RecordingTarget::new(120, 30);
        chart.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(
            !texts(&t)
                .iter()
                .any(|(_, _, s, _)| s.contains('#') || s.contains('|'))
        );
    }

    /// A custom model (computed outside the widget).
    struct Calc;
    impl BarChartModel for Calc {
        fn len(&self) -> usize {
            2
        }
        fn label(&self, i: usize) -> &str {
            ["x", "y"][i]
        }
        fn value(&self, i: usize) -> u16 {
            [3, 6][i]
        }
    }

    #[test]
    fn barchart_custom_model() {
        let m = Calc;
        let chart = BarChart::new(&m);
        let mut t = RecordingTarget::new(120, 30);
        chart.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(texts(&t).iter().any(|(_, _, s, _)| s == "x"));
        assert!(bars(&t).iter().any(|(_, p, _)| *p == 500)); // 3/6
    }
}
