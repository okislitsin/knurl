use crate::{Area, BorderStyle, Component, Msg, Padding, RenderTarget};

// ── Constraint ──────────────────────────────────────────────────────────────

/// How a segment claims space along a stack's main axis, in **pixels**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constraint {
    /// A fixed size in pixels.
    Length(u16),
    /// A weighted share of the pixels left after all `Length`s (weight 0 → 0px).
    Fill(u16),
}

/// Distributes `total` pixels across `constraints` along one axis.
///
/// `Length`s take their exact size first; the remaining pixels are split between
/// the `Fill`s in proportion to their weight, with any rounding leftover handed
/// to the last `Fill` so the sizes sum back to `total` exactly.
fn distribute<const N: usize>(total: u16, constraints: &[Constraint; N]) -> [u16; N] {
    let mut fixed: u16 = 0;
    let mut weight: u32 = 0;
    for c in constraints {
        match c {
            Constraint::Length(n) => fixed = fixed.saturating_add(*n),
            Constraint::Fill(w) => weight += *w as u32,
        }
    }

    let avail = total.saturating_sub(fixed) as u32;

    let mut sizes = [0u16; N];
    let mut fill_sum: u32 = 0;
    let mut last_fill: Option<usize> = None;
    for (i, c) in constraints.iter().enumerate() {
        match c {
            Constraint::Length(n) => sizes[i] = *n,
            Constraint::Fill(w) => {
                let size = (avail * *w as u32).checked_div(weight).unwrap_or(0);
                sizes[i] = size as u16;
                fill_sum += size;
                last_fill = Some(i);
            }
        }
    }

    // Hand the rounding leftover to the last Fill so the sum is exact.
    if let Some(idx) = last_fill {
        let remainder = avail.saturating_sub(fill_sum);
        sizes[idx] = sizes[idx].saturating_add(remainder as u16);
    }

    sizes
}

// ── VStack / HStack ─────────────────────────────────────────────────────────

/// Splits an [`Area`] vertically into a stack of rows.
pub struct VStack;

/// Splits an [`Area`] horizontally into a row of columns.
pub struct HStack;

impl VStack {
    /// Divides `area` **vertically**. Each sub-area spans the full width of
    /// `area`; heights (in pixels) are distributed across `constraints` in order.
    pub fn split<const N: usize>(area: Area, constraints: &[Constraint; N]) -> [Area; N] {
        let sizes = distribute(area.h, constraints);
        let mut out = [Area::new(0, 0, 0, 0); N];
        let mut offset: u16 = 0;
        for i in 0..N {
            out[i] = Area {
                x: area.x,
                y: area.y.saturating_add(offset),
                w: area.w,
                h: sizes[i],
            };
            offset = offset.saturating_add(sizes[i]);
        }
        out
    }
}

impl HStack {
    /// Divides `area` **horizontally**. Each sub-area spans the full height of
    /// `area`; widths (in pixels) are distributed across `constraints` in order.
    pub fn split<const N: usize>(area: Area, constraints: &[Constraint; N]) -> [Area; N] {
        let sizes = distribute(area.w, constraints);
        let mut out = [Area::new(0, 0, 0, 0); N];
        let mut offset: u16 = 0;
        for i in 0..N {
            out[i] = Area {
                x: area.x.saturating_add(offset),
                y: area.y,
                w: sizes[i],
                h: area.h,
            };
            offset = offset.saturating_add(sizes[i]);
        }
        out
    }
}

// ── Padded ──────────────────────────────────────────────────────────────────

/// Wraps a child component, insetting it by [`Padding`] (pixels) on every side.
#[derive(Debug)]
pub struct Padded<C> {
    child: C,
    padding: Padding,
}

impl<C> Padded<C> {
    pub const fn new(child: C, padding: Padding) -> Self {
        Self { child, padding }
    }

    pub fn child(&self) -> &C {
        &self.child
    }

    pub fn child_mut(&mut self) -> &mut C {
        &mut self.child
    }
}

impl<C: Component> Component for Padded<C> {
    fn update(&mut self, msg: &Msg) {
        self.child.update(msg);
    }

    // Container: dispatch to the child (which self-gates and clears its own
    // inner area). The padding itself draws nothing, so nothing to clear here.
    fn view(&self, target: &mut dyn RenderTarget, area: Area) {
        if let Some(inner) = self.padding.inner(area) {
            self.child.view(target, inner);
        }
    }

    fn focus(&mut self) {
        self.child.focus();
    }

    fn blur(&mut self) {
        self.child.blur();
    }

    fn dirty(&self) -> bool {
        self.child.dirty()
    }

    fn mark_clean(&self) {
        self.child.mark_clean();
    }

    fn mark_dirty(&self) {
        self.child.mark_dirty();
    }
}

// ── Bordered ──────────────────────────────────────────────────────────────────

/// Wraps a child component, drawing a thin pixel [`BorderStyle`] box around it
/// and rendering the child in the inner region.
///
/// The chrome is **compact**: the content insets by exactly the border's pixel
/// thickness ([`BorderStyle::thickness`]) - 1px for `Single`/`Rounded`, 2px for
/// `Thick` - never by a whole text row/column. On a tiny panel that is the
/// difference between a usable interior and a wasted one.
#[derive(Debug)]
pub struct Bordered<C> {
    child: C,
    border: BorderStyle,
}

impl<C> Bordered<C> {
    pub const fn new(child: C, border: BorderStyle) -> Self {
        Self { child, border }
    }

    pub fn child(&self) -> &C {
        &self.child
    }

    pub fn child_mut(&mut self) -> &mut C {
        &mut self.child
    }
}

impl<C: Component> Component for Bordered<C> {
    fn update(&mut self, msg: &Msg) {
        self.child.update(msg);
    }

    // Container: redraw the border chrome only when dirty (its pixels persist
    // otherwise), then dispatch to the child, which self-gates and clears its
    // own inner area. The box and the child's inner area together tile the whole
    // area, so no separate clear is needed.
    fn view(&self, target: &mut dyn RenderTarget, area: Area) {
        if self.dirty() {
            target.draw_box(area, self.border);
        }
        // Inset by exactly the stroke thickness - compact chrome.
        if let Some(inner) = area.inner_by(self.border.thickness()) {
            self.child.view(target, inner);
        }
    }

    fn focus(&mut self) {
        self.child.focus();
    }

    fn blur(&mut self) {
        self.child.blur();
    }

    fn dirty(&self) -> bool {
        self.child.dirty()
    }

    fn mark_clean(&self) {
        self.child.mark_clean();
    }

    fn mark_dirty(&self) {
        self.child.mark_dirty();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use crate::{Label, List};
    use Constraint::{Fill, Length};

    fn first_text(t: &RecordingTarget) -> Option<(u16, u16, alloc::string::String)> {
        t.ops().iter().find_map(|op| match op {
            Op::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
            _ => None,
        })
    }

    // ── VStack (pixels) ─────────────────────────────────────────────────────────

    #[test]
    fn vstack_fixed_lengths_are_pixels() {
        let out = VStack::split(Area::new(0, 0, 100, 60), &[Length(20), Length(40)]);
        assert_eq!(out, [Area::new(0, 0, 100, 20), Area::new(0, 20, 100, 40)]);
    }

    #[test]
    fn vstack_fill_splits_remaining_pixels_evenly() {
        let out = VStack::split(Area::new(0, 0, 100, 60), &[Fill(1), Fill(1)]);
        assert_eq!(out, [Area::new(0, 0, 100, 30), Area::new(0, 30, 100, 30)]);
    }

    #[test]
    fn vstack_fill_remainder_to_last() {
        let out = VStack::split(Area::new(0, 0, 100, 61), &[Fill(1), Fill(1)]);
        assert_eq!(out, [Area::new(0, 0, 100, 30), Area::new(0, 30, 100, 31)]);
    }

    #[test]
    fn vstack_mixed_length_and_fill() {
        let out = VStack::split(Area::new(0, 0, 100, 100), &[Length(20), Fill(1), Length(30)]);
        assert_eq!(
            out,
            [
                Area::new(0, 0, 100, 20),
                Area::new(0, 20, 100, 50),
                Area::new(0, 70, 100, 30),
            ]
        );
    }

    // ── HStack (pixels) ─────────────────────────────────────────────────────────

    #[test]
    fn hstack_mixed_widths_are_pixels() {
        let out = HStack::split(Area::new(0, 0, 100, 40), &[Length(20), Fill(1), Length(30)]);
        assert_eq!(
            out,
            [
                Area::new(0, 0, 20, 40),
                Area::new(20, 0, 50, 40),
                Area::new(70, 0, 30, 40),
            ]
        );
    }

    #[test]
    fn hstack_weighted_fill() {
        // 90px between Fill(1) and Fill(2) → 30 / 60.
        let out = HStack::split(Area::new(0, 0, 90, 10), &[Fill(1), Fill(2)]);
        assert_eq!(out, [Area::new(0, 0, 30, 10), Area::new(30, 0, 60, 10)]);
    }

    // ── Padded (pixel inset) ──────────────────────────────────────────────────

    #[test]
    fn padded_insets_child_in_pixels() {
        let mut t = RecordingTarget::new(100, 40);
        Padded::new(Label::new("Hi"), Padding::uniform(4)).view(&mut t, Area::new(0, 0, 100, 40));
        // Label draws at the padded origin (4, 4).
        assert_eq!(first_text(&t).map(|(x, y, _)| (x, y)), Some((4, 4)));
    }

    #[test]
    fn padded_asymmetric() {
        let mut t = RecordingTarget::new(100, 40);
        Padded::new(Label::new("Hi"), Padding::new(2, 3, 5, 7))
            .view(&mut t, Area::new(10, 10, 100, 40));
        // Origin offset by (left=7, top=2) from the area origin (10,10).
        assert_eq!(first_text(&t).map(|(x, y, _)| (x, y)), Some((17, 12)));
    }

    #[test]
    fn padded_collapse_is_noop() {
        let mut t = RecordingTarget::new(20, 20);
        // Horizontal inset (10+10) == width → collapses → child not drawn.
        Padded::new(Label::new("Hi"), Padding::symmetric(0, 10))
            .view(&mut t, Area::new(0, 0, 20, 20));
        assert!(t.ops().is_empty());
    }

    // ── Bordered (compact chrome) ──────────────────────────────────────────────

    #[test]
    fn bordered_draws_box_and_insets_by_one_pixel() {
        let mut t = RecordingTarget::new(100, 40);
        Bordered::new(Label::new("X"), BorderStyle::Single).view(&mut t, Area::new(0, 0, 100, 40));
        // The box covers the whole area…
        assert!(t.ops().contains(&Op::Box {
            area: Area::new(0, 0, 100, 40),
            border: BorderStyle::Single,
        }));
        // …and the child insets by exactly 1px (not a whole row/column).
        assert_eq!(first_text(&t).map(|(x, y, _)| (x, y)), Some((1, 1)));
    }

    #[test]
    fn bordered_thick_insets_by_two_pixels() {
        let mut t = RecordingTarget::new(100, 40);
        Bordered::new(Label::new("X"), BorderStyle::Thick).view(&mut t, Area::new(0, 0, 100, 40));
        assert_eq!(first_text(&t).map(|(x, y, _)| (x, y)), Some((2, 2)));
    }

    #[test]
    fn bordered_none_does_not_inset() {
        let mut t = RecordingTarget::new(100, 40);
        Bordered::new(Label::new("X"), BorderStyle::None).view(&mut t, Area::new(5, 5, 100, 40));
        // No border → child fills the whole area (origin unchanged).
        assert_eq!(first_text(&t).map(|(x, y, _)| (x, y)), Some((5, 5)));
    }

    #[test]
    fn bordered_compactness_vs_old_cell_model() {
        // A 1px border must leave a (w-2, h-2) pixel interior - not shrink by
        // a font row/column.
        let inner = Area::new(0, 0, 100, 40).inner_by(BorderStyle::Single.thickness()).unwrap();
        assert_eq!(inner, Area::new(1, 1, 98, 38));
    }

    #[test]
    fn wrapper_forwards_update() {
        let mut b = Bordered::new(List::new(&["A", "B", "C"]), BorderStyle::Single);
        b.update(&Msg::Down);
        assert_eq!(b.child().selected(), 1);
    }
}
