use core::cell::Cell;

use crate::{Area, Component, Msg, RenderTarget, Style, V_SCROLL_RESERVE, draw_v_scroll};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the first `max_chars` Unicode scalar values of `s` as a `&str`.
/// No allocation - slices at a char boundary.
fn truncate_str(s: &str, max_chars: usize) -> &str {
    s.char_indices().nth(max_chars).map(|(i, _)| &s[..i]).unwrap_or(s)
}

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
/// Parent nodes get a pixel expander ([`draw_expander`](RenderTarget::draw_expander)
/// - triangle on a pixel target); each nesting level draws a thin indent guide.
/// The selected node is `Style::Focus`, the rest `Muted`. Scrolls (never
/// truncates) and shows the built-in scroll indicator on overflow.
///
/// ## Capacity
/// Expansion is a `u64` bitmask, so at most **64 nodes** can be expanded
/// individually; nodes at index `>= 64` are always collapsed.
pub struct Tree<'a, M: TreeModel + ?Sized = [TreeItem<'a>]> {
    model: &'a M,
    selected: usize,
    offset: usize,
    expanded: u64,
    focused: bool,
    indent: u16,
    page_size: Cell<usize>,
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
            page_size: Cell::new(usize::MAX),
        }
    }

    /// Sets the per-depth indentation, in pixels.
    pub fn with_indent(mut self, px: u16) -> Self {
        self.indent = px;
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
        (0..self.model.item_count()).filter(|&i| self.is_visible(i)).count()
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
    fn update(&mut self, msg: &Msg) {
        if self.model.item_count() == 0 {
            return;
        }
        match msg {
            Msg::Down => {
                if let Some(n) = self.next_visible(self.selected) {
                    self.selected = n;
                    self.scroll_into_view();
                }
            }
            Msg::Up => {
                if let Some(p) = self.prev_visible(self.selected) {
                    self.selected = p;
                    self.scroll_into_view();
                }
            }
            Msg::Right => {
                if self.has_children(self.selected) {
                    self.set_expanded(self.selected, true);
                }
            }
            Msg::Left if self.has_children(self.selected) && self.is_expanded(self.selected) => {
                self.set_expanded(self.selected, false);
            }
            Msg::Select if self.has_children(self.selected) => {
                let e = self.is_expanded(self.selected);
                self.set_expanded(self.selected, !e);
            }
            _ => {}
        }
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
            let style = if idx == self.selected { Style::Focus } else { Style::Muted };

            // Indent guides: a thin vertical line at each ancestor level.
            for level in 0..d {
                let gx = area.x.saturating_add(level.saturating_mul(indent_px));
                target.fill_rect(Area::new(gx, y, 1, line_h), Style::Muted);
            }

            // Expander for parents only (leaves get none).
            if self.has_children(idx) {
                target.draw_expander(
                    Area::new(base_x, y, cw, line_h),
                    self.is_expanded(idx),
                    style,
                );
            }

            // Label one expander-slot + gap (2 chars) past the node's base.
            let label_x = base_x.saturating_add(2 * cw);
            let used = d.saturating_mul(indent_px).saturating_add(2 * cw);
            let avail = content_w.saturating_sub(used);
            let max = (avail / cw) as usize;
            if max > 0 {
                target.draw_text(label_x, y, truncate_str(self.model.get_item(idx), max), style);
            }

            maybe = self.next_visible(idx);
        }

        if overflow {
            draw_v_scroll(target, area, total, visible_rows, self.visible_before(self.offset));
        }
    }

    fn focus(&mut self) {
        self.focused = true;
    }

    fn blur(&mut self) {
        self.focused = false;
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
        let tree = Tree::new(ITEMS);
        let mut t = RecordingTarget::new(160, 80); // 8 rows
        tree.view(&mut t, Area::new(0, 0, 160, 80));
        let tx = texts(&t);
        // Roots: Settings (sel, parent), Sensors (parent), About (leaf).
        assert!(tx.contains(&(0, 0, ">".into(), Style::Focus))); // Settings expander
        assert!(tx.contains(&(12, 0, "Settings".into(), Style::Focus)));
        assert!(tx.contains(&(0, 10, ">".into(), Style::Muted))); // Sensors expander
        assert!(tx.iter().any(|(x, y, s, _)| *x == 12 && *y == 10 && s == "Sensors"));
        // About is a leaf → no expander on its row (y = 20).
        assert!(tx.iter().any(|(x, y, s, _)| *x == 12 && *y == 20 && s == "About"));
        assert!(!tx.iter().any(|(x, y, ..)| *x == 0 && *y == 20));
        // Child nodes hidden.
        assert!(!tx.iter().any(|(_, _, s, _)| s == "Display" || s == "Brightness"));
    }

    #[test]
    fn tree_expand_shows_child_with_guide_and_triangle() {
        let mut tree = Tree::new(ITEMS);
        tree.update(&Msg::Select); // expand Settings
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
        tree.update(&Msg::Down); // Settings collapsed → next root Sensors (5)
        assert_eq!(tree.selected(), 5);
        assert_eq!(tree.selected_item(), "Sensors");
    }

    #[test]
    fn tree_right_expands_left_collapses() {
        let mut tree = Tree::new(ITEMS);
        tree.update(&Msg::Right);
        assert!(tree.is_expanded_node(0));
        tree.update(&Msg::Left);
        assert!(!tree.is_expanded_node(0));
    }

    #[test]
    fn tree_select_again_collapses() {
        let mut tree = Tree::new(ITEMS);
        tree.update(&Msg::Select);
        tree.update(&Msg::Select);
        assert!(!tree.is_expanded_node(0));
    }

    #[test]
    fn tree_scroll_indicator_on_overflow_keeps_focus_visible() {
        // Expand everything visible and constrain to 2 rows so it overflows.
        let mut tree = Tree::new(ITEMS);
        let mut t0 = RecordingTarget::new(160, 20); // 2 rows
        tree.view(&mut t0, Area::new(0, 0, 160, 20)); // page ← 2
        // 3 visible roots > 2 rows → indicator present.
        assert!(fills(&t0).iter().any(|(_, st)| *st == Style::Focus)); // thumb
        // Move down twice → About (7); offset advances so it stays visible.
        tree.update(&Msg::Down); // Sensors (5)
        tree.update(&Msg::Down); // About (7)
        let mut t1 = RecordingTarget::new(160, 20);
        tree.view(&mut t1, Area::new(0, 0, 160, 20));
        assert!(texts(&t1).iter().any(|(_, _, s, st)| s == "About" && *st == Style::Focus));
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
}
