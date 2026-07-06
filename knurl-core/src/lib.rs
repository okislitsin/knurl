#![no_std]

use core::cell::Cell;

mod basics;
mod chart;
mod dialog;
mod form;
mod help;
mod info;
mod interactive;
mod layout;
mod list;
mod pager;
mod radio;
mod router;
mod statusbar;
mod table;
mod tabs;
mod textinput;
mod tree;

pub use basics::{Separator, Spacer, Title};
pub use chart::{BarChart, BarChartModel};
pub use dialog::Dialog;
pub use form::{Form, FormField};
pub use help::Help;
pub use info::{LineGauge, Paginator, ProgressBar, Scrollbar, Spinner, SpinnerStyle};
pub use interactive::{Button, Checkbox, Counter, Picker, Slider, Toggle};
pub use layout::{Bordered, Constraint, HStack, Padded, VStack};
pub use list::{List, ListModel};
pub use pager::{LinesModel, Pager};
pub use radio::Radio;
pub use router::{Nav, Router};
pub use statusbar::StatusBar;
pub use table::{Table, TableModel};
pub use tabs::Tabs;
pub use textinput::TextInput;
pub use tree::{Tree, TreeItem, TreeModel};

// ── Primitive types ──────────────────────────────────────────────────────────

/// A rectangular region on the display, in **pixels**.
///
/// As of v2 the whole core is pixel-native: `x`/`y`/`w`/`h` are pixel
/// coordinates and extents, not character cells. `u16` is required because a
/// 320×240 TFT does not fit in `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Area {
    /// The standard chrome border thickness, in pixels: a single hairline.
    ///
    /// This is the canonical thin border the UI draws around content. Pixel
    /// chrome shrinks content by the *exact* stroke thickness (see
    /// [`inner_by`](Area::inner_by) and `Bordered`), never by a whole text row or
    /// column - that compactness is the point on a small panel. Thicker styles
    /// report their own thickness via [`BorderStyle::thickness`].
    pub const BORDER_PX: u16 = 1;

    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }

    /// Whether the pixel `(x, y)` falls inside this area.
    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x
            && y >= self.y
            && x < self.x.saturating_add(self.w)
            && y < self.y.saturating_add(self.h)
    }

    /// The content region inside a `thickness`-pixel border - the area shrunk by
    /// exactly `thickness` pixels on every side. Returns `None` when the area is
    /// too small to leave any interior, or when `thickness` is 0 (no inset → the
    /// caller should use the area unchanged).
    pub fn inner_by(&self, thickness: u16) -> Option<Self> {
        if thickness == 0 {
            return Some(*self);
        }
        let two = thickness.saturating_mul(2);
        if self.w <= two || self.h <= two {
            return None;
        }
        Some(Self {
            x: self.x + thickness,
            y: self.y + thickness,
            w: self.w - two,
            h: self.h - two,
        })
    }

    /// The content region inside the standard [`BORDER_PX`](Area::BORDER_PX)
    /// hairline border (shorthand for `inner_by(BORDER_PX)`).
    pub fn inner(&self) -> Option<Self> {
        self.inner_by(Self::BORDER_PX)
    }
}

// ── Padding ───────────────────────────────────────────────────────────────────

/// Inset on each side of an [`Area`], in **pixels**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Padding {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Padding {
    pub const ZERO: Self = Self { top: 0, right: 0, bottom: 0, left: 0 };

    pub const fn new(top: u16, right: u16, bottom: u16, left: u16) -> Self {
        Self { top, right, bottom, left }
    }

    /// Equal padding on all four sides (pixels).
    pub const fn uniform(n: u16) -> Self {
        Self { top: n, right: n, bottom: n, left: n }
    }

    /// Equal `vertical` padding top and bottom, equal `horizontal` left and right.
    pub const fn symmetric(vertical: u16, horizontal: u16) -> Self {
        Self { top: vertical, right: horizontal, bottom: vertical, left: horizontal }
    }

    /// The area remaining inside the padding. Returns `None` when the inset
    /// collapses the region to zero width or height. Uses saturating arithmetic.
    pub fn inner(&self, area: Area) -> Option<Area> {
        let horizontal = self.left.saturating_add(self.right);
        let vertical = self.top.saturating_add(self.bottom);
        let w = area.w.saturating_sub(horizontal);
        let h = area.h.saturating_sub(vertical);
        if w == 0 || h == 0 {
            return None;
        }
        Some(Area {
            x: area.x.saturating_add(self.left),
            y: area.y.saturating_add(self.top),
            w,
            h,
        })
    }
}

// ── Input messages ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Char(char),
    Tick,
}

// ── Text styling ─────────────────────────────────────────────────────────────

/// Semantic role of a piece of text - *what* it is, not *how* it's drawn.
///
/// Each [`RenderTarget`] decides how to render every variant for its medium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Ordinary body text.
    Normal,
    /// The selected element of a list.
    Inverted,
    /// The element holding input focus (under the cursor). Distinct from
    /// `Accent`: `Accent` is a semantic emphasis (a heading), `Focus` is the
    /// navigational focus.
    Focus,
    /// A heading or the active element.
    Accent,
    /// Secondary text, hints.
    Muted,
    /// Errors and warnings.
    Danger,
}

impl Default for Style {
    fn default() -> Self {
        Style::Normal
    }
}

/// Horizontal alignment of text within an [`Area`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

// ── Marker ────────────────────────────────────────────────────────────────────

/// The selection cursor drawn beside list items (and future selectable elements).
///
/// `selected` and `unselected` **must** have the same displayed width - they are
/// drawn in the same slot, so a width mismatch would misalign item text. Layout
/// uses [`width`](Marker::width), computed from `selected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub selected: &'static str,
    pub unselected: &'static str,
}

impl Marker {
    pub const ARROW: Self = Self { selected: "> ", unselected: "  " };
    pub const NONE: Self = Self { selected: "", unselected: "" };

    pub const fn new(selected: &'static str, unselected: &'static str) -> Self {
        Self { selected, unselected }
    }

    /// Prefix width in characters (the displayed slot width), measured from
    /// `selected`. Both fields are required to share this width.
    pub fn width(&self) -> usize {
        self.selected.chars().count()
    }
}

impl Default for Marker {
    fn default() -> Self {
        Marker::ARROW
    }
}

// ── Border styles ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Single,
    Double,
    Rounded,
    Thick,
}

impl BorderStyle {
    /// Pixel thickness of the drawn stroke - how far content must inset on each
    /// side to clear the border. `None` is 0 (no border, no inset); `Single` and
    /// `Rounded` are a 1px hairline; `Thick` is 2px; `Double` spans 3px (two 1px
    /// lines with a 1px gap). Used by `Bordered`/[`Area::inner_by`] so the chrome
    /// eats only its own pixels.
    pub const fn thickness(self) -> u16 {
        match self {
            BorderStyle::None => 0,
            BorderStyle::Single | BorderStyle::Rounded => 1,
            BorderStyle::Thick => 2,
            BorderStyle::Double => 3,
        }
    }

    /// Returns `(top_left, top_right, bottom_left, bottom_right, horizontal, vertical)`.
    pub const fn chars(self) -> (char, char, char, char, char, char) {
        match self {
            BorderStyle::None    => (' ', ' ', ' ', ' ', ' ', ' '),
            BorderStyle::Single  => ('┌', '┐', '└', '┘', '─', '│'),
            BorderStyle::Double  => ('╔', '╗', '╚', '╝', '═', '║'),
            BorderStyle::Rounded => ('╭', '╮', '╰', '╯', '─', '│'),
            BorderStyle::Thick   => ('┏', '┓', '┗', '┛', '━', '┃'),
        }
    }
}

// ── Core traits ───────────────────────────────────────────────────────────────

/// An abstract pixel surface that components can draw onto.
///
/// Every coordinate and extent is in **pixels** (`u16`). Implementations exist
/// for:
/// - `RecordingTarget` (host tests - records draw calls; see [`mock`])
/// - `knurl-graphics` `GraphicsTarget` / `ColorGraphicsTarget` (real panels via
///   embedded-graphics)
///
/// ## Font metrics
///
/// Widgets lay text out in pixels but do not know the font, so the target
/// exposes the three metrics they need: [`line_height`](RenderTarget::line_height),
/// [`char_width`](RenderTarget::char_width) (monospace advance) and
/// [`text_width`](RenderTarget::text_width). A widget asks the target how tall a
/// line is / how wide a string is, then positions text by pixel coordinate.
pub trait RenderTarget {
    /// Display width in pixels.
    fn width(&self) -> u16;
    /// Display height in pixels.
    fn height(&self) -> u16;
    /// Returns `true` for pixel displays. (Always `true` in the pixel-native
    /// core; retained so component code can branch without assuming a concrete
    /// target.)
    fn is_graphical(&self) -> bool;

    /// Height of one text line, in pixels (the font's cell height).
    fn line_height(&self) -> u16;
    /// Horizontal advance of one character, in pixels (monospace step: glyph
    /// width + inter-character spacing).
    fn char_width(&self) -> u16;
    /// Pixel width of `s` when rendered. The default assumes a monospace font
    /// (`char_width * chars`); a proportional target may override it.
    fn text_width(&self, s: &str) -> u16 {
        self.char_width().saturating_mul(s.chars().count() as u16)
    }

    /// Renders `text` with its top-left at pixel `(x, y)`. Clipping at screen
    /// edges is the implementation's responsibility.
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style);
    /// Draws a rectangular border around the pixel `area`. The interior is left
    /// untouched.
    fn draw_box(&mut self, area: Area, border: BorderStyle);
    /// Clears the pixel `area` to the background.
    fn clear(&mut self, area: Area);
    /// Fills the pixel `area` with the foreground of `style` - the pixel-native
    /// primitive behind rules, cursors and solid backgrounds.
    fn fill_rect(&mut self, area: Area, style: Style);

    /// Draws a horizontal progress/level bar filling `area` to the fraction
    /// `fill_permille / 1000`, in the given `style`.
    ///
    /// This is a **semantic** primitive: pixel targets in `knurl-graphics`
    /// override it to render a smooth track + rounded fill. The default here
    /// fills the leading fraction of `area` via
    /// [`fill_rect`](RenderTarget::fill_rect), so any target gets a usable bar
    /// without extra work.
    fn draw_bar(&mut self, area: Area, fill_permille: u16, style: Style) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let fw = ((area.w as u32 * fill_permille as u32) / 1000).min(area.w as u32) as u16;
        if fw > 0 {
            self.fill_rect(Area::new(area.x, area.y, fw, area.h), style);
        }
    }

    /// Draws a checkbox indicator within `area`, reflecting the `on` state.
    ///
    /// Semantic primitive: pixel targets in `knurl-graphics` override it to render
    /// a rounded square (filled when `on`). The default here draws `[x]` / `[ ]`
    /// at the area origin so character/recording targets keep working.
    fn draw_check(&mut self, area: Area, on: bool, style: Style) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        self.draw_text(area.x, area.y, if on { "[x]" } else { "[ ]" }, style);
    }

    /// Draws a radio indicator within `area`, reflecting the `on` state.
    ///
    /// Semantic primitive: pixel targets in `knurl-graphics` override it to render
    /// a circle (with a centre dot when `on`). The default here draws `(*)` / `( )`
    /// at the area origin so character/recording targets keep working.
    fn draw_radio(&mut self, area: Area, on: bool, style: Style) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        self.draw_text(area.x, area.y, if on { "(*)" } else { "( )" }, style);
    }

    /// Draws a tree-node expander within `area`: an `expanded` (open) or collapsed
    /// indicator. Only parent nodes get one; leaves draw nothing.
    ///
    /// Semantic primitive: pixel targets in `knurl-graphics` override it to render
    /// a small triangle (pointing down when expanded, right when collapsed). The
    /// default here draws `v` / `>` at the area origin so character/recording
    /// targets keep working.
    fn draw_expander(&mut self, area: Area, expanded: bool, style: Style) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        self.draw_text(area.x, area.y, if expanded { "v" } else { ">" }, style);
    }

    /// Draws one spinner animation `frame` within `area`. The widget chooses the
    /// frame character from its style's set (Line `|/-\`, Braille dots, Pulse
    /// blocks, Meter), advancing per tick.
    ///
    /// Semantic primitive: pixel targets in `knurl-graphics` override it to
    /// pixel-draw the frame (a Braille dot matrix, a pulsing block) so it reads
    /// like a terminal loader even though the ASCII font lacks those glyphs. The
    /// default here draws the frame character so character/recording targets keep
    /// working - that symbolic frame is what the recording mock sees.
    fn draw_spinner(&mut self, area: Area, frame: char, style: Style) {
        if area.w == 0 || area.h == 0 {
            return;
        }
        let mut b = [0u8; 4];
        self.draw_text(area.x, area.y, frame.encode_utf8(&mut b), style);
    }
}

// ── Shared scroll indicator ─────────────────────────────────────────────────

/// Pixels a scrolling widget should reserve on the right for the built-in
/// vertical scroll indicator (band + 1px gap).
pub(crate) const V_SCROLL_RESERVE: u16 = 4;

/// Draws a thin vertical scroll indicator (track + thumb) flush with the right
/// edge of `area`, via [`fill_rect`](RenderTarget::fill_rect). `total` items, of
/// which `visible` show at once, the first visible one being item `rank` in the
/// scrollable sequence. Callers draw it only when `total > visible` and should
/// reserve [`V_SCROLL_RESERVE`] px of content width for it.
///
/// The track is 1px wide; the thumb spans the full 3px band, so it reads as a
/// distinct handle even on monochrome (where `fill_rect` ignores the style).
pub(crate) fn draw_v_scroll(
    target: &mut dyn RenderTarget,
    area: Area,
    total: usize,
    visible: usize,
    rank: usize,
) {
    const BAND_W: u16 = 3;
    const TRACK_W: u16 = 1;
    const MIN_THUMB_PX: u16 = 3;

    if area.w < BAND_W || area.h == 0 {
        return;
    }
    let band_x = area.x + area.w - BAND_W;
    let track_x = band_x + (BAND_W - TRACK_W) / 2;
    target.fill_rect(Area::new(track_x, area.y, TRACK_W, area.h), Style::Muted);

    let track_h = area.h;
    let thumb_h = (((track_h as usize * visible) / total.max(1)) as u16)
        .max(MIN_THUMB_PX)
        .min(track_h);
    let max_off = total.saturating_sub(visible);
    let progress = ((track_h - thumb_h) as usize * rank)
        .checked_div(max_off)
        .unwrap_or(0) as u16;
    target.fill_rect(Area::new(band_x, area.y + progress, BAND_W, thumb_h), Style::Focus);
}

/// An isolated, composable UI element following the Elm update/view cycle.
///
/// ## Partial redraw
///
/// knurl is immediate-mode (no retained widget tree), so each widget self-gates
/// inside its own render. The render entry point is [`view`](Component::view),
/// which is **provided**:
///
/// 1. if the widget is not [`dirty`](Component::dirty) → return, drawing nothing
///    (its pixels persist in the framebuffer);
/// 2. otherwise [`clear`](RenderTarget::clear) the widget's own `area`, call
///    [`draw`](Component::draw), then [`mark_clean`](Component::mark_clean).
///
/// Leaf widgets implement [`draw`](Component::draw) (the actual painting) and a
/// `Cell<bool>` dirty flag; **there is no global per-frame screen clear** - a
/// clean widget leaves its pixels untouched, so animating one widget (e.g. a
/// `Spinner`) repaints only that widget's area. Container widgets (`Padded`,
/// `Bordered`, …) instead override [`view`](Component::view) to dispatch to their
/// children (each child self-gates) without clearing the whole area.
///
/// Widgets that don't opt in keep the safe default (`dirty()==true`,
/// `mark_clean()` a no-op): they self-clear and repaint every frame - correct,
/// just not optimized.
pub trait Component {
    /// Called once per event. Components update only their own state, and set
    /// their dirty flag when that state actually changes.
    fn update(&mut self, msg: &Msg);

    /// Paints the component into `area`. The `area` has already been cleared by
    /// [`view`](Component::view), which only calls this when the component is
    /// dirty. **Leaf widgets implement this**; the default is a no-op (for
    /// container widgets that override [`view`](Component::view) instead).
    fn draw(&self, _target: &mut dyn RenderTarget, _area: Area) {}

    /// Renders the component, gating on its dirty flag (see the trait docs).
    ///
    /// Provided: skip when clean, else clear `area`, [`draw`](Component::draw),
    /// and [`mark_clean`](Component::mark_clean). Containers override this to
    /// dispatch to children without clearing.
    fn view(&self, target: &mut dyn RenderTarget, area: Area) {
        if area.w == 0 || area.h == 0 || !self.dirty() {
            return;
        }
        target.clear(area);
        self.draw(target, area);
        self.mark_clean();
    }

    /// Called when keyboard/encoder focus enters this component.
    fn focus(&mut self) {}
    /// Called when focus leaves this component.
    fn blur(&mut self) {}

    /// Whether this component needs repainting since it was last
    /// [`mark_clean`](Component::mark_clean)ed.
    ///
    /// The default is **`true` - "always dirty"**: a component that does not opt
    /// in self-clears and repaints every frame, which is safe (never stale) if
    /// not free. Stateful widgets override this with a `Cell<bool>` set in
    /// [`update`](Component::update) only when their state *actually* changes (a
    /// no-op `update` leaves it clean), so a clean widget's [`view`](Component::view)
    /// draws nothing and its pixels persist.
    fn dirty(&self) -> bool {
        true
    }

    /// Clears the dirty flag after the component has been painted.
    ///
    /// Default is a no-op, so an "always dirty" component stays dirty. Uses
    /// interior mutability so it pairs with the `&self` [`view`](Component::view).
    fn mark_clean(&self) {}

    /// Forces a repaint on the next [`view`](Component::view) - sets the dirty
    /// flag. The app calls this on **structural transitions** (navigation,
    /// modal open/close, first show) so the affected widgets repaint cleanly
    /// over whatever pixels were there before. Default no-op (always-dirty
    /// widgets are already going to repaint).
    fn mark_dirty(&self) {}
}

// ── Label component ───────────────────────────────────────────────────────────

/// A non-interactive text display.
///
/// Holds a borrowed `&str` - no heap allocation required. For a mutable owned
/// label (when `alloc` is available in a higher layer), wrap in a `String`-backed
/// newtype in the `knurl` facade crate.
#[derive(Debug)]
pub struct Label<'a> {
    text: &'a str,
    style: Style,
    dirty: Cell<bool>,
}

impl<'a> Label<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self { text, style: Style::Normal, dirty: Cell::new(true) }
    }

    pub const fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn set_text(&mut self, text: &'a str) {
        self.text = text;
        self.dirty.set(true);
    }

    pub fn text(&self) -> &str {
        self.text
    }

    pub fn style(&self) -> Style {
        self.style
    }
}

impl<'a> Component for Label<'a> {
    fn update(&mut self, _msg: &Msg) {
        // Labels are static; nothing to update.
    }

    fn draw(&self, target: &mut dyn RenderTarget, area: Area) {
        target.draw_text(area.x, area.y, self.text, self.style);
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

// ── RecordingTarget (test harness) ──────────────────────────────────────────────

/// A recording [`RenderTarget`] for host tests, available under `#[cfg(test)]`
/// or the `mock` feature.
///
/// The pixel model makes a character-buffer mock meaningless, so this target
/// records *what* was drawn *where* (in pixels) into a [`Vec`] of [`Op`]s and
/// reports fixed font metrics (default `char_width = 6`, `line_height = 10`).
/// Tests assert on the recorded ops rather than on a glyph grid.
///
/// # Example
/// ```ignore
/// use knurl_core::mock::RecordingTarget;
/// let mut t = RecordingTarget::new(128, 64);
/// ```
#[cfg(any(test, feature = "mock"))]
pub mod mock {
    extern crate alloc;

    use super::*;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    /// One recorded draw call.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Op {
        Text { x: u16, y: u16, text: String, style: Style },
        Box { area: Area, border: BorderStyle },
        Clear { area: Area },
        Fill { area: Area, style: Style },
        Bar { area: Area, fill_permille: u16, style: Style },
    }

    /// Records every draw call and exposes fixed font metrics.
    pub struct RecordingTarget {
        width: u16,
        height: u16,
        char_w: u16,
        line_h: u16,
        ops: Vec<Op>,
    }

    impl RecordingTarget {
        /// A target `width` × `height` pixels with default metrics
        /// (`char_width = 6`, `line_height = 10`).
        pub fn new(width: u16, height: u16) -> Self {
            Self { width, height, char_w: 6, line_h: 10, ops: Vec::new() }
        }

        /// Overrides the reported font metrics.
        pub fn with_metrics(mut self, char_w: u16, line_h: u16) -> Self {
            self.char_w = char_w;
            self.line_h = line_h;
            self
        }

        /// All recorded ops, in draw order.
        pub fn ops(&self) -> &[Op] {
            &self.ops
        }

        /// The first recorded text op, if any (`(x, y, text, style)`).
        pub fn first_text(&self) -> Option<(u16, u16, &str, Style)> {
            self.ops.iter().find_map(|op| match op {
                Op::Text { x, y, text, style } => Some((*x, *y, text.as_str(), *style)),
                _ => None,
            })
        }
    }

    impl RenderTarget for RecordingTarget {
        fn width(&self) -> u16 {
            self.width
        }

        fn height(&self) -> u16 {
            self.height
        }

        fn is_graphical(&self) -> bool {
            true
        }

        fn line_height(&self) -> u16 {
            self.line_h
        }

        fn char_width(&self) -> u16 {
            self.char_w
        }

        fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style) {
            self.ops.push(Op::Text { x, y, text: text.to_string(), style });
        }

        fn draw_box(&mut self, area: Area, border: BorderStyle) {
            self.ops.push(Op::Box { area, border });
        }

        fn clear(&mut self, area: Area) {
            self.ops.push(Op::Clear { area });
        }

        fn fill_rect(&mut self, area: Area, style: Style) {
            self.ops.push(Op::Fill { area, style });
        }

        // Recorded verbatim (rather than via the fill_rect default) so tests can
        // assert on the semantic bar call and its fraction.
        fn draw_bar(&mut self, area: Area, fill_permille: u16, style: Style) {
            self.ops.push(Op::Bar { area, fill_permille, style });
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mock::{Op, RecordingTarget};

    // ── Area ────────────────────────────────────────────────────────────────

    #[test]
    fn area_contains_corners() {
        let a = Area::new(2, 3, 4, 2); // px 2..5 × 3..4
        assert!(a.contains(2, 3));
        assert!(a.contains(5, 4));
        assert!(!a.contains(6, 3));
        assert!(!a.contains(2, 5));
        assert!(!a.contains(1, 3));
    }

    #[test]
    fn area_inner_shrinks_by_border_px() {
        // BORDER_PX == 1 → shrink one pixel on every side.
        let inner = Area::new(0, 0, 5, 4).inner().unwrap();
        assert_eq!(inner, Area::new(1, 1, 3, 2));
    }

    #[test]
    fn area_inner_too_small_returns_none() {
        assert!(Area::new(0, 0, 2, 4).inner().is_none());
        assert!(Area::new(0, 0, 3, 2).inner().is_none());
        assert!(Area::new(0, 0, 3, 3).inner().is_some());
    }

    #[test]
    fn area_is_u16_pixels() {
        // 320×240 must fit - it would overflow the old u8 model.
        let a = Area::new(0, 0, 320, 240);
        assert!(a.contains(319, 239));
        assert!(!a.contains(320, 240));
    }

    // ── Padding ───────────────────────────────────────────────────────────────

    #[test]
    fn padding_inner_shrinks() {
        let inner = Padding::new(1, 2, 1, 2).inner(Area::new(0, 0, 10, 4)).unwrap();
        assert_eq!(inner, Area::new(2, 1, 6, 2));
    }

    #[test]
    fn padding_uniform_and_symmetric() {
        assert_eq!(Padding::uniform(3), Padding::new(3, 3, 3, 3));
        assert_eq!(Padding::symmetric(1, 4), Padding::new(1, 4, 1, 4));
    }

    #[test]
    fn padding_inner_collapse_returns_none() {
        assert!(Padding::new(0, 5, 0, 5).inner(Area::new(0, 0, 10, 4)).is_none());
        assert!(Padding::new(2, 0, 2, 0).inner(Area::new(0, 0, 10, 4)).is_none());
    }

    // ── Marker ──────────────────────────────────────────────────────────────

    #[test]
    fn marker_width() {
        assert_eq!(Marker::ARROW.width(), 2);
        assert_eq!(Marker::new("*", " ").width(), 1);
    }

    #[test]
    fn marker_none_zero_width() {
        assert_eq!(Marker::NONE.width(), 0);
    }

    // ── RenderTarget metrics ──────────────────────────────────────────────────

    #[test]
    fn target_metrics_and_dimensions() {
        let t = RecordingTarget::new(128, 64);
        assert_eq!(t.width(), 128);
        assert_eq!(t.height(), 64);
        assert!(t.is_graphical());
        assert_eq!(t.char_width(), 6);
        assert_eq!(t.line_height(), 10);
        // Default text_width is monospace: 6 px per char.
        assert_eq!(t.text_width("Hi"), 12);
        assert_eq!(t.text_width(""), 0);
    }

    #[test]
    fn target_custom_metrics() {
        let t = RecordingTarget::new(160, 80).with_metrics(8, 16);
        assert_eq!(t.char_width(), 8);
        assert_eq!(t.line_height(), 16);
        assert_eq!(t.text_width("abc"), 24);
    }

    // ── Label ───────────────────────────────────────────────────────────────

    #[test]
    fn label_records_text_at_pixel_origin() {
        let mut t = RecordingTarget::new(128, 64);
        Label::new("Hello").view(&mut t, Area::new(0, 0, 128, 10));
        assert_eq!(t.first_text(), Some((0, 0, "Hello", Style::Normal)));
    }

    #[test]
    fn label_records_text_at_pixel_offset() {
        let mut t = RecordingTarget::new(128, 64);
        Label::new("Hi")
            .with_style(Style::Accent)
            .view(&mut t, Area::new(18, 20, 60, 10));
        assert_eq!(t.first_text(), Some((18, 20, "Hi", Style::Accent)));
    }

    #[test]
    fn label_zero_area_noop() {
        let mut t = RecordingTarget::new(128, 64);
        Label::new("X").view(&mut t, Area::new(0, 0, 0, 0));
        assert!(t.ops().is_empty());
    }

    #[test]
    fn label_with_style_builder() {
        let l = Label::new("x").with_style(Style::Accent);
        assert_eq!(l.style(), Style::Accent);
    }

    // ── Partial redraw (the self-gate convention) ──────────────────────────────

    /// A dirty widget's `view` clears its own `area`, then draws (no global
    /// clear), and ends clean.
    #[test]
    fn dirty_view_self_clears_then_draws_then_clean() {
        let l = Label::new("Hi");
        let area = Area::new(3, 4, 60, 10);
        let mut t = RecordingTarget::new(128, 64);
        assert!(l.dirty());
        l.view(&mut t, area);
        // First op is the self-clear of exactly this widget's area…
        assert_eq!(t.ops().first(), Some(&Op::Clear { area }));
        // …followed by the text.
        assert_eq!(t.first_text(), Some((3, 4, "Hi", Style::Normal)));
        // And the widget is now clean.
        assert!(!l.dirty());
    }

    /// A clean widget's `view` issues ZERO draw ops - its pixels persist. This is
    /// what lets an idle frame skip a static widget entirely.
    #[test]
    fn clean_view_issues_no_ops() {
        let l = Label::new("Hi");
        let area = Area::new(0, 0, 60, 10);
        let mut t1 = RecordingTarget::new(128, 64);
        l.view(&mut t1, area); // first paint cleans it
        assert!(!l.dirty());

        let mut t2 = RecordingTarget::new(128, 64);
        l.view(&mut t2, area); // clean → nothing drawn
        assert!(t2.ops().is_empty());
    }

    /// `mark_dirty` forces a structural repaint (navigation / modal close /
    /// first show): a previously clean widget paints again.
    #[test]
    fn mark_dirty_forces_repaint() {
        let l = Label::new("Hi");
        let area = Area::new(0, 0, 60, 10);
        let mut t1 = RecordingTarget::new(128, 64);
        l.view(&mut t1, area);
        assert!(!l.dirty());

        l.mark_dirty();
        assert!(l.dirty());
        let mut t2 = RecordingTarget::new(128, 64);
        l.view(&mut t2, area);
        assert!(!t2.ops().is_empty()); // repainted
    }

    /// A "screen" of several widgets: when only one is dirty, only that one
    /// paints - the others leave their pixels untouched. (A container's `view`
    /// dispatches to children the same way; each child self-gates.)
    #[test]
    fn only_dirty_child_paints() {
        let a = Label::new("AAA");
        let b = Label::new("BBB");
        // Initial frame: both paint and go clean.
        let mut t0 = RecordingTarget::new(128, 64);
        a.view(&mut t0, Area::new(0, 0, 60, 10));
        b.view(&mut t0, Area::new(0, 20, 60, 10));
        assert!(!a.dirty() && !b.dirty());

        // Re-dirty only `b` (e.g. its state changed), then re-render the screen.
        b.mark_dirty();
        let mut t1 = RecordingTarget::new(128, 64);
        a.view(&mut t1, Area::new(0, 0, 60, 10));
        b.view(&mut t1, Area::new(0, 20, 60, 10));
        // Only `b`'s text was drawn; `a` issued nothing.
        let drew_a = t1.ops().iter().any(|op| matches!(op, Op::Text { text, .. } if text == "AAA"));
        let drew_b = t1.ops().iter().any(|op| matches!(op, Op::Text { text, .. } if text == "BBB"));
        assert!(drew_b && !drew_a);
    }

    /// The smoke scenario, asserted on draw calls: a static `Title` over an
    /// animating `Spinner`. After the first paint, an idle `Tick` advances only
    /// the spinner, so a re-render touches ONLY the spinner's area - no op falls
    /// in the title's row. (This is the partial-redraw win the smoke shows.)
    #[test]
    fn idle_tick_repaints_only_spinner_area() {
        let title = Title::new("Static");
        let mut spinner = Spinner::new();
        let title_area = Area::new(0, 0, 128, 10);
        let spin_area = Area::new(0, 12, 12, 10);

        // First frame: both paint, then go clean.
        let mut t0 = RecordingTarget::new(128, 64);
        title.view(&mut t0, title_area);
        spinner.view(&mut t0, spin_area);
        assert!(!title.dirty() && !spinner.dirty());

        // Idle tick: only the spinner is dirtied.
        spinner.update(&Msg::Tick);
        assert!(spinner.dirty() && !title.dirty());

        // Re-render the screen. The title self-gates (clean → nothing); only the
        // spinner repaints, and every op it issues lies in the spinner's row
        // (y >= 12). Nothing touches the title's row.
        let mut t1 = RecordingTarget::new(128, 64);
        title.view(&mut t1, title_area);
        spinner.view(&mut t1, spin_area);
        assert!(!t1.ops().is_empty());
        let touched_title_row = t1.ops().iter().any(|op| match op {
            Op::Text { y, .. } => *y < 12,
            Op::Clear { area }
            | Op::Fill { area, .. }
            | Op::Box { area, .. }
            | Op::Bar { area, .. } => area.y < 12,
        });
        assert!(!touched_title_row);
    }

    // ── RenderTarget primitives (recording) ───────────────────────────────────

    #[test]
    fn fill_rect_is_recorded() {
        let mut t = RecordingTarget::new(64, 32);
        t.fill_rect(Area::new(2, 4, 10, 1), Style::Accent);
        assert_eq!(
            t.ops(),
            &[Op::Fill { area: Area::new(2, 4, 10, 1), style: Style::Accent }]
        );
    }

    #[test]
    fn draw_box_and_clear_are_recorded() {
        let mut t = RecordingTarget::new(64, 32);
        t.draw_box(Area::new(0, 0, 20, 12), BorderStyle::Single);
        t.clear(Area::new(1, 1, 4, 1));
        assert_eq!(
            t.ops(),
            &[
                Op::Box { area: Area::new(0, 0, 20, 12), border: BorderStyle::Single },
                Op::Clear { area: Area::new(1, 1, 4, 1) },
            ]
        );
    }

    #[test]
    fn draw_bar_is_recorded_verbatim() {
        let mut t = RecordingTarget::new(64, 32);
        t.draw_bar(Area::new(0, 0, 40, 6), 250, Style::Accent);
        assert_eq!(
            t.ops(),
            &[Op::Bar { area: Area::new(0, 0, 40, 6), fill_permille: 250, style: Style::Accent }]
        );
    }
}
