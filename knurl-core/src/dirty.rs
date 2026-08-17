//! The dirty rectangle: the one thing a target has to remember so that a
//! partial repaint can stay partial all the way to the bus.
//!
//! Widgets already repaint only themselves, but that saving lives in the RAM
//! framebuffer: an application that cannot say *which* pixels moved has no
//! choice but to push the whole panel. [`DirtyRect`] is the bookkeeping that
//! closes the gap - a growing bounding box, four `min`/`max` per draw call -
//! and [`RenderTarget::take_dirty_rect`](crate::RenderTarget::take_dirty_rect)
//! is where an application reads it.

use crate::Area;

/// A growing bounding box over every pixel drawn since it was last taken.
///
/// A target owns one, unions each draw call into it, and hands it out through
/// [`take_dirty_rect`](crate::RenderTarget::take_dirty_rect). The box is
/// **clamped to the display** on the way in - it is constructed with the panel's
/// size for exactly that reason, so no implementation has to remember to clamp
/// and none can hand back a rectangle that runs off the panel.
///
/// It is one rectangle, not a list: the union of two distant widgets covers the
/// untouched space between them, and that is the deliberate trade. Tracking a
/// set of rectangles costs allocation (or a fixed-size arena and a merge
/// policy) to save a region that is still a fraction of the frame.
///
/// ```
/// use knurl_core::{Area, DirtyRect};
///
/// let mut dirty = DirtyRect::new(320, 240);
/// assert_eq!(dirty.take(), None); // nothing drawn, nothing to push
///
/// dirty.add(Area::new(10, 20, 4, 4));
/// dirty.add(Area::new(30, 20, 4, 4));
/// // One box over both, clamped to the panel.
/// assert_eq!(dirty.take(), Some(Area::new(10, 20, 24, 4)));
/// // ...and taking it resets: the frame has been pushed.
/// assert_eq!(dirty.take(), None);
/// ```
#[derive(Debug, Clone)]
pub struct DirtyRect {
    /// The panel, in target coordinates - the clamp every `add` goes through.
    bounds: Area,
    rect: Option<Area>,
}

impl DirtyRect {
    /// An empty box for a `width` × `height` panel.
    pub const fn new(width: u16, height: u16) -> Self {
        Self {
            bounds: Area::new(0, 0, width, height),
            rect: None,
        }
    }

    /// Unions `area` into the box, clamped to the panel.
    ///
    /// An empty area (zero width or height), or one entirely off the panel,
    /// changes nothing: a draw call that paints no pixel does not make a frame
    /// worth pushing.
    pub fn add(&mut self, area: Area) {
        let Some(a) = area.intersect(self.bounds) else {
            return;
        };
        self.rect = Some(match self.rect {
            None => a,
            Some(cur) => {
                let x = cur.x.min(a.x);
                let y = cur.y.min(a.y);
                // Both operands are already clamped to the panel, so the right
                // and bottom edges fit u16 and the arithmetic cannot wrap.
                let r = (cur.x + cur.w).max(a.x + a.w);
                let b = (cur.y + cur.h).max(a.y + a.h);
                Area::new(x, y, r - x, b - y)
            }
        });
    }

    /// Marks the **whole panel** dirty - what a target says when it has handed
    /// out a surface it cannot follow (`knurl-graphics`' unclipped
    /// `display_mut` escape hatch does exactly this).
    pub fn all(&mut self) {
        self.add(self.bounds);
    }

    /// The accumulated box, resetting it - `None` when nothing was drawn.
    pub fn take(&mut self) -> Option<Area> {
        self.rect.take()
    }

    /// The accumulated box without resetting it.
    pub fn peek(&self) -> Option<Area> {
        self.rect
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_until_something_is_drawn() {
        let mut d = DirtyRect::new(128, 64);
        assert_eq!(d.peek(), None);
        assert_eq!(d.take(), None);
    }

    #[test]
    fn one_area_is_itself() {
        let mut d = DirtyRect::new(128, 64);
        d.add(Area::new(4, 8, 20, 10));
        assert_eq!(d.take(), Some(Area::new(4, 8, 20, 10)));
    }

    #[test]
    fn union_is_the_bounding_box() {
        let mut d = DirtyRect::new(128, 64);
        d.add(Area::new(10, 10, 5, 5)); // 10..15 x 10..15
        d.add(Area::new(20, 4, 5, 5)); // 20..25 x  4..9
        assert_eq!(d.take(), Some(Area::new(10, 4, 15, 11)));
    }

    #[test]
    fn take_resets() {
        let mut d = DirtyRect::new(128, 64);
        d.add(Area::new(0, 0, 8, 8));
        assert!(d.take().is_some());
        assert_eq!(d.take(), None);
    }

    /// An area running off the panel contributes only the part that lands on it.
    #[test]
    fn add_is_clamped_to_the_panel() {
        let mut d = DirtyRect::new(128, 64);
        d.add(Area::new(120, 60, 100, 100));
        assert_eq!(d.take(), Some(Area::new(120, 60, 8, 4)));
    }

    #[test]
    fn empty_and_offscreen_areas_do_not_dirty() {
        let mut d = DirtyRect::new(128, 64);
        d.add(Area::new(4, 4, 0, 10));
        d.add(Area::new(4, 4, 10, 0));
        d.add(Area::new(200, 0, 10, 10));
        d.add(Area::new(0, 100, 10, 10));
        assert_eq!(d.take(), None);
    }

    #[test]
    fn all_marks_the_panel() {
        let mut d = DirtyRect::new(128, 64);
        d.all();
        assert_eq!(d.take(), Some(Area::new(0, 0, 128, 64)));
    }
}
