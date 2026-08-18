use core::cell::Cell;

use crate::{
    Area, Component, DataGate, Marker, Msg, Outcome, RenderTarget, Style, V_SCROLL_RESERVE,
    draw_cursor_band, draw_v_scroll,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max_chars` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    s.char_indices()
        .nth(max_chars)
        .map(|(i, _)| &s[..i])
        .unwrap_or(s)
}

/// The slot a row's expander (or a leaf's cursor marker) occupies, in
/// characters - the label starts after it plus a one-character gap.
const SLOT_CHARS: u16 = 2;

// ── TreeItem ────────────────────────────────────────────────────────────────

/// A single node in a [`Tree`], in depth-first order.
///
/// `depth` is the nesting level (0 for roots). Children of a node are the
/// following nodes at `depth + 1`, until the depth returns to that node's level
/// or lower.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeItem<'a> {
    pub label: &'a str,
    pub depth: u8,
}

impl<'a> TreeItem<'a> {
    pub const fn new(label: &'a str, depth: u8) -> Self {
        Self { label, depth }
    }
}

// ── TreeModel (data provider) ───────────────────────────────────────────────

/// Structure behind a [`Tree`] (mirrors [`ListModel`](crate::ListModel)): a node
/// count, each node's label, and its nesting `depth`. Expansion and selection are
/// **widget** state, not the model's.
///
/// `get_item` returns a borrowed `&str` - enough for stored/static content;
/// streaming content is a `Pager` concern instead.
pub trait TreeModel {
    /// Number of nodes (in depth-first order).
    fn item_count(&self) -> usize;
    /// Label of node `i`.
    fn get_item(&self, i: usize) -> &str;
    /// Nesting level of node `i` (0 for roots).
    fn depth(&self, i: usize) -> u8;

    /// A number that changes whenever the nodes do - see
    /// [`ListModel::revision`](crate::ListModel::revision), which this mirrors.
    /// The default is a constant ("I keep no revision"), so a static tree costs
    /// nothing and behaves as before.
    fn revision(&self) -> u32 {
        0
    }
}

/// Backwards-compatible static impl over a slice of [`TreeItem`]s.
impl TreeModel for [TreeItem<'_>] {
    fn item_count(&self) -> usize {
        self.len()
    }
    fn get_item(&self, i: usize) -> &str {
        self[i].label
    }
    fn depth(&self, i: usize) -> u8 {
        self[i].depth
    }
}

/// Array impl so an inline literal works directly under the generic `M`.
impl<const N: usize> TreeModel for [TreeItem<'_>; N] {
    fn item_count(&self) -> usize {
        N
    }
    fn get_item(&self, i: usize) -> &str {
        self[i].label
    }
    fn depth(&self, i: usize) -> u8 {
        self[i].depth
    }
}

// ── Tree ──────────────────────────────────────────────────────────────────────

/// A vertically scrolling tree of expandable nodes, pixel-laid-out and backed by
/// a [`TreeModel`].
///
/// Parent nodes get a pixel expander
/// ([`draw_expander`](RenderTarget::draw_expander) - triangle on a pixel
/// target); each nesting level draws a thin indent guide. Nodes are `Muted`; the
/// selected one follows the focus language (see [`draw_cursor_band`]) - a
/// full-width band while the tree holds focus, plain `Normal` when it does not.
/// A leaf under the cursor gets a [`Marker`] in its (otherwise empty) expander
/// slot, so an unfocused tree shows where the cursor is on any row, not only on
/// parents - configurable, like the one on [`List`](crate::List),
/// [`Table`](crate::Table) and [`Radio`](crate::Radio).
/// Scrolls (never truncates) and shows the built-in scroll indicator on overflow.
///
/// ## Capacity
/// Expansion is a `u64` bitmask, so at most **64 nodes** can be expanded
/// individually; nodes at index `>= 64` are always collapsed.
///
/// ## Elm cycle note
/// The visible-row count is captured from `area.h / line_height` on each
/// [`view`](Tree::view) call and consumed by the next [`update`](Tree::update) to
/// compute scroll offsets. In the standard embedded loop - **render, then handle
/// input** - this is always in sync. Before the first frame it is `usize::MAX`
/// ("everything fits"), so an `update` that arrives ahead of any `view` moves
/// the cursor without scrolling the window under it.
pub struct Tree<'a, M: TreeModel + ?Sized = [TreeItem<'a>]> {
    model: &'a M,
    selected: usize,
    offset: usize,
    expanded: u64,
    focused: bool,
    indent: u16,
    marker: Marker,
    page_size: Cell<usize>,
    // Repaint gate: set when the selection, the scroll offset, the expansion
    // mask or focus changes. Starts dirty so the first frame always draws, and
    // carries the model revision painted with it (see `TreeModel::revision`).
    gate: DataGate,
}

impl<'a, M: TreeModel + ?Sized> Tree<'a, M> {
    pub fn new(model: &'a M) -> Self {
        Self {
            model,
            selected: 0,
            offset: 0,
            expanded: 0,
            focused: false,
            indent: 8, // per-depth indent, in pixels
            marker: Marker::ARROW,
            // usize::MAX → "everything fits" until the first view() call.
            page_size: Cell::new(usize::MAX),
            gate: DataGate::new(),
        }
    }

    /// Sets the per-depth indentation, in pixels.
    pub fn with_indent(mut self, px: u16) -> Self {
        self.indent = px;
        self
    }

    /// Sets the cursor marker drawn on a **leaf** under the cursor
    /// ([`Marker::NONE`] to drop it).
    ///
    /// Only the marker's `selected` half is ever drawn, and only on a leaf: the
    /// slot it goes in belongs to the expander, which a parent row uses for its
    /// triangle. So this is the cursor glyph, not a column - the slot is
    /// reserved on every row regardless, and nothing shifts as the cursor moves.
    /// A marker wider than the two-character slot is truncated by the label
    /// that follows it.
    pub fn with_marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Index of the currently highlighted node.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Label of the currently highlighted node, or `""` for an empty tree.
    pub fn selected_item(&self) -> &str {
        if self.selected < self.model.item_count() {
            self.model.get_item(self.selected)
        } else {
            ""
        }
    }

    /// Whether the node at `idx` is currently expanded.
    pub fn is_expanded_node(&self, idx: usize) -> bool {
        self.is_expanded(idx)
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// A node has children when the next node is deeper.
    fn has_children(&self, idx: usize) -> bool {
        let n = self.model.item_count();
        idx + 1 < n && self.model.depth(idx + 1) > self.model.depth(idx)
    }

    fn is_expanded(&self, idx: usize) -> bool {
        idx < 64 && (self.expanded >> idx) & 1 == 1
    }

    fn set_expanded(&mut self, idx: usize, v: bool) {
        if idx >= 64 {
            return;
        }
        if v {
            self.expanded |= 1 << idx;
        } else {
            self.expanded &= !(1 << idx);
        }
    }

    /// A node is visible when all of its ancestors are expanded.
    fn is_visible(&self, idx: usize) -> bool {
        let d = self.model.depth(idx);
        if d == 0 {
            return true;
        }
        let mut need = d;
        let mut j = idx;
        while j > 0 {
            j -= 1;
            let dj = self.model.depth(j);
            if dj < need {
                if !self.is_expanded(j) {
                    return false;
                }
                need = dj;
                if need == 0 {
                    return true;
                }
            }
        }
        true
    }

    /// Smallest visible index greater than `from`.
    fn next_visible(&self, from: usize) -> Option<usize> {
        let n = self.model.item_count();
        let mut i = from + 1;
        while i < n {
            if self.is_visible(i) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Largest visible index smaller than `from`.
    fn prev_visible(&self, from: usize) -> Option<usize> {
        let mut i = from;
        while i > 0 {
            i -= 1;
            if self.is_visible(i) {
                return Some(i);
            }
        }
        None
    }

    /// Number of visible nodes in `a..=b`.
    fn visible_between(&self, a: usize, b: usize) -> usize {
        let n = self.model.item_count();
        let mut count = 0;
        let mut i = a;
        while i <= b && i < n {
            if self.is_visible(i) {
                count += 1;
            }
            i += 1;
        }
        count
    }

    /// Total number of currently-visible nodes.
    fn total_visible(&self) -> usize {
        (0..self.model.item_count())
            .filter(|&i| self.is_visible(i))
            .count()
    }

    /// Number of visible nodes strictly before `idx` (the scroll rank of `idx`).
    fn visible_before(&self, idx: usize) -> usize {
        (0..idx).filter(|&i| self.is_visible(i)).count()
    }

    fn scroll_into_view(&mut self) {
        let page = self.page_size.get();
        if page == 0 {
            return;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
            return;
        }
        while self.visible_between(self.offset, self.selected) > page {
            match self.next_visible(self.offset) {
                Some(j) => self.offset = j,
                None => break,
            }
        }
    }
}

impl<'a, M: TreeModel + ?Sized> Component for Tree<'a, M> {
    fn update(&mut self, msg: &Msg) -> Outcome {
        if self.model.item_count() == 0 {
            // Nothing to walk: every event stays available to the container.
            return Outcome::Ignored;
        }
        // Navigation, expansion and scrolling all read out of three fields;
        // comparing them afterwards is cheaper than threading a flag through
        // every arm - and it cannot forget one. Re-expanding an already open
        // node, or Up at the top, leaves all three alone and stays clean.
        let before = (self.selected, self.offset, self.expanded);
        let outcome = match msg {
            Msg::Down => match self.next_visible(self.selected) {
                Some(n) => {
                    self.selected = n;
                    self.scroll_into_view();
                    Outcome::Consumed
                }
                // Last visible node: the cursor has run out of tree.
                None => Outcome::Ignored,
            },
            Msg::Up => match self.prev_visible(self.selected) {
                Some(p) => {
                    self.selected = p;
                    self.scroll_into_view();
                    Outcome::Consumed
                }
                None => Outcome::Ignored,
            },
            // Opening an already-open node (or a leaf) has nothing to do.
            Msg::Right if self.has_children(self.selected) && !self.is_expanded(self.selected) => {
                self.set_expanded(self.selected, true);
                Outcome::Consumed
            }
            Msg::Left if self.has_children(self.selected) && self.is_expanded(self.selected) => {
                self.set_expanded(self.selected, false);
                Outcome::Consumed
            }
            // On a parent, Select is the tree's own fold/unfold - it goes no
            // further. On a leaf there is nothing to fold, and picking a leaf
            // is the choice the app acts on (it reads selected()).
            Msg::Select if self.has_children(self.selected) => {
                let e = self.is_expanded(self.selected);
                self.set_expanded(self.selected, !e);
                Outcome::Consumed
            }
            Msg::Select => Outcome::Activated,
            _ => Outcome::Ignored,
        };
        if (self.selected, self.offset, self.expanded) != before {
            self.gate.mark_dirty();
        }
        outcome
    }

    /// A widget that stacks rows needs a row: given less, it can paint nothing
    /// at all, and `view` must not call that a paint (see
    /// [`min_size`](Component::min_size)).
    fn min_size(&self, target: &dyn RenderTarget) -> (u16, u16) {
        (1, target.line_height().max(1))
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        let line_h = target.line_height().max(1);
        let visible_rows = (area.h / line_h) as usize;
        self.page_size.set(visible_rows.max(1));

        let n = self.model.item_count();
        if area.w == 0 || area.h == 0 || visible_rows == 0 || n == 0 {
            return;
        }

        let cw = target.char_width().max(1);
        let indent_px = self.indent.max(1);

        let total = self.total_visible();
        let overflow = total > visible_rows;
        let reserve = if overflow { V_SCROLL_RESERVE } else { 0 };
        let content_w = area.w.saturating_sub(reserve);

        let mut maybe = Some(self.offset);
        for row in 0..visible_rows {
            let idx = match maybe {
                Some(i) if i < n => i,
                _ => break,
            };
            let y = area.y.saturating_add(row as u16 * line_h);
            let d = self.model.depth(idx) as u16;
            let base_x = area.x.saturating_add(d.saturating_mul(indent_px));
            let style = if idx == self.selected {
                draw_cursor_band(
                    target,
                    Area::new(area.x, y, content_w, line_h),
                    self.focused,
                )
            } else {
                Style::Muted
            };

            // Indent guides: a thin vertical line at each ancestor level. Drawn
            // over the band, so a colour panel keeps them inside the selected
            // row; on monochrome the band swallows them, which is the point -
            // the focused row is one block.
            for level in 0..d {
                let gx = area.x.saturating_add(level.saturating_mul(indent_px));
                target.fill_rect(Area::new(gx, y, 1, line_h), Style::Muted);
            }

            // Expander for parents; for a leaf, the same slot carries the
            // cursor marker (and only then) - a leaf under the cursor of an
            // unfocused tree would otherwise be indistinguishable from its
            // neighbours, since a monochrome theme draws `Normal` and `Muted` in
            // the same ink. Using the slot the layout already reserves keeps
            // every row exactly as wide as before.
            if self.has_children(idx) {
                target.draw_expander(
                    Area::new(base_x, y, cw, line_h),
                    self.is_expanded(idx),
                    style,
                );
            } else if idx == self.selected && !self.marker.selected.is_empty() {
                target.draw_text(base_x, y, self.marker.selected, style);
            }

            // Label one expander-slot + gap (2 chars) past the node's base.
            let label_x = base_x.saturating_add(SLOT_CHARS * cw);
            let used = d.saturating_mul(indent_px).saturating_add(SLOT_CHARS * cw);
            let avail = content_w.saturating_sub(used);
            let max = (avail / cw) as usize;
            if max > 0 {
                target.draw_text(
                    label_x,
                    y,
                    truncate_str(self.model.get_item(idx), max),
                    style,
                );
            }

            maybe = self.next_visible(idx);
        }

        if overflow {
            draw_v_scroll(
                target,
                area,
                total,
                visible_rows,
                self.visible_before(self.offset),
            );
        }
    }

    fn focus(&mut self) {
        self.focused = true;
        self.gate.mark_dirty(); // focus decides whether the cursor row bands
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

    const ITEMS: &[TreeItem] = &[
        TreeItem::new("Settings", 0),   // 0  parent
        TreeItem::new("Display", 1),    // 1  parent
        TreeItem::new("Brightness", 2), // 2  leaf
        TreeItem::new("Contrast", 2),   // 3  leaf
        TreeItem::new("Sound", 1),      // 4  leaf
        TreeItem::new("Sensors", 0),    // 5  parent
        TreeItem::new("IMU", 1),        // 6  leaf
        TreeItem::new("About", 0),      // 7  leaf
    ];

    // Default RecordingTarget metrics: char_width = 6, line_height = 10.
    // Tree default indent = 8px. Label sits 2 chars (12px) past a node's base_x.

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
    fn tree_initial_shows_roots_only() {
        let mut tree = Tree::new(ITEMS);
        tree.focus(); // the selected row inverts only for the focused widget
        let mut t = RecordingTarget::new(160, 80); // 8 rows
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        let tx = texts(&t);
        // Roots: Settings (sel, parent), Sensors (parent), About (leaf).
        assert!(tx.contains(&(0, 0, ">".into(), Style::Focus))); // Settings expander
        assert!(tx.contains(&(12, 0, "Settings".into(), Style::Focus)));
        assert!(tx.contains(&(0, 10, ">".into(), Style::Muted))); // Sensors expander
        assert!(
            tx.iter()
                .any(|(x, y, s, _)| *x == 12 && *y == 10 && s == "Sensors")
        );
        // About is a leaf → no expander on its row (y = 20).
        assert!(
            tx.iter()
                .any(|(x, y, s, _)| *x == 12 && *y == 20 && s == "About")
        );
        assert!(!tx.iter().any(|(x, y, ..)| *x == 0 && *y == 20));
        // Child nodes hidden.
        assert!(
            !tx.iter()
                .any(|(_, _, s, _)| s == "Display" || s == "Brightness")
        );
    }

    #[test]
    fn tree_expand_shows_child_with_guide_and_triangle() {
        let mut tree = Tree::new(ITEMS);
        tree.focus();
        let _ = tree.update(&Msg::Select); // expand Settings
        assert!(tree.is_expanded_node(0));
        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        let tx = texts(&t);
        // Settings now expanded → "v".
        assert!(tx.contains(&(0, 0, "v".into(), Style::Focus)));
        // Display (depth 1) on row 1: base_x = 8.
        assert!(tx.contains(&(8, 10, ">".into(), Style::Muted))); // its own expander
        assert!(tx.contains(&(20, 10, "Display".into(), Style::Muted))); // label at 8+12
        // Indent guide for level 0 on the depth-1 row.
        assert!(fills(&t).contains(&(Area::new(0, 10, 1, 10), Style::Muted)));
    }

    #[test]
    fn tree_down_skips_hidden() {
        let mut tree = Tree::new(ITEMS);
        let _ = tree.update(&Msg::Down); // Settings collapsed → next root Sensors (5)
        assert_eq!(tree.selected(), 5);
        assert_eq!(tree.selected_item(), "Sensors");
    }

    #[test]
    fn tree_right_expands_left_collapses() {
        let mut tree = Tree::new(ITEMS);
        let _ = tree.update(&Msg::Right);
        assert!(tree.is_expanded_node(0));
        let _ = tree.update(&Msg::Left);
        assert!(!tree.is_expanded_node(0));
    }

    #[test]
    fn tree_select_again_collapses() {
        let mut tree = Tree::new(ITEMS);
        let _ = tree.update(&Msg::Select);
        let _ = tree.update(&Msg::Select);
        assert!(!tree.is_expanded_node(0));
    }

    #[test]
    fn tree_scroll_indicator_on_overflow_keeps_focus_visible() {
        // Expand everything visible and constrain to 2 rows so it overflows.
        let mut tree = Tree::new(ITEMS);
        tree.focus();
        let mut t0 = RecordingTarget::new(160, 20); // 2 rows
        tree.view(&mut t0, Area::new(0, 0, 160, 20)); // page ← 2
        // 3 visible roots > 2 rows → indicator present.
        assert!(fills(&t0).iter().any(|(_, st)| *st == Style::Focus)); // thumb
        // Move down twice → About (7); offset advances so it stays visible.
        let _ = tree.update(&Msg::Down); // Sensors (5)
        let _ = tree.update(&Msg::Down); // About (7)
        let mut t1 = RecordingTarget::new(160, 20);
        tree.view(&mut t1, Area::new(0, 0, 160, 20));
        assert!(
            texts(&t1)
                .iter()
                .any(|(_, _, s, st)| s == "About" && *st == Style::Focus)
        );
    }

    // ── Focus language (band vs bare cursor) ───────────────────────────────────

    fn bands(t: &RecordingTarget) -> Vec<(Area, Style)> {
        t.ops()
            .iter()
            .filter_map(|op| match op {
                Op::Band { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect()
    }

    /// A focused tree bands the selected node's whole row - the indent guides and
    /// the expander live inside the block, not beside it.
    #[test]
    fn focused_tree_bands_the_selected_row() {
        let mut tree = Tree::new(ITEMS);
        tree.focus();
        let mut t = RecordingTarget::new(160, 80); // 8 rows, 3 roots → no overflow
        tree.view(&mut t, Area::new(0, 0, 160, 80));

        assert_eq!(bands(&t), [(Area::new(0, 0, 160, 10), Style::Focus)]);
        assert!(texts(&t).contains(&(12, 0, "Settings".into(), Style::Focus)));
    }

    /// An unfocused tree keeps its cursor visible (the row is `Normal` among
    /// `Muted` ones) without inverting it.
    #[test]
    fn unfocused_tree_marks_the_cursor_row_without_a_band() {
        let tree = Tree::new(ITEMS);
        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));

        assert!(bands(&t).is_empty());
        let tx = texts(&t);
        assert!(tx.contains(&(12, 0, "Settings".into(), Style::Normal)));
        assert!(tx.contains(&(0, 0, ">".into(), Style::Normal))); // its expander
        assert!(
            tx.iter()
                .any(|(_, y, s, st)| *y == 10 && s == "Sensors" && *st == Style::Muted)
        );
    }

    // ── Leaf cursor ───────────────────────────────────────────────────────────

    /// The expander marks the cursor only on rows that *have* children. A leaf
    /// under the cursor of an unfocused tree used to be indistinguishable from
    /// its neighbours; it now gets a marker in the same slot, so no width moves.
    #[test]
    fn unfocused_tree_marks_a_leaf_cursor() {
        let mut tree = Tree::new(ITEMS);
        let _ = tree.update(&Msg::Down); // Sensors (parent)
        let _ = tree.update(&Msg::Down); // About (leaf, row 2)
        assert_eq!(tree.selected_item(), "About");

        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        let tx = texts(&t);
        assert!(bands(&t).is_empty());
        // The marker sits in the (otherwise empty) expander slot at x = 0.
        assert!(
            tx.contains(&(0, 20, Marker::ARROW.selected.into(), Style::Normal)),
            "no leaf cursor: {tx:?}"
        );
        // Rows 0 and 1 are parents, so their slots hold expanders, not markers.
        assert!(tx.contains(&(0, 0, ">".into(), Style::Muted)));
        assert!(tx.contains(&(0, 10, ">".into(), Style::Muted)));
        // (A leaf that is *not* under the cursor keeps an empty slot - see
        // `tree_initial_shows_roots_only`, which asserts exactly that for this
        // same "About" row while the cursor sits on "Settings".)
    }

    /// Focused, that marker is part of the band like everything else on the row.
    #[test]
    fn focused_tree_leaf_marker_joins_the_band() {
        let mut tree = Tree::new(ITEMS);
        tree.focus();
        let _ = tree.update(&Msg::Down);
        let _ = tree.update(&Msg::Down); // About
        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        assert!(texts(&t).contains(&(0, 20, Marker::ARROW.selected.into(), Style::Focus)));
        assert_eq!(bands(&t), [(Area::new(0, 20, 160, 10), Style::Focus)]);
    }

    /// The glyph is a `Marker`, like every other cursor in the library - and
    /// `Marker::NONE` gives it up without moving the labels, since the slot
    /// belongs to the expander either way.
    #[test]
    fn the_leaf_cursor_is_a_marker() {
        let pick = |marker: Marker| {
            let mut tree = Tree::new(ITEMS).with_marker(marker);
            let _ = tree.update(&Msg::Down);
            let _ = tree.update(&Msg::Down); // About, a leaf
            let mut t = RecordingTarget::new(160, 80);
            tree.view(&mut t, Area::new(0, 0, 160, 80));
            texts(&t)
        };

        let starred = pick(Marker::new("*", " "));
        assert!(starred.contains(&(0, 20, "*".into(), Style::Normal)));
        // …and the label has not moved: the slot was already reserved.
        assert!(starred.contains(&(12, 20, "About".into(), Style::Normal)));

        let none = pick(Marker::NONE);
        assert!(!none.iter().any(|(x, y, ..)| *x == 0 && *y == 20));
        assert!(none.contains(&(12, 20, "About".into(), Style::Normal)));
    }

    /// A parent keeps its expander - the marker is only for the rows that had
    /// nothing in that slot.
    #[test]
    fn tree_parent_cursor_still_draws_the_expander() {
        let tree = Tree::new(ITEMS); // cursor on "Settings", a parent
        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        let tx = texts(&t);
        assert!(tx.contains(&(0, 0, ">".into(), Style::Normal)));
        assert_eq!(
            tx.iter().filter(|(x, y, ..)| *x == 0 && *y == 0).count(),
            1,
            "the expander and a marker must not stack in one slot"
        );
    }

    #[test]
    fn tree_empty_safe() {
        let tree = Tree::new(&[] as &[TreeItem]);
        let mut t = RecordingTarget::new(160, 80);
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        // Partial-redraw: view() clears its own area but draws no nodes.
        assert!(t.ops().iter().all(|op| matches!(op, Op::Clear { .. })));
    }

    /// A tiny custom model (structure computed outside the widget).
    struct Flat;
    impl TreeModel for Flat {
        fn item_count(&self) -> usize {
            2
        }
        fn get_item(&self, i: usize) -> &str {
            ["a", "b"][i]
        }
        fn depth(&self, _i: usize) -> u8 {
            0
        }
    }

    #[test]
    fn tree_custom_model() {
        let m = Flat;
        let tree = Tree::new(&m);
        assert_eq!(tree.selected_item(), "a");
        let mut t = RecordingTarget::new(120, 30);
        tree.view(&mut t, Area::new(0, 0, 120, 30));
        assert!(texts(&t).iter().any(|(_, _, s, _)| s == "a"));
    }

    // ── Dirty gate ────────────────────────────────────────────────────────────

    #[test]
    fn tree_dirty_gate() {
        let mut tree = Tree::new(ITEMS);
        assert!(tree.dirty());
        tree.mark_clean();

        // Up at the top, Left on a collapsed node, Select on a leaf: no state
        // moves, so the picture does not change.
        let _ = tree.update(&Msg::Up);
        let _ = tree.update(&Msg::Left);
        let _ = tree.update(&Msg::Tick);
        assert!(!tree.dirty());

        let _ = tree.update(&Msg::Right); // expands "Settings"
        assert!(tree.dirty());
        tree.mark_clean();
        let _ = tree.update(&Msg::Right); // already expanded → no change
        assert!(!tree.dirty());

        let _ = tree.update(&Msg::Down); // moves to "Display"
        assert!(tree.dirty());
        tree.mark_clean();

        tree.focus();
        assert!(tree.dirty());
        tree.mark_clean();
        tree.blur();
        assert!(tree.dirty());
    }

    #[test]
    fn tree_clean_view_draws_nothing() {
        let mut tree = Tree::new(ITEMS);
        let area = Area::new(0, 0, 160, 80);
        let mut t0 = RecordingTarget::new(160, 80);
        tree.view(&mut t0, area);
        assert!(!t0.ops().is_empty());

        let _ = tree.update(&Msg::Up); // clamped at the top
        let mut t1 = RecordingTarget::new(160, 80);
        tree.view(&mut t1, area);
        assert!(t1.ops().is_empty());
    }

    // ── Outcome (event routing) ─────────────────────────────────────────────

    #[test]
    fn tree_spends_a_step_and_hands_back_an_edge() {
        let mut tree = Tree::new(ITEMS);
        // Collapsed: the three roots are the only visible nodes.
        assert_eq!(
            tree.update(&Msg::Up),
            Outcome::Ignored,
            "Up on the first root"
        );
        assert_eq!(
            tree.update(&Msg::Down),
            Outcome::Consumed,
            "a step in the middle"
        );
        assert_eq!(tree.update(&Msg::Down), Outcome::Consumed);
        assert_eq!(
            tree.update(&Msg::Down),
            Outcome::Ignored,
            "Down on the last node"
        );
    }

    /// Select means two different things in a tree, so it reports two different
    /// outcomes: folding a parent is the tree's own business, picking a leaf is
    /// the app's.
    #[test]
    fn tree_select_folds_a_parent_and_activates_a_leaf() {
        let mut tree = Tree::new(ITEMS);
        assert_eq!(
            tree.update(&Msg::Select),
            Outcome::Consumed,
            "expand \"Settings\""
        );
        assert_eq!(
            tree.update(&Msg::Select),
            Outcome::Consumed,
            "collapse it again"
        );

        // Walk to "About" (index 7), a root-level leaf.
        while tree.selected() != 7 {
            let _ = tree.update(&Msg::Down);
        }
        assert_eq!(
            tree.update(&Msg::Select),
            Outcome::Activated,
            "picking a leaf"
        );
    }

    /// Left/Right (keyboards only) fold explicitly: asking for the state the
    /// node is already in has nothing to do, so the event comes back.
    #[test]
    fn tree_explicit_fold_hands_back_a_no_op() {
        let mut tree = Tree::new(ITEMS);
        assert_eq!(
            tree.update(&Msg::Left),
            Outcome::Ignored,
            "already collapsed"
        );
        assert_eq!(tree.update(&Msg::Right), Outcome::Consumed, "expanded");
        assert_eq!(
            tree.update(&Msg::Right),
            Outcome::Ignored,
            "already expanded"
        );
        assert_eq!(tree.update(&Msg::Left), Outcome::Consumed, "collapsed");
    }

    #[test]
    fn empty_tree_hands_back_everything() {
        const NONE: &[TreeItem] = &[];
        let mut tree = Tree::new(NONE);
        for msg in [Msg::Up, Msg::Down, Msg::Select, Msg::Tick] {
            assert_eq!(
                tree.update(&msg),
                Outcome::Ignored,
                "{msg:?} on an empty tree"
            );
        }
    }

    // ── A model that keeps a revision ───────────────────────────────────────

    /// A device list that gains nodes as things are discovered.
    struct Discovered {
        nodes: [TreeItem<'static>; 3],
        len: Cell<usize>,
        writes: Cell<u32>,
    }

    impl TreeModel for Discovered {
        fn item_count(&self) -> usize {
            self.len.get()
        }
        fn get_item(&self, i: usize) -> &str {
            self.nodes[i].label
        }
        fn depth(&self, i: usize) -> u8 {
            self.nodes[i].depth
        }
        fn revision(&self) -> u32 {
            self.writes.get()
        }
    }

    #[test]
    fn a_tree_repaints_when_its_model_says_the_nodes_moved() {
        let found = Discovered {
            nodes: [
                TreeItem::new("bus", 0),
                TreeItem::new("sensor", 1),
                TreeItem::new("relay", 1),
            ],
            len: Cell::new(1),
            writes: Cell::new(0),
        };
        let tree = Tree::new(&found);
        let area = Area::new(0, 0, 120, 30);
        let mut t = RecordingTarget::new(128, 64);

        tree.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area));
        tree.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), None, "nothing moved");

        found.len.set(3);
        found.writes.set(1);
        tree.view(&mut t, area);
        assert_eq!(t.take_dirty_rect(), Some(area), "the nodes that arrived");
    }
}
