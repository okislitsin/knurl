use crate::{Area, Component, Msg, RenderTarget, Style};

// Geometry of the built-in scroll indicator (a thin track + thumb at the right
// edge, drawn only when the field stack overflows the form area). Mirrors the
// indicator baked into [`List`](crate::List) so the two read identically.
const SCROLLBAR_W: u16 = 3;
const SCROLLBAR_GAP: u16 = 1;
const TRACK_W: u16 = 1;
const MIN_THUMB_PX: u16 = 3;

// ── FormField ─────────────────────────────────────────────────────────────────

/// A field that can live inside a [`Form`].
///
/// `editable()` selects the behaviour under the 3-button scheme:
/// - `false` (*momentary*): `Select` acts immediately (a toggle); `Up`/`Down`
///   always move the focus between fields.
/// - `true` (*editable*): `Select` enters/leaves an edit mode; while editing,
///   `Up`/`Down` are delivered to the field instead of moving focus.
pub trait FormField: Component {
    /// The vertical space, in pixels, the field needs in the form's layout.
    ///
    /// The default is a single text row ([`line_height`](RenderTarget::line_height)).
    /// A multi-row field overrides this - e.g. [`TextInput`](crate::TextInput)
    /// reports two rows *while editing* (text + token ribbon). [`Form::view`]
    /// stacks fields by this height, so a field may declare a **state-dependent**
    /// height; the form re-lays-out on every `view()`, so fields below it shift
    /// as it grows/shrinks (an accepted trade-off - see [`Form::view`]).
    fn height(&self, target: &dyn RenderTarget) -> u16 {
        target.line_height()
    }

    fn editable(&self) -> bool {
        false
    }

    /// Notifies an editable field whether the form is currently in edit mode on
    /// it. The field can use this to distinguish "focused" from "being edited"
    /// when rendering (e.g. a [`Counter`](crate::Counter) highlights its value
    /// only while editing). Momentary fields ignore it - the default is empty.
    fn set_editing(&mut self, _editing: bool) {}

    /// Whether the field consumes `Select` (and `Up`/`Down`) itself **while the
    /// form is editing it**, instead of `Select` leaving edit mode.
    ///
    /// Most editable fields ([`Counter`](crate::Counter), `Slider`, `Picker`,
    /// `Radio`) return `false`: `Select` enters/leaves edit and `Up`/`Down`
    /// adjust the value. A field like [`TextInput`](crate::TextInput) needs
    /// `Select` *inside* the edit (to pick a character / backspace / finish), so
    /// it returns `true`; the form then routes `Select` into the field and only
    /// leaves edit mode when [`editing_finished`](FormField::editing_finished)
    /// reports completion.
    fn captures_select_while_editing(&self) -> bool {
        false
    }

    /// For a select-capturing editable field, reports that its edit interaction
    /// has finished (e.g. [`TextInput`](crate::TextInput)'s "done" token), so the
    /// form should leave edit mode. Default `false`.
    fn editing_finished(&self) -> bool {
        false
    }
}

// ── Form ──────────────────────────────────────────────────────────────────────

/// A focus/edit-mode controller for a set of [`FormField`]s under 3 buttons
/// (`Up`/`Down`/`Select`).
///
/// `Form` holds no references to its fields and carries no lifetime - the field
/// slice is passed to each call. The currently focused field is highlighted by
/// its own focused style; `Form` only drives `focus()`/`blur()` as focus moves.
#[derive(Debug)]
pub struct Form {
    focus: usize,
    editing: bool,
}

impl Form {
    pub const fn new() -> Self {
        Self {
            focus: 0,
            editing: false,
        }
    }

    /// Index of the currently focused field.
    pub fn focus_index(&self) -> usize {
        self.focus
    }

    /// Whether the focused (editable) field is currently in edit mode.
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// Focuses the current field and blurs the rest. Call once after creation
    /// (and whenever the field set changes) so the initial focus renders.
    pub fn sync_focus(&self, fields: &mut [&mut dyn FormField]) {
        for (i, f) in fields.iter_mut().enumerate() {
            if i == self.focus {
                f.focus();
                // Keep the focused field's edit flag in step with the form.
                f.set_editing(self.editing);
            } else {
                f.blur();
                // Edit mode must never linger on an unfocused field.
                f.set_editing(false);
            }
        }
    }

    pub fn update(&mut self, msg: &Msg, fields: &mut [&mut dyn FormField]) {
        let n = fields.len();
        if n == 0 {
            return;
        }
        if self.focus >= n {
            self.focus = n - 1;
        }
        match msg {
            Msg::Select => {
                if self.editing && fields[self.focus].captures_select_while_editing() {
                    // The field wants Select for itself (e.g. TextInput picks a
                    // character). Deliver it, then leave edit only if the field
                    // says its interaction is finished.
                    fields[self.focus].update(&Msg::Select);
                    if fields[self.focus].editing_finished() {
                        self.editing = false;
                        fields[self.focus].set_editing(false);
                    }
                } else if fields[self.focus].editable() {
                    self.editing = !self.editing; // enter/leave edit mode
                    fields[self.focus].set_editing(self.editing);
                } else {
                    fields[self.focus].update(&Msg::Select); // momentary toggle
                }
            }
            // While editing, Up/Down go to the field. This guarded arm MUST sit
            // above the plain Up/Down arms or the guard never gets a chance.
            Msg::Up | Msg::Down if self.editing => {
                fields[self.focus].update(msg);
            }
            Msg::Up if self.focus > 0 => {
                self.focus -= 1;
                self.sync_focus(fields);
            }
            Msg::Down if self.focus + 1 < n => {
                self.focus += 1;
                self.sync_focus(fields);
            }
            _ => {}
        }
    }

    /// Stacks the fields top to bottom, each given the height it reports via
    /// [`FormField::height`] (so a 2-row field like an editing
    /// [`TextInput`](crate::TextInput) gets two rows). Focus navigation stays
    /// index-based; the layout is recomputed here on every call.
    ///
    /// **Scrolling (no truncation of the focused field):** if the summed field
    /// heights exceed `area.h`, the stack is scrolled by whole pixels so the
    /// focused field is fully visible, and a thin scroll indicator is drawn at the
    /// right edge (as in [`List`](crate::List)). Fields that would only partially
    /// fit the window are skipped - the focused field, by construction of the
    /// scroll offset, always fits.
    ///
    /// Because a field's height may depend on its state (an editing TextInput
    /// grows from one row to two), entering edit can shift the fields below it.
    /// This is intentional and cheap - the alternative (reserving each field's
    /// max height up front) wastes rows on the common non-editing case.
    pub fn view(&self, target: &mut dyn RenderTarget, area: Area, fields: &[&dyn FormField]) {
        if area.w == 0 || area.h == 0 || fields.is_empty() {
            return;
        }
        let focus = self.focus.min(fields.len() - 1);

        // Pass 1: total stack height + the focused field's top/bottom.
        let mut total: u16 = 0;
        let mut focus_top: u16 = 0;
        let mut focus_h: u16 = 0;
        for (i, f) in fields.iter().enumerate() {
            let h = f.height(&*target).max(1);
            if i == focus {
                focus_top = total;
                focus_h = h;
            }
            total = total.saturating_add(h);
        }
        let focus_bottom = focus_top.saturating_add(focus_h);

        // Scroll the stack up so the focused field is fully visible. Bring its
        // bottom into view, but never push its top above the fold (for a field
        // taller than the area, showing its top wins).
        let overflowing = total > area.h;
        let mut scroll: u16 = 0;
        if overflowing {
            if focus_bottom > area.h {
                scroll = focus_bottom - area.h;
            }
            if scroll > focus_top {
                scroll = focus_top;
            }
        }

        // Reserve a thin column for the scroll indicator only when overflowing.
        let bar_w = if overflowing {
            SCROLLBAR_W + SCROLLBAR_GAP
        } else {
            0
        };
        let content_w = area.w.saturating_sub(bar_w);

        // Pass 2: place each field at its cumulative top minus the scroll offset,
        // drawing only the fields that fully fit the window.
        if content_w > 0 {
            let mut top: u16 = 0;
            for f in fields.iter() {
                let h = f.height(&*target).max(1);
                let vy = top as i32 - scroll as i32;
                if vy >= 0 && vy + h as i32 <= area.h as i32 {
                    let y = area.y + vy as u16;
                    f.view(target, Area::new(area.x, y, content_w, h));
                }
                top = top.saturating_add(h);
            }
        }

        if overflowing {
            self.draw_scroll_indicator(target, area, total, scroll);
        }
    }

    /// Draws the scroll track + thumb at the right edge (see [`List`](crate::List)
    /// for the matching geometry). Only called when the stack overflows, so
    /// `total > area.h` and `max_scroll > 0`.
    fn draw_scroll_indicator(
        &self,
        target: &mut dyn RenderTarget,
        area: Area,
        total: u16,
        scroll: u16,
    ) {
        let band_x = area.x + area.w - SCROLLBAR_W;
        let track_x = band_x + (SCROLLBAR_W - TRACK_W) / 2;
        target.fill_rect(Area::new(track_x, area.y, TRACK_W, area.h), Style::Muted);

        let track_h = area.h;
        let thumb_h = (((track_h as u32 * area.h as u32) / total as u32) as u16)
            .max(MIN_THUMB_PX)
            .min(track_h);
        let max_scroll = total - area.h;
        let progress = ((track_h - thumb_h) as u32 * scroll as u32 / max_scroll as u32) as u16;
        target.fill_rect(
            Area::new(band_x, area.y + progress, SCROLLBAR_W, thumb_h),
            Style::Focus,
        );
    }
}

impl Default for Form {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::*;
    use crate::mock::{Op, RecordingTarget};
    use crate::{Checkbox, Counter, TextInput, Toggle};

    /// A field with a fixed, configurable height that draws its tag at the row
    /// origin - lets layout tests read each field's `y` straight from the ops.
    struct Probe {
        tag: &'static str,
        h: u16,
    }
    impl Probe {
        fn new(tag: &'static str, h: u16) -> Self {
            Self { tag, h }
        }
    }
    impl Component for Probe {
        fn update(&mut self, _msg: &Msg) {}
        fn view(&self, t: &mut dyn RenderTarget, a: Area) {
            t.draw_text(a.x, a.y, self.tag, crate::Style::Normal);
        }
    }
    impl FormField for Probe {
        fn height(&self, _t: &dyn RenderTarget) -> u16 {
            self.h
        }
    }

    /// `y` of the op drawing `tag`, if present.
    fn tag_y(t: &RecordingTarget, tag: &str) -> Option<u16> {
        t.ops().iter().find_map(|op| match op {
            Op::Text { y, text, .. } if text == tag => Some(*y),
            _ => None,
        })
    }

    /// A test double for a select-capturing editable field (the role
    /// `TextInput` will play when it is ported): each `Select` while editing is
    /// delivered to it, and it reports `editing_finished` once it has received
    /// one, so the form leaves edit on the second Select.
    #[derive(Default)]
    struct Capturing {
        selects: u32,
        editing: bool,
    }
    impl Component for Capturing {
        fn update(&mut self, msg: &Msg) {
            if let Msg::Select = msg {
                self.selects += 1;
            }
        }
        fn view(&self, _t: &mut dyn RenderTarget, _a: Area) {}
    }
    impl FormField for Capturing {
        fn editable(&self) -> bool {
            true
        }
        fn set_editing(&mut self, e: bool) {
            self.editing = e;
        }
        fn captures_select_while_editing(&self) -> bool {
            true
        }
        fn editing_finished(&self) -> bool {
            self.selects >= 1
        }
    }

    #[test]
    fn form_navigates_focus() {
        let mut cb = Checkbox::new("A");
        let mut tg = Toggle::new("B");
        let mut ct = Counter::new("C");
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 3] = [&mut cb, &mut tg, &mut ct];
            form.sync_focus(&mut fields);

            form.update(&Msg::Down, &mut fields);
            assert_eq!(form.focus_index(), 1);
            form.update(&Msg::Down, &mut fields);
            assert_eq!(form.focus_index(), 2);
            form.update(&Msg::Down, &mut fields);
            assert_eq!(form.focus_index(), 2); // clamped at last
            form.update(&Msg::Up, &mut fields);
            assert_eq!(form.focus_index(), 1);
        }
    }

    #[test]
    fn form_select_toggles_momentary() {
        let mut cb = Checkbox::new("A");
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 1] = [&mut cb];
            form.update(&Msg::Select, &mut fields);
        }
        assert!(cb.is_checked());
    }

    #[test]
    fn form_counter_edit_mode() {
        let mut cb = Checkbox::new("A");
        let mut ct = Counter::new("X")
            .with_range(0, 100)
            .with_step(10)
            .with_value(50);
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 2] = [&mut cb, &mut ct];
            form.update(&Msg::Down, &mut fields);
            assert_eq!(form.focus_index(), 1); // Counter focused
            form.update(&Msg::Select, &mut fields);
            assert!(form.is_editing());
            form.update(&Msg::Down, &mut fields); // edit: Down decrements
            form.update(&Msg::Select, &mut fields);
            assert!(!form.is_editing());
            form.update(&Msg::Down, &mut fields); // focus stays, value unchanged
            assert_eq!(form.focus_index(), 1);
        }
        assert_eq!(ct.value(), 40);
    }

    #[test]
    fn form_edit_blocks_navigation() {
        let mut cb = Checkbox::new("A");
        let mut ct = Counter::new("X");
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 2] = [&mut cb, &mut ct];
            form.update(&Msg::Down, &mut fields); // focus 1 (Counter)
            form.update(&Msg::Select, &mut fields); // enter edit
            assert!(form.is_editing());
            form.update(&Msg::Up, &mut fields);
            assert_eq!(form.focus_index(), 1);
            form.update(&Msg::Down, &mut fields);
            assert_eq!(form.focus_index(), 1);
        }
    }

    #[test]
    fn form_view_lays_fields_on_pixel_rows() {
        let a = Checkbox::new("A");
        let b = Checkbox::new("B");
        let form = Form::new();
        // line_height = 10 (default RecordingTarget metric).
        let mut t = RecordingTarget::new(120, 30);
        let fields: [&dyn FormField; 2] = [&a, &b];
        form.view(&mut t, Area::new(0, 0, 120, 30), &fields);
        // Each Checkbox draws its "[ ]" indicator at the row origin: y = 0 and 10.
        let ys: alloc::vec::Vec<u16> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Text { x: 0, y, text, .. } if text == "[ ]" => Some(*y),
                _ => None,
            })
            .collect();
        assert_eq!(ys, [0, 10]);
    }

    #[test]
    fn form_view_stacks_fields_by_reported_height() {
        // Heights 10 / 20 / 10 → tops at 0, 10, 30.
        let a = Probe::new("A", 10);
        let b = Probe::new("B", 20);
        let c = Probe::new("C", 10);
        let form = Form::new();
        let mut t = RecordingTarget::new(120, 100);
        let fields: [&dyn FormField; 3] = [&a, &b, &c];
        form.view(&mut t, Area::new(0, 0, 120, 100), &fields);
        assert_eq!(tag_y(&t, "A"), Some(0));
        assert_eq!(tag_y(&t, "B"), Some(10));
        assert_eq!(tag_y(&t, "C"), Some(30));
    }

    #[test]
    fn form_view_honours_textinput_height_growth() {
        // The TextInput sits first; the Probe below it must shift down by a row
        // when the TextInput enters edit (1 row → 2 rows). Default focus (0) is
        // the TextInput, so the editing TextInput stays fully visible either way.
        let ti = TextInput::<8>::new("N");
        let probe = Probe::new("P", 10);
        let form = Form::new();

        // Not editing: TextInput is one row (10), Probe at y = 10.
        let mut t0 = RecordingTarget::new(120, 100);
        {
            let fields: [&dyn FormField; 2] = [&ti, &probe];
            form.view(&mut t0, Area::new(0, 0, 120, 100), &fields);
        }
        assert_eq!(tag_y(&t0, "P"), Some(10));

        // Enter edit on the TextInput → its height becomes 2 rows; Probe moves to 20.
        let mut editing = TextInput::<8>::new("N");
        editing.set_editing(true);
        let mut t1 = RecordingTarget::new(120, 100);
        {
            let fields: [&dyn FormField; 2] = [&editing, &probe];
            form.view(&mut t1, Area::new(0, 0, 120, 100), &fields);
        }
        assert_eq!(tag_y(&t1, "P"), Some(20));
    }

    #[test]
    fn form_scrolls_to_keep_focused_field_visible() {
        // 5 fields × 10px = 50px stack in a 30px area (3 rows). Focus the last;
        // the stack must scroll so the focused field (top 40) is fully visible.
        let a = Probe::new("A", 10);
        let b = Probe::new("B", 10);
        let c = Probe::new("C", 10);
        let d = Probe::new("D", 10);
        let e = Probe::new("E", 10);
        let mut form = Form::new();
        {
            let mut m: [&mut dyn FormField; 5] = [
                &mut Probe::new("A", 10),
                &mut Probe::new("B", 10),
                &mut Probe::new("C", 10),
                &mut Probe::new("D", 10),
                &mut Probe::new("E", 10),
            ];
            form.sync_focus(&mut m);
            for _ in 0..4 {
                form.update(&Msg::Down, &mut m); // focus → index 4 (E)
            }
        }
        assert_eq!(form.focus_index(), 4);

        let mut t = RecordingTarget::new(120, 30);
        let fields: [&dyn FormField; 5] = [&a, &b, &c, &d, &e];
        form.view(&mut t, Area::new(0, 0, 120, 30), &fields);

        // scroll = 50 - 30 = 20 → C/D/E visible at y = 0/10/20; A/B clipped out.
        assert_eq!(tag_y(&t, "A"), None);
        assert_eq!(tag_y(&t, "B"), None);
        assert_eq!(tag_y(&t, "C"), Some(0));
        assert_eq!(tag_y(&t, "D"), Some(10));
        assert_eq!(tag_y(&t, "E"), Some(20)); // focused, fully visible

        // Overflow → a scroll indicator (track + thumb) is drawn at the right edge.
        let fills: alloc::vec::Vec<_> = t
            .ops()
            .iter()
            .filter_map(|op| match op {
                Op::Fill { area, style } => Some((*area, *style)),
                _ => None,
            })
            .collect();
        assert!(
            fills
                .iter()
                .any(|&(a, st)| st == crate::Style::Muted && a.w == TRACK_W)
        );
        assert!(fills.iter().any(|&(a, st)| st == crate::Style::Focus
            && a.w == SCROLLBAR_W
            && a.x == 120 - SCROLLBAR_W));
    }

    #[test]
    fn form_capturing_field_routes_select_then_finishes() {
        let mut cap = Capturing::default();
        let mut ct = Counter::new("C");
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 2] = [&mut cap, &mut ct];
            form.sync_focus(&mut fields);

            // Select enters edit on the (focused) capturing field.
            form.update(&Msg::Select, &mut fields);
            assert!(form.is_editing());
            assert_eq!(form.focus_index(), 0);
            // Next Select is routed INTO the field (selects += 1); the field then
            // reports editing_finished → the form leaves edit mode.
            form.update(&Msg::Select, &mut fields);
            assert!(!form.is_editing());
        }
        assert_eq!(cap.selects, 1);
    }

    #[test]
    fn form_non_capturing_counter_select_still_toggles_edit() {
        // Counter does NOT capture Select, so behaviour is unchanged: Select
        // enters edit, a second Select leaves it (it does not reach the field).
        let mut ct = Counter::new("C").with_range(0, 10).with_value(5);
        let mut form = Form::new();
        {
            let mut fields: [&mut dyn FormField; 1] = [&mut ct];
            form.update(&Msg::Select, &mut fields);
            assert!(form.is_editing());
            form.update(&Msg::Select, &mut fields);
            assert!(!form.is_editing());
        }
        assert_eq!(ct.value(), 5); // unchanged by the Selects
    }
}
