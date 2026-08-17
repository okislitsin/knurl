//! The repaint gate of a widget that draws data it does not own.
//!
//! A widget with state of its own knows when to repaint: it changed the state,
//! so it sets its flag. A widget over a **borrowed model** ([`List`](crate::List)
//! over a [`ListModel`](crate::ListModel), and every other data widget) does
//! not - the data can change without the widget being told, and re-reading the
//! model every frame to find out costs more than the repaint it would save.
//!
//! [`DataGate`] is the answer, and it is one question asked of the model:
//! [`revision`](crate::ListModel::revision) - a number that changes when the
//! content does. The gate remembers the one it last painted and compares. A
//! model that keeps no revision returns the same number forever, the comparison
//! never fires, and the widget behaves exactly as it did before: it repaints
//! when its own state moves, and when its owner says the data moved
//! ([`mark_dirty`](crate::Component::mark_dirty)).

use core::cell::Cell;

/// A [`Component`](crate::Component)'s dirty flag, plus the revision of the
/// data it last painted.
///
/// This is the whole of "how does a widget over somebody else's data know that
/// the data changed", and it is the same three lines in a built-in widget and
/// in one written outside the library:
///
/// ```
/// use core::cell::Cell;
/// use knurl_core::{Area, Component, DataGate, ListModel, Msg, Outcome, RenderTarget, Style};
///
/// struct Ticker<'a, M: ListModel + ?Sized> {
///     model: &'a M,
///     gate: DataGate,
/// }
///
/// impl<M: ListModel + ?Sized> Component for Ticker<'_, M> {
///     fn update(&mut self, _msg: &Msg) -> Outcome { Outcome::Ignored }
///
///     fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
///         if self.model.item_count() > 0 {
///             target.draw_text(area.x, area.y, self.model.get_item(0), Style::Normal);
///         }
///     }
///
///     // The three lines. `dirty` asks the model where it is; `mark_clean`
///     // records the answer that has just been painted.
///     fn dirty(&self) -> bool { self.gate.is_dirty(self.model.revision()) }
///     fn mark_clean(&self) { self.gate.mark_clean(self.model.revision()); }
///     fn mark_dirty(&self) { self.gate.mark_dirty(); }
/// }
///
/// /// A model that counts its own edits - the `u32` is the whole cost.
/// struct Feed { lines: [&'static str; 2], writes: Cell<u32> }
/// impl ListModel for Feed {
///     fn item_count(&self) -> usize { self.lines.len() }
///     fn get_item(&self, i: usize) -> &str { self.lines[i] }
///     fn revision(&self) -> u32 { self.writes.get() }
/// }
///
/// let feed = Feed { lines: ["boot", "ready"], writes: Cell::new(0) };
/// let t = Ticker { model: &feed, gate: DataGate::new() };
/// assert!(t.dirty(), "a fresh widget owes its first paint");
/// t.mark_clean();
/// assert!(!t.dirty(), "nothing changed, nothing to send");
///
/// feed.writes.set(1); // the application wrote a line
/// assert!(t.dirty(), "…and the widget noticed on its own");
/// ```
///
/// A gate is **not** needed by a widget that owns its state - a
/// [`Counter`](crate::Counter) sets a plain `Cell<bool>` in its own `update`.
/// Reach for it when the picture depends on something the widget only borrows.
#[derive(Debug)]
pub struct DataGate {
    /// Set by the widget's own state changes, and by the owner.
    dirty: Cell<bool>,
    /// The model's revision as of the last paint.
    seen: Cell<u32>,
}

impl DataGate {
    /// A gate that owes a first paint.
    pub const fn new() -> Self {
        Self {
            dirty: Cell::new(true),
            seen: Cell::new(0),
        }
    }

    /// Whether the widget has to repaint: its own flag is set, **or** the model
    /// is at a different revision than the one on screen.
    pub fn is_dirty(&self, revision: u32) -> bool {
        self.dirty.get() || revision != self.seen.get()
    }

    /// Records a paint: the flag goes down and `revision` becomes what the
    /// panel is showing. Called from
    /// [`Component::mark_clean`](crate::Component::mark_clean), so it takes
    /// `&self`.
    pub fn mark_clean(&self, revision: u32) {
        self.dirty.set(false);
        self.seen.set(revision);
    }

    /// Demands a repaint - the widget's own state moved, or its owner changed
    /// data behind a model that keeps no revision.
    pub fn mark_dirty(&self) {
        self.dirty.set(true);
    }
}

impl Default for DataGate {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The default a model gets for free: a constant revision. The comparison
    /// can then never fire, so the gate is exactly the `Cell<bool>` it
    /// replaced, which is why adding it to a widget changes nothing for the
    /// models that were already there.
    #[test]
    fn a_constant_revision_leaves_the_plain_flag_behaviour() {
        let g = DataGate::new();
        assert!(g.is_dirty(0));
        g.mark_clean(0);
        assert!(!g.is_dirty(0));
        g.mark_dirty();
        assert!(g.is_dirty(0));
        g.mark_clean(0);
        assert!(!g.is_dirty(0));
    }

    /// ...and a model that does keep one is noticed without being asked.
    #[test]
    fn a_changed_revision_is_a_repaint() {
        let g = DataGate::new();
        g.mark_clean(7);
        assert!(!g.is_dirty(7));
        assert!(g.is_dirty(8), "the data moved under the widget");
        g.mark_clean(8);
        assert!(!g.is_dirty(8));
    }

    /// The revision is compared, not ordered: a model free to hand back a cheap
    /// fingerprint of its content (rather than a counter) may return a value it
    /// has used before, and going back to an old revision is still a change.
    #[test]
    fn any_different_value_counts_including_an_older_one() {
        let g = DataGate::new();
        g.mark_clean(9);
        assert!(g.is_dirty(2));
        g.mark_clean(2);
        assert!(g.is_dirty(9));
    }

    /// A non-zero starting revision is not mistaken for a change: the gate
    /// starts dirty anyway, and the first paint records whatever it was.
    #[test]
    fn the_first_paint_adopts_whatever_revision_it_found() {
        let g = DataGate::new();
        assert!(g.is_dirty(1234));
        g.mark_clean(1234);
        assert!(!g.is_dirty(1234));
    }
}
