use core::cell::Cell;

use crate::{Area, Component, Msg, Outcome, RenderTarget};

// ── Canvas ────────────────────────────────────────────────────────────────────

/// Free-hand drawing without declaring a type: a [`Component`] that paints
/// whatever a closure paints.
///
/// Writing a widget from scratch is four separate obligations - the dirty
/// `Cell`, the self-clear, the zero-area guard and the [`Outcome`] - and a
/// dial, an arrow or a mini-graph needs none of them, only somewhere to draw.
/// `Canvas` is that somewhere: it brings the four, the closure brings the
/// picture.
///
/// ```
/// use knurl_core::{Area, Canvas, Component, RenderTarget, Style};
///
/// // A crosshair, in the area it is given - and in a Style, not a colour, so
/// // it works on a mono OLED, on a colour TFT and against a test target.
/// let cross = Canvas::new(|t: &mut dyn RenderTarget, a: Area| {
///     t.draw_line(a.x, a.y + a.h / 2, a.x + a.w - 1, a.y + a.h / 2, Style::Accent);
///     t.draw_line(a.x + a.w / 2, a.y, a.x + a.w / 2, a.y + a.h - 1, Style::Accent);
/// });
///
/// // Drawn like any other component: `cross.view(target, area)`.
/// assert!(cross.dirty(), "a fresh canvas owes its first paint");
/// ```
///
/// The closure is held **by value** behind a type parameter - no `alloc`, no
/// `dyn` on the drawing path - and it draws through the same
/// [`RenderTarget`] every widget uses. For the drawing that portable
/// primitives cannot express (arcs, images, your own font) reach for
/// `knurl-graphics`' clipped escape hatch instead; that is a trade the core
/// will not make.
///
/// ## The canvas and its dirty flag
///
/// **A closure cannot say that it changed.** It is called to paint and returns
/// nothing; whatever it reads - a sensor value, a rolling buffer - it reads
/// from outside. So the canvas cannot work out for itself when to repaint, and
/// there are exactly two honest ways to run one:
///
/// - **built where it is drawn**, like a [`FormZone`](crate::FormZone) or a
///   [`TabZone`](crate::TabZone): a fresh canvas is dirty, so it repaints every
///   frame. Right for a live readout that changes every tick, and it costs a
///   repaint of its own area - nothing else on the screen is affected;
/// - **kept as a field**, for a picture that mostly sits still. It paints once
///   and then stays quiet, and the owner says when that stops being true:
///   [`mark_dirty`](Component::mark_dirty) takes `&self`, so it can be called
///   from wherever the data behind the drawing was changed.
///
/// A long-lived canvas needs a nameable type, which a closure does not have -
/// declare the field as `Canvas<fn(&mut dyn RenderTarget, Area)>` and pass a
/// non-capturing closure (or a plain `fn`), which coerces.
///
/// One trap worth knowing: [`Screen::enter`](crate::Screen::enter) cascades its
/// repaint over the screen's **zones**, and a canvas is not focusable, so it is
/// not one. A stored canvas that has to survive leaving and re-entering a
/// screen is marked dirty in [`on_enter`](crate::Screen::on_enter) - one line,
/// exactly like a `Spinner` drawn inside a hand-painted stack.
///
/// ## What a canvas is not
///
/// It handles no events ([`update`](Component::update) always reports
/// [`Ignored`](Outcome::Ignored)) and refuses the focus, so a
/// [`FocusChain`](crate::FocusChain) steps over it instead of costing the user
/// a click on a picture. Something the user drives is a widget with state, and
/// that is [`Component`] - the trait is public for exactly that.
pub struct Canvas<F> {
    draw: F,
    dirty: Cell<bool>,
}

impl<F: Fn(&mut dyn RenderTarget, Area)> Canvas<F> {
    /// A canvas that paints through `draw`, starting dirty (so it paints on the
    /// first frame it is given).
    pub const fn new(draw: F) -> Self {
        Self {
            draw,
            dirty: Cell::new(true),
        }
    }
}

impl<F: Fn(&mut dyn RenderTarget, Area)> Component for Canvas<F> {
    /// Nothing to handle: the event stays available to whoever is next.
    fn update(&mut self, _msg: &Msg) -> Outcome {
        Outcome::Ignored
    }

    /// A picture is not a place the cursor can usefully stop.
    fn focusable(&self) -> bool {
        false
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        (self.draw)(target, area);
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
    use crate::Style;
    use crate::mock::{Op, RecordingTarget};

    fn dial() -> Canvas<fn(&mut dyn RenderTarget, Area)> {
        Canvas::new(|t: &mut dyn RenderTarget, a: Area| {
            t.draw_line(a.x, a.y, a.x + a.w - 1, a.y + a.h - 1, Style::Accent);
        })
    }

    /// The four obligations a hand-written widget would have to remember: the
    /// self-clear of its own area, the drawing, and going clean afterwards.
    #[test]
    fn a_dirty_canvas_clears_its_area_draws_and_goes_clean() {
        let c = dial();
        let area = Area::new(2, 3, 20, 10);
        let mut t = RecordingTarget::new(64, 32);
        c.view(&mut t, area);
        assert_eq!(t.ops().first(), Some(&Op::Clear { area }));
        assert_eq!(
            t.ops().get(1),
            Some(&Op::Line {
                x0: 2,
                y0: 3,
                x1: 21,
                y1: 12,
                style: Style::Accent
            }),
            "the closure gets the area it was given"
        );
        assert!(!c.dirty());
    }

    /// A canvas kept as a field paints once and then stays quiet - until the
    /// owner says the picture is stale, which is the only way a closure's
    /// change can be reported.
    #[test]
    fn a_clean_canvas_paints_nothing_until_it_is_marked() {
        let c = dial();
        let area = Area::new(0, 0, 20, 10);
        let mut first = RecordingTarget::new(64, 32);
        c.view(&mut first, area);
        assert!(!first.ops().is_empty());

        let mut idle = RecordingTarget::new(64, 32);
        c.view(&mut idle, area);
        assert!(idle.ops().is_empty(), "a clean canvas repaints nothing");

        c.mark_dirty();
        let mut again = RecordingTarget::new(64, 32);
        c.view(&mut again, area);
        assert!(!again.ops().is_empty(), "and mark_dirty brings it back");
    }

    /// Built where it is drawn, it is dirty by construction - the pattern a
    /// live readout uses.
    #[test]
    fn a_canvas_built_per_frame_always_paints() {
        for _ in 0..3 {
            let mut t = RecordingTarget::new(64, 32);
            dial().view(&mut t, Area::new(0, 0, 20, 10));
            assert!(!t.ops().is_empty());
        }
    }

    #[test]
    fn zero_area_draws_nothing() {
        let c = dial();
        let mut t = RecordingTarget::new(64, 32);
        c.view(&mut t, Area::new(0, 0, 0, 10));
        c.view(&mut t, Area::new(0, 0, 20, 0));
        assert!(t.ops().is_empty());
        assert!(c.dirty(), "and it is still owed a paint");
    }

    /// It handles nothing and refuses the focus, so a chain steps over it.
    #[test]
    fn a_canvas_is_not_a_stop_for_the_cursor() {
        let mut c = dial();
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Tick] {
            assert_eq!(c.update(&msg), Outcome::Ignored, "{msg:?}");
        }
        assert!(!c.focusable());
    }

    /// A closure that captures by value: the data it draws is copied in when
    /// the canvas is built, which is what makes the per-frame pattern work
    /// without `alloc`.
    #[test]
    fn a_canvas_can_capture_the_data_it_draws() {
        let samples = [3u16, 7, 1];
        let spark = Canvas::new(move |t: &mut dyn RenderTarget, a: Area| {
            for (i, v) in samples.iter().enumerate() {
                t.set_pixel(a.x + i as u16, a.y + a.h - 1 - *v, Style::Normal);
            }
        });
        let mut t = RecordingTarget::new(64, 32);
        spark.view(&mut t, Area::new(0, 0, 8, 8));
        let ys: alloc::vec::Vec<u16> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Pixel { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(ys, alloc::vec![4, 0, 6]);
    }
}
