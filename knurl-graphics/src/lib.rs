#![no_std]

use embedded_graphics::{
    draw_target::{Clipped, DrawTargetExt},
    mono_font::{MonoFont, MonoTextStyleBuilder},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
    primitives::{
        Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, RoundedRectangle,
        StrokeAlignment, Triangle,
    },
    text::{Baseline, Text},
};

use knurl_core::{Area, BorderStyle, DirtyRect, RenderTarget, Style, bitmap_runs};

pub use knurl_core as core;

/// The pixel box a line from `(x0, y0)` to `(x1, y1)` occupies - both ends
/// inclusive, so it is one pixel past the larger coordinate on each axis. What
/// the [dirty region](RenderTarget::take_dirty_rect) needs from a primitive
/// whose extent is a pair of points rather than an [`Area`].
fn line_box(x0: u16, y0: u16, x1: u16, y1: u16) -> Area {
    Area::new(
        x0.min(x1),
        y0.min(y1),
        x0.abs_diff(x1) + 1,
        y0.abs_diff(y1) + 1,
    )
}

/// A gentle corner radius (in pixels) for a `RoundedRectangle` of `size`, used by
/// rounded borders and the rounded fill bars. Scales with the smaller side and is
/// clamped so it never exceeds half the box.
fn corner_radius(size: Size) -> u32 {
    let m = size.width.min(size.height);
    (m / 5).min(8).min(m / 2)
}

/// Top-left pixel and side length for a square check/radio/expander indicator.
///
/// The rule, on both axes:
/// - **side** = the cell height less a 1px breathing gap top and bottom (like
///   the bars), then clamped to the cell **width** so it can never overflow a
///   narrow cell;
/// - **vertically centred** in the cell - always, including when the width is
///   what clamped the side, which is what keeps the indicator level with the
///   label beside it and centred on a focus band;
/// - **flush with the cell's left edge**, deliberately not centred there: the
///   label starts at a fixed column (four characters in, see `indicator_slot`),
///   so a horizontally centred square would drift with the row height and stop
///   lining up with the indicators above and below it.
///
/// An odd remainder falls to the bottom (integer division), so the square can
/// sit one pixel high in a cell of odd spare height.
fn indicator_square(rect: Rectangle) -> (Point, u32) {
    let h = rect.size.height;
    if h == 0 || rect.size.width == 0 {
        return (rect.top_left, 0);
    }
    let s = (if h > 2 { h - 2 } else { h }).min(rect.size.width);
    let top = rect.top_left + Point::new(0, ((h - s) / 2) as i32);
    (top, s)
}

/// Block-shade / meter spinner glyphs → a fill fraction in quarters (4 = full).
/// `None` for any other char (drawn as text instead).
fn block_fraction(c: char) -> Option<u32> {
    match c {
        '█' | '▰' => Some(4),
        '▓' => Some(3),
        '▒' => Some(2),
        '░' | '▱' => Some(1),
        _ => None,
    }
}

/// Pixel-draws a spinner `frame` into `area` (top-left `tl`) in `color`, for any
/// `DrawTarget`. Returns `true` if it rendered (Braille dot matrix or pulsing
/// block); `false` for a plain glyph the caller should draw as text (e.g. the
/// Line style `|/-\`, whose glyphs exist in the font).
fn spinner_pixels<D, C>(display: &mut D, tl: Point, area: Area, frame: char, color: C) -> bool
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    let c = frame as u32;
    let (w, h) = (area.w as u32, area.h as u32);
    let fill = PrimitiveStyle::with_fill(color);

    if (0x2800..=0x28FF).contains(&c) {
        // Braille: light the dots of a 2-col × 4-row matrix per the code's bits.
        let bits = (c - 0x2800) as u8;
        let col_w = (w / 2).max(1);
        let row_h = (h / 4).max(1);
        let dot = col_w.min(row_h).saturating_sub(1).max(1);
        // (bit, col, row) - standard 8-dot Braille layout.
        const MAP: [(u8, u32, u32); 8] = [
            (0x01, 0, 0),
            (0x02, 0, 1),
            (0x04, 0, 2),
            (0x40, 0, 3),
            (0x08, 1, 0),
            (0x10, 1, 1),
            (0x20, 1, 2),
            (0x80, 1, 3),
        ];
        for (bit, cx, ry) in MAP {
            if bits & bit != 0 {
                let p = tl + Point::new((cx * col_w) as i32, (ry * row_h) as i32);
                let _ = Rectangle::new(p, Size::new(dot, dot))
                    .into_styled(fill)
                    .draw(display);
            }
        }
        true
    } else if let Some(quarters) = block_fraction(frame) {
        // Pulse/Meter: a centred square scaled by the shade fraction.
        let side = (w.min(h) * quarters / 4).max(1);
        let p = tl + Point::new(((w - side) / 2) as i32, ((h - side) / 2) as i32);
        let _ = Rectangle::new(p, Size::new(side, side))
            .into_styled(fill)
            .draw(display);
        true
    } else {
        false
    }
}

/// A small filled triangle for a tree expander: pointing **down** when
/// `expanded`, **right** when collapsed, fitting an `s`-pixel square at `top`.
///
/// Vertices span `0..=s - 1`, so the figure is exactly `s` px wide and tall -
/// spanning `0..=s` would put a pixel outside the square
/// [`indicator_square`] handed out. Both callers drop `s == 0` before getting
/// here; `s == 1` degenerates to the single pixel at `top`.
fn expander_triangle(top: Point, s: u32, expanded: bool) -> Triangle {
    let last = s.saturating_sub(1) as i32;
    let (x, y) = (top.x, top.y);
    if expanded {
        Triangle::new(
            Point::new(x, y),
            Point::new(x + last, y),
            Point::new(x + last / 2, y + last),
        )
    } else {
        Triangle::new(
            Point::new(x, y),
            Point::new(x, y + last),
            Point::new(x + last, y + last / 2),
        )
    }
}

// ── Free-hand primitives (shared by both targets) ────────────────────────────
//
// The core's defaults reach every one of these through `fill_rect`, which is
// correct but pays a rectangle per pixel on a diagonal - and on monochrome
// loses the style, since `fill_rect` there is deliberately style-blind. Both
// targets override them with the native embedded-graphics primitives and
// resolve the colour from the style, so a hand-drawn widget inverts with its
// row like everything else. The bodies are shared here rather than written
// twice, colour type and all.

/// One pixel at `p`.
fn px_one<D, C>(display: &mut D, p: Point, color: C)
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    // `::core` spelled out: this crate re-exports `knurl_core as core`.
    let _ = display.draw_iter(::core::iter::once(Pixel(p, color)));
}

/// A 1px line from `a` to `b`, both ends included.
fn stroke_line<D, C>(display: &mut D, a: Point, b: Point, color: C)
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    let _ = Line::new(a, b)
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display);
}

/// A 1px rectangle outline, stroked **inside** `rect` - the default alignment
/// would centre the stroke on the boundary and spill onto the neighbour.
fn stroke_rect<D, C>(display: &mut D, rect: Rectangle, color: C)
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    let style = PrimitiveStyleBuilder::new()
        .stroke_color(color)
        .stroke_width(1)
        .stroke_alignment(StrokeAlignment::Inside)
        .build();
    let _ = rect.into_styled(style).draw(display);
}

/// A 1-bit sprite, one rectangle per run of set bits. The bit order is not
/// re-derived here: [`bitmap_runs`] is the format, and this is one of its two
/// callers by design.
fn blit<D, C>(display: &mut D, origin: Point, area: Area, bits: &[u8], color: C)
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    let fill = PrimitiveStyle::with_fill(color);
    bitmap_runs(area, bits, |x, y, w| {
        let _ = Rectangle::new(
            origin + Point::new(i32::from(x), i32::from(y)),
            Size::new(u32::from(w), 1),
        )
        .into_styled(fill)
        .draw(display);
    });
}

// ── Theme ─────────────────────────────────────────────────────────────────────

/// A monochrome theme: maps each [`Style`] to inversion (an `Off` glyph on an
/// `On` background) and optional blinking.
///
/// On a monochrome display only inversion distinguishes one style from another,
/// so a theme decides which styles render inverted - including the focused
/// widget (rendered [`Style::Focus`]), which is inverted by default so focus is
/// visible. The blink masks let a style toggle its inversion with the
/// [`blink_on`](Theme::set_blink_on) phase - e.g. add `FOCUS` to the blink mask
/// for a blinking focus cursor.
///
/// Masks are built from the `Theme::NORMAL`/`INVERTED`/`FOCUS`/`ACCENT`/`MUTED`/
/// `DANGER` bit constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    inverted: u8,
    blink: u8,
    blink_on: bool,
}

impl Theme {
    pub const NORMAL: u8 = 1 << 0;
    pub const INVERTED: u8 = 1 << 1;
    pub const ACCENT: u8 = 1 << 2;
    pub const MUTED: u8 = 1 << 3;
    pub const DANGER: u8 = 1 << 4;
    pub const FOCUS: u8 = 1 << 5;

    /// Default theme: [`Style::Inverted`] and [`Style::Focus`] render inverted
    /// (so focus is visible on a monochrome display), no blinking,
    /// `blink_on = true`.
    pub const fn new() -> Self {
        Self {
            inverted: Self::INVERTED | Self::FOCUS,
            blink: 0,
            blink_on: true,
        }
    }

    /// Sets the mask of styles drawn inverted.
    pub const fn with_inverted(mut self, mask: u8) -> Self {
        self.inverted = mask;
        self
    }

    /// Sets the mask of styles that blink.
    pub const fn with_blink(mut self, mask: u8) -> Self {
        self.blink = mask;
        self
    }

    /// Sets the current blink phase (the application toggles this on a timer).
    pub fn set_blink_on(&mut self, on: bool) {
        self.blink_on = on;
    }

    pub fn toggle_blink(&mut self) {
        self.blink_on = !self.blink_on;
    }

    /// Whether `style` should render inverted, accounting for the blink phase.
    fn resolve(&self, style: Style) -> bool {
        let bit = match style {
            Style::Normal => Self::NORMAL,
            Style::Inverted => Self::INVERTED,
            Style::Focus => Self::FOCUS,
            Style::Accent => Self::ACCENT,
            Style::Muted => Self::MUTED,
            Style::Danger => Self::DANGER,
        };
        let mut inv = self.inverted & bit != 0;
        if self.blink & bit != 0 && !self.blink_on {
            inv = !inv;
        }
        inv
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

// ── GraphicsTarget ────────────────────────────────────────────────────────────

/// A [`RenderTarget`] adapter for any monochrome
/// [`DrawTarget`](embedded_graphics::draw_target::DrawTarget).
///
/// Covers SSD1306, SH1107, SH1108, ST7565, and any other display supported by
/// an `embedded-graphics` driver that uses [`BinaryColor`].
///
/// Coordinates passed to [`RenderTarget`] methods are in **pixels**; the font's
/// `character_size`/`character_spacing` only inform the font-metric queries
/// ([`line_height`](RenderTarget::line_height) /
/// [`char_width`](RenderTarget::char_width)).
///
/// # Example
/// ```ignore
/// use knurl_graphics::GraphicsTarget;
/// use embedded_graphics::mono_font::ascii::FONT_6X10;
///
/// let mut target = GraphicsTarget::new(&mut display, FONT_6X10);
/// ```
pub struct GraphicsTarget<'a, D> {
    display: &'a mut D,
    font: MonoFont<'static>,
    theme: Theme,
    dirty: DirtyRect,
}

impl<'a, D: DrawTarget<Color = BinaryColor>> GraphicsTarget<'a, D> {
    pub fn new(display: &'a mut D, font: MonoFont<'static>) -> Self {
        let size = display.bounding_box().size;
        Self {
            display,
            font,
            theme: Theme::new(),
            dirty: DirtyRect::new(
                size.width.min(u16::MAX as u32) as u16,
                size.height.min(u16::MAX as u32) as u16,
            ),
        }
    }

    /// Sets the [`Theme`] controlling per-style inversion and blinking.
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// The display's top-left pixel (origin offset; usually `(0, 0)`).
    fn origin(&self) -> Point {
        self.display.bounding_box().top_left
    }

    /// The pixel `Point` for `(x, y)`, offset by the display origin.
    fn px_point(&self, x: u16, y: u16) -> Point {
        self.origin() + Point::new(x as i32, y as i32)
    }

    /// The pixel `Rectangle` for `area`, offset by the display origin.
    fn px_rect(&self, area: Area) -> Rectangle {
        Rectangle::new(
            self.px_point(area.x, area.y),
            Size::new(area.w as u32, area.h as u32),
        )
    }

    /// Records `area` as touched - every drawing method's first line, and the
    /// only bookkeeping behind
    /// [`take_dirty_rect`](RenderTarget::take_dirty_rect). The [`DirtyRect`]
    /// clamps to the panel, so a method may hand it whatever it was asked to
    /// draw.
    fn mark(&mut self, area: Area) {
        self.dirty.add(area);
    }

    /// Records the pixel box `text` occupies at `(x, y)`: the monospace advance
    /// times its characters, one line tall - which is exactly the cell run
    /// `MonoTextStyle` paints, background included. Text is the one primitive
    /// whose extent is not in its arguments, and taking "the rest of the row"
    /// instead would inflate every region a label appears in.
    fn mark_text(&mut self, x: u16, y: u16, text: &str) {
        let (w, h) = (self.text_width(text), self.line_height());
        self.mark(Area::new(x, y, w, h));
    }

    /// The `(foreground, background)` pair `style` renders in: `(Off, On)` when
    /// the [`Theme`] inverts it (accounting for the blink phase), `(On, Off)`
    /// otherwise. Every style-aware primitive resolves its colours through this,
    /// so an indicator and the label beside it share one highlight.
    fn mono_pair(&self, style: Style) -> (BinaryColor, BinaryColor) {
        if self.theme.resolve(style) {
            (BinaryColor::Off, BinaryColor::On)
        } else {
            (BinaryColor::On, BinaryColor::Off)
        }
    }

    /// Paints `area` in `bg` - the wash an indicator lays down before its shape,
    /// the equivalent of the `background_color` a `MonoTextStyle` paints under a
    /// glyph cell.
    fn fill_bg(&mut self, area: Area, bg: BinaryColor) {
        let rect = self.px_rect(area);
        let _ = rect
            .into_styled(PrimitiveStyle::with_fill(bg))
            .draw(self.display);
    }

    /// The `(foreground, background)` this target renders `style` in - what a
    /// hand-drawn escape-hatch shape needs so it stays themed instead of
    /// hardcoding a colour. See [`clipped`](GraphicsTarget::clipped).
    pub fn colors(&self, style: Style) -> (BinaryColor, BinaryColor) {
        self.mono_pair(style)
    }

    /// The raw embedded-graphics `DrawTarget`, **clipped to `area`** - the
    /// escape hatch, for the drawing the portable primitives cannot express:
    /// arcs, sectors, images, a font of your own.
    ///
    /// It lives here and not in `knurl-core` on purpose. Reaching through it
    /// ties the widget to embedded-graphics and to a colour type, and that is a
    /// trade the core must not be able to make: a `RenderTarget` is what a
    /// character LCD, a recording mock or a future backend can also be.
    ///
    /// The clip is the safety rail. `area` is translated to display coordinates
    /// and everything drawn through the returned target is masked to it, so an
    /// arc that overshoots by two pixels cannot land on the widget next door -
    /// the same contract every `Component::draw` already has, only enforced
    /// rather than promised. Pair it with [`colors`](GraphicsTarget::colors) to
    /// keep the ink on the theme.
    ///
    /// The clip is also what keeps the escape hatch honest about **partial
    /// redraw**: a raw `DrawTarget` draws behind this target's back, so `area`
    /// is added to the [dirty region](RenderTarget::take_dirty_rect) the moment
    /// it is handed out - before a single pixel is drawn, and whether or not
    /// any is. That over-reports (a hatch that drew nothing still costs its
    /// area) and cannot under-report, which is the only direction a partial
    /// redraw may be wrong in: an under-reported region leaves stale pixels on
    /// the panel with nothing to point at.
    ///
    /// ```
    /// use embedded_graphics::{
    ///     mock_display::MockDisplay, mono_font::ascii::FONT_6X10, pixelcolor::BinaryColor,
    ///     prelude::*, primitives::{Circle, PrimitiveStyle},
    /// };
    /// use knurl_graphics::GraphicsTarget;
    /// use knurl_core::{Area, RenderTarget, Style};
    ///
    /// let mut display = MockDisplay::<BinaryColor>::new();
    /// let mut target = GraphicsTarget::new(&mut display, FONT_6X10);
    ///
    /// let area = Area::new(0, 0, 8, 8);
    /// let (ink, _) = target.colors(Style::Accent);
    /// let mut raw = target.clipped(area);
    /// // Twice the width of the area - the clip keeps it off the neighbours.
    /// Circle::new(Point::new(0, 0), 16)
    ///     .into_styled(PrimitiveStyle::with_stroke(ink, 1))
    ///     .draw(&mut raw)
    ///     .unwrap();
    ///
    /// // ...and the region says so, so the application pushes those pixels.
    /// assert_eq!(target.take_dirty_rect(), Some(area));
    /// assert!(display.affected_area().size.width <= 8);
    /// ```
    pub fn clipped(&mut self, area: Area) -> Clipped<'_, D> {
        // Conservative by necessity: what goes through the returned target is
        // invisible here, so its area is dirty as soon as it is asked for.
        self.mark(area);
        let rect = self.px_rect(area);
        self.display.clipped(&rect)
    }

    /// The underlying `DrawTarget`, **unclipped**. Prefer
    /// [`clipped`](GraphicsTarget::clipped): this one will happily draw over the
    /// whole panel, and the caller carries the bounds check.
    ///
    /// It also **marks the whole panel dirty**, which is the same statement in
    /// the other currency: an unclipped target can paint any pixel, so the only
    /// region that is certainly enough is all of them. That costs a frame the
    /// bus time it costs today - and the way to get a tight region back is the
    /// one that was already the recommendation, [`clipped`](GraphicsTarget::clipped),
    /// which reports exactly its area. Documenting a "you account for it
    /// yourself" contract instead would make the failure silent: an application
    /// that forgot would push too little and leave stale pixels on the panel.
    pub fn display_mut(&mut self) -> &mut D {
        self.dirty.all();
        self.display
    }
}

// ── RenderTarget ──────────────────────────────────────────────────────────────

impl<'a, D: DrawTarget<Color = BinaryColor>> RenderTarget for GraphicsTarget<'a, D> {
    fn width(&self) -> u16 {
        self.display.bounding_box().size.width.min(u16::MAX as u32) as u16
    }

    fn height(&self) -> u16 {
        self.display.bounding_box().size.height.min(u16::MAX as u32) as u16
    }

    fn is_graphical(&self) -> bool {
        true
    }

    fn line_height(&self) -> u16 {
        self.font.character_size.height.min(u16::MAX as u32) as u16
    }

    fn char_width(&self) -> u16 {
        (self.font.character_size.width + self.font.character_spacing).min(u16::MAX as u32) as u16
    }

    fn take_dirty_rect(&mut self) -> Option<Area> {
        self.dirty.take()
    }

    /// Render `text` with its top-left at pixel `(x, y)`.
    ///
    /// Whether a style renders inverted (`Off` glyph on an `On` background) or
    /// normal (`On` glyph on an `Off` background) is decided by the
    /// [`Theme`](Theme::resolve), accounting for the current blink phase.
    ///
    /// Setting `background_color` on the `MonoTextStyle` ensures every cell
    /// background pixel is drawn, which is required for correct inversion on
    /// a pixel display.
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style) {
        self.mark_text(x, y, text);
        // Compute the pixel origin before borrowing `self.font`/`self.display`
        // simultaneously - the bounding_box() borrow ends here (NLL).
        let pos = self.px_point(x, y);

        let (text_color, bg_color) = self.mono_pair(style);

        let char_style = MonoTextStyleBuilder::new()
            .font(&self.font)
            .text_color(text_color)
            .background_color(bg_color)
            .build();

        let _ = Text::with_baseline(text, pos, char_style, Baseline::Top).draw(self.display);
    }

    /// Draw a rectangular border using `Rectangle` outlines.
    ///
    /// | Style          | Pixel rendering                             |
    /// |----------------|---------------------------------------------|
    /// | `None`         | no-op                                       |
    /// | `Single`       | 1-px stroked rectangle                      |
    /// | `Rounded`      | 1-px stroked `RoundedRectangle` (real corners)|
    /// | `Thick`        | 2-px stroked rectangle, aligned *inside*    |
    /// | `Double`       | two concentric 1-px rectangles, 2-px apart  |
    ///
    /// Every stroke stays within `area`: `Thick` asks for
    /// [`StrokeAlignment::Inside`] because the default (`Center`) would lay one
    /// of its two pixels *outside*, clipping at the screen edge and bleeding
    /// onto the neighbouring widget - and breaking the contract that chrome eats
    /// exactly [`BorderStyle::thickness`] pixels.
    fn draw_box(&mut self, area: Area, border: BorderStyle) {
        if matches!(border, BorderStyle::None) {
            return;
        }
        self.mark(area);

        let rect = self.px_rect(area);
        let top_left = rect.top_left;
        let size = rect.size;

        match border {
            BorderStyle::None => unreachable!(),

            BorderStyle::Single => {
                let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
                let _ = Rectangle::new(top_left, size)
                    .into_styled(style)
                    .draw(self.display);
            }

            BorderStyle::Rounded => {
                let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
                let r = corner_radius(size);
                let _ = RoundedRectangle::with_equal_corners(
                    Rectangle::new(top_left, size),
                    Size::new(r, r),
                )
                .into_styled(style)
                .draw(self.display);
            }

            BorderStyle::Thick => {
                let style = PrimitiveStyleBuilder::new()
                    .stroke_color(BinaryColor::On)
                    .stroke_width(2)
                    .stroke_alignment(StrokeAlignment::Inside)
                    .build();
                let _ = Rectangle::new(top_left, size)
                    .into_styled(style)
                    .draw(self.display);
            }

            BorderStyle::Double => {
                let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
                let _ = Rectangle::new(top_left, size)
                    .into_styled(stroke)
                    .draw(self.display);

                // Inner line inset by 2 px on every side - only if there is room
                // for both lines plus at least 1 px interior.
                if size.width > 5 && size.height > 5 {
                    let inner = Rectangle::new(
                        top_left + Point::new(2, 2),
                        Size::new(size.width - 4, size.height - 4),
                    );
                    let _ = inner.into_styled(stroke).draw(self.display);
                }
            }
        }
    }

    /// Fill the pixel region with `BinaryColor::Off` (clear).
    fn clear(&mut self, area: Area) {
        self.mark(area);
        let rect = self.px_rect(area);
        let style = PrimitiveStyle::with_fill(BinaryColor::Off);
        let _ = rect.into_styled(style).draw(self.display);
    }

    /// Fill the pixel region solid. `style` is **deliberately** ignored here -
    /// the fill is always `On`, unlike the style-aware indicators. Callers
    /// distinguish two fills by geometry, not colour: the shared scroll
    /// indicator draws a `Muted` track and a `Focus` thumb that differ only in
    /// width, and inverting either would make it vanish.
    fn fill_rect(&mut self, area: Area, _style: Style) {
        self.mark(area);
        let rect = self.px_rect(area);
        let style = PrimitiveStyle::with_fill(BinaryColor::On);
        let _ = rect.into_styled(style).draw(self.display);
    }

    /// Wash `area` in the `style`'s **background**: `On` when the [`Theme`]
    /// inverts the style, `Off` when it does not
    /// ([`mono_pair`](GraphicsTarget::mono_pair)) - the same pair every
    /// indicator resolves, so `draw_text(.., Style::Focus)` over the band lands
    /// `Off` glyphs on `On` and the row reads as one solid block.
    ///
    /// Unlike [`fill_rect`](RenderTarget::fill_rect) the style is honoured here:
    /// a band is *meant* to disappear for a style the theme leaves plain.
    fn fill_band(&mut self, area: Area, style: Style) {
        self.mark(area);
        let (_, bg) = self.mono_pair(style);
        self.fill_bg(area, bg);
    }

    /// One pixel in the `style`'s **foreground** - `Off` where the theme
    /// inverts the style, so a dot drawn on a focus band is visible.
    ///
    /// Unlike [`fill_rect`](RenderTarget::fill_rect), which is style-blind on
    /// purpose (the scroll indicator depends on it), the free-hand primitives
    /// honour the style: a hand-drawn widget has nothing else to say "ink" with.
    fn set_pixel(&mut self, x: u16, y: u16, style: Style) {
        self.mark(Area::new(x, y, 1, 1));
        let (fg, _) = self.mono_pair(style);
        let p = self.px_point(x, y);
        px_one(self.display, p, fg);
    }

    /// A native 1px line in the `style`'s foreground (no per-pixel dispatch).
    fn draw_line(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, style: Style) {
        self.mark(line_box(x0, y0, x1, y1));
        let (fg, _) = self.mono_pair(style);
        let (a, b) = (self.px_point(x0, y0), self.px_point(x1, y1));
        stroke_line(self.display, a, b, fg);
    }

    /// A 1px outline inside `area`, in the `style`'s foreground.
    fn draw_rect(&mut self, area: Area, style: Style) {
        self.mark(area);
        let (fg, _) = self.mono_pair(style);
        let rect = self.px_rect(area);
        stroke_rect(self.display, rect, fg);
    }

    /// A 1-bit sprite in the `style`'s foreground, its clear bits left
    /// transparent - so an icon over a focus band inverts with the row instead
    /// of punching a hole in it.
    fn draw_bitmap(&mut self, area: Area, bits: &[u8], style: Style) {
        self.mark(area);
        let (fg, _) = self.mono_pair(style);
        let origin = self.origin();
        blit(self.display, origin, area, bits, fg);
    }

    /// A smooth pixel bar: a rounded outline track with a solid rounded fill of
    /// width `fill_permille/1000`. `style` is **deliberately** ignored - track
    /// and fill are both `On` and read apart by outline vs solid. A `Slider`
    /// being edited passes `Style::Focus`; inverting on that would paint the
    /// fill `Off` on an `Off` background and the bar would disappear.
    fn draw_bar(&mut self, area: Area, fill_permille: u16, _style: Style) {
        self.mark(area);
        let cell = self.px_rect(area);
        let (w, h) = (cell.size.width, cell.size.height);
        if w == 0 || h == 0 {
            return;
        }
        // Inset vertically so stacked bars keep a gap and don't merge.
        let bh = if h > 2 { h - 2 } else { h };
        let top = cell.top_left + Point::new(0, ((h - bh) / 2) as i32);
        let rect = Rectangle::new(top, Size::new(w, bh));
        let r = corner_radius(rect.size);
        let track = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let _ = RoundedRectangle::with_equal_corners(rect, Size::new(r, r))
            .into_styled(track)
            .draw(self.display);
        let fw = (w * fill_permille as u32 / 1000).min(w);
        if fw > 0 {
            let frect = Rectangle::new(rect.top_left, Size::new(fw, bh));
            let fr = corner_radius(frect.size);
            let fill = PrimitiveStyle::with_fill(BinaryColor::On);
            let _ = RoundedRectangle::with_equal_corners(frect, Size::new(fr, fr))
                .into_styled(fill)
                .draw(self.display);
        }
    }

    /// A rounded square checkbox; filled inner square when `on`. The cell is
    /// washed in the `style`'s background and the square drawn in its
    /// foreground ([`mono_pair`](GraphicsTarget::mono_pair)), so a focused
    /// indicator inverts along with its label.
    fn draw_check(&mut self, area: Area, on: bool, style: Style) {
        self.mark(area);
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let (fg, bg) = self.mono_pair(style);
        self.fill_bg(area, bg);
        let bx = Rectangle::new(top, Size::new(s, s));
        let r = corner_radius(bx.size);
        let _ = RoundedRectangle::with_equal_corners(bx, Size::new(r, r))
            .into_styled(PrimitiveStyle::with_stroke(fg, 1))
            .draw(self.display);
        if on {
            let inset = (s / 4).max(1);
            if s > inset * 2 {
                let inner = Rectangle::new(
                    top + Point::new(inset as i32, inset as i32),
                    Size::new(s - inset * 2, s - inset * 2),
                );
                let ir = corner_radius(inner.size);
                let _ = RoundedRectangle::with_equal_corners(inner, Size::new(ir, ir))
                    .into_styled(PrimitiveStyle::with_fill(fg))
                    .draw(self.display);
            }
        }
    }

    /// A circle radio; filled centre dot when `on`. Inverts with the `style`,
    /// like [`draw_check`](RenderTarget::draw_check).
    fn draw_radio(&mut self, area: Area, on: bool, style: Style) {
        self.mark(area);
        let (top, d) = indicator_square(self.px_rect(area));
        if d == 0 {
            return;
        }
        let (fg, bg) = self.mono_pair(style);
        self.fill_bg(area, bg);
        let _ = Circle::new(top, d)
            .into_styled(PrimitiveStyle::with_stroke(fg, 1))
            .draw(self.display);
        if on {
            let inset = (d / 4).max(1);
            if d > inset * 2 {
                let dot = Circle::new(top + Point::new(inset as i32, inset as i32), d - inset * 2);
                let _ = dot
                    .into_styled(PrimitiveStyle::with_fill(fg))
                    .draw(self.display);
            }
        }
    }

    /// A filled triangle expander (down = expanded, right = collapsed). Inverts
    /// with the `style`, like [`draw_check`](RenderTarget::draw_check).
    fn draw_expander(&mut self, area: Area, expanded: bool, style: Style) {
        self.mark(area);
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let (fg, bg) = self.mono_pair(style);
        self.fill_bg(area, bg);
        let _ = expander_triangle(top, s, expanded)
            .into_styled(PrimitiveStyle::with_fill(fg))
            .draw(self.display);
    }

    /// Pixel spinner frame (Braille dot matrix / pulsing block); Line-style glyphs
    /// fall back to text. Both paths honour the `style`'s inversion, so one
    /// spinner does not change look between frame styles.
    fn draw_spinner(&mut self, area: Area, frame: char, style: Style) {
        self.mark(area);
        let (fg, bg) = self.mono_pair(style);
        let tl = self.px_point(area.x, area.y);
        self.fill_bg(area, bg);
        if !spinner_pixels(self.display, tl, area, frame, fg) {
            let mut b = [0u8; 4];
            self.draw_text(area.x, area.y, frame.encode_utf8(&mut b), style);
        }
    }
}

// ── ColorTheme ──────────────────────────────────────────────────────────────

/// A colour theme: maps each [`Style`] to an explicit `(foreground, background)`
/// colour pair, for full-colour displays (TFT panels in e.g. [`Rgb565`]).
///
/// This is a **separate model** from the monochrome [`Theme`] (which only knows
/// inversion + blink) - the two are deliberately not merged. Components are
/// unchanged: they still emit text tagged with a [`Style`]; the theme decides
/// the two colours each style renders in.
///
/// Default colours are defined for [`Rgb565`] (see [`ColorTheme::new`]); for any
/// other colour type, build one explicitly via [`ColorTheme::with_colors`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorTheme<C: PixelColor> {
    normal: (C, C),
    inverted: (C, C),
    accent: (C, C),
    muted: (C, C),
    danger: (C, C),
    focus: (C, C),
}

impl<C: PixelColor> ColorTheme<C> {
    /// The `(foreground, background)` colours for `style`.
    pub fn resolve(&self, style: Style) -> (C, C) {
        match style {
            Style::Normal => self.normal,
            Style::Inverted => self.inverted,
            Style::Accent => self.accent,
            Style::Muted => self.muted,
            Style::Danger => self.danger,
            Style::Focus => self.focus,
        }
    }

    /// Foreground colour for `style`.
    pub fn foreground(&self, style: Style) -> C {
        self.resolve(style).0
    }

    /// Background colour for `style`.
    pub fn background(&self, style: Style) -> C {
        self.resolve(style).1
    }

    /// Overrides the `(foreground, background)` colours for one [`Style`].
    pub fn with_colors(mut self, style: Style, fg: C, bg: C) -> Self {
        let slot = match style {
            Style::Normal => &mut self.normal,
            Style::Inverted => &mut self.inverted,
            Style::Accent => &mut self.accent,
            Style::Muted => &mut self.muted,
            Style::Danger => &mut self.danger,
            Style::Focus => &mut self.focus,
        };
        *slot = (fg, bg);
        self
    }
}

impl ColorTheme<Rgb565> {
    /// The default palette - Charm's Lip Gloss colours mapped to [`Rgb565`]
    /// (8-bit → 5/6/5 via `>>3, >>2, >>3`). Calm, not loud: accent/danger are
    /// **text** on the dark background (no colour blocks), and the selection
    /// (`Focus`/`Inverted`) is soft lilac on a subtle dark-grey row.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#FAFAFA` text| `#1A1A1A` bg    |
    /// | `Muted`    | `#767676` grey| `#1A1A1A` bg    |
    /// | `Accent`   | `#7D56F4` purple (text) | `#1A1A1A` bg |
    /// | `Focus`    | `#C5ADF9` lilac | `#3C3C3C` dark-grey |
    /// | `Inverted` | `#C5ADF9` lilac | `#3C3C3C` dark-grey |
    /// | `Danger`   | `#EB4268` pink (text) | `#1A1A1A` bg |
    pub fn new() -> Self {
        let text = Rgb565::new(31, 62, 31); // #FAFAFA
        let bg = Rgb565::new(3, 6, 3); // #1A1A1A
        let muted = Rgb565::new(14, 29, 14); // #767676
        let accent = Rgb565::new(15, 21, 30); // #7D56F4
        let lilac = Rgb565::new(24, 43, 31); // #C5ADF9
        let dark_grey = Rgb565::new(7, 15, 7); // #3C3C3C
        let danger = Rgb565::new(29, 16, 13); // #EB4268
        Self {
            normal: (text, bg),
            inverted: (lilac, dark_grey), // selection
            accent: (accent, bg),         // accent text, not a block
            muted: (muted, bg),
            danger: (danger, bg),      // danger text, not a block
            focus: (lilac, dark_grey), // selection focus
        }
    }

    /// Nord palette - Ice and frost tones mapped to [`Rgb565`]
    /// Arctic, cold, and professional.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#D8DEE9` silk| `#2E3440` polar |
    /// | `Muted`    | `#4C566A` grey| `#2E3440` polar |
    /// | `Accent`   | `#88C0D0` frost (text) | `#2E3440` polar |
    /// | `Focus`    | `#E5E9F0` snow | `#434C5E` night |
    /// | `Inverted` | `#E5E9F0` snow | `#434C5E` night |
    /// | `Danger`   | `#BF616A` red (text)  | `#2E3440` polar |
    pub fn nord() -> Self {
        let text = Rgb565::new(27, 55, 29); // #D8DEE9
        let bg = Rgb565::new(5, 13, 8); // #2E3440
        let muted = Rgb565::new(9, 21, 13); // #4C566A
        let accent = Rgb565::new(17, 48, 26); // #88C0D0
        let snow = Rgb565::new(28, 58, 30); // #E5E9F0
        let dark_grey = Rgb565::new(8, 19, 11); // #434C5E
        let danger = Rgb565::new(23, 24, 13); // #BF616A
        Self {
            normal: (text, bg),
            inverted: (snow, dark_grey),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (snow, dark_grey),
        }
    }

    /// Dracula palette - Vampire cyberpunk mapped to [`Rgb565`]
    /// High-vibrancy accents on a rich dark background.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#F8F8F2` white| `#282A36` dark |
    /// | `Muted`    | `#6272A4` blue| `#282A36` dark  |
    /// | `Accent`   | `#BD93F9` purple (text) | `#282A36` dark |
    /// | `Focus`    | `#F8F8F2` white| `#44475A` selection |
    /// | `Inverted` | `#F8F8F2` white| `#44475A` selection |
    /// | `Danger`   | `#FF5555` red (text) | `#282A36` dark |
    pub fn dracula() -> Self {
        let text = Rgb565::new(31, 62, 30); // #F8F8F2
        let bg = Rgb565::new(5, 10, 6); // #282A36
        let muted = Rgb565::new(12, 28, 20); // #6272A4
        let accent = Rgb565::new(23, 36, 31); // #BD93F9
        let selection = Rgb565::new(8, 17, 11); // #44475A
        let danger = Rgb565::new(31, 21, 10); // #FF5555
        Self {
            normal: (text, bg),
            inverted: (text, selection),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (text, selection),
        }
    }

    /// Gruvbox Dark palette - Retro groove mapped to [`Rgb565`]
    /// Warm, yellowish and soft vintage aesthetic.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#EBDBB2` sand | `#282828` pitch |
    /// | `Muted`    | `#928374` clay | `#282828` pitch |
    /// | `Accent`   | `#FABD2F` gold (text) | `#282828` pitch |
    /// | `Focus`    | `#FBF1C7` light-sand | `#504945` bark |
    /// | `Inverted` | `#FBF1C7` light-sand | `#504945` bark |
    /// | `Danger`   | `#FB4934` orange-red | `#282828` pitch |
    pub fn gruvbox() -> Self {
        let text = Rgb565::new(29, 54, 22); // #EBDBB2
        let bg = Rgb565::new(5, 10, 5); // #282828
        let muted = Rgb565::new(18, 32, 14); // #928374
        let accent = Rgb565::new(31, 47, 5); // #FABD2F
        let light_sand = Rgb565::new(31, 60, 24); // #FBF1C7
        let bark = Rgb565::new(10, 18, 8); // #504945
        let danger = Rgb565::new(31, 18, 6); // #FB4934
        Self {
            normal: (text, bg),
            inverted: (light_sand, bark),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (light_sand, bark),
        }
    }

    /// Matrix OLED palette - High-contrast monochrome & neon green mapped to [`Rgb565`]
    /// Pure black background with glowing cyber elements.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#FFFFFF` white| `#000000` oled-black |
    /// | `Muted`    | `#808080` gray | `#000000` oled-black |
    /// | `Accent`   | `#00FF33` matrix green | `#000000` oled-black |
    /// | `Focus`    | `#000000` black | `#FFFFFF` white |
    /// | `Inverted` | `#000000` black | `#FFFFFF` white |
    /// | `Danger`   | `#FF0033` neon red | `#000000` oled-black |
    pub fn matrix_oled() -> Self {
        let text = Rgb565::new(31, 63, 31); // #FFFFFF
        let bg = Rgb565::new(0, 0, 0); // #000000
        let muted = Rgb565::new(16, 32, 16); // #808080
        let accent = Rgb565::new(0, 63, 6); // #00FF33 (r>>3=0, g>>2=63, b>>3=6)
        let danger = Rgb565::new(31, 0, 6); // #FF0033
        Self {
            normal: (text, bg),
            inverted: (bg, text), // Полная инверсия для фокуса
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (bg, text),
        }
    }

    /// Cyberpunk Red palette - High contrast tactical sci-fi mapped to [`Rgb565`]
    /// Aggressive contrast for specialized terminal interfaces.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#00F0FF` cyan | `#0A0E17` deep-space |
    /// | `Muted`    | `#5D6978` slate| `#0A0E17` deep-space |
    /// | `Accent`   | `#FF0055` crimson | `#0A0E17` deep-space |
    /// | `Focus`    | `#0A0E17` deep | `#00F0FF` cyan row |
    /// | `Inverted` | `#0A0E17` deep | `#00F0FF` cyan row |
    /// | `Danger`   | `#FF0055` crimson | `#0A0E17` deep-space |
    pub fn cyberpunk() -> Self {
        let cyan = Rgb565::new(0, 60, 31); // #00F0FF
        let bg = Rgb565::new(1, 3, 2); // #0A0E17
        let muted = Rgb565::new(11, 26, 15); // #5D6978
        let crimson = Rgb565::new(31, 0, 10); // #FF0055
        Self {
            normal: (cyan, bg),
            inverted: (bg, cyan),
            accent: (crimson, bg),
            muted: (muted, bg),
            danger: (crimson, bg), // В этой теме Accent и Danger могут перекликаться
            focus: (bg, cyan),
        }
    }

    /// Synthwave '84 palette - Neon sunset mapped to [`Rgb565`]
    /// Deep purples with glowing pink and warm yellow accents.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#FDFDFD` white| `#262335` purple-ink |
    /// | `Muted`    | `#848BB8` lavender| `#262335` purple-ink |
    /// | `Accent`   | `#FF7EDB` neon pink (text) | `#262335` purple-ink |
    /// | `Focus`    | `#FEFA6B` yellow | `#372963` intense purple |
    /// | `Inverted` | `#FEFA6B` yellow | `#372963` intense purple |
    /// | `Danger`   | `#FE4450` hot red | `#262335` purple-ink |
    pub fn synthwave() -> Self {
        let text = Rgb565::new(31, 63, 31); // #FDFDFD
        let bg = Rgb565::new(4, 8, 6); // #262335
        let muted = Rgb565::new(16, 34, 23); // #848BB8
        let accent = Rgb565::new(31, 31, 27); // #FF7EDB
        let yellow = Rgb565::new(31, 62, 13); // #FEFA6B
        let dark_purple = Rgb565::new(6, 10, 12); // #372963
        let danger = Rgb565::new(31, 17, 10); // #FE4450
        Self {
            normal: (text, bg),
            inverted: (yellow, dark_purple),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (yellow, dark_purple),
        }
    }

    /// Tokyo Night palette - Clean neon Tokyo mapped to [`Rgb565`]
    /// Deep blue-indigo background with crisp cyan and pink elements.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#A9B1D6` light-blue | `#1A1B26` storm-bg |
    /// | `Muted`    | `#565F89` slate-blue | `#1A1B26` storm-bg |
    /// | `Accent`   | `#7AA2F7` blue (text) | `#1A1B26` storm-bg |
    /// | `Focus`    | `#73DACA` cyan | `#33467C` deep-blue |
    /// | `Inverted` | `#73DACA` cyan | `#33467C` deep-blue |
    /// | `Danger`   | `#F7768E` pink-red | `#1A1B26` storm-bg |
    pub fn tokyo_night() -> Self {
        let text = Rgb565::new(21, 44, 26); // #A9B1D6
        let bg = Rgb565::new(3, 6, 4); // #1A1B26
        let muted = Rgb565::new(10, 23, 17); // #565F89
        let accent = Rgb565::new(15, 40, 30); // #7AA2F7
        let cyan = Rgb565::new(14, 54, 25); // #73DACA
        let focus_bg = Rgb565::new(6, 17, 15); // #33467C
        let danger = Rgb565::new(30, 29, 17); // #F7768E
        Self {
            normal: (text, bg),
            inverted: (cyan, focus_bg),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (cyan, focus_bg),
        }
    }

    /// Everforest palette - Warm organic green mapped to [`Rgb565`]
    /// Forest tones, highly readable, ultra soft on the eyes.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#D3C6AA` oatmeal | `#1E2326` pine-wood |
    /// | `Muted`    | `#7A8478` moss | `#1E2326` pine-wood |
    /// | `Accent`   | `#A7C080` sage green (text) | `#1E2326` pine-wood |
    /// | `Focus`    | `#D3C6AA` oatmeal | `#3A4246` charcoal |
    /// | `Inverted` | `#D3C6AA` oatmeal | `#3A4246` charcoal |
    /// | `Danger`   | `#E67E80` terracotta | `#1E2326` pine-wood |
    pub fn everforest() -> Self {
        let text = Rgb565::new(26, 49, 21); // #D3C6AA
        let bg = Rgb565::new(3, 8, 4); // #1E2326
        let muted = Rgb565::new(15, 33, 15); // #7A8478
        let accent = Rgb565::new(20, 48, 16); // #A7C080
        let focus_bg = Rgb565::new(7, 16, 8); // #3A4246
        let danger = Rgb565::new(28, 31, 16); // #E67E80
        Self {
            normal: (text, bg),
            inverted: (text, focus_bg),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (text, focus_bg),
        }
    }

    /// Monokai Classic - Time-tested high contrast mapped to [`Rgb565`]
    /// Dark warm-grey background with vivid neon-pop highlights.
    ///
    /// | Style      | fg            | bg              |
    /// |------------|---------------|-----------------|
    /// | `Normal`   | `#F8F8F2` white-gray | `#272822` dark-stone |
    /// | `Muted`    | `#75715E` ash-gray | `#272822` dark-stone |
    /// | `Accent`   | `#E6DB74` yellow (text) | `#272822` dark-stone |
    /// | `Focus`    | `#F8F8F2` white-gray | `#49483E` medium-stone |
    /// | `Inverted` | `#F8F8F2` white-gray | `#49483E` medium-stone |
    /// | `Danger`   | `#F92672` candy-pink | `#272822` dark-stone |
    pub fn monokai() -> Self {
        let text = Rgb565::new(31, 62, 30); // #F8F8F2
        let bg = Rgb565::new(4, 10, 4); // #272822
        let muted = Rgb565::new(14, 28, 11); // #75715E
        let accent = Rgb565::new(28, 54, 14); // #E6DB74
        let focus_bg = Rgb565::new(9, 18, 7); // #49483E
        let danger = Rgb565::new(31, 9, 14); // #F92672
        Self {
            normal: (text, bg),
            inverted: (text, focus_bg),
            accent: (accent, bg),
            muted: (muted, bg),
            danger: (danger, bg),
            focus: (text, focus_bg),
        }
    }
}

impl Default for ColorTheme<Rgb565> {
    fn default() -> Self {
        Self::new()
    }
}

// ── ColorGraphicsTarget ───────────────────────────────────────────────────────

/// A [`RenderTarget`] adapter for any full-colour
/// [`DrawTarget`](embedded_graphics::draw_target::DrawTarget) - the colour twin
/// of [`GraphicsTarget`], left untouched alongside it.
///
/// Covers ST7789, ILI9341, and any other `embedded-graphics` driver whose colour
/// type is `C` (typically [`Rgb565`]). Coordinates are in **pixels**, exactly as
/// in [`GraphicsTarget`]; per-[`Style`] colours come from a [`ColorTheme`].
///
/// # Example
/// ```ignore
/// use knurl_graphics::ColorGraphicsTarget;
/// use embedded_graphics::mono_font::ascii::FONT_6X10;
///
/// // `display: SimulatorDisplay<Rgb565>` or a real ST7789 driver
/// let mut target = ColorGraphicsTarget::new(&mut display, FONT_6X10);
/// ```
pub struct ColorGraphicsTarget<'a, D, C: PixelColor> {
    display: &'a mut D,
    font: MonoFont<'static>,
    theme: ColorTheme<C>,
    dirty: DirtyRect,
}

impl<'a, D, C> ColorGraphicsTarget<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
    ColorTheme<C>: Default,
{
    /// Creates a target using the default [`ColorTheme`] for `C`.
    pub fn new(display: &'a mut D, font: MonoFont<'static>) -> Self {
        let size = display.bounding_box().size;
        Self {
            display,
            font,
            theme: ColorTheme::default(),
            dirty: DirtyRect::new(
                size.width.min(u16::MAX as u32) as u16,
                size.height.min(u16::MAX as u32) as u16,
            ),
        }
    }
}

impl<'a, D, C> ColorGraphicsTarget<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    /// Sets the [`ColorTheme`] controlling per-style colours.
    pub fn with_theme(mut self, theme: ColorTheme<C>) -> Self {
        self.theme = theme;
        self
    }

    /// The display's top-left pixel (origin offset; usually `(0, 0)`).
    fn origin(&self) -> Point {
        self.display.bounding_box().top_left
    }

    /// The pixel `Point` for `(x, y)`, offset by the display origin.
    fn px_point(&self, x: u16, y: u16) -> Point {
        self.origin() + Point::new(x as i32, y as i32)
    }

    /// The pixel `Rectangle` for `area`, offset by the display origin.
    fn px_rect(&self, area: Area) -> Rectangle {
        Rectangle::new(
            self.px_point(area.x, area.y),
            Size::new(area.w as u32, area.h as u32),
        )
    }

    /// Records `area` as touched - see [`GraphicsTarget::mark`] for the whole
    /// of the bookkeeping.
    fn mark(&mut self, area: Area) {
        self.dirty.add(area);
    }

    /// Records the pixel box `text` occupies at `(x, y)`, by the font metrics -
    /// the colour twin of [`GraphicsTarget::mark_text`].
    fn mark_text(&mut self, x: u16, y: u16, text: &str) {
        let (w, h) = (self.text_width(text), self.line_height());
        self.mark(Area::new(x, y, w, h));
    }

    /// The `(foreground, background)` colours this target renders `style` in -
    /// so drawing through [`clipped`](ColorGraphicsTarget::clipped) can stay on
    /// the theme instead of naming an `Rgb565`.
    pub fn colors(&self, style: Style) -> (C, C) {
        self.theme.resolve(style)
    }

    /// The raw embedded-graphics `DrawTarget`, **clipped to `area`** - the
    /// escape hatch for arcs, images and fonts of your own. The colour twin of
    /// [`GraphicsTarget::clipped`]; the same reasoning, the same clip, and the
    /// same conservative dirtying of `area` on the way out.
    pub fn clipped(&mut self, area: Area) -> Clipped<'_, D> {
        self.mark(area);
        let rect = self.px_rect(area);
        self.display.clipped(&rect)
    }

    /// The underlying `DrawTarget`, **unclipped**. Prefer
    /// [`clipped`](ColorGraphicsTarget::clipped) - this one marks the whole
    /// panel dirty, for the reason [`GraphicsTarget::display_mut`] spells out.
    pub fn display_mut(&mut self) -> &mut D {
        self.dirty.all();
        self.display
    }
}

impl<'a, D, C> RenderTarget for ColorGraphicsTarget<'a, D, C>
where
    D: DrawTarget<Color = C>,
    C: PixelColor,
{
    fn width(&self) -> u16 {
        self.display.bounding_box().size.width.min(u16::MAX as u32) as u16
    }

    fn height(&self) -> u16 {
        self.display.bounding_box().size.height.min(u16::MAX as u32) as u16
    }

    fn is_graphical(&self) -> bool {
        true
    }

    fn line_height(&self) -> u16 {
        self.font.character_size.height.min(u16::MAX as u32) as u16
    }

    fn char_width(&self) -> u16 {
        (self.font.character_size.width + self.font.character_spacing).min(u16::MAX as u32) as u16
    }

    fn take_dirty_rect(&mut self) -> Option<Area> {
        self.dirty.take()
    }

    /// Render `text` with its top-left at pixel `(x, y)`, in the theme's
    /// `(fg, bg)` for `style`. The background colour is always set so the whole
    /// cell repaints (required on a pixel display).
    fn draw_text(&mut self, x: u16, y: u16, text: &str, style: Style) {
        self.mark_text(x, y, text);
        let pos = self.px_point(x, y);
        let (fg, bg) = self.theme.resolve(style);
        let char_style = MonoTextStyleBuilder::new()
            .font(&self.font)
            .text_color(fg)
            .background_color(bg)
            .build();
        let _ = Text::with_baseline(text, pos, char_style, Baseline::Top).draw(self.display);
    }

    /// Draw a rectangular border, stroked in the `Normal` foreground colour.
    /// Same geometry rules as the monochrome target (`Single`/`Rounded` 1px,
    /// `Thick` 2px aligned inside, `Double` two concentric rectangles).
    fn draw_box(&mut self, area: Area, border: BorderStyle) {
        if matches!(border, BorderStyle::None) {
            return;
        }
        self.mark(area);
        let rect = self.px_rect(area);
        let top_left = rect.top_left;
        let size = rect.size;
        let stroke_color = self.theme.foreground(Style::Normal);

        match border {
            BorderStyle::None => unreachable!(),

            BorderStyle::Single => {
                let s = PrimitiveStyle::with_stroke(stroke_color, 1);
                let _ = Rectangle::new(top_left, size)
                    .into_styled(s)
                    .draw(self.display);
            }

            BorderStyle::Rounded => {
                let s = PrimitiveStyle::with_stroke(stroke_color, 1);
                let r = corner_radius(size);
                let _ = RoundedRectangle::with_equal_corners(
                    Rectangle::new(top_left, size),
                    Size::new(r, r),
                )
                .into_styled(s)
                .draw(self.display);
            }

            BorderStyle::Thick => {
                let s = PrimitiveStyleBuilder::new()
                    .stroke_color(stroke_color)
                    .stroke_width(2)
                    .stroke_alignment(StrokeAlignment::Inside)
                    .build();
                let _ = Rectangle::new(top_left, size)
                    .into_styled(s)
                    .draw(self.display);
            }

            BorderStyle::Double => {
                let s = PrimitiveStyle::with_stroke(stroke_color, 1);
                let _ = Rectangle::new(top_left, size)
                    .into_styled(s)
                    .draw(self.display);
                if size.width > 5 && size.height > 5 {
                    let inner = Rectangle::new(
                        top_left + Point::new(2, 2),
                        Size::new(size.width - 4, size.height - 4),
                    );
                    let _ = inner.into_styled(s).draw(self.display);
                }
            }
        }
    }

    /// Fill the pixel region with the `Normal` background colour.
    fn clear(&mut self, area: Area) {
        self.mark(area);
        let rect = self.px_rect(area);
        let s = PrimitiveStyle::with_fill(self.theme.background(Style::Normal));
        let _ = rect.into_styled(s).draw(self.display);
    }

    /// Fill the pixel region solid in the `style`'s foreground colour.
    fn fill_rect(&mut self, area: Area, style: Style) {
        self.mark(area);
        let rect = self.px_rect(area);
        let s = PrimitiveStyle::with_fill(self.theme.foreground(style));
        let _ = rect.into_styled(s).draw(self.display);
    }

    /// Wash `area` in the `style`'s **background** colour - the row background
    /// the theme pairs with that style's text (e.g. the selection's dark grey
    /// under `Focus`'s lilac), so the band and the text drawn over it agree.
    fn fill_band(&mut self, area: Area, style: Style) {
        self.mark(area);
        let rect = self.px_rect(area);
        let s = PrimitiveStyle::with_fill(self.theme.background(style));
        let _ = rect.into_styled(s).draw(self.display);
    }

    /// One pixel in the `style`'s foreground colour.
    fn set_pixel(&mut self, x: u16, y: u16, style: Style) {
        self.mark(Area::new(x, y, 1, 1));
        let color = self.theme.foreground(style);
        let p = self.px_point(x, y);
        px_one(self.display, p, color);
    }

    /// A native 1px line in the `style`'s foreground colour.
    fn draw_line(&mut self, x0: u16, y0: u16, x1: u16, y1: u16, style: Style) {
        self.mark(line_box(x0, y0, x1, y1));
        let color = self.theme.foreground(style);
        let (a, b) = (self.px_point(x0, y0), self.px_point(x1, y1));
        stroke_line(self.display, a, b, color);
    }

    /// A 1px outline inside `area`, in the `style`'s foreground colour.
    fn draw_rect(&mut self, area: Area, style: Style) {
        self.mark(area);
        let color = self.theme.foreground(style);
        let rect = self.px_rect(area);
        stroke_rect(self.display, rect, color);
    }

    /// A 1-bit sprite in the `style`'s foreground colour; clear bits leave
    /// whatever was underneath.
    fn draw_bitmap(&mut self, area: Area, bits: &[u8], style: Style) {
        self.mark(area);
        let color = self.theme.foreground(style);
        let origin = self.origin();
        blit(self.display, origin, area, bits, color);
    }

    /// A smooth Charm-style bar: a dark-grey rounded track with a rounded fill in
    /// the `style`'s colour (e.g. purple for `Accent`, lilac for `Focus`), filled
    /// to `fill_permille/1000`.
    fn draw_bar(&mut self, area: Area, fill_permille: u16, style: Style) {
        self.mark(area);
        let cell = self.px_rect(area);
        let (w, h) = (cell.size.width, cell.size.height);
        if w == 0 || h == 0 {
            return;
        }
        // Inset vertically so stacked bars keep a gap and don't merge.
        let bh = if h > 2 { h - 2 } else { h };
        let top = cell.top_left + Point::new(0, ((h - bh) / 2) as i32);
        let rect = Rectangle::new(top, Size::new(w, bh));
        // Track colour = the selection's dark-grey background; fill = the
        // requested style's foreground (accent purple / focus lilac).
        let track_color = self.theme.background(Style::Focus);
        let fill_color = self.theme.foreground(style);
        let r = corner_radius(rect.size);

        let track = PrimitiveStyle::with_fill(track_color);
        let _ = RoundedRectangle::with_equal_corners(rect, Size::new(r, r))
            .into_styled(track)
            .draw(self.display);

        let fw = (w * fill_permille as u32 / 1000).min(w);
        if fw > 0 {
            let frect = Rectangle::new(rect.top_left, Size::new(fw, bh));
            let fr = corner_radius(frect.size);
            let fill = PrimitiveStyle::with_fill(fill_color);
            let _ = RoundedRectangle::with_equal_corners(frect, Size::new(fr, fr))
                .into_styled(fill)
                .draw(self.display);
        }
    }

    /// A rounded square checkbox in the `style`'s colour; filled when `on`.
    fn draw_check(&mut self, area: Area, on: bool, style: Style) {
        self.mark(area);
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let color = self.theme.foreground(style);
        let bx = Rectangle::new(top, Size::new(s, s));
        let r = corner_radius(bx.size);
        let _ = RoundedRectangle::with_equal_corners(bx, Size::new(r, r))
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(self.display);
        if on {
            let inset = (s / 4).max(1);
            if s > inset * 2 {
                let inner = Rectangle::new(
                    top + Point::new(inset as i32, inset as i32),
                    Size::new(s - inset * 2, s - inset * 2),
                );
                let ir = corner_radius(inner.size);
                let _ = RoundedRectangle::with_equal_corners(inner, Size::new(ir, ir))
                    .into_styled(PrimitiveStyle::with_fill(color))
                    .draw(self.display);
            }
        }
    }

    /// A circle radio in the `style`'s colour; filled centre dot when `on`.
    fn draw_radio(&mut self, area: Area, on: bool, style: Style) {
        self.mark(area);
        let (top, d) = indicator_square(self.px_rect(area));
        if d == 0 {
            return;
        }
        let color = self.theme.foreground(style);
        let _ = Circle::new(top, d)
            .into_styled(PrimitiveStyle::with_stroke(color, 1))
            .draw(self.display);
        if on {
            let inset = (d / 4).max(1);
            if d > inset * 2 {
                let dot = Circle::new(top + Point::new(inset as i32, inset as i32), d - inset * 2);
                let _ = dot
                    .into_styled(PrimitiveStyle::with_fill(color))
                    .draw(self.display);
            }
        }
    }

    /// A filled triangle expander in the `style`'s colour (down = expanded,
    /// right = collapsed).
    fn draw_expander(&mut self, area: Area, expanded: bool, style: Style) {
        self.mark(area);
        let (top, s) = indicator_square(self.px_rect(area));
        if s == 0 {
            return;
        }
        let _ = expander_triangle(top, s, expanded)
            .into_styled(PrimitiveStyle::with_fill(self.theme.foreground(style)))
            .draw(self.display);
    }

    /// Pixel spinner frame in the `style`'s colour; Line-style glyphs fall back to
    /// text.
    fn draw_spinner(&mut self, area: Area, frame: char, style: Style) {
        self.mark(area);
        let tl = self.px_point(area.x, area.y);
        let color = self.theme.foreground(style);
        if !spinner_pixels(self.display, tl, area, frame, color) {
            let mut b = [0u8; 4];
            self.draw_text(area.x, area.y, frame.encode_utf8(&mut b), style);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_default_only_inverted() {
        let t = Theme::new();
        assert!(t.resolve(Style::Inverted));
        assert!(!t.resolve(Style::Normal));
        assert!(!t.resolve(Style::Accent));
        assert!(!t.resolve(Style::Muted));
        assert!(!t.resolve(Style::Danger));
    }

    #[test]
    fn theme_accent_inverted() {
        let t = Theme::new().with_inverted(Theme::INVERTED | Theme::ACCENT);
        assert!(t.resolve(Style::Accent));
        assert!(!t.resolve(Style::Normal));
    }

    #[test]
    fn theme_blink_phase() {
        let mut t = Theme::new()
            .with_inverted(Theme::ACCENT)
            .with_blink(Theme::ACCENT);
        assert!(t.resolve(Style::Accent));
        t.toggle_blink();
        assert!(!t.resolve(Style::Accent));
        assert!(!t.resolve(Style::Inverted));
    }

    #[test]
    fn theme_default_focus_visible() {
        assert!(Theme::new().resolve(Style::Focus));
    }

    #[test]
    fn theme_focus_blink() {
        let mut t = Theme::new().with_blink(Theme::FOCUS);
        assert!(t.resolve(Style::Focus));
        t.toggle_blink();
        assert!(!t.resolve(Style::Focus));
    }

    #[test]
    fn theme_blink_does_not_affect_others() {
        let mut t = Theme::new().with_blink(Theme::ACCENT);
        let before = t.resolve(Style::Normal);
        t.toggle_blink();
        assert_eq!(t.resolve(Style::Normal), before);
    }

    // ── Metrics ───────────────────────────────────────────────────────────────

    #[test]
    fn target_reports_pixel_dimensions_and_font_metrics() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        disp.set_allow_overdraw(true);
        disp.set_allow_out_of_bounds_drawing(true);
        let tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
        // MockDisplay is 64×64 px. FONT_6X10 → 6px advance, 10px line.
        assert_eq!(tgt.width(), 64);
        assert_eq!(tgt.height(), 64);
        assert_eq!(tgt.char_width(), 6);
        assert_eq!(tgt.line_height(), 10);
        assert_eq!(tgt.text_width("Hi"), 12);
    }

    // ── ColorTheme ──────────────────────────────────────────────────────────

    #[test]
    fn color_theme_defaults() {
        let t = ColorTheme::<Rgb565>::default();
        let text = Rgb565::new(31, 62, 31);
        let bg = Rgb565::new(3, 6, 3);
        let muted = Rgb565::new(14, 29, 14);
        let accent = Rgb565::new(15, 21, 30);
        let lilac = Rgb565::new(24, 43, 31);
        let dark_grey = Rgb565::new(7, 15, 7);
        let danger = Rgb565::new(29, 16, 13);

        assert_eq!(t.resolve(Style::Normal), (text, bg));
        assert_eq!(t.resolve(Style::Muted), (muted, bg));
        assert_eq!(t.resolve(Style::Accent), (accent, bg));
        assert_eq!(t.resolve(Style::Danger), (danger, bg));
        assert_eq!(t.resolve(Style::Focus), (lilac, dark_grey));
        assert_eq!(t.resolve(Style::Inverted), (lilac, dark_grey));
    }

    #[test]
    fn color_theme_override() {
        let t =
            ColorTheme::<Rgb565>::new().with_colors(Style::Accent, Rgb565::WHITE, Rgb565::GREEN);
        assert_eq!(t.resolve(Style::Accent), (Rgb565::WHITE, Rgb565::GREEN));
        assert_eq!(
            t.resolve(Style::Normal),
            (Rgb565::new(31, 62, 31), Rgb565::new(3, 6, 3))
        );
    }

    #[test]
    fn color_target_renders_via_rendertarget() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<Rgb565>::new();
        disp.set_allow_overdraw(true);
        disp.set_allow_out_of_bounds_drawing(true);

        let mut tgt = ColorGraphicsTarget::new(&mut disp, FONT_6X10);
        assert!(tgt.is_graphical());
        // Exercise every (ported) RenderTarget method in pixel coordinates.
        tgt.draw_text(0, 0, "Hi", Style::Accent);
        tgt.draw_box(Area::new(0, 0, 24, 18), BorderStyle::Single);
        tgt.draw_box(Area::new(0, 0, 30, 24), BorderStyle::Rounded);
        tgt.fill_rect(Area::new(0, 0, 10, 1), Style::Muted);
        tgt.draw_bar(Area::new(0, 0, 36, 6), 600, Style::Accent);
        tgt.draw_check(Area::new(0, 0, 12, 10), true, Style::Focus);
        tgt.draw_check(Area::new(0, 12, 12, 10), false, Style::Muted);
        tgt.draw_radio(Area::new(0, 24, 12, 10), true, Style::Focus);
        tgt.draw_radio(Area::new(0, 36, 12, 10), false, Style::Muted);
        tgt.draw_expander(Area::new(0, 48, 10, 10), true, Style::Focus);
        tgt.draw_expander(Area::new(0, 56, 10, 10), false, Style::Muted);
        tgt.draw_spinner(Area::new(0, 0, 6, 10), '⠋', Style::Accent); // braille
        tgt.draw_spinner(Area::new(0, 0, 6, 10), '▓', Style::Accent); // block
        tgt.draw_spinner(Area::new(0, 0, 6, 10), '/', Style::Accent); // glyph fallback
        tgt.clear(Area::new(0, 0, 12, 10));
    }

    #[test]
    fn mono_box_bar_and_fill_draw_without_panic() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        disp.set_allow_overdraw(true);
        disp.set_allow_out_of_bounds_drawing(true);
        let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
        tgt.draw_box(Area::new(0, 0, 36, 24), BorderStyle::Rounded);
        tgt.draw_bar(Area::new(0, 0, 36, 6), 400, Style::Accent);
        tgt.fill_rect(Area::new(0, 10, 36, 1), Style::Muted);
        tgt.draw_check(Area::new(0, 12, 12, 10), true, Style::Focus);
        tgt.draw_radio(Area::new(0, 24, 12, 10), true, Style::Focus);
    }

    // ── Mono pixel tests ────────────────────────────────────────────────────

    /// Which indicator a pixel test drives.
    #[derive(Debug, Clone, Copy)]
    enum Ind {
        Check,
        Radio,
        Expander,
        Braille,
        Glyph,
    }

    /// Draws one indicator in `style` into a fresh mock and counts the `On`
    /// pixels inside `area`.
    fn mono_on_pixels(ind: Ind, style: Style, area: Area) -> usize {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        // The background wash goes down first, the shape on top.
        disp.set_allow_overdraw(true);
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            match ind {
                Ind::Check => tgt.draw_check(area, true, style),
                Ind::Radio => tgt.draw_radio(area, false, style),
                Ind::Expander => tgt.draw_expander(area, true, style),
                Ind::Braille => tgt.draw_spinner(area, '⠋', style),
                Ind::Glyph => tgt.draw_spinner(area, '/', style),
            }
        }
        let mut on = 0;
        for y in area.y..area.y + area.h {
            for x in area.x..area.x + area.w {
                if disp.get_pixel(Point::new(x as i32, y as i32)) == Some(BinaryColor::On) {
                    on += 1;
                }
            }
        }
        on
    }

    /// A focused indicator must invert exactly like the label beside it: the
    /// `Focus` rendering is the pixel-for-pixel inverse of the `Normal` one.
    #[test]
    fn mono_indicators_invert_under_focus() {
        let area = Area::new(0, 0, 12, 10);
        let total = (area.w * area.h) as usize;
        for ind in [
            Ind::Check,
            Ind::Radio,
            Ind::Expander,
            Ind::Braille,
            Ind::Glyph,
        ] {
            let normal = mono_on_pixels(ind, Style::Normal, area);
            let focus = mono_on_pixels(ind, Style::Focus, area);
            assert!(normal > 0, "{ind:?}: nothing drawn for Style::Normal");
            assert_ne!(normal, focus, "{ind:?}: Focus renders like Normal");
            assert_eq!(
                normal + focus,
                total,
                "{ind:?}: Focus is not the inverse of Normal"
            );
        }
    }

    /// The band is the inverse of a plain row: `Focus` (inverted by the default
    /// theme) fills it solid `On`, `Normal` leaves it `Off`, and text drawn over
    /// a `Focus` band lands `Off`-on-`On` - one solid block, no holes.
    #[test]
    fn mono_band_fills_only_for_an_inverted_style() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let area = Area::new(0, 0, 60, 10);
        let total = (area.w * area.h) as usize;

        let count = |style: Style, with_text: bool| {
            let mut disp = MockDisplay::<BinaryColor>::new();
            disp.set_allow_overdraw(true);
            {
                let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
                tgt.fill_band(area, style);
                if with_text {
                    tgt.draw_text(0, 0, "Bright", style);
                }
            }
            let mut on = 0;
            for y in area.y..area.y + area.h {
                for x in area.x..area.x + area.w {
                    if disp.get_pixel(Point::new(x as i32, y as i32)) == Some(BinaryColor::On) {
                        on += 1;
                    }
                }
            }
            on
        };

        assert_eq!(count(Style::Focus, false), total, "Focus must fill the row");
        assert_eq!(count(Style::Normal, false), 0, "Normal must not fill");
        // Glyphs punch dark holes in the band, but the band survives around them:
        // the row stays overwhelmingly lit, which is what "one block" means.
        let with_text = count(Style::Focus, true);
        assert!(with_text < total, "the glyphs must be readable on the band");
        assert!(
            with_text * 10 > total * 8,
            "the band must survive the text: {with_text}/{total} lit"
        );
    }

    /// The focus band, end to end, on a real widget: a focused `List` on a
    /// monochrome panel must draw its selected row as a **solid block** - far
    /// more lit than the row below it, and with no gap running down its right
    /// side where the text ran out. An unfocused list must not invert anything.
    #[test]
    fn mono_focused_list_row_is_a_solid_block() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;
        use knurl_core::{Component, List};

        const ITEMS: &[&str] = &["Alpha", "Beta", "Gamma"];
        let area = Area::new(0, 0, 60, 30); // 3 rows of FONT_6X10

        // Lit pixels per row, for a list that is (or is not) focused.
        let rows = |focused: bool| {
            let mut disp = MockDisplay::<BinaryColor>::new();
            disp.set_allow_overdraw(true);
            {
                let mut list = List::new(ITEMS);
                if focused {
                    list.focus();
                }
                let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
                list.view(&mut tgt, area);
            }
            let mut per_row = [0usize; 3];
            for (r, count) in per_row.iter_mut().enumerate() {
                for y in (r as i32 * 10)..(r as i32 * 10 + 10) {
                    for x in 0..area.w as i32 {
                        if disp.get_pixel(Point::new(x, y)) == Some(BinaryColor::On) {
                            *count += 1;
                        }
                    }
                }
            }
            per_row
        };

        let focused = rows(true);
        let plain = rows(false);
        let row_px = (area.w * 10) as usize;

        // The banded row is nearly the whole row lit; the glyphs are the holes.
        assert!(
            focused[0] * 10 > row_px * 8,
            "selected row not a block: {}/{row_px} lit",
            focused[0]
        );
        // …and it stands far apart from the unselected row below it.
        assert!(
            focused[0] > focused[1] * 4,
            "selected {} vs next {} - not a clear difference",
            focused[0],
            focused[1]
        );
        // Unfocused: no inversion anywhere, just glyph ink on every row.
        for (r, lit) in plain.iter().enumerate() {
            assert!(*lit * 10 < row_px * 5, "row {r} inverted while unfocused");
        }
        assert!(plain[0] > 0, "the cursor row must still be drawn");
    }

    /// A style the theme does not invert (`Muted`) renders like `Normal`.
    #[test]
    fn mono_indicator_keeps_non_inverting_styles() {
        let area = Area::new(0, 0, 12, 10);
        assert_eq!(
            mono_on_pixels(Ind::Check, Style::Normal, area),
            mono_on_pixels(Ind::Check, Style::Muted, area)
        );
    }

    /// The expander triangle must fit the square `indicator_square` hands out -
    /// vertices spanning `0..=s` put a pixel outside it, one column right and
    /// one row below.
    #[test]
    fn mono_expander_fits_the_indicator_square() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        for expanded in [true, false] {
            for (w, h) in [(12u16, 10u16), (6, 10), (10, 4), (3, 3), (2, 2), (1, 1)] {
                let area = Area::new(1, 1, w, h);
                let (top, s) = indicator_square(Rectangle::new(
                    Point::new(1, 1),
                    Size::new(u32::from(w), u32::from(h)),
                ));
                let square = Rectangle::new(top, Size::new(s, s));

                let mut disp = MockDisplay::<BinaryColor>::new();
                disp.set_allow_overdraw(true);
                {
                    let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
                    // Style::Normal: the triangle is On, the cell wash Off.
                    tgt.draw_expander(area, expanded, Style::Normal);
                }
                let mut lit = 0;
                for y in 0..64 {
                    for x in 0..64 {
                        let p = Point::new(x, y);
                        if disp.get_pixel(p) == Some(BinaryColor::On) {
                            lit += 1;
                            assert!(
                                square.contains(p),
                                "{w}x{h} expanded={expanded}: pixel {p:?} outside {square:?}"
                            );
                        }
                    }
                }
                assert!(lit > 0, "{w}x{h} expanded={expanded}: nothing drawn");
            }
        }
    }

    /// A `Thick` border must stay within its `Area` - the 2px stroke is aligned
    /// inside, not centred on the boundary (which would spill 1px outwards).
    #[test]
    fn mono_thick_border_stays_inside_the_area() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.draw_box(Area::new(2, 2, 10, 8), BorderStyle::Thick);
        }
        assert_eq!(
            disp.affected_area(),
            Rectangle::new(Point::new(2, 2), Size::new(10, 8)),
            "the stroke escapes the area"
        );
        // Both stroke rings lit, the interior left untouched.
        assert_eq!(disp.get_pixel(Point::new(2, 2)), Some(BinaryColor::On));
        assert_eq!(disp.get_pixel(Point::new(3, 3)), Some(BinaryColor::On));
        assert_eq!(disp.get_pixel(Point::new(4, 4)), None);
        assert_eq!(disp.get_pixel(Point::new(11, 9)), Some(BinaryColor::On));
    }

    /// The colour target aligns its `Thick` stroke the same way.
    #[test]
    fn color_thick_border_stays_inside_the_area() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<Rgb565>::new();
        {
            let mut tgt = ColorGraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.draw_box(Area::new(2, 2, 10, 8), BorderStyle::Thick);
        }
        assert_eq!(
            disp.affected_area(),
            Rectangle::new(Point::new(2, 2), Size::new(10, 8)),
            "the stroke escapes the area"
        );
    }

    // ── Free-hand primitives, natively ──────────────────────────────────────

    /// The native line is the line that was asked for: both endpoints, one
    /// pixel per column on a shallow slope, nothing outside the bounding box.
    #[test]
    fn mono_line_is_drawn_end_to_end() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.draw_line(2, 2, 10, 5, Style::Normal);
        }
        assert_eq!(disp.get_pixel(Point::new(2, 2)), Some(BinaryColor::On));
        assert_eq!(disp.get_pixel(Point::new(10, 5)), Some(BinaryColor::On));
        assert_eq!(
            disp.affected_area(),
            Rectangle::new(Point::new(2, 2), Size::new(9, 4)),
            "the line escapes its own bounding box"
        );
    }

    /// The free-hand primitives honour the style where `fill_rect` cannot: on a
    /// focus band the ink has to go `Off`, or a hand-drawn widget disappears the
    /// moment the cursor lands on it.
    #[test]
    fn mono_free_hand_ink_inverts_with_the_style() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        for style in [Style::Normal, Style::Focus] {
            let inverted = Theme::new().resolve(style);
            let mut disp = MockDisplay::<BinaryColor>::new();
            disp.set_allow_overdraw(true);
            {
                let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
                tgt.set_pixel(1, 1, style);
                tgt.draw_line(3, 1, 6, 4, style);
                tgt.draw_rect(Area::new(8, 1, 5, 4), style);
                tgt.draw_bitmap(Area::new(1, 6, 8, 1), &[0b1111_0000], style);
            }
            let want = if inverted {
                BinaryColor::Off
            } else {
                BinaryColor::On
            };
            for p in [
                Point::new(1, 1), // the pixel
                Point::new(3, 1), // the line's start
                Point::new(8, 1), // the outline's corner
                Point::new(1, 6), // the sprite's first lit bit
            ] {
                assert_eq!(disp.get_pixel(p), Some(want), "{style:?} at {p:?}");
            }
            // …and `fill_rect` still is not style-aware, which is the contract
            // the scroll indicator rests on.
            assert_eq!(disp.get_pixel(Point::new(9, 2)), None, "outline is hollow");
        }
    }

    /// The outline stays inside its area, like every other piece of chrome.
    #[test]
    fn mono_rect_outline_stays_inside_and_stays_hollow() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.draw_rect(Area::new(2, 3, 8, 6), Style::Normal);
        }
        assert_eq!(
            disp.affected_area(),
            Rectangle::new(Point::new(2, 3), Size::new(8, 6))
        );
        assert_eq!(disp.get_pixel(Point::new(2, 3)), Some(BinaryColor::On));
        assert_eq!(disp.get_pixel(Point::new(9, 8)), Some(BinaryColor::On));
        assert_eq!(
            disp.get_pixel(Point::new(5, 5)),
            None,
            "the interior is left"
        );
    }

    /// The native blit reads the same bits as the shared default: row-major,
    /// MSB first, rows padded to whole bytes, clear bits transparent.
    #[test]
    fn mono_bitmap_matches_the_documented_format() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            // 12px wide → 2 bytes per row; row 0 lights columns 0, 7 and 8.
            tgt.draw_bitmap(
                Area::new(1, 1, 12, 2),
                &[0b1000_0001, 0b1000_1111, 0b0000_0000, 0b0100_0000],
                Style::Normal,
            );
        }
        for p in [
            Point::new(1, 1),
            Point::new(8, 1),
            Point::new(9, 1),
            Point::new(10, 2),
        ] {
            assert_eq!(disp.get_pixel(p), Some(BinaryColor::On), "{p:?} unlit");
        }
        // The 4 padding bits of row 0's second byte are not pixels of row 1.
        assert_eq!(disp.get_pixel(Point::new(1, 2)), None);
        assert_eq!(
            disp.affected_area(),
            Rectangle::new(Point::new(1, 1), Size::new(10, 2)),
            "the sprite paints outside its own columns"
        );
    }

    /// The colour target draws the same shapes in the theme's colours.
    #[test]
    fn color_free_hand_primitives_use_the_theme() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let theme = ColorTheme::<Rgb565>::default();
        let mut disp = MockDisplay::<Rgb565>::new();
        disp.set_allow_overdraw(true);
        {
            let mut tgt = ColorGraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.set_pixel(0, 0, Style::Danger);
            tgt.draw_line(2, 0, 8, 3, Style::Accent);
            tgt.draw_rect(Area::new(0, 5, 6, 4), Style::Muted);
            tgt.draw_bitmap(Area::new(0, 10, 8, 1), &[0b1100_0000], Style::Accent);
        }
        assert_eq!(
            disp.get_pixel(Point::new(0, 0)),
            Some(theme.foreground(Style::Danger))
        );
        assert_eq!(
            disp.get_pixel(Point::new(2, 0)),
            Some(theme.foreground(Style::Accent))
        );
        assert_eq!(
            disp.get_pixel(Point::new(0, 5)),
            Some(theme.foreground(Style::Muted))
        );
        assert_eq!(
            disp.get_pixel(Point::new(1, 10)),
            Some(theme.foreground(Style::Accent))
        );
        assert_eq!(
            disp.get_pixel(Point::new(2, 10)),
            None,
            "clear bits are clear"
        );
    }

    // ── Escape hatch ────────────────────────────────────────────────────────

    /// The clip is the point of the escape hatch: a shape that overshoots its
    /// area is masked, not painted onto the widget next door. Checked on both
    /// targets, and against the area's **offset** too - the hatch works in the
    /// widget's own pixel coordinates, translated by the display origin.
    #[test]
    fn the_escape_hatch_is_clipped_to_the_area() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let area = Area::new(4, 4, 6, 6);
        let long = Line::new(Point::new(0, 6), Point::new(40, 6));

        let mut mono = MockDisplay::<BinaryColor>::new();
        {
            let mut tgt = GraphicsTarget::new(&mut mono, FONT_6X10);
            let (ink, _) = tgt.colors(Style::Normal);
            let mut raw = tgt.clipped(area);
            long.into_styled(PrimitiveStyle::with_stroke(ink, 1))
                .draw(&mut raw)
                .unwrap();
        }
        assert_eq!(
            mono.affected_area(),
            Rectangle::new(Point::new(4, 6), Size::new(6, 1)),
            "the line was not clipped to the area"
        );

        let mut color = MockDisplay::<Rgb565>::new();
        {
            let mut tgt = ColorGraphicsTarget::new(&mut color, FONT_6X10);
            let (ink, _) = tgt.colors(Style::Accent);
            let mut raw = tgt.clipped(area);
            long.into_styled(PrimitiveStyle::with_stroke(ink, 1))
                .draw(&mut raw)
                .unwrap();
        }
        assert_eq!(
            color.affected_area(),
            Rectangle::new(Point::new(4, 6), Size::new(6, 1))
        );
        assert_eq!(
            color.get_pixel(Point::new(4, 6)),
            Some(ColorTheme::<Rgb565>::default().foreground(Style::Accent)),
            "the hatch must be able to stay on the theme"
        );
    }

    /// The indicator square's placement rule, stated as a test: centred
    /// vertically in its cell, flush with the cell's left edge, and never wider
    /// than the cell. Left-aligned rather than centred because the label beside
    /// it starts at a fixed column - a centred square would drift with the row
    /// height and stop lining up with the labels above and below it.
    #[test]
    fn indicator_square_is_centred_vertically_and_flush_left() {
        for (w, h) in [(18u32, 10u32), (18, 20), (4, 10), (1, 1), (18, 2), (18, 3)] {
            let cell = Rectangle::new(Point::new(3, 7), Size::new(w, h));
            let (top, s) = indicator_square(cell);
            assert!(s <= w && s <= h, "{w}x{h}: square {s} escapes the cell");
            assert_eq!(top.x, cell.top_left.x, "{w}x{h}: not flush left");
            // Equal slack above and below (odd remainders fall to the bottom).
            let above = top.y - cell.top_left.y;
            let below = (cell.top_left.y + h as i32) - (top.y + s as i32);
            assert!(
                above <= below && below - above <= 1,
                "{w}x{h}: {above} above vs {below} below - not centred"
            );
        }
    }

    /// …and it stays centred once a focus band is under it: the band fills the
    /// same cell, so the lit margin above and below the square must match.
    #[test]
    fn mono_indicator_sits_centred_on_a_focus_band() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let area = Area::new(0, 0, 18, 10);
        let mut disp = MockDisplay::<BinaryColor>::new();
        disp.set_allow_overdraw(true);
        {
            let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
            tgt.fill_band(area, Style::Focus);
            tgt.draw_check(area, false, Style::Focus);
        }
        // On the band the square is drawn `Off`; the rows it does not touch stay
        // fully lit. Count the untouched rows above and below it.
        let row_all_on = |y: i32| {
            (0..area.w as i32).all(|x| disp.get_pixel(Point::new(x, y)) == Some(BinaryColor::On))
        };
        let above = (0..10).take_while(|&y| row_all_on(y)).count();
        let below = (0..10).rev().take_while(|&y| row_all_on(y)).count();
        assert!(above > 0 && below > 0, "the square fills the whole cell");
        assert!(
            above.abs_diff(below) <= 1,
            "{above} lit rows above vs {below} below - the square is off-centre"
        );
    }

    // ── The dirty region ────────────────────────────────────────────────────

    /// A frame in which nothing was drawn costs nothing to send.
    #[test]
    fn an_untouched_target_has_no_region() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
        assert_eq!(tgt.take_dirty_rect(), None);
    }

    /// The region is one box over every call, and `take` means "sent".
    #[test]
    fn the_region_unions_the_calls_and_take_resets_it() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
        tgt.fill_rect(Area::new(2, 4, 6, 2), Style::Normal); // 2..8  x 4..6
        tgt.fill_rect(Area::new(20, 30, 4, 4), Style::Normal); // 20..24 x 30..34
        assert_eq!(tgt.take_dirty_rect(), Some(Area::new(2, 4, 22, 30)));
        assert_eq!(tgt.take_dirty_rect(), None);
    }

    /// Text dirties the cells its glyphs occupy - by the font's metrics, not the
    /// row it sits on. FONT_6X10 on a 64px panel: two characters are 12px, not
    /// 54 to the right edge.
    #[test]
    fn text_dirties_its_glyph_cells_only() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        for font_h in [10u16, 18] {
            let font = if font_h == 10 {
                FONT_6X10
            } else {
                embedded_graphics::mono_font::ascii::FONT_9X18_BOLD
            };
            let mut disp = MockDisplay::<BinaryColor>::new();
            disp.set_allow_out_of_bounds_drawing(true);
            let mut tgt = GraphicsTarget::new(&mut disp, font);
            let (cw, lh) = (tgt.char_width(), tgt.line_height());
            tgt.draw_text(10, 20, "Hi", Style::Normal);
            assert_eq!(tgt.take_dirty_rect(), Some(Area::new(10, 20, 2 * cw, lh)));
        }
    }

    /// The escape hatch draws behind the target's back, so handing it out is
    /// itself the dirty event: `area` is in the region whether or not a pixel
    /// was drawn through it. Over-reporting is the safe direction; a hatch that
    /// did draw and was not counted would leave stale pixels on the panel.
    #[test]
    fn the_escape_hatch_is_counted_when_it_is_handed_out() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let area = Area::new(4, 4, 6, 6);

        let mut mono = MockDisplay::<BinaryColor>::new();
        let mut tgt = GraphicsTarget::new(&mut mono, FONT_6X10);
        {
            let (ink, _) = tgt.colors(Style::Normal);
            let mut raw = tgt.clipped(area);
            Line::new(Point::new(0, 6), Point::new(40, 6))
                .into_styled(PrimitiveStyle::with_stroke(ink, 1))
                .draw(&mut raw)
                .unwrap();
        }
        assert_eq!(tgt.take_dirty_rect(), Some(area));

        let mut color = MockDisplay::<Rgb565>::new();
        let mut ctgt = ColorGraphicsTarget::new(&mut color, FONT_6X10);
        let _ = ctgt.clipped(area); // not one pixel drawn - still conservative
        assert_eq!(ctgt.take_dirty_rect(), Some(area));
    }

    /// The unclipped hatch can paint any pixel, so it says so: the whole panel.
    #[test]
    fn the_unclipped_hatch_marks_the_whole_panel() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let mut disp = MockDisplay::<BinaryColor>::new();
        let (w, h) = {
            let bb = disp.bounding_box().size;
            (bb.width as u16, bb.height as u16)
        };
        let mut tgt = GraphicsTarget::new(&mut disp, FONT_6X10);
        let _ = tgt.display_mut();
        assert_eq!(tgt.take_dirty_rect(), Some(Area::new(0, 0, w, h)));
    }

    /// Both targets track, and they agree: the same calls give the same region.
    #[test]
    fn mono_and_colour_report_the_same_region() {
        use embedded_graphics::mock_display::MockDisplay;
        use embedded_graphics::mono_font::ascii::FONT_6X10;

        let calls = |t: &mut dyn RenderTarget| {
            t.clear(Area::new(0, 12, 40, 10));
            t.draw_text(2, 12, "Ok", Style::Normal);
            t.draw_line(4, 30, 9, 34, Style::Accent);
        };

        let mut mono = MockDisplay::<BinaryColor>::new();
        mono.set_allow_overdraw(true);
        let mut m = GraphicsTarget::new(&mut mono, FONT_6X10);
        calls(&mut m);
        let mono_region = m.take_dirty_rect();

        let mut color = MockDisplay::<Rgb565>::new();
        color.set_allow_overdraw(true);
        let mut c = ColorGraphicsTarget::new(&mut color, FONT_6X10);
        calls(&mut c);

        // clear 0..40 x 12..22, text inside it, line 4..10 x 30..35.
        assert_eq!(mono_region, Some(Area::new(0, 12, 40, 23)));
        assert_eq!(c.take_dirty_rect(), mono_region);
    }
}
